// ============================================================
//  src/macro_asm/mod.rs
//
//  对外导出：
//    MacroAssembler<B>          —— 用户面向的汇编器句柄
//    MacroAssemblerBackend      —— Backend trait
//    VReg, Cond                 —— 虚拟寄存器 / 条件码
//    NativeBackend              —— 当前平台的 Backend 类型别名
//    NativeMasm                 —— 当前平台的 MacroAssembler 别名
// ============================================================

pub mod backend;
pub mod x64_backend;
pub mod arm64_backend;
pub mod arm_backend;

pub use backend::{Cond, MacroAssembler, MacroAssemblerBackend, VReg};
pub use x64_backend::X64Backend;
pub use arm64_backend::Arm64Backend;
pub use arm_backend::ArmBackend;

// ── 平台别名：让 stub 代码可以直接写 NativeMasm ──────────────

#[cfg(target_arch = "x86_64")]
pub type NativeBackend = X64Backend;

#[cfg(target_arch = "aarch64")]
pub type NativeBackend = Arm64Backend;

#[cfg(target_arch = "arm")]
pub type NativeBackend = ArmBackend;

/// 当前平台的 MacroAssembler 别名，stub 代码的主要入口类型
pub type NativeMasm = MacroAssembler<NativeBackend>;
