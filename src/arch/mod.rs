// ============================================================
//  src/arch/mod.rs  —  架构无关的核心抽象层
//
//  所有具体架构（x64 / ARM / AArch64）都实现这些 trait，
//  上层代码只依赖这些接口，从而实现真正的跨架构移植。
// ============================================================

pub mod arm;
pub mod arm64;
pub mod x64;

// ──────────────────────────────────────────────────────────────
// § 1  Label：跳转目标，支持前向 / 后向引用，自动回填
// ──────────────────────────────────────────────────────────────

/// 标签句柄，由各 Assembler 内部管理生命周期
#[derive(Debug, Clone)]
pub struct Label {
    pub id: usize,
}

/// Label 的内部状态（每个 Assembler 持有一张表）
pub struct LabelState {
    /// 已绑定时，该 label 在字节缓冲区中的偏移
    pub bound_at: Option<usize>,
    /// 所有等待回填的 rel32 字段偏移（前向引用）
    pub patch_sites: Vec<PatchSite>,
}

/// 一个待回填点
pub struct PatchSite {
    /// rel32 字段在 buf 中的起始偏移
    pub offset: usize,
    /// 字段宽度：x64=4, ARM=4, AArch64=4（均为 4 字节编码）
    pub width: PatchWidth,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchWidth {
    /// 4 字节 little-endian 有符号偏移（x64 rel32）
    Rel32Le,
    /// 4 字节 ARM/AArch64 指令字中的分支偏移（特殊编码）
    ArmBranch26,
    /// AArch64 CBZ/CBNZ/B.cond 中的 19-bit 偏移
    Aarch64Imm19,
}

impl LabelState {
    pub fn new() -> Self {
        LabelState {
            bound_at: None,
            patch_sites: Vec::new(),
        }
    }
}

// ──────────────────────────────────────────────────────────────
// § 2  RelocationKind：重定位类型（未来扩展用）
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocationKind {
    /// PC 相对 32-bit 偏移（x86-64 调用 / jmp）
    PcRel32,
    /// ARM/AArch64 26-bit 分支
    ArmBranch26,
    /// AArch64 19-bit 条件分支
    Aarch64Imm19,
    /// 绝对 64-bit 地址
    Abs64,
}

// ──────────────────────────────────────────────────────────────
// § 3  ArchAssembler trait：架构层统一接口
// ──────────────────────────────────────────────────────────────

/// 所有 Assembler 实现的公共接口
pub trait ArchAssembler {
    // ── label 管理 ───────────────────────────────────────────

    fn new_label(&mut self) -> Label;

    /// 把 label 绑定到**当前**字节位置，并回填所有已记录的前向引用
    fn bind(&mut self, label: &Label);

    // ── 字节缓冲区访问 ────────────────────────────────────────

    fn current_offset(&self) -> usize;

    /// 消耗 self，返回已验证（所有 label 均已回填）的字节序列
    fn into_bytes(self) -> Vec<u8>;

    // ── 通用控制流 ───────────────────────────────────────────

    fn ret(&mut self);

    // ── 架构信息 ─────────────────────────────────────────────

    fn arch(&self) -> Arch;

    fn pointer_size(&self) -> usize {
        match self.arch() {
            Arch::X86_64 | Arch::Aarch64 => 8,
            Arch::Arm => 4,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// § 4  Arch 枚举
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Arm,
    Aarch64,
}

impl Arch {
    /// 探测当前编译目标架构
    pub fn native() -> Self {
        #[cfg(target_arch = "x86_64")]
        return Arch::X86_64;
        #[cfg(target_arch = "aarch64")]
        return Arch::Aarch64;
        #[cfg(all(target_arch = "arm", not(target_arch = "aarch64")))]
        return Arch::Arm;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm")))]
        compile_error!("Unsupported target architecture");
    }
}

impl std::fmt::Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arch::X86_64 => write!(f, "x86-64"),
            Arch::Arm => write!(f, "ARM (32-bit Thumb-2)"),
            Arch::Aarch64 => write!(f, "AArch64"),
        }
    }
}

// ──────────────────────────────────────────────────────────────
// § 5  CallingConvention：调用约定抽象
// ──────────────────────────────────────────────────────────────

/// 描述函数调用约定中整型参数/返回寄存器编号
/// （编号含义由各架构 Assembler 解释）
pub trait CallingConvention {
    /// 整型参数寄存器序列（最多 8 个）
    fn int_param_regs() -> &'static [u8];
    /// 整型返回值寄存器
    fn int_return_reg() -> u8;
    /// 调用者保存（caller-saved）寄存器集合
    fn caller_saved() -> &'static [u8];
    /// 被调用者保存（callee-saved）寄存器集合
    fn callee_saved() -> &'static [u8];
}

// ──────────────────────────────────────────────────────────────
// § 6  内部工具：Label 管理的公共逻辑（不属于 trait，作为 mixin）
// ──────────────────────────────────────────────────────────────

/// 供各 Assembler 内部使用的 label 表帮助函数
pub struct LabelTable(pub Vec<LabelState>);

impl LabelTable {
    pub fn new() -> Self {
        LabelTable(Vec::new())
    }

    pub fn new_label(&mut self) -> Label {
        let id = self.0.len();
        self.0.push(LabelState::new());
        Label { id }
    }

    /// 绑定 label 到 `pos`，返回需要回填的 patch sites
    pub fn bind(&mut self, label: &Label, pos: usize) -> Vec<PatchSite> {
        let state = &mut self.0[label.id];
        assert!(state.bound_at.is_none(), "label {} already bound", label.id);
        state.bound_at = Some(pos);
        state.patch_sites.drain(..).collect()
    }

    pub fn add_patch_site(&mut self, label: &Label, site: PatchSite) -> Option<usize> {
        let state = &mut self.0[label.id];
        if let Some(bound) = state.bound_at {
            Some(bound) // 后向引用，直接返回目标
        } else {
            state.patch_sites.push(site);
            None // 前向引用，等待 bind 时回填
        }
    }

    pub fn assert_all_bound(&self) {
        for (i, s) in self.0.iter().enumerate() {
            assert!(
                s.patch_sites.is_empty(),
                "label {} has {} unpatched forward references",
                i,
                s.patch_sites.len()
            );
        }
    }
}
