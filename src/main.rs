// ============================================================
//  src/main.rs  —  跨平台 JIT Assembler 演示 + 测试
// ============================================================

use jit_assembler::{
    arch::Arch,
    stubs::{bubble_sort, build_bubblesort, build_const_add, build_factorial, build_quicksort, build_sum_array},
    util::hexdump,
};

#[cfg(target_arch = "x86_64")]
fn show_sum_array_bytes() {
    use jit_assembler::arch::ArchAssembler;
    use jit_assembler::arch::x64::{reg::*, X64Assembler};
    use jit_assembler::runtime::JitRuntime;
    let mut asm = X64Assembler::new();
    asm.push_rbp();
    asm.mov_rbp_rsp();
    asm.xor_r64_r64(rax, rax);
    #[cfg(target_os = "windows")]
    let (arr, len, idx) = (rcx, rdx, r11);
    #[cfg(not(target_os = "windows"))]
    let (arr, len, idx) = (rdi, rsi, rcx);
    asm.xor_r64_r64(idx, idx);
    let ls = asm.new_label();
    let dn = asm.new_label();
    asm.bind(&ls);
    asm.cmp_r64_r64(idx, len);
    asm.jge(&dn);
    asm.mov_r64_mem_base_idx8(r10, arr, idx);
    asm.add_r64_r64(rax, r10);
    asm.inc_r64(idx);
    asm.jmp(&ls);
    asm.bind(&dn);
    asm.pop_rbp();
    asm.ret();
    hexdump("sum_array (x86-64 raw)", &JitRuntime::assemble_bytes(asm));
}

#[cfg(not(target_arch = "x86_64"))]
fn show_sum_array_bytes() {}

fn main() {
    let arch = Arch::native();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Cross-Platform JIT Assembler  │  arch = {}", arch);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    show_sum_array_bytes();

    // ── sum_array ─────────────────────────────────────────────
    println!("\n[*] sum_array …");
    let jit_sum = build_sum_array();
    println!("    entry: {:p}  size: {} bytes", jit_sum.entry_addr(), jit_sum.code_size());

    let cases: Vec<(&str, Vec<i64>)> = vec![
        ("empty",     vec![]),
        ("single 42", vec![42]),
        ("1..=10",    (1..=10).collect()),
        ("pow2",      vec![1,2,4,8,16,32,64,128]),
        ("negatives", vec![-5,-3,0,3,5]),
        ("1..=1000",  (1..=1000).collect()),
    ];

    println!("\n  ┌─────────────────────┬──────────────┬──────────────┬──────┐");
    println!("  │ Array               │ JIT          │ Rust         │  OK? │");
    println!("  ├─────────────────────┼──────────────┼──────────────┼──────┤");
    for (name, arr) in &cases {
        let jit_r  = unsafe { (jit_sum.get())(arr.as_ptr(), arr.len() as i64) };
        let rust_r: i64 = arr.iter().sum();
        let ok = jit_r == rust_r;
        println!("  │ {:<19} │ {:>12} │ {:>12} │  {}  │",
            name, jit_r, rust_r, if ok {"✓"} else {"✗ FAIL"});
        assert!(ok, "sum_array FAIL: {}", name);
    }
    println!("  └─────────────────────┴──────────────┴──────────────┴──────┘");

    // ── factorial ─────────────────────────────────────────────
    println!("\n[*] factorial …");
    let jit_fact = build_factorial();
    for (n, want) in [(0i64,1i64),(1,1),(5,120),(10,3628800),(12,479001600)] {
        let r = unsafe { (jit_fact.get())(n) };
        if r == want { println!("  [✓] {}! = {}", n, r); }
        else { panic!("  [✗] {}! = {} (expected {})", n, r, want); }
    }

    // ── const_add ─────────────────────────────────────────────
    println!("\n[*] const_add …");
    let jit_add = build_const_add();
    for (x, want) in [(10i64, 20i64), (0, 10)] {
        let r = unsafe { (jit_add.get())(x) };
        if r == want { println!("  [✓] const_add({}) = {}", x, r); }
        else { panic!("  [✗] const_add({}) = {} (expected {})", x, r, want); }
    }

    // ── quicksort ─────────────────────────────────────────────
    println!("\n[*] quicksort …");
    let jit_qs = build_quicksort();
    println!("    entry: {:p}  size: {} bytes", jit_qs.entry_addr(), jit_qs.code_size());

    let sort = |arr: &mut Vec<isize>| {
        let n = arr.len() as isize;
        if n > 1 {
            unsafe { (jit_qs.get())(arr.as_mut_ptr(), 0, n - 1) };
        }
    };
    let is_sorted = |arr: &[isize]| arr.windows(2).all(|w| w[0] <= w[1]);

    let mut a: Vec<isize> = vec![];
    sort(&mut a);
    assert!(is_sorted(&a));
    println!("  [✓] empty      → {:?}", a);

    let mut a = vec![42isize];
    sort(&mut a);
    assert_eq!(a, vec![42]);
    println!("  [✓] single     → {:?}", a);

    let mut a: Vec<isize> = (1..=8).collect();
    sort(&mut a);
    assert!(is_sorted(&a));
    println!("  [✓] sorted     → {:?}", a);

    let mut a: Vec<isize> = (1..=8).rev().collect();
    sort(&mut a);
    assert!(is_sorted(&a));
    println!("  [✓] reverse    → {:?}", a);

    let mut a: Vec<isize> = vec![3,1,4,1,5,9,2,6,5,3,5];
    let mut want = a.clone(); want.sort();
    sort(&mut a);
    assert_eq!(a, want);
    println!("  [✓] random     → {:?}", a);

    let mut a = vec![7isize; 8];
    sort(&mut a);
    assert!(a.iter().all(|&x| x == 7));
    println!("  [✓] equal      → {:?}", a);

    let mut a = vec![9isize, 1];
    sort(&mut a);
    assert_eq!(a, vec![1, 9]);
    println!("  [✓] two-elem   → {:?}", a);

    let mut a: Vec<isize> = vec![-5,10,-3,0,7,-1,4];
    let mut want = a.clone(); want.sort();
    sort(&mut a);
    assert_eq!(a, want);
    println!("  [✓] negatives  → {:?}", a);

    {
        let mut rng: u64 = 0xDEAD_BEEF_1234_5678;
        let mut a: Vec<isize> = (0..1000).map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((rng >> 33) as isize) % 10000 - 5000
        }).collect();
        let mut want = a.clone(); want.sort();
        sort(&mut a);
        assert_eq!(a, want);
        println!("  [✓] large n=1000  ✓ sorted");
    }

    {
        let mut rng: u64 = 0xCAFE_BABE_0000_0001;
        let mut a: Vec<isize> = (0..256).map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (rng >> 33) as isize % 1000
        }).collect();
        let mut want = a.clone(); want.sort();
        sort(&mut a);
        assert_eq!(a, want);
        println!("  [✓] vs stdlib n=256 ✓ identical");
    }

    println!("\n[✓] All tests passed on {} !", arch);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
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