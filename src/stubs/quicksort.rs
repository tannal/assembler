// ============================================================
//  src/stubs/quicksort.rs  —  跨平台 JIT 快速排序（单份代码）
//
//  签名: extern "C" fn(ptr: *mut isize, low: isize, high: isize)
//
//  完全无 #[cfg]，x86-64 / AArch64 / ARM Thumb-2 共用一份 emit。
//
//  算法: Lomuto partition（原地递归）
//
//  ┌─────────────────────────────────────────────────────────┐
//  │  栈帧布局（每次递归调用都建立独立帧）                     │
//  │                                                         │
//  │  [prologue 保存 fp/lr]                                  │
//  │  push ptr   (Arg0)                                      │
//  │  push low   (Arg1)                                      │
//  │  push high  (Arg2)                                      │
//  │  push pivot (Tmp0)   ← partition 结束后仍需              │
//  │  push ppos  (Tmp1)   ← pivot_pos，供右递归用             │
//  │  [partition 使用 Tmp2=j  Tmp3=tmp  Ret=swap_tmp]        │
//  │  第一次递归：call entry (左半, 参数已设好)                │
//  │  pop ppos, pivot (恢复)                                 │
//  │  重设参数 → 第二次递归：call entry (右半)                 │
//  │  pop high, low, ptr                                     │
//  │  epilogue                                               │
//  │  ret                                                    │
//  └─────────────────────────────────────────────────────────┘
//
//  VReg 分配:
//    Arg(0) = ptr
//    Arg(1) = low
//    Arg(2) = high
//    Tmp(0) = pivot   = arr[high]
//    Tmp(1) = i       partition 游标（最终变为 pivot_pos）
//    Tmp(2) = j       遍历游标
//    Tmp(3) = swap_t  交换临时值
//    Ret    = swap_t2 第二个交换临时值（arr[i] 在 swap 时）
//    Cnt    = pivot_pos  递归间传递用（push/pop 额外保存）
// ============================================================

use crate::macro_asm::{Cond, NativeMasm, VReg::*};
use crate::runtime::JitFn;

pub fn build_quicksort() -> JitFn<unsafe extern "C" fn(*mut isize, isize, isize)> {
    let mut m = NativeMasm::new();

    // ─────────────────────────────────────────────────────────
    // 函数入口（递归 call_label 跳回此处）
    // ─────────────────────────────────────────────────────────
    let entry = m.new_label();
    let done = m.new_label();

    m.bind(&entry);

    // ── 边界检查：if low >= high → return ────────────────────
    m.cmp(Arg(1), Arg(2));
    m.jump_if(Cond::Ge, &done);

    // ── 建帧 + 保存所有跨递归寄存器 ──────────────────────────
    m.prologue(); // 平台标准 prologue（保 fp/lr 等）
    m.push_vreg(Arg(0)); // ptr
    m.push_vreg(Arg(1)); // low
    m.push_vreg(Arg(2)); // high

    // ─────────────────────────────────────────────────────────
    // PARTITION (Lomuto)
    //
    //   pivot = arr[high]      → Tmp(0)
    //   i     = low - 1        → Tmp(1)
    //   j     = low            → Tmp(2)
    //
    //   while j < high:
    //     if arr[j] <= pivot:
    //       i++; swap(arr[i], arr[j])
    //     j++
    //   pivot 归位: swap(arr[i+1], arr[high])
    //   pivot_pos = i + 1      → Cnt
    // ─────────────────────────────────────────────────────────

    // pivot = arr[high]
    m.load_ptr_scaled(Tmp(0), Arg(0), Arg(2));

    // i = low - 1
    m.mov(Tmp(1), Arg(1));
    m.dec(Tmp(1));

    // j = low
    m.mov(Tmp(2), Arg(1));

    let loop_top = m.new_label();
    let loop_exit = m.new_label();
    let no_swap = m.new_label();

    m.bind(&loop_top);

    // while j < high
    m.cmp(Tmp(2), Arg(2));
    m.jump_if(Cond::Ge, &loop_exit);

    // tmp3 = arr[j]
    m.load_ptr_scaled(Tmp(3), Arg(0), Tmp(2));

    // if arr[j] > pivot → no_swap
    m.cmp(Tmp(3), Tmp(0));
    m.jump_if(Cond::Gt, &no_swap);

    // i++
    m.inc(Tmp(1));

    // swap arr[i] ↔ arr[j]
    //   Ret = arr[i]  (借 Ret 做第二临时)
    m.load_ptr_scaled(Ret, Arg(0), Tmp(1)); // Ret   = arr[i]
    m.store_ptr_scaled(Arg(0), Tmp(1), Tmp(3)); // arr[i] = arr[j]
    m.store_ptr_scaled(Arg(0), Tmp(2), Ret); // arr[j] = old arr[i]

    m.bind(&no_swap);
    m.inc(Tmp(2)); // j++
    m.jump(&loop_top);

    m.bind(&loop_exit);

    // pivot 归位：swap arr[i+1] ↔ arr[high]
    m.inc(Tmp(1)); // Tmp(1) = pivot_pos

    m.load_ptr_scaled(Tmp(3), Arg(0), Tmp(1)); // Tmp(3) = arr[pivot_pos]
    m.store_ptr_scaled(Arg(0), Tmp(1), Tmp(0)); // arr[pivot_pos] = pivot
    m.store_ptr_scaled(Arg(0), Arg(2), Tmp(3)); // arr[high]      = old arr[pivot_pos]

    // pivot_pos 需要跨两次递归存活 → 保存到栈
    m.push_vreg(Tmp(1)); // push pivot_pos

    // ─────────────────────────────────────────────────────────
    // 递归左半：quicksort(ptr, low, pivot_pos - 1)
    //   Arg(0) = ptr   ← 未变
    //   Arg(1) = low   ← 未变
    //   Arg(2) = pivot_pos - 1
    // ─────────────────────────────────────────────────────────
    m.mov(Arg(2), Tmp(1));
    m.dec(Arg(2)); // Arg(2) = pivot_pos - 1
                   // Arg(0) 和 Arg(1) 在 partition 中未改变

    m.call_label(&entry); // ← 递归左半

    // ─────────────────────────────────────────────────────────
    // 递归右半：quicksort(ptr, pivot_pos + 1, high)
    //   恢复 pivot_pos，再从栈上恢复 high 与 ptr
    // ─────────────────────────────────────────────────────────
    m.pop_vreg(Tmp(1)); // Tmp(1) = pivot_pos（恢复）
    m.pop_vreg(Arg(2)); // Arg(2) = high（恢复）
    m.pop_vreg(Arg(1)); // Arg(1) = low（恢复，此处不再使用但要平栈）
    m.pop_vreg(Arg(0)); // Arg(0) = ptr（恢复）

    // 重设右递归参数
    m.mov(Arg(1), Tmp(1));
    m.inc(Arg(1)); // Arg(1) = pivot_pos + 1
                   // Arg(0) = ptr（刚恢复）, Arg(2) = high（刚恢复）

    m.call_label(&entry); // ← 递归右半

    // ─────────────────────────────────────────────────────────
    // 清理并返回
    // ─────────────────────────────────────────────────────────
    m.epilogue();

    m.bind(&done);
    m.ret();

    unsafe { m.compile() }
}
#[test]
fn test_jit_quicksort() {
    println!("\n[*] Testing JIT Quicksort (Cross-Platform Stub) ...");

    // 1. 构建 JIT 函数
    let jit_qs = build_quicksort();
    println!(
        "    Entry: {:p}  Size: {} bytes",
        jit_qs.entry_addr(),
        jit_qs.code_size()
    );

    // 2. 封装调用闭包，处理参数对齐
    // 签名: fn(ptr, low, high)
    let sort = |arr: &mut Vec<isize>| {
        let n = arr.len() as isize;
        if n > 1 {
            unsafe { (jit_qs.get())(arr.as_mut_ptr(), 0, n - 1) };
        }
    };

    // 3. 辅助工具：检查有序性
    let is_sorted = |arr: &[isize]| arr.windows(2).all(|w| w[0] <= w[1]);

    // --- 测试用例 1: 基础乱序 ---
    let mut a1 = vec![3, 1, 4, 1, 5, 9, 2, 6, 5];
    sort(&mut a1);
    assert!(is_sorted(&a1), "Basic sort failed: {:?}", a1);
    println!("  [✓] Random w/ Duplicates -> {:?}", a1);

    // --- 测试用例 2: 边界检查（空与单元素） ---
    let mut a2_empty: Vec<isize> = vec![];
    let mut a2_single = vec![42];
    sort(&mut a2_empty);
    sort(&mut a2_single);
    assert!(is_sorted(&a2_empty));
    assert!(is_sorted(&a2_single));
    println!("  [✓] Empty & Single Element checked.");

    // --- 测试用例 3: 逆序数组（Lomuto 最差情况，测试栈深） ---
    let mut a3 = vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    sort(&mut a3);
    assert!(is_sorted(&a3));
    println!("  [✓] Reverse Sorted -> {:?}", a3);

    // --- 测试用例 4: 已排序数组 ---
    let mut a4 = vec![1, 2, 3, 4, 5];
    sort(&mut a4);
    assert!(is_sorted(&a4));
    println!("  [✓] Already Sorted checked.");

    // --- 测试用例 5: 随机压力测试 ---
    use std::time::Instant;
    let size = 1000;
    let mut a5: Vec<isize> = (0..size).rev().collect(); // 构造大逆序

    let now = Instant::now();
    sort(&mut a5);
    let elapsed = now.elapsed();

    assert!(is_sorted(&a5));
    println!("  [✓] {} elements sorted in {:?}", size, elapsed);
}
