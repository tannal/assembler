// ============================================================
//  src/main.rs  —  跨平台 JIT Assembler 演示
// ============================================================

use jit_assembler::{
    arch::{Arch, ArchAssembler},
    stubs::{build_const_add, build_factorial, build_sum_array, build_const_return},
    util::hexdump,
};

// 只在 x86-64 上展示 hexdump（其他架构同理）
#[cfg(target_arch = "x86_64")]
fn inspect_sum_array() {
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

    let bytes = JitRuntime::assemble_bytes(asm);
    hexdump("sum_array (x86-64)", &bytes);
}

#[cfg(target_arch = "aarch64")]
fn inspect_sum_array() {
    // AArch64 上不展示 hexdump，直接运行测试
}

#[cfg(target_arch = "arm")]
fn inspect_sum_array() {
    // ARM 上不展示 hexdump，直接运行测试
}

fn main() {
    let arch = Arch::native();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Cross-Platform JIT Assembler  │  arch = {}", arch);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // ── 1. 字节检查 ──────────────────────────────────────────
    inspect_sum_array();

    // ── 2. sum_array ─────────────────────────────────────────
    println!("\n[*] Building sum_array JIT stub …");
    let jit_sum = build_sum_array();
    println!("    entry: {:p}  size: {} bytes", jit_sum.entry_addr(), jit_sum.code_size());

    // x86-64 / AArch64 版本操作 i64；ARM 32-bit 操作 i32
    // 用 cfg 选择测试用例类型

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        type Elem = i64;
        let cases: Vec<(&str, Vec<Elem>)> = vec![
            ("empty",          vec![]),
            ("single",         vec![42]),
            ("1..=10",         (1..=10).collect()),
            ("powers of 2",    vec![1, 2, 4, 8, 16, 32, 64, 128]),
            ("negatives",      vec![-5, -3, 0, 3, 5]),
            ("1..=1000",       (1..=1000).collect()),
        ];

        println!("\n┌─────────────────────┬───────────────┬───────────────┬───────┐");
        println!("│ Array               │ JIT result    │ Rust sum      │  OK?  │");
        println!("├─────────────────────┼───────────────┼───────────────┼───────┤");

        for (name, arr) in &cases {
            let jit_result = unsafe { (jit_sum.get())(arr.as_ptr(), arr.len() as i64) };
            let rust_result: Elem = arr.iter().sum();
            let ok = jit_result == rust_result;
            println!(
                "│ {:<19} │ {:>13} │ {:>13} │  {}   │",
                name, jit_result, rust_result,
                if ok { "✓" } else { "✗ FAIL" }
            );
            assert!(ok, "FAIL: {}", name);
        }
        println!("└─────────────────────┴───────────────┴───────────────┴───────┘");
    }

    #[cfg(target_arch = "arm")]
    {
        type Elem = i32;
        let cases: Vec<(&str, Vec<Elem>)> = vec![
            ("single",  vec![42]),
            ("1..=10",  (1i32..=10).collect()),
        ];
        for (name, arr) in &cases {
            let jit_result = unsafe { (jit_sum.get())(arr.as_ptr(), arr.len() as i32) };
            let rust_result: Elem = arr.iter().sum();
            assert_eq!(jit_result, rust_result, "sum_array FAIL: {}", name);
            println!("  [✓] sum_array({}) = {}", name, jit_result);
        }
    }

    // ── 3. factorial ─────────────────────────────────────────
    println!("\n[*] Building factorial JIT stub …");
    let jit_fact = build_factorial();

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let cases = [(0i64, 1i64), (1, 1), (5, 120), (10, 3628800), (12, 479001600)];
        for (n, expected) in cases {
            let result = unsafe { (jit_fact.get())(n) };
            assert_eq!(result, expected, "factorial({}) FAIL", n);
            println!("  [✓] factorial({}) = {}", n, result);
        }
    }

    #[cfg(target_arch = "arm")]
    {
        let cases = [(0i32, 1i32), (1, 1), (5, 120), (10, 3628800)];
        for (n, expected) in cases {
            let result = unsafe { (jit_fact.get())(n) };
            assert_eq!(result, expected, "factorial({}) FAIL", n);
            println!("  [✓] factorial({}) = {}", n, result);
        }
    }

    // ── 4. const_add ─────────────────────────────────────────
    println!("\n[*] Building const_add JIT stub …");
    let jit_add = build_const_add();

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let result = unsafe { (jit_add.get())(10i64) };
        assert_eq!(result, 20, "const_add FAIL");
        println!("  [✓] const_add(10) = {}", result);
    }

    #[cfg(target_arch = "arm")]
    {
        let result = unsafe { (jit_add.get())(10i32) };
        assert_eq!(result, 20, "const_add FAIL");
        println!("  [✓] const_add(10) = {}", result);
    }

    println!("\n[*] Building const_return JIT stub");
    let jit_return = build_const_return();
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let result = unsafe { (jit_return.get())() };
        assert_eq!(result, 10, "const_return FAIL");
        println!("  [✓] const_return(10) = {}", result);
    }
    println!("\n[✓] All JIT stubs verified on {} !", arch);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}
