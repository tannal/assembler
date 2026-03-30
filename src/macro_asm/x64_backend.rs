// ============================================================
//  src/macro_asm/x64_backend.rs
//
//  把 MacroAssemblerBackend 的虚拟操作翻译到 x86-64 机器码
//
//  VReg 映射（SysV AMD64 / Microsoft x64 统一处理）:
//
//    VReg        SysV (Linux/macOS)    Win64
//    ─────────── ──────────────────    ──────
//    Arg(0)      rdi                   rcx
//    Arg(1)      rsi                   rdx
//    Arg(2)      rdx                   r8
//    Arg(3)      rcx                   r9
//    Ret         rax                   rax
//    Tmp(0)      r10                   r10
//    Tmp(1)      r11                   r11
//    Tmp(2)      r9  (caller-saved)    (r9 is param, use xmm or push)
//    Tmp(3)      r8                    (r8 is param)
//    Cnt         rcx (SysV free)       r12 (callee-saved, push/pop)
//    Ptr         rdi (== Arg0)         rcx (== Arg0)
//    Acc         rax                   rax
//
//  注：Cnt 在 Win64 上使用 r12（callee-saved），需要 prologue 保存/恢复。
// ============================================================

use crate::arch::{Arch, ArchAssembler, Label};
use crate::arch::x64::{reg::*, Reg64, X64Assembler};
use crate::runtime::JitFn;
use super::backend::{Cond, MacroAssemblerBackend, VReg};

// ──────────────────────────────────────────────────────────────
// VReg → Reg64 映射表
// ──────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn vreg_to_reg(v: VReg) -> Reg64 {
    match v {
        VReg::Arg(0) => rcx,
        VReg::Arg(1) => rdx,
        VReg::Arg(2) => r8,
        VReg::Arg(3) => r9,
        VReg::Ret    => rax,
        VReg::Tmp(0) => r10,
        VReg::Tmp(1) => r11,
        VReg::Tmp(2) => rsi,   // Win64: rsi is callee-saved, but safe as scratch here
        VReg::Tmp(3) => rdi,
        VReg::Cnt    => r12,   // callee-saved → 由 prologue/epilogue 保护
        VReg::Ptr    => rcx,   // 与 Arg(0) 相同：ptr 参数通过 rcx 传入
        _ => panic!("unsupported VReg {:?}", v),
    }
}

#[cfg(not(target_os = "windows"))]
fn vreg_to_reg(v: VReg) -> Reg64 {
    match v {
        VReg::Arg(0) => rdi,
        VReg::Arg(1) => rsi,
        VReg::Arg(2) => rdx,
        VReg::Arg(3) => rcx,
        VReg::Ret    => rax,
        VReg::Tmp(0) => r10,
        VReg::Tmp(1) => r11,
        VReg::Tmp(2) => r9,
        VReg::Tmp(3) => r8,
        VReg::Cnt    => rcx,   // SysV: rcx 不是参数寄存器（参数用 rdi/rsi/rdx）
        VReg::Ptr    => rdi,   // 与 Arg(0) 相同
        _ => panic!("unsupported VReg {:?}", v),
    }
}

// ──────────────────────────────────────────────────────────────
// X64Backend
// ──────────────────────────────────────────────────────────────

pub struct X64Backend {
    asm: X64Assembler,
    /// Win64 下需要在 prologue 保存 r12（Cnt），epilogue 恢复
    #[cfg(target_os = "windows")]
    needs_r12: bool,
}

impl X64Backend {
    fn r(&self, v: VReg) -> Reg64 {
        vreg_to_reg(v)
    }
}

impl MacroAssemblerBackend for X64Backend {
    fn new() -> Self {
        X64Backend {
            asm: X64Assembler::new(),
            #[cfg(target_os = "windows")]
            needs_r12: false,
        }
    }

    unsafe fn compile<F: Copy>(self) -> JitFn<F> {
        crate::runtime::JitRuntime::compile(self.asm)
    }

    fn arch() -> Arch { Arch::X86_64 }

    // ── Label ─────────────────────────────────────────────────

    fn new_label(&mut self) -> Label { self.asm.new_label() }
    fn bind(&mut self, label: &Label) { self.asm.bind(label) }

    // ── 帧管理 ───────────────────────────────────────────────

    fn prologue(&mut self) {
        self.asm.push_rbp();
        self.asm.mov_rbp_rsp();
        // Win64：Cnt 映射到 r12（callee-saved），必须保存
        #[cfg(target_os = "windows")]
        {
            // PUSH r12: REX.B + 0x54
            self.asm.push_r12();
            self.needs_r12 = true;
        }
    }

    fn epilogue(&mut self) {
        #[cfg(target_os = "windows")]
        if self.needs_r12 {
            self.asm.pop_r12();
        }
        self.asm.pop_rbp();
    }

    fn ret(&mut self) { self.asm.ret(); }

    // ── 数据移动 ─────────────────────────────────────────────

    fn mov(&mut self, dst: VReg, src: VReg) {
        let (d, s) = (self.r(dst), self.r(src));
        if d != s {
            self.asm.mov_r64_r64(d, s);
        }
    }

    fn mov_imm(&mut self, dst: VReg, imm: i64) {
        self.asm.mov_r64_imm64(self.r(dst), imm);
    }

    /// dst = *(base_reg + idx_reg * ptr_size)
    /// x86-64: MOV dst, [base + idx*8]  (SIB scale=3)
    fn load_ptr_scaled(&mut self, dst: VReg, base: VReg, idx: VReg) {
        let (d, b, i) = (self.r(dst), self.r(base), self.r(idx));
        self.asm.mov_r64_mem_base_idx8(d, b, i);
    }

    fn store_ptr_scaled(&mut self, base: VReg, idx: VReg, src: VReg) {
        let (b, i, s) = (self.r(base), self.r(idx), self.r(src));
        self.asm.mov_mem_base_idx8_r64(b, i, s);
    }

    // ── 算术 ─────────────────────────────────────────────────

    fn add(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        let (d, l, r) = (self.r(dst), self.r(lhs), self.r(rhs));
        if d == l {
            self.asm.add_r64_r64(d, r);
        } else if d == r {
            self.asm.add_r64_r64(d, l);   // ADD 可交换
        } else {
            self.asm.mov_r64_r64(d, l);
            self.asm.add_r64_r64(d, r);
        }
    }

    fn add_imm(&mut self, dst: VReg, imm: i32) {
        self.asm.add_r64_imm32(self.r(dst), imm);
    }

    fn sub(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        let (d, l, r) = (self.r(dst), self.r(lhs), self.r(rhs));
        if d != l {
            self.asm.mov_r64_r64(d, l);
        }
        self.asm.sub_r64_r64(d, r);
    }

    fn sub_imm(&mut self, dst: VReg, imm: i32) {
        self.asm.sub_r64_imm32(self.r(dst), imm);
    }

    fn mul(&mut self, dst: VReg, lhs: VReg, rhs: VReg) {
        let (d, l, r) = (self.r(dst), self.r(lhs), self.r(rhs));
        if d == l {
            self.asm.imul_r64_r64(d, r);
        } else if d == r {
            self.asm.imul_r64_r64(d, l);
        } else {
            self.asm.mov_r64_r64(d, l);
            self.asm.imul_r64_r64(d, r);
        }
    }

    fn zero(&mut self, dst: VReg) {
        let r = self.r(dst);
        self.asm.xor_r64_r64(r, r);
    }

    // ── 比较与跳转 ───────────────────────────────────────────

    fn cmp(&mut self, lhs: VReg, rhs: VReg) {
        let lhs = self.r(lhs);
        let rhs = self.r(rhs);
        self.asm.cmp_r64_r64(lhs, rhs);
    }

    fn cmp_imm(&mut self, reg: VReg, imm: i64) {
        let reg = self.r(reg);
        self.asm.cmp_reg_imm(reg, imm);
    }

    fn jump_if(&mut self, cond: Cond, label: &Label) {
        match cond {
            Cond::Eq => self.asm.je(label),
            Cond::Ne => self.asm.jne(label),
            Cond::Lt => self.asm.jl(label),
            Cond::Le => self.asm.jle(label),
            Cond::Gt => self.asm.jg(label),
            Cond::Ge => self.asm.jge(label),
        }
    }

    fn jump(&mut self, label: &Label) {
        self.asm.jmp(label);
    }

    fn call_label(&mut self, label: &Label) {
        self.asm.call_label(label);
    }

    fn push_vreg(&mut self, r: VReg) {
        self.asm.push_r64(self.r(r));
    }

    fn pop_vreg(&mut self, r: VReg) {
        self.asm.pop_r64(self.r(r));
    }
}
