// ============================================================
//  src/macro_asm/arm64_backend.rs
//
//  AAPCS64 VReg 映射：
//
//    VReg     AArch64 寄存器
//    ──────── ───────────────
//    Arg(0)   x0
//    Arg(1)   x1
//    Arg(2)   x2
//    Arg(3)   x3
//    Ret      x0
//    Tmp(0)   x9
//    Tmp(1)   x10
//    Tmp(2)   x11
//    Tmp(3)   x12
//    Cnt      x13   (caller-saved, safe as counter)
//    Ptr      x0    (与 Arg(0) 相同)
//    Acc      x8    (间接返回寄存器，此处复用为累加器)
// ============================================================

use crate::arch::{Arch, Label};
use crate::arch::arm64::{reg::*, XReg, Arm64Assembler};
use crate::runtime::JitFn;
use super::backend::{Cond, MacroAssemblerBackend, VReg};

fn vreg_to_reg(v: VReg) -> XReg {
    match v {
        VReg::Arg(0) => x0,
        VReg::Arg(1) => x1,
        VReg::Arg(2) => x2,
        VReg::Arg(3) => x3,
        VReg::Ret    => x0,
        VReg::Tmp(0) => x9,
        VReg::Tmp(1) => x10,
        VReg::Tmp(2) => x11,
        VReg::Tmp(3) => x12,
        VReg::Cnt    => x13,
        VReg::Ptr    => x0,
        _ => panic!("unsupported VReg {:?}", v),
    }
}

pub struct Arm64Backend {
    asm: Arm64Assembler,
}

impl Arm64Backend {
    fn r(&self, v: VReg) -> XReg { vreg_to_reg(v) }
}

impl MacroAssemblerBackend for Arm64Backend {
    fn new() -> Self { Arm64Backend { asm: Arm64Assembler::new() } }

    unsafe fn compile<F: Copy>(self) -> JitFn<F> {
        crate::runtime::JitRuntime::compile(self.asm)
    }

    fn arch() -> Arch { Arch::Aarch64 }

    fn new_label(&mut self) -> Label   { self.asm.new_label() }
    fn bind(&mut self, l: &Label)      { self.asm.bind(l) }

    fn prologue(&mut self) {
        self.asm.stp_fp_lr_pre();
        self.asm.mov_fp_sp();
    }

    fn epilogue(&mut self) {
        self.asm.ldp_fp_lr_post();
    }

    fn ret(&mut self) { self.asm.ret(); }

    // ── 数据移动 ─────────────────────────────────────────────

    fn mov(&mut self, dst: VReg, src: VReg) {
        let (d, s) = (self.r(dst), self.r(src));
        if d != s { self.asm.mov_reg(d, s); }
    }

    fn mov_imm(&mut self, dst: VReg, imm: i64) {
        self.asm.mov_imm64(self.r(dst), imm as u64);
    }

    fn load_ptr_scaled(&mut self, dst: VReg, base: VReg, idx: VReg) {
        // LDR Xd, [Xbase, Xidx, LSL #3]
        self.asm.ldr_reg_base_idx_lsl3(self.r(dst), self.r(base), self.r(idx));
    }

    fn store_ptr_scaled(&mut self, base: VReg, idx: VReg, src: VReg) {
        self.asm.str_reg_base_idx_lsl3(self.r(src), self.r(base), self.r(idx));
    }

    // ── 算术 ─────────────────────────────────────────────────

    fn add(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.add_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn add_imm(&mut self, dst: VReg, imm: i32) {
        let r = self.r(dst);
        if imm >= 0 {
            self.asm.add_imm12(r, r, imm as u16);
        } else {
            self.asm.sub_imm12(r, r, (-imm) as u16);
        }
    }

    fn sub(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.sub_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn sub_imm(&mut self, dst: VReg, imm: i32) {
        let r = self.r(dst);
        if imm >= 0 {
            self.asm.sub_imm12(r, r, imm as u16);
        } else {
            self.asm.add_imm12(r, r, (-imm) as u16);
        }
    }

    fn mul(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        self.asm.mul_reg(self.r(dst), self.r(lhs), self.r(rhs));
    }

    fn zero(&mut self, dst: VReg) {
        // MOVZ Xd, #0
        self.asm.movz(self.r(dst), 0);
    }

    // ── 比较与跳转 ───────────────────────────────────────────

    fn cmp(&mut self, lhs: VReg, rhs: VReg) {
        self.asm.cmp_reg(self.r(lhs), self.r(rhs));
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
        // AArch64 没有单寄存器 push；用 STP Xr, Xr, [sp, #-16]! 保存（占 16 字节，浪费但安全）
        // 更好的方案：成对 push，此处简化用 xzr 凑对
        let reg = self.r(r);
        self.asm.stp_pre(reg, xzr);
    }

    fn pop_vreg(&mut self, r: VReg) {
        let reg = self.r(r);
        self.asm.ldp_post(reg, xzr);
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
    
    fn store_mem(&mut self, base: VReg, offset: i64, src: VReg) {
        todo!()
    }
}
