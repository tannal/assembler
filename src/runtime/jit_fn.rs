// ============================================================
//  src/runtime/jit_fn.rs  —  类型化 JIT 函数句柄
// ============================================================

use crate::{arch::Label, platform::ExecutableBuffer};

/// 持有可执行内存 + 类型化函数指针的 RAII 容器。
///
/// 泛型参数 `F` 应为函数指针类型，例如：
///   `unsafe extern "C" fn(*const i64, i64) -> i64`
pub struct JitFn<F: Copy> {
    /// 内存的所有权在此，drop 时自动释放
    _buf:  ExecutableBuffer,
    /// 类型化函数指针（指向 _buf 中的代码）
    func: F,
}

impl<F: Copy> JitFn<F> {
    /// 内部构造（只由 JitRuntime 调用）
    pub(crate) fn new(buf: ExecutableBuffer, func: F) -> Self {
        JitFn { _buf: buf, func }
    }

    /// 获取函数指针（生命周期与 JitFn 绑定，不可 outlive）
    pub fn get(&self) -> F {
        self.func
    }

    /// 返回代码入口地址（用于调试/日志）
    pub fn entry_addr(&self) -> *const u8 {
        self._buf.as_ptr()
    }

    /// 获取生成的机器码字节切片
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            // 将执行内存的起始地址和长度转为 &[u8]
            std::slice::from_raw_parts(self._buf.as_ptr(), self._buf.len())
        }
    }

    pub fn get_label_offset(&self, label: &Label) -> usize {
        // 后端在 bind 时会记录 Label 对应的相对位置
        label.offset() 
    }

    /// 代码大小（字节）
    pub fn code_size(&self) -> usize {
        self._buf.len()
    }
}

impl<F: Copy> std::fmt::Debug for JitFn<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitFn")
            .field("entry", &self._buf.as_ptr())
            .field("size",  &self._buf.len())
            .finish()
    }
}
