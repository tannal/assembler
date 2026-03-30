// ============================================================
//  src/stubs/bubblesort.rs  —  跨平台 JIT 冒泡排序（单份代码）
//
//  签名: extern "C" fn(ptr: *mut isize, len: isize)
//
//  完全无 #[cfg]，x86-64 / AArch64 / ARM Thumb-2 共用一份 emit。
//
//  算法: 标准冒泡排序（含 early-exit 优化）
//
//  ┌─────────────────────────────────────────────────────────┐
//  │  栈帧布局                                               │
//  │                                                         │
//  │  [prologue 保存 fp/lr]                                  │
//  │  push ptr    (Arg(0))  ← 保存原始参数                   │
//  │  push len    (Arg(1))                                   │
//  │                                                         │
//  │  外层循环: i = 0..len-1                                  │
//  │    swapped = 0          (Tmp(3))  early-exit 标志        │
//  │    内层循环: j = 0..len-i-2                              │
//  │      if arr[j] > arr[j+1]: swap; swapped = 1            │
//  │    if swapped == 0: break                               │
//  │                                                         │
//  │  VReg 分配:                                             │
//  │    Arg(0) = ptr         数组首地址（全程不变）           │
//  │    Arg(1) = len         数组长度（全程不变）             │
//  │    Tmp(0) = i           外层游标                         │
//  │    Tmp(1) = j           内层游标                         │
//  │    Tmp(2) = inner_end   内层上界 = len - i - 1           │
//  │    Tmp(3) = swapped     本轮是否发生过交换               │
//  │    Ret    = a            arr[j]   交换临时值             │
//  │    Cnt    = b            arr[j+1] 交换临时值             │
//  └─────────────────────────────────────────────────────────┘
// ============================================================

use crate::macro_asm::{Cond, NativeMasm, VReg::*};
use crate::runtime::JitFn;

pub fn build_bubblesort() -> JitFn<unsafe extern "C" fn(*mut isize, isize)> {
    let mut m = NativeMasm::new();

    let entry      = m.new_label();
    let done       = m.new_label();
    let outer_top  = m.new_label();
    let outer_exit = m.new_label();
    let inner_top  = m.new_label();
    let inner_exit = m.new_label();
    let no_swap    = m.new_label();

    m.bind(&entry);

    // ── 边界检查：if len <= 1 → return ───────────────────────
    m.mov_imm(Tmp(0), 1);
    m.cmp(Arg(1), Tmp(0));
    m.jump_if(Cond::Le, &done);

    // ── 建帧 + 保存跨循环寄存器 ──────────────────────────────
    m.prologue();
    m.push_vreg(Arg(0));   // ptr
    m.push_vreg(Arg(1));   // len

    // ── 外层初始化：i = 0 ────────────────────────────────────
    m.mov_imm(Tmp(0), 0);  // i = 0

    // ─────────────────────────────────────────────────────────
    // 外层循环  for i in 0 .. len-1
    // ─────────────────────────────────────────────────────────
    m.bind(&outer_top);

    //  if i >= len - 1 → exit
    m.mov(Tmp(2), Arg(1));
    m.dec(Tmp(2));                      // Tmp(2) = len - 1
    m.cmp(Tmp(0), Tmp(2));
    m.jump_if(Cond::Ge, &outer_exit);

    //  inner_end = len - i - 1  （内层 j 的上界，j < inner_end）
    m.mov(Tmp(2), Arg(1));
    m.sub(Tmp(2), Tmp(2),Tmp(0));              // Tmp(2) = len - i
    m.dec(Tmp(2));                      // Tmp(2) = len - i - 1

    //  swapped = 0
    m.mov_imm(Tmp(3), 0);

    //  j = 0
    m.mov_imm(Tmp(1), 0);

    // ─────────────────────────────────────────────────────────
    // 内层循环  for j in 0 .. inner_end
    // ─────────────────────────────────────────────────────────
    m.bind(&inner_top);

    //  if j >= inner_end → exit inner
    m.cmp(Tmp(1), Tmp(2));
    m.jump_if(Cond::Ge, &inner_exit);

    //  a = arr[j]
    m.load_ptr_scaled(Ret, Arg(0), Tmp(1));

    //  b = arr[j+1]
    m.mov(Cnt, Tmp(1));
    m.inc(Cnt);                         // Cnt = j + 1
    m.load_ptr_scaled(Cnt, Arg(0), Cnt);

    //  if a <= b → no_swap
    m.cmp(Ret, Cnt);
    m.jump_if(Cond::Le, &no_swap);

    //  swap: arr[j] = b,  arr[j+1] = a
    m.store_ptr_scaled(Arg(0), Tmp(1), Cnt); // arr[j]   = b

    m.mov(Cnt, Tmp(1));
    m.inc(Cnt);                              // Cnt = j + 1（重算，Cnt 被 store 覆盖）
    m.store_ptr_scaled(Arg(0), Cnt, Ret);    // arr[j+1] = a

    //  swapped = 1
    m.mov_imm(Tmp(3), 1);

    m.bind(&no_swap);

    //  j++
    m.inc(Tmp(1));
    m.jump(&inner_top);

    // ─────────────────────────────────────────────────────────
    // 内层退出
    // ─────────────────────────────────────────────────────────
    m.bind(&inner_exit);

    //  early-exit：if swapped == 0 → outer_exit
    m.cmp_imm(Tmp(3), 0);
    m.jump_if(Cond::Eq, &outer_exit);

    //  i++，继续外层
    m.inc(Tmp(0));
    m.jump(&outer_top);

    // ─────────────────────────────────────────────────────────
    // 外层退出 / 清理
    // ─────────────────────────────────────────────────────────
    m.bind(&outer_exit);

    m.pop_vreg(Arg(1));   // 平栈：len
    m.pop_vreg(Arg(0));   // 平栈：ptr

    m.epilogue();

    m.bind(&done);
    m.ret();

    unsafe { m.compile() }
}

#[test]
fn test_jit_bubblesort() {
    println!("\n[*] Testing JIT BubbleSort ...");
    let jit_bs = build_bubblesort();
    println!("    Entry: {:p}  Size: {} bytes", jit_bs.entry_addr(), jit_bs.code_size());

    // 封装调用逻辑
    let sort = |arr: &mut Vec<isize>| {
        let n = arr.len() as isize;
        if n > 1 {
            unsafe { 
                // 注意：冒泡排序通常传入 (指针, 长度)
                (jit_bs.get())(arr.as_mut_ptr(), n) 
            };
        }
    };

    // 辅助函数：检查是否有序
    let is_sorted = |arr: &[isize]| arr.windows(2).all(|w| w[0] <= w[1]);

    // --- 测试用例 1: 空数组 ---
    let mut a1: Vec<isize> = vec![];
    sort(&mut a1);
    assert!(is_sorted(&a1));
    println!("  [✓] Empty array      -> {:?}", a1);

    // --- 测试用例 2: 单个元素 ---
    let mut a2 = vec![42];
    sort(&mut a2);
    assert!(is_sorted(&a2));
    println!("  [✓] Single element   -> {:?}", a2);

    // --- 测试用例 3: 逆序数组 (最差情况) ---
    let mut a3 = vec![5, 4, 3, 2, 1];
    sort(&mut a3);
    assert!(is_sorted(&a3));
    println!("  [✓] Reverse sorted   -> {:?}", a3);

    // --- 测试用例 4: 包含重复项的乱序数组 ---
    let mut a4 = vec![3, 1, 4, 1, 5, 9, 2, 6, 5];
    sort(&mut a4);
    assert!(is_sorted(&a4));
    println!("  [✓] Random w/ dups   -> {:?}", a4);

    // --- 测试用例 5: 大规模随机测试 ---
    use std::time::Instant;
    let mut a5: Vec<isize> = (0..1000).rev().collect(); // 1000 到 0
    let now = Instant::now();
    sort(&mut a5);
    let elapsed = now.elapsed();
    assert!(is_sorted(&a5));
    println!("  [✓] 1000 items (rev) -> Sorted in {:?}", elapsed);
}