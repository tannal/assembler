// ============================================================
//  src/main.rs  —  跨平台 JIT Assembler 演示 + 测试
// ============================================================

use jit_assembler::{
    arch::Arch,
    stubs::{build_const_add, build_factorial, build_sum_array},
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

    println!("\n[✓] All tests passed on {} !", arch);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

