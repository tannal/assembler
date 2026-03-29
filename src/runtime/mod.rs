// ============================================================
//  src/runtime/mod.rs  —  JIT 运行时
//
//  JitFn<F>：持有 ExecutableBuffer（防止内存提前释放）
//            以及经过类型化的函数指针。
//
//  JitRuntime：工厂，负责：
//    1. 接收任意 ArchAssembler 生成的字节序列
//    2. 分配 ExecutableBuffer
//    3. 返回类型安全的 JitFn<F>
// ============================================================

pub mod jit_fn;
pub use jit_fn::JitFn;

pub mod runtime;
pub use runtime::JitRuntime;
