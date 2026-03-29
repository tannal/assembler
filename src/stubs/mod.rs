// ============================================================
//  src/stubs/mod.rs  —  跨平台 JIT Stub 生成器
//
//  每个 stub 函数根据编译时的 target_arch 选择合适的
//  Assembler 实现，对外暴露统一的 build_xxx() 接口。
// ============================================================

pub mod sum_array;
pub mod factorial;
pub mod const_add;
pub mod const_return;

pub use sum_array::build_sum_array;
pub use factorial::build_factorial;
pub use const_add::build_const_add;
pub use const_return::build_const_return;
