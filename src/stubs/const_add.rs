// ============================================================
//  src/stubs/const_add.rs
//
//  生成签名为:  extern "C" fn(x: i64) -> i64
//  语义: return x + 10  (循环 10 次，每次 +1)
// ============================================================

use crate::runtime::{JitFn, JitRuntime};

// ── x86-64 ───────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::arch::x64::{reg::*, X64Assembler};
    use crate::arch::ArchAssembler;

    let mut asm = X64Assembler::new();

    #[cfg(target_os = "windows")]
    let param = rcx;
    #[cfg(not(target_os = "windows"))]
    let param = rdi;

    asm.mov_r64_imm64(rbx, 10); // counter = 10

    let loop_start = asm.new_label();

    asm.bind(&loop_start);
    asm.add_r64_imm32(param, 1); // x++
    asm.sub_r64_imm32(rbx, 1);   // counter--
    asm.jnz(&loop_start);         // if counter != 0 continue

    asm.mov_r64_r64(rax, param);  // return x
    asm.ret();

    unsafe { JitRuntime::compile(asm) }
}

// ── AArch64 ──────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::arch::arm64::{reg::*, Arm64Assembler};
    use crate::arch::ArchAssembler;

    let mut asm = Arm64Assembler::new();

    asm.mov_imm64(x1, 10); // counter = 10

    let loop_start = asm.new_label();

    asm.bind(&loop_start);
    asm.add_imm12(x0, x0, 1); // x++
    asm.sub_imm12(x1, x1, 1); // counter--
    asm.cbnz(x1, &loop_start); // if counter != 0 continue

    asm.ret(); // return x0

    unsafe { JitRuntime::compile(asm) }
}

// ── ARM Thumb-2 ──────────────────────────────────────────────

#[cfg(target_arch = "arm")]
pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    use crate::arch::arm::{reg::*, ArmAssembler};
    use crate::arch::ArchAssembler;

    let mut asm = ArmAssembler::new();

    asm.push_r4_lr();
    asm.mov_imm8(r1, 10); // counter = 10

    let loop_start = asm.new_label();

    asm.bind(&loop_start);
    asm.add_imm8(r0, r0, 1);
    asm.sub_imm8(r1, r1, 1);
    asm.bne(&loop_start);

    asm.pop_r4_pc();

    unsafe { JitRuntime::compile(asm) }
}
