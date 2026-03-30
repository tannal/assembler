// ============================================================
//  src/macro_asm/backend.rs
//
//  MacroAssemblerBackend trait
//  ───────────────────────────
//  这是整个跨架构设计的核心接口。
//
//  设计模型（对标 V8 CSA）：
//
//    ┌─────────────────────────────────┐
//    │   Stub 代码  (一份，无 #[cfg])   │  ← 用户只写这一层
//    │   build_sum_array(masm)         │
//    └────────────┬────────────────────┘
//                 │ 调用 MacroAssembler<B> 的方法
//    ┌────────────▼────────────────────┐
//    │   MacroAssembler<B>             │  ← 本文件定义
//    │   虚拟寄存器 VReg               │
//    │   架构无关操作（mov/add/cmp…）  │
//    └────────────┬────────────────────┘
//                 │ B: MacroAssemblerBackend
//    ┌────────────▼──────────────────────────────────────┐
//    │  X64Backend  │  Arm64Backend  │  ArmBackend       │  ← 翻译层
//    │  把虚拟操作  → 真实机器指令                        │
//    └───────────────────────────────────────────────────┘
//
//  虚拟寄存器（VReg）：
//    固定角色寄存器，在每个 backend 里映射到真实寄存器：
//      Arg0..Arg3   → 平台参数寄存器
//      Ret          → 返回值寄存器
//      Tmp0..Tmp3   → 临时寄存器（caller-saved）
//      Acc          → 累加器（固定用途，如 x64 的 rax）
//      Cnt          → 计数器（如 x86 的 rcx / arm 的 r4）
//      Ptr          → 指针基址寄存器
// ============================================================

use crate::arch::{Arch, Label};
use crate::runtime::JitFn;

// ──────────────────────────────────────────────────────────────
// § 1  VReg — 虚拟寄存器角色
// ──────────────────────────────────────────────────────────────

/// 架构无关的虚拟寄存器角色
/// Stub 代码只使用这些符号，Backend 负责映射到真实寄存器
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VReg {
    /// 第 0..3 个整型参数（调用约定入参）
    Arg(u8),
    /// 整型返回值寄存器（通常也作累加器）
    Ret,
    /// 通用临时寄存器 0..3（caller-saved，不跨 call 保活）
    Tmp(u8),
    /// 专用计数器（循环 index 等）
    Cnt,
    /// 专用指针寄存器（数组基址等）
    Ptr,
}

// ──────────────────────────────────────────────────────────────
// § 2  Cond — 架构无关的条件码
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    Eq,  // ==
    Ne,  // !=
    Lt,  // <  (有符号)
    Le,  // <= (有符号)
    Gt,  // >  (有符号)
    Ge,  // >= (有符号)
}

// ──────────────────────────────────────────────────────────────
// § 3  MacroAssemblerBackend trait
// ──────────────────────────────────────────────────────────────

/// 每个架构实现此 trait，将虚拟操作翻译为真实机器指令
pub trait MacroAssemblerBackend: Sized {
    // ── 生命周期 ─────────────────────────────────────────────

    fn new() -> Self;

    /// 消耗 self，生成字节序列并分配可执行内存
    /// Safety: F 必须与生成的代码签名匹配
    unsafe fn compile<F: Copy>(self) -> JitFn<F>;

    // ── 架构信息 ─────────────────────────────────────────────

    fn arch() -> Arch;

    /// 指针大小（字节）：64-bit 架构=8，32-bit=4
    fn ptr_size() -> usize {
        match Self::arch() {
            Arch::X86_64 | Arch::Aarch64 => 8,
            Arch::Arm => 4,
        }
    }

    // ── Label ────────────────────────────────────────────────

    fn new_label(&mut self) -> Label;
    fn bind(&mut self, label: &Label);

    // ── 函数帧 ───────────────────────────────────────────────

    /// 生成标准函数 prologue（保存帧指针等）
    fn prologue(&mut self);

    /// 生成标准函数 epilogue（恢复帧指针）
    fn epilogue(&mut self);

    /// RET 指令
    fn ret(&mut self);

    // ── 数据移动 ─────────────────────────────────────────────

    /// dst = src  (寄存器间拷贝)
    fn mov(&mut self, dst: VReg, src: VReg);

    /// dst = imm  (加载立即数，pointer-sized)
    fn mov_imm(&mut self, dst: VReg, imm: i64);

    /// dst = *(base + idx * ptr_size)  (加载指针大小的内存字)
    fn load_ptr_scaled(&mut self, dst: VReg, base: VReg, idx: VReg);

    /// *(base + idx * ptr_size) = src  (存储指针大小的内存字)
    fn store_ptr_scaled(&mut self, base: VReg, idx: VReg, src: VReg);

    // ── 算术 ─────────────────────────────────────────────────

    /// dst = lhs + rhs
    fn add(&mut self, dst: VReg, lhs: VReg, rhs: VReg);

    /// dst = dst + imm  (原地加立即数)
    fn add_imm(&mut self, dst: VReg, imm: i32);

    /// dst = lhs - rhs
    fn sub(&mut self, dst: VReg, lhs: VReg, rhs: VReg);

    /// dst = dst - imm
    fn sub_imm(&mut self, dst: VReg, imm: i32);

    /// dst = lhs * rhs
    fn mul(&mut self, dst: VReg, lhs: VReg, rhs: VReg);

    /// dst = 0  (零化，通常用 XOR 实现)
    fn zero(&mut self, dst: VReg);

    // ── 比较与跳转 ───────────────────────────────────────────

    /// 设置 flags: lhs cmp rhs（不写结果）
    fn cmp(&mut self, lhs: VReg, rhs: VReg);

    /// 设置 flags: lhs cmp imm
    /// 注意：如果 imm 超过架构限制（如 32位），实现方可能需要借用临时寄存器
    fn cmp_imm(&mut self, lhs: VReg, imm: i64);

    /// 根据上次 cmp 的结果条件跳转
    fn jump_if(&mut self, cond: Cond, label: &Label);

    /// 无条件跳转
    fn jump(&mut self, label: &Label);

    /// CALL label（保存返回地址，跳到 label）
    fn call_label(&mut self, label: &Label);

    /// 将寄存器压栈保存（callee-saved 场景）
    fn push_vreg(&mut self, r: VReg);

    /// 从栈恢复寄存器
    fn pop_vreg(&mut self, r: VReg);
}

// ──────────────────────────────────────────────────────────────
// § 4  MacroAssembler<B> — 用户写 stub 时持有的句柄
// ──────────────────────────────────────────────────────────────

/// 用户（Stub 作者）面对的接口
/// 内部持有 B: MacroAssemblerBackend，对外暴露高层 API
pub struct MacroAssembler<B: MacroAssemblerBackend> {
    backend: B,
}

impl<B: MacroAssemblerBackend> MacroAssembler<B> {
    pub fn new() -> Self {
        MacroAssembler { backend: B::new() }
    }

    // ── 转发所有 backend 方法 ────────────────────────────────

    pub fn new_label(&mut self) -> Label            { self.backend.new_label() }
    pub fn bind(&mut self, l: &Label)               { self.backend.bind(l) }
    pub fn prologue(&mut self)                      { self.backend.prologue() }
    pub fn epilogue(&mut self)                      { self.backend.epilogue() }
    pub fn ret(&mut self)                           { self.backend.ret() }
    pub fn mov(&mut self, d: VReg, s: VReg)         { self.backend.mov(d, s) }
    pub fn mov_imm(&mut self, d: VReg, imm: i64)    { self.backend.mov_imm(d, imm) }
    pub fn load_ptr_scaled(&mut self, d: VReg, base: VReg, idx: VReg) {
        self.backend.load_ptr_scaled(d, base, idx)
    }
    pub fn store_ptr_scaled(&mut self, base: VReg, idx: VReg, src: VReg) {
        self.backend.store_ptr_scaled(base, idx, src)
    }
    pub fn add(&mut self, d: VReg, l: VReg, r: VReg) { self.backend.add(d, l, r) }
    pub fn add_imm(&mut self, d: VReg, imm: i32)    { self.backend.add_imm(d, imm) }
    pub fn sub(&mut self, d: VReg, l: VReg, r: VReg) { self.backend.sub(d, l, r) }
    pub fn sub_imm(&mut self, d: VReg, imm: i32)    { self.backend.sub_imm(d, imm) }
    pub fn mul(&mut self, d: VReg, l: VReg, r: VReg) { self.backend.mul(d, l, r) }
    pub fn zero(&mut self, d: VReg)                 { self.backend.zero(d) }
    pub fn cmp(&mut self, l: VReg, r: VReg)         { self.backend.cmp(l, r) }
    pub fn cmp_imm(&mut self, lhs: VReg, imm: i64) {
        self.backend.cmp_imm(lhs, imm);
    }
    pub fn jump_if(&mut self, c: Cond, lbl: &Label) { self.backend.jump_if(c, lbl) }
    pub fn jump(&mut self, lbl: &Label)             { self.backend.jump(lbl) }
    pub fn call_label(&mut self, lbl: &Label)       { self.backend.call_label(lbl) }
    pub fn push_vreg(&mut self, r: VReg)            { self.backend.push_vreg(r) }
    pub fn pop_vreg(&mut self, r: VReg)             { self.backend.pop_vreg(r) }

    // ── 便捷组合操作 ─────────────────────────────────────────

    /// inc dst  (dst += 1)
    pub fn inc(&mut self, dst: VReg) {
        self.backend.add_imm(dst, 1);
    }

    /// dec dst  (dst -= 1)
    pub fn dec(&mut self, dst: VReg) {
        self.backend.sub_imm(dst, 1);
    }

    // ── 编译 ─────────────────────────────────────────────────

    /// 消耗 MacroAssembler，编译为可执行函数
    /// Safety: F 必须与 stub 的实际签名匹配
    pub unsafe fn compile<F: Copy>(self) -> JitFn<F> {
        self.backend.compile()
    }

    /// 仅生成字节（用于 hexdump / 单元测试）
    pub fn into_bytes(self) -> Vec<u8>
    where
        B: Into<Vec<u8>>,
    {
        self.backend.into()
    }
}
