// ============================================================
//  src/platform/exec_mem.rs  —  可执行内存缓冲区
//
//  跨平台策略：
//    Linux / macOS  → mmap(PROT_READ|PROT_WRITE|PROT_EXEC)
//    Windows        → VirtualAlloc(PAGE_EXECUTE_READWRITE)
//
//  安全性说明：
//    生产场景建议分两步分配（W^X：先 RW 写入，再 mprotect 变 RX）。
//    本实现为 JIT 原型，使用 RWX 权限以简化代码。
// ============================================================

use std::ptr::NonNull;

/// 持有一段 RWX 可执行内存的 RAII 封装
pub struct ExecutableBuffer {
    ptr: NonNull<u8>,
    len: usize,
    /// 平台相关标记（Windows 需要区分 MEM_RELEASE 时 size=0）
    _marker: (),
}

// SAFETY: JIT 生成的代码只读，不共享可变状态
unsafe impl Send for ExecutableBuffer {}
unsafe impl Sync for ExecutableBuffer {}

impl ExecutableBuffer {
    /// 分配 `len` 字节的 RWX 内存并复制 `code` 内容
    pub fn new(code: &[u8]) -> Self {
        assert!(!code.is_empty(), "ExecutableBuffer: empty code slice");
        let len = code.len();

        let ptr = unsafe { platform_alloc(len) };
        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), ptr.as_ptr(), len);
            // 在写入完成后，某些平台（尤其 macOS Apple Silicon）需要 icache flush
            platform_flush_icache(ptr.as_ptr(), len);
        }

        ExecutableBuffer { ptr, len, _marker: () }
    }

    /// 返回代码入口指针
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// 代码长度（字节）
    pub fn len(&self) -> usize {
        self.len
    }

    /// 将入口地址 transmute 为函数指针（调用者确保签名正确）
    ///
    /// # Safety
    /// 调用者必须保证：
    ///  1. 函数签名 F 与缓冲区中的 JIT 代码匹配
    ///  2. 代码符合目标平台 ABI
    pub unsafe fn as_fn<F: Copy>(&self) -> F {
        debug_assert_eq!(
            std::mem::size_of::<F>(),
            std::mem::size_of::<*const u8>(),
            "Function pointer size mismatch"
        );
        std::mem::transmute_copy(&self.ptr.as_ptr())
    }
}

impl Drop for ExecutableBuffer {
    fn drop(&mut self) {
        unsafe { platform_free(self.ptr, self.len) };
    }
}

impl std::fmt::Debug for ExecutableBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExecutableBuffer {{ ptr: {:p}, len: {} }}", self.ptr.as_ptr(), self.len)
    }
}

// ──────────────────────────────────────────────────────────────
// 平台实现
// ──────────────────────────────────────────────────────────────

// ── Linux ────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
unsafe fn platform_alloc(len: usize) -> NonNull<u8> {
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        len,
        libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed: {}", errno());
    NonNull::new_unchecked(ptr as *mut u8)
}

#[cfg(target_os = "linux")]
unsafe fn platform_free(ptr: NonNull<u8>, len: usize) {
    let ret = libc::munmap(ptr.as_ptr() as *mut libc::c_void, len);
    debug_assert_eq!(ret, 0, "munmap failed");
}

#[cfg(target_os = "linux")]
unsafe fn platform_flush_icache(_ptr: *mut u8, _len: usize) {
    // 在 x86-64 上 CPU 自动保持 I/D cache 一致；
    // 在 ARM/AArch64 Linux 上使用 __builtin___clear_cache (通过 sys_cacheflush)
    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    {
        extern "C" {
            fn __clear_cache(begin: *mut u8, end: *mut u8);
        }
        __clear_cache(_ptr, _ptr.add(_len));
    }
}

// ── macOS ────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
unsafe fn platform_alloc(len: usize) -> NonNull<u8> {
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        len,
        libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        libc::MAP_PRIVATE | libc::MAP_ANON,
        -1,
        0,
    );
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
    NonNull::new_unchecked(ptr as *mut u8)
}

#[cfg(target_os = "macos")]
unsafe fn platform_free(ptr: NonNull<u8>, len: usize) {
    libc::munmap(ptr.as_ptr() as *mut libc::c_void, len);
}

#[cfg(target_os = "macos")]
unsafe fn platform_flush_icache(ptr: *mut u8, len: usize) {
    // Apple Silicon (AArch64) 需要显式 sys_icache_invalidate
    #[cfg(target_arch = "aarch64")]
    {
        extern "C" {
            fn sys_icache_invalidate(start: *mut u8, size: usize);
        }
        sys_icache_invalidate(ptr, len);
    }
    // x86-64 macOS 不需要
    let _ = (ptr, len);
}

// ── Windows ──────────────────────────────────────────────────
// 直接链接 kernel32，无需任何第三方 crate

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    // 手工声明所需的 Win32 API，避免引入 windows-sys 依赖
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(
            lp_address:        *mut c_void,
            dw_size:           usize,
            fl_allocation_type: u32,
            fl_protect:        u32,
        ) -> *mut c_void;

        fn VirtualFree(
            lp_address:   *mut c_void,
            dw_size:      usize,
            dw_free_type: u32,
        ) -> i32;

        fn GetLastError() -> u32;

        fn FlushInstructionCache(
            h_process:      *mut c_void, // HANDLE
            lp_base_address: *const c_void,
            dw_size:         usize,
        ) -> i32;

        fn GetCurrentProcess() -> *mut c_void;
    }

    const MEM_COMMIT:              u32 = 0x00001000;
    const MEM_RESERVE:             u32 = 0x00002000;
    const MEM_RELEASE:             u32 = 0x00008000;
    const PAGE_EXECUTE_READWRITE:  u32 = 0x40;

    pub(super) unsafe fn platform_alloc(len: usize) -> NonNull<u8> {
        let ptr = VirtualAlloc(
            std::ptr::null_mut(),
            len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        );
        assert!(
            !ptr.is_null(),
            "VirtualAlloc({} bytes) failed: error={}",
            len,
            GetLastError()
        );
        NonNull::new_unchecked(ptr as *mut u8)
    }

    pub(super) unsafe fn platform_free(ptr: NonNull<u8>, _len: usize) {
        VirtualFree(ptr.as_ptr() as *mut c_void, 0, MEM_RELEASE);
    }

    pub(super) unsafe fn platform_flush_icache(ptr: *mut u8, len: usize) {
        // x86-64 Windows：CPU 硬件保证 I/D cache 一致，无需显式 flush
        // ARM / AArch64 Windows：必须调用 FlushInstructionCache
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            FlushInstructionCache(
                GetCurrentProcess(),
                ptr as *const c_void,
                len,
            );
        }
        // 避免 unused variable 警告
        let _ = (ptr, len);
    }
}

#[cfg(target_os = "windows")]
use win::{platform_alloc, platform_free, platform_flush_icache};

// ── 工具函数 ─────────────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
