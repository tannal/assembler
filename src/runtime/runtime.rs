// ============================================================
//  src/runtime/runtime.rs  —  JIT 运行时工厂
// ============================================================

use crate::arch::{Arch, ArchAssembler};
use crate::platform::ExecutableBuffer;
use super::JitFn;

/// JIT 运行时：负责把汇编字节序列变成可调用的函数。
///
/// 使用示例：
/// ```ignore
/// let mut asm = X64Assembler::new();
/// // ... emit instructions ...
/// let jit = JitRuntime::compile::<unsafe extern "C" fn() -> i64>(asm);
/// let result = unsafe { jit.get()() };
/// ```
pub struct JitRuntime {
    arch: Arch,
}

impl JitRuntime {
    /// 创建一个与当前 native 架构匹配的运行时
    pub fn native() -> Self {
        JitRuntime { arch: Arch::native() }
    }

    /// 使用指定架构创建（用于交叉编译分析，不能执行）
    pub fn with_arch(arch: Arch) -> Self {
        JitRuntime { arch }
    }

    pub fn arch(&self) -> Arch { self.arch }

    /// 将 Assembler 编译为可执行的 JitFn<F>
    ///
    /// # Safety
    /// `F` 必须与 JIT 代码的实际函数签名完全匹配，
    /// 并遵守目标平台的 ABI。
    pub unsafe fn compile<F: Copy>(asm: impl ArchAssembler) -> JitFn<F> {
        assert_eq!(
            std::mem::size_of::<F>(),
            std::mem::size_of::<*const u8>(),
            "F must be a function pointer type"
        );
        let code = asm.into_bytes();
        let buf  = ExecutableBuffer::new(&code);
        let func: F = buf.as_fn::<F>();
        JitFn::new(buf, func)
    }

    /// 仅将 Assembler 序列化为字节（用于检查、反汇编，不执行）
    pub fn assemble_bytes(asm: impl ArchAssembler) -> Vec<u8> {
        asm.into_bytes()
    }
}
