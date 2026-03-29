// ============================================================
//  src/stubs/factorial.rs
//
//  生成签名为:  extern "C" fn(n: i64) -> i64
//
//  x86-64 SysV:  n=rdi, ret=rax
//  x86-64 Win64: n=rcx, ret=rax
//  AArch64:      n=x0,  ret=x0
//  ARM:          n=r0,  ret=r0
// ============================================================

use crate::runtime::{JitFn, JitRuntime};

// ── x86-64 ───────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::arch::x64::{reg::*, X64Assembler};
    use crate::arch::ArchAssembler;

    let mut asm = X64Assembler::new();

    // 参数寄存器（n）
    #[cfg(target_os = "windows")]
    let n_reg = rcx;
    #[cfg(not(target_os = "windows"))]
    let n_reg = rdi;

    asm.mov_r64_imm64(rax, 1); // acc = 1

    let loop_start = asm.new_label();
    let end        = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_r64_imm32(n_reg, 1);
    asm.jle(&end);             // if n <= 1 goto end

    asm.imul_r64_r64(rax, n_reg); // acc *= n
    asm.sub_r64_imm32(n_reg, 1);  // n--
    asm.jmp(&loop_start);

    asm.bind(&end);
    asm.ret();

    unsafe { JitRuntime::compile(asm) }
}

// ── AArch64 ──────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::arch::arm64::{reg::*, Arm64Assembler};
    use crate::arch::ArchAssembler;

    //  x0 = n (parameter & loop counter)
    //  x1 = acc

    let mut asm = Arm64Assembler::new();

    asm.mov_imm64(x1, 1); // acc = 1

    let loop_start = asm.new_label();
    let end        = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_imm12(x0, 1);
    asm.ble(&end);          // if n <= 1 goto end

    asm.mul_reg(x1, x1, x0);  // acc *= n
    asm.sub_imm12(x0, x0, 1); // n--
    asm.b(&loop_start);

    asm.bind(&end);
    asm.mov_reg(x0, x1); // return acc
    asm.ret();

    unsafe { JitRuntime::compile(asm) }
}

// ── ARM Thumb-2 ──────────────────────────────────────────────

#[cfg(target_arch = "arm")]
pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    use crate::arch::arm::{reg::*, ArmAssembler};
    use crate::arch::ArchAssembler;

    //  r0 = n, r1 = acc

    let mut asm = ArmAssembler::new();

    asm.push_r4_lr();
    asm.mov_imm8(r1, 1); // acc = 1

    let loop_start = asm.new_label();
    let end        = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_imm8(r0, 1);
    asm.ble(&end);

    asm.mul_reg(r1, r1, r0);
    asm.sub_imm8(r0, r0, 1);
    asm.b(&loop_start);

    asm.bind(&end);
    asm.mov_reg(r0, r1); // return acc
    asm.pop_r4_pc();

    unsafe { JitRuntime::compile(asm) }
}
