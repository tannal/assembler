// ============================================================
//  src/macro_asm/arm_backend.rs
//
//  AAPCS32 Thumb-2 VReg 映射：
//
//    VReg     ARM 寄存器    备注
//    ──────── ──────────    ───────────────────────────────
//    Arg(0)   r0            参数 / 返回
//    Arg(1)   r1
//    Arg(2)   r2
//    Arg(3)   r3
//    Ret      r0
//    Tmp(0)   r5            callee-saved → prologue 保存
//    Tmp(1)   r6            callee-saved → prologue 保存
//    Tmp(2)   r7            callee-saved → prologue 保存
//    Tmp(3)   ip (r12)      caller-saved scratch
//    Cnt      r4            callee-saved → prologue 保存
//    Ptr      r0            与 Arg(0) 相同
//
//  prologue: PUSH {r4-r7, lr}   epilogue: POP {r4-r7, pc}
// ============================================================

use crate::arch::{Arch, Label};
use crate::arch::arm::{reg::*, Reg, ArmAssembler};
use crate::runtime::JitFn;
use super::backend::{Cond, MacroAssemblerBackend, VReg};

fn vreg_to_reg(v: VReg) -> Reg {
    match v {
        VReg::Arg(0) => r0,
        VReg::Arg(1) => r1,
        VReg::Arg(2) => r2,
        VReg::Arg(3) => r3,
        VReg::Ret    => r0,
        VReg::Tmp(0) => r5,
        VReg::Tmp(1) => r6,
        VReg::Tmp(2) => r7,
        VReg::Tmp(3) => ip,
        VReg::Cnt    => r4,
        VReg::Ptr    => r0,
        _ => panic!("unsupported VReg {:?}", v),
    }
}

pub struct ArmBackend {
    asm: ArmAssembler,
}

impl ArmBackend {
    fn r(&self, v: VReg) -> Reg { vreg_to_reg(v) }
}

impl MacroAssemblerBackend for ArmBackend {
    fn new() -> Self { ArmBackend { asm: ArmAssembler::new() } }

    unsafe fn compile<F: Copy>(self) -> JitFn<F> {
        crate::runtime::JitRuntime::compile(self.asm)
    }

    fn arch() -> Arch { Arch::Arm }

    fn new_label(&mut self) -> Label  { self.asm.new_label() }
    fn bind(&mut self, l: &Label)     { self.asm.bind(l) }

    fn prologue(&mut self) {
        // PUSH {r4, r5, r6, r7, lr}
        // Thumb-2 32-bit STMDB sp!, {r4-r7, lr}
        // hw1=0xE92D hw2=0x40F0  (bit14=lr, bit7-4=r4..r7)
        self.asm.emit_push_r4_r7_lr();
    }

    fn epilogue(&mut self) {
        // POP {r4, r5, r6, r7, pc}
        self.asm.emit_pop_r4_r7_pc();
    }

    fn ret(&mut self) {
        // RET = BX lr
        self.asm.bx_lr();
    }

    // ── 数据移动 ─────────────────────────────────────────────

    fn mov(&mut self, dst: VReg, src: VReg) {
        let (d, s) = (self.r(dst), self.r(src));
        if d != s { self.asm.mov_reg(d, s); }
    }

    fn mov_imm(&mut self, dst: VReg, imm: i64) {
        // ARM 32-bit 只支持 32-bit 值
        self.asm.mov_imm32(self.r(dst), imm as u32);
    }

    fn load_ptr_scaled(&mut self, dst: VReg, base: VReg, idx: VReg) {
        // LDR Rd, [Rbase, Ridx, LSL #2]  (32-bit word = 4 bytes)
        self.asm.ldr_reg_lsl2(self.r(dst), self.r(base), self.r(idx));
    }

    fn store_ptr_scaled(&mut self, base: VReg, idx: VReg, src: VReg) {
        self.asm.str_reg_lsl2(self.r(src), self.r(base), self.r(idx));
    }

    // ── 算术 ─────────────────────────────────────────────────

    fn add(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.add_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn add_imm(&mut self, dst: VReg, imm: i32) {
        let r = self.r(dst);
        if imm >= 0 && imm <= 255 {
            self.asm.add_imm8(r, r, imm as u8);
        } else if imm < 0 && (-imm) <= 255 {
            self.asm.sub_imm8(r, r, (-imm) as u8);
        } else {
            // 大立即数：用 ip 做临时寄存器加载后再 ADD
            self.asm.mov_imm32(ip, imm as u32);
            self.asm.add_reg(r, r, ip);
        }
    }

    fn sub(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.sub_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn sub_imm(&mut self, dst: VReg, imm: i32) {
        let r = self.r(dst);
        if imm >= 0 && imm <= 255 {
            self.asm.sub_imm8(r, r, imm as u8);
        } else if imm < 0 && (-imm) <= 255 {
            self.asm.add_imm8(r, r, (-imm) as u8);
        } else {
            self.asm.mov_imm32(ip, imm as u32);
            self.asm.sub_reg(r, r, ip);
        }
    }

    fn mul(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.mul_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn zero(&mut self, dst: VReg) {
        let r = self.r(dst);
        // EOR r, r  (XOR with self = 0)，仅 r0-r7 可用 T1
        if r.0 < 8 {
            self.asm.eor_reg_t16(r, r);
        } else {
            self.asm.mov_imm8(r, 0);
        }
    }

    // ── 比较与跳转 ───────────────────────────────────────────

    fn cmp(&mut self, lhs: VReg, rhs: VReg) {
        let (l, r) = (self.r(lhs), self.r(rhs));
        if l.0 < 8 && r.0 < 8 {
            self.asm.cmp_reg_t16(l, r);
        } else {
            // 32-bit CMP
            self.asm.cmp_reg_t32(l, r);
        }
    }

    fn jump_if(&mut self, cond: Cond, label: &Label) {
        match cond {
            Cond::Eq => self.asm.beq(label),
            Cond::Ne => self.asm.bne(label),
            Cond::Lt => self.asm.blt(label),
            Cond::Le => self.asm.ble(label),
            Cond::Gt => self.asm.bgt(label),
            Cond::Ge => self.asm.bge(label),
        }
    }

    fn jump(&mut self, label: &Label) { self.asm.b(label); }

    fn call_label(&mut self, label: &Label) { self.asm.bl(label); }

    fn push_vreg(&mut self, r: VReg) {
        // PUSH {Rd} 用 STMDB sp!, {Rd}
        // 构造 32-bit Thumb-2 STMDB: 0xE92D_0000 | (1 << rd)
        let bit = 1u32 << (self.r(r).0 as u32);
        let insn = 0xE92D_0000u32 | bit;
        self.asm.emit_t32_pub(insn);
    }

    fn pop_vreg(&mut self, r: VReg) {
        // POP {Rd} 用 LDMIA sp!, {Rd}
        let bit = 1u32 << (self.r(r).0 as u32);
        let insn = 0xE8BD_0000u32 | bit;
        self.asm.emit_t32_pub(insn);
    }
    
    fn cmp_imm(&mut self, lhs: VReg, imm: i64) {
        todo!()
    }
    
    fn push(&mut self, reg: VReg) {
        todo!()
    }
    
    fn pop(&mut self, reg: VReg) {
        todo!()
    }
    
    fn call_reg(&mut self, reg: VReg) {
        todo!()
    }
}
