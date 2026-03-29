// ============================================================
//  src/stubs/sum_array.rs
//
//  生成签名为:
//    extern "C" fn(ptr: *const i64, len: i64) -> i64
//
//  x86-64 SysV:  ptr=rdi, len=rsi, ret=rax
//  x86-64 Win64: ptr=rcx, len=rdx, ret=rax
//  AArch64:      ptr=x0,  len=x1,  ret=x0
//  ARM Thumb-2:  ptr=r0,  len=r1,  ret=r0 (i32 elements)
// ============================================================

use crate::runtime::{JitFn, JitRuntime};

// ── x86-64 ───────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i64, i64) -> i64> {
    use crate::arch::x64::{reg::*, X64Assembler};
    use crate::arch::ArchAssembler;

    let mut asm = X64Assembler::new();

    asm.push_rbp();
    asm.mov_rbp_rsp();
    asm.xor_r64_r64(rax, rax); // acc = 0

    // 循环计数器
    #[cfg(target_os = "windows")]
    let (arr_reg, len_reg) = (rcx, rdx);
    #[cfg(not(target_os = "windows"))]
    let (arr_reg, len_reg) = (rdi, rsi);

    // 在非 Windows 上使用 rcx 作为 index（Windows 上用 r11 避免破坏参数）
    #[cfg(not(target_os = "windows"))]
    let idx_reg = rcx;
    #[cfg(target_os = "windows")]
    let idx_reg = r11;

    asm.xor_r64_r64(idx_reg, idx_reg); // i = 0

    let loop_start = asm.new_label();
    let done       = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_r64_r64(idx_reg, len_reg);
    asm.jge(&done);                                    // if i >= len goto done
    asm.mov_r64_mem_base_idx8(r10, arr_reg, idx_reg); // r10 = arr[i]
    asm.add_r64_r64(rax, r10);                        // acc += r10
    asm.inc_r64(idx_reg);                             // i++
    asm.jmp(&loop_start);

    asm.bind(&done);
    asm.pop_rbp();
    asm.ret();

    unsafe { JitRuntime::compile(asm) }
}

// ── AArch64 ──────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i64, i64) -> i64> {
    use crate::arch::arm64::{reg::*, Arm64Assembler};
    use crate::arch::ArchAssembler;

    //  x0 = ptr, x1 = len
    //  x2 = accumulator
    //  x3 = index i
    //  x4 = temp (loaded element)

    let mut asm = Arm64Assembler::new();

    asm.stp_fp_lr_pre();
    asm.mov_fp_sp();

    // x2 = 0 (acc), x3 = 0 (i)
    asm.movz(x2, 0);
    asm.movz(x3, 0);

    let loop_start = asm.new_label();
    let done       = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_reg(x3, x1);    // cmp i, len
    asm.bge(&done);          // if i >= len goto done

    asm.ldr_reg_base_idx_lsl3(x4, x0, x3); // x4 = arr[i]
    asm.add_reg(x2, x2, x4);               // acc += x4
    asm.add_imm12(x3, x3, 1);              // i++
    asm.b(&loop_start);

    asm.bind(&done);
    asm.mov_reg(x0, x2);  // return acc
    asm.ldp_fp_lr_post();
    asm.ret();

    unsafe { JitRuntime::compile(asm) }
}

// ── ARM Thumb-2 ──────────────────────────────────────────────
// ARM 32-bit 版本操作 i32 元素（受限于 32-bit 地址空间）

#[cfg(target_arch = "arm")]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i32, i32) -> i32> {
    use crate::arch::arm::{reg::*, ArmAssembler};
    use crate::arch::ArchAssembler;

    //  r0 = ptr, r1 = len
    //  r2 = acc
    //  r3 = i
    //  r4 = temp

    let mut asm = ArmAssembler::new();

    asm.push_r4_lr();

    asm.mov_imm8(r2, 0); // acc = 0
    asm.mov_imm8(r3, 0); // i   = 0

    let loop_start = asm.new_label();
    let done       = asm.new_label();

    asm.bind(&loop_start);
    asm.cmp_reg_t16(r3, r1); // cmp i, len
    asm.bge(&done);

    asm.ldr_reg_lsl2(r4, r0, r3); // r4 = arr[i] (i32, LSL #2)
    asm.add_reg_t16(r2, r2, r4);  // acc += r4
    asm.add_imm8(r3, r3, 1);       // i++
    asm.b(&loop_start);

    asm.bind(&done);
    asm.mov_reg(r0, r2); // return acc
    asm.pop_r4_pc();

    unsafe { JitRuntime::compile(asm) }
}
