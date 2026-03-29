// ============================================================
//  src/platform/mod.rs  —  跨平台可执行内存管理
//
//  提供统一的 ExecutableBuffer 接口，内部根据目标平台
//  选择 mmap (Unix) 或 VirtualAlloc (Windows) 实现。
// ============================================================

pub mod exec_mem;
pub use exec_mem::ExecutableBuffer;
