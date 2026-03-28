// ============================================================
//  Tiny x86-64 JIT Assembler – inspired by V8 CodeStubAssembler
//  Targets: Linux / macOS, System V AMD64 ABI
//  Generates a sum_array stub, mmap's it, and calls it.
// ============================================================

#![allow(dead_code)]

use std::{collections::HashMap, f32::consts::LN_10};

// ──────────────────────────────────────────────────────────────
// § 1 – Registers
// ──────────────────────────────────────────────────────────────

/// 64-bit general-purpose registers (REX.W = 1 encoding index)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg64(pub u8); // 0-15

#[allow(non_upper_case_globals)]
pub mod reg {
    use super::Reg64;
    pub const rax: Reg64 = Reg64(0);
    pub const rcx: Reg64 = Reg64(1);
    pub const rdx: Reg64 = Reg64(2);
    pub const rbx: Reg64 = Reg64(3);
    pub const rsp: Reg64 = Reg64(4);
    pub const rbp: Reg64 = Reg64(5);
    pub const rsi: Reg64 = Reg64(6);
    pub const rdi: Reg64 = Reg64(7);
    pub const r8: Reg64 = Reg64(8);
    pub const r9: Reg64 = Reg64(9);
    pub const r10: Reg64 = Reg64(10);
    pub const r11: Reg64 = Reg64(11);
    pub const r12: Reg64 = Reg64(12);
    pub const r13: Reg64 = Reg64(13);
    pub const r14: Reg64 = Reg64(14);
    pub const r15: Reg64 = Reg64(15);
}

// ──────────────────────────────────────────────────────────────
// § 2 – Label (forward / backward jumps with patch-list)
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Label {
    id: usize,
}

struct LabelState {
    bound_at: Option<usize>, // byte offset once bound
    patch_sites: Vec<usize>, // offsets of 4-byte rel32 fields to patch
}

// ──────────────────────────────────────────────────────────────
// § 3 – Assembler
// ──────────────────────────────────────────────────────────────

pub struct Assembler {
    buf: Vec<u8>,
    labels: Vec<LabelState>,
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            buf: Vec::with_capacity(256),
            labels: Vec::new(),
        }
    }

    // ── label management ──────────────────────────────────────

    pub fn new_label(&mut self) -> Label {
        let id = self.labels.len();
        self.labels.push(LabelState {
            bound_at: None,
            patch_sites: Vec::new(),
        });
        Label { id }
    }

    /// Bind a label to the *current* emit position.
    pub fn bind(&mut self, lbl: &Label) {
        let pos = self.buf.len();
        let state = &mut self.labels[lbl.id];
        assert!(state.bound_at.is_none(), "label already bound");
        state.bound_at = Some(pos);
        // Patch all forward-reference sites
        let sites: Vec<usize> = state.patch_sites.drain(..).collect();
        for site in sites {
            let rel32 = (pos as i64 - (site as i64 + 4)) as i32;
            let bytes = rel32.to_le_bytes();
            self.buf[site..site + 4].copy_from_slice(&bytes);
        }
    }

    // ── raw emit helpers ──────────────────────────────────────

    fn emit1(&mut self, b: u8) {
        self.buf.push(b);
    }

    fn emit2(&mut self, a: u8, b: u8) {
        self.buf.push(a);
        self.buf.push(b);
    }

    fn emit_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn emit_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// REX prefix: W=64-bit, R=reg extension, B=rm/base extension
    fn rex(&mut self, w: bool, r: u8, b: u8) {
        let rex = 0x40
            | (if w { 0x08 } else { 0 })
            | (if r & 8 != 0 { 0x04 } else { 0 })
            | (if b & 8 != 0 { 0x01 } else { 0 });
        if rex != 0x40 {
            self.emit1(rex);
        }
    }

    /// REX.W always emitted (64-bit operand size)
    fn rex_w(&mut self, r: u8, b: u8) {
        self.emit1(
            0x40 | 0x08 | (if r & 8 != 0 { 0x04 } else { 0 }) | (if b & 8 != 0 { 0x01 } else { 0 }),
        );
    }

    /// ModRM byte: mod=3 (register-register)
    fn modrm_rr(&mut self, reg: u8, rm: u8) {
        self.emit1(0xC0 | ((reg & 7) << 3) | (rm & 7));
    }

    // ── instructions ─────────────────────────────────────────

    /// PUSH rbp
    pub fn push_rbp(&mut self) {
        self.emit1(0x55);
    }

    /// POP rbp
    pub fn pop_rbp(&mut self) {
        self.emit1(0x5D);
    }

    /// MOV rbp, rsp
    pub fn mov_rbp_rsp(&mut self) {
        self.rex_w(reg::rbp.0, reg::rsp.0);
        self.emit1(0x89);
        self.modrm_rr(reg::rsp.0, reg::rbp.0);
    }

    /// RET
    pub fn ret(&mut self) {
        self.emit1(0xC3);
    }

    /// XOR reg64, reg64  (zero a register)
    pub fn xor_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(dst.0, src.0);
        self.emit1(0x33);
        self.modrm_rr(dst.0, src.0);
    }

    /// MOV reg64, imm64  (REX.W + B8+rd id)
    pub fn mov_r64_imm64(&mut self, dst: Reg64, imm: i64) {
        self.rex_w(0, dst.0);
        self.emit1(0xB8 | (dst.0 & 7));
        self.emit_i64(imm);
    }

    /// MOV reg64, reg64
    pub fn mov_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(src.0, dst.0);
        self.emit1(0x89);
        self.modrm_rr(src.0, dst.0);
    }

    /// ADD reg64, reg64
    pub fn add_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(src.0, dst.0);
        self.emit1(0x01);
        self.modrm_rr(src.0, dst.0);
    }

    /// ADD reg64, imm32 (sign-extended)
    pub fn add_r64_imm32(&mut self, dst: Reg64, imm: i32) {
        self.rex_w(0, dst.0);
        if imm >= -128 && imm <= 127 {
            self.emit1(0x83);
            self.modrm_rr(0, dst.0); // opcode ext = 0 (/0)
            self.emit1(imm as u8);
        } else {
            self.emit1(0x81);
            self.modrm_rr(0, dst.0);
            self.emit_i32(imm);
        }
    }

    /// IMUL r64, r64 -> 0x48 0x0F 0xAF /r
    pub fn imul_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.emit1(0x48); // REX.W
        self.emit2(0x0F, 0xAF);
        // ModRM 字节编码：0xC0 | (dst << 3) | src
        self.emit1(0xC0 | ((dst.0) << 3) | (src.0 as u8));
    }

    /// SUB reg64, imm32
    pub fn sub_r64_imm32(&mut self, dst: Reg64, imm: i32) {
        self.rex_w(0, dst.0);
        if imm >= -128 && imm <= 127 {
            self.emit1(0x83);
            self.modrm_rr(5, dst.0); // /5
            self.emit1(imm as u8);
        } else {
            self.emit1(0x81);
            self.modrm_rr(5, dst.0);
            self.emit_i32(imm);
        }
    }

    /// CMP reg64, reg64
    pub fn cmp_r64_r64(&mut self, lhs: Reg64, rhs: Reg64) {
        self.rex_w(rhs.0, lhs.0);
        self.emit1(0x3B);
        self.modrm_rr(rhs.0, lhs.0);
    }

    /// CMP reg64, imm32
    pub fn cmp_r64_imm32(&mut self, reg: Reg64, imm: i32) {
        self.rex_w(0, reg.0);
        self.emit1(0x81);
        self.modrm_rr(7, reg.0); // /7
        self.emit_i32(imm);
    }

    /// INC reg64  (FF /0)
    pub fn inc_r64(&mut self, r: Reg64) {
        self.rex_w(0, r.0);
        self.emit1(0xFF);
        self.modrm_rr(0, r.0);
    }

    /// Load 64-bit value from memory: MOV dst, [base + index*8]
    /// (SIB: scale=3 → *8, index, base, disp=0, mod=0)
    pub fn mov_r64_mem_base_idx8(&mut self, dst: Reg64, base: Reg64, idx: Reg64) {
        // REX.W + R(dst) + X(idx) + B(base)
        let rex = 0x48
            | (if dst.0 & 8 != 0 { 0x04 } else { 0 })
            | (if idx.0 & 8 != 0 { 0x02 } else { 0 })
            | (if base.0 & 8 != 0 { 0x01 } else { 0 });
        self.emit1(rex);
        self.emit1(0x8B);
        // ModRM: mod=00, reg=dst&7, rm=100 (SIB follows)
        self.emit1((dst.0 & 7) << 3 | 0x04);
        // SIB: scale=3 (×8), index, base
        self.emit1((3 << 6) | ((idx.0 & 7) << 3) | (base.0 & 7));
    }

    // ── jumps (rel32) ─────────────────────────────────────────

    /// JLE (Jump if Less or Equal, (ZF=1) OR (SF≠OF)) rel32 — 0x0F 0x8E
    pub fn jle(&mut self, lbl: &Label) {
        // 0x0F 0x8E 是 JLE 的 32 位相对跳转操作码
        self.emit2(0x0F, 0x8E);
        self.emit_rel32(lbl.id);
    }

    /// JZ (Jump if Zero, ZF=1)  rel32 — 0x0F 0x84
    /// 逻辑上等同于 JE (Jump if Equal)
    pub fn jz(&mut self, lbl: &Label) {
        self.emit2(0x0F, 0x84);
        self.emit_rel32(lbl.id);
    }
    /// JNZ (Jump if Not Zero, ZF=0)  0x0F 0x85
    pub fn jnz(&mut self, lbl: &Label) {
        // JNZ 和 JNE 使用相同的机器码 0x0F 0x85
        self.emit2(0x0F, 0x85);
        self.emit_rel32(lbl.id);
    }
    /// JMP rel32
    pub fn jmp(&mut self, lbl: &Label) {
        self.emit1(0xE9);
        self.emit_rel32(lbl.id);
    }

    /// JL (jump if less, SF≠OF)  rel32  — 0x0F 0x8C
    pub fn jl(&mut self, lbl: &Label) {
        self.emit2(0x0F, 0x8C);
        self.emit_rel32(lbl.id);
    }

    /// JGE (jump if ≥)  0x0F 0x8D
    pub fn jge(&mut self, lbl: &Label) {
        self.emit2(0x0F, 0x8D);
        self.emit_rel32(lbl.id);
    }

    /// JNE  0x0F 0x85
    pub fn jne(&mut self, lbl: &Label) {
        self.emit2(0x0F, 0x85);
        self.emit_rel32(lbl.id);
    }

    /// Emit a 4-byte rel32 placeholder; record site for later patching.
    fn emit_rel32(&mut self, label_id: usize) {
        let site = self.buf.len();
        if let Some(bound) = self.labels[label_id].bound_at {
            // Backward reference: fill in immediately
            let rel32 = (bound as i64 - (site as i64 + 4)) as i32;
            self.emit_i32(rel32);
        } else {
            // Forward reference: emit 0 and remember the site
            self.emit_i32(0);
            self.labels[label_id].patch_sites.push(site);
        }
    }

    // ── finalise ──────────────────────────────────────────────

    pub fn current_offset(&self) -> usize {
        self.buf.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        for (id, state) in self.labels.iter().enumerate() {
            assert!(
                state.patch_sites.is_empty(),
                "label {} has unpatched forward references",
                id
            );
        }
        self.buf
    }
}

// ──────────────────────────────────────────────────────────────
// § 4 – Executable memory (mmap)
// ──────────────────────────────────────────────────────────────

pub struct MmapBuffer {
    ptr: *mut u8,
    len: usize,
}

unsafe impl Send for MmapBuffer {}
unsafe impl Sync for MmapBuffer {}

impl MmapBuffer {
    pub fn new(code: &[u8]) -> Self {
        let len = code.len().max(1);
        #[cfg(target_os = "windows")]
        let ptr = unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn VirtualAlloc(
                    lpAddress: *mut std::ffi::c_void,
                    dwSize: usize,
                    flAllocationType: u32,
                    flProtect: u32,
                ) -> *mut std::ffi::c_void;
            }
            const MEM_COMMIT: u32 = 0x1000;
            const PAGE_EXECUTE_READWRITE: u32 = 0x40;
            let ptr = VirtualAlloc(
                std::ptr::null_mut(),
                len,
                MEM_COMMIT,
                PAGE_EXECUTE_READWRITE,
            );
            assert_ne!(ptr, std::ptr::null_mut(), "VirtualAlloc failed");
            std::ptr::copy_nonoverlapping(code.as_ptr(), ptr as *mut u8, code.len());
            ptr as *mut u8
        };
        #[cfg(not(target_os = "windows"))]
        let ptr = unsafe { libc_mmap(len) };
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len());
        }
        MmapBuffer { ptr, len }
    }

    pub fn as_fn_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl Drop for MmapBuffer {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn VirtualFree(
                    lpAddress: *mut std::ffi::c_void,
                    dwSize: usize,
                    dwFreeType: u32,
                ) -> i32;
            }
            const MEM_RELEASE: u32 = 0x8000;
            VirtualFree(self.ptr as *mut std::ffi::c_void, 0, MEM_RELEASE);
        }
        #[cfg(not(target_os = "windows"))]
        unsafe {
            libc_munmap(self.ptr, self.len);
        }
    }
}

#[cfg(target_os = "linux")]
unsafe fn libc_mmap(len: usize) -> *mut u8 {
    use std::ffi::c_void;
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        len,
        libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
    ptr as *mut u8
}

#[cfg(target_os = "macos")]
unsafe fn libc_mmap(len: usize) -> *mut u8 {
    use std::ffi::c_void;
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        len,
        libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        libc::MAP_PRIVATE | libc::MAP_ANON,
        -1,
        0,
    );
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
    ptr as *mut u8
}

#[cfg(target_os = "windows")]
unsafe fn libc_mmap(len: usize) -> *mut u8 {
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(
            lpAddress: *mut std::ffi::c_void,
            dwSize: usize,
            flAllocationType: u32,
            flProtect: u32,
        ) -> *mut std::ffi::c_void;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    let ptr = VirtualAlloc(
        std::ptr::null_mut(),
        len,
        MEM_COMMIT,
        PAGE_EXECUTE_READWRITE,
    );
    assert_ne!(ptr, std::ptr::null_mut(), "VirtualAlloc failed");
    ptr as *mut u8
}

#[cfg(target_os = "windows")]
unsafe fn libc_munmap(ptr: *mut u8, len: usize) {
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualFree(lpAddress: *mut std::ffi::c_void, dwSize: usize, dwFreeType: u32) -> i32;
    }
    const MEM_RELEASE: u32 = 0x8000;
    VirtualFree(ptr as *mut std::ffi::c_void, 0, MEM_RELEASE);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn libc_munmap(ptr: *mut u8, len: usize) {
    libc::munmap(ptr as *mut libc::c_void, len);
}

// ──────────────────────────────────────────────────────────────
// § 5 – JitFn wrapper (owns MmapBuffer + typed fn pointer)
// ──────────────────────────────────────────────────────────────

pub struct JitFn<F> {
    _buf: MmapBuffer,
    func: F,
}

impl<F: Copy> JitFn<F> {
    fn new(buf: MmapBuffer, func: F) -> Self {
        JitFn { _buf: buf, func }
    }

    pub fn get(&self) -> F {
        self.func
    }
}

pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use reg::*;
    let mut asm = Assembler::new();
    let loop_start = asm.new_label();
    let end = asm.new_label();

    // 1. 初始化 rax = 1
    asm.mov_r64_imm64(rax, 1);

    asm.bind(&loop_start);
    
    // 2. 终止条件：如果 n <= 1，退出循环
    // Windows x64: 参数在 rcx
    asm.cmp_r64_imm32(rcx, 1);
    asm.jle(&end);             // 改用 jle (Jump if Less or Equal)
                               // 如果 n <= 1，直接跳到 end

    // 3. 计算：rax = rax * rcx
    asm.imul_r64_r64(rax, rcx);

    // 4. 递减：rcx = rcx - 1
    asm.sub_r64_imm32(rcx, 1);

    // 5. 跳回循环开头
    asm.jmp(&loop_start);

    asm.bind(&end);
    asm.ret();

    // ... 后续逻辑 ...
    let code = asm.into_bytes();
    let buf = MmapBuffer::new(&code);
    let fn_ptr: unsafe extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(buf.as_fn_ptr()) };
    JitFn::new(buf, fn_ptr)
}

pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use reg::*;

    let mut asm = Assembler::new();
    let loop_start = asm.new_label();

    // 1. 初始化：假设输入参数在 rcx (Windows x64 调用约定)，i 用 rbx 表示
    // 如果你想在原来的 rcx 基础上加 10 次：
    asm.mov_r64_imm64(rbx, 10); // rbx = i = 10

    // 2. 绑定循环起点
    asm.bind(&loop_start);

    // 4. 循环体内容
    asm.add_r64_imm32(rcx, 1); // rcx = rcx + 1
    asm.sub_r64_imm32(rbx, 1);

    asm.jnz(&loop_start); // 继续下一次循环

    // 7. 返回结果：x64 约定返回值存放在 rax
    asm.mov_r64_r64(rax, rcx); // 将计算结果从 rcx 移到 rax
    asm.ret();

    // ... 后续的 Mmap 和 Transmute 逻辑 ...
    let code = asm.into_bytes();
    let buf = MmapBuffer::new(&code);
    let fn_ptr: unsafe extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(buf.as_fn_ptr()) };
    JitFn::new(buf, fn_ptr)
}

pub fn build_return_const() -> JitFn<unsafe extern "C" fn() -> i64> {
    use reg::*;

    let mut asm = Assembler::new();

    asm.mov_r64_imm64(rbx, 42);
    asm.mov_r64_r64(rax, rbx);
    asm.ret();

    let code = asm.into_bytes();
    let buf = MmapBuffer::new(&code);

    let fn_ptr: unsafe extern "C" fn() -> i64 = unsafe { std::mem::transmute(buf.as_fn_ptr()) };

    JitFn::new(buf, fn_ptr)
}

// ──────────────────────────────────────────────────────────────
// § 6 – CodeStub: sum_array
//
// System V AMD64 ABI (Linux/macOS):
//   i64 sum_array(const i64 *arr, i64 len);
//                          rdi         rsi
//
// Microsoft x64 ABI (Windows):
//   i64 sum_array(const i64 *arr, i64 len);
//                          rcx         rdx
//
// Generated pseudo-code:
//   rax = 0                 ; accumulator
//   r11 = 0                 ; index i
//   loop_start:
//     cmp r11, len_reg
//     jge done
//     add rax, [arr_reg + r11*8]
//     inc r11
//     jmp loop_start
//   done:
//   ret
// ──────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i64, i64) -> i64> {
    use reg::*;

    let mut asm = Assembler::new();

    asm.push_rbp();
    asm.mov_rbp_rsp();

    asm.xor_r64_r64(rax, rax);
    asm.xor_r64_r64(r11, r11);

    let loop_start = asm.new_label();
    let done = asm.new_label();

    asm.bind(&loop_start);

    asm.cmp_r64_r64(rdx, r11);
    asm.jge(&done);

    asm.mov_r64_mem_base_idx8(r10, rcx, r11);
    asm.add_r64_r64(rax, r10);

    asm.inc_r64(r11);

    asm.jmp(&loop_start);

    asm.bind(&done);

    asm.pop_rbp();
    asm.ret();

    let code = asm.into_bytes();

    let buf = MmapBuffer::new(&code);

    let fn_ptr: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(buf.as_fn_ptr()) };

    JitFn::new(buf, fn_ptr)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i64, i64) -> i64> {
    use reg::*;

    let mut asm = Assembler::new();

    // Prologue
    asm.push_rbp();
    asm.mov_rbp_rsp();

    // rax = 0  (accumulator)
    asm.xor_r64_r64(rax, rax);

    // rcx = 0  (loop index)
    asm.xor_r64_r64(rcx, rcx);

    // loop_start:
    let loop_start = asm.new_label();
    let done = asm.new_label();

    asm.bind(&loop_start);

    // if (rcx >= rsi) goto done
    asm.cmp_r64_r64(rcx, rsi);
    asm.jge(&done);

    // rax += *(rdi + rcx * 8)
    // use r10 as temp for the loaded element
    asm.mov_r64_mem_base_idx8(r10, rdi, rcx);
    asm.add_r64_r64(rax, r10);

    // rcx++
    asm.inc_r64(rcx);

    // jmp loop_start
    asm.jmp(&loop_start);

    // done:
    asm.bind(&done);

    // Epilogue
    asm.pop_rbp();
    asm.ret();

    let code = asm.into_bytes();
    let buf = MmapBuffer::new(&code);

    let fn_ptr: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(buf.as_fn_ptr()) };

    JitFn::new(buf, fn_ptr)
}

// ──────────────────────────────────────────────────────────────
// § 7 – Pretty-print the emitted bytes
// ──────────────────────────────────────────────────────────────

fn hexdump(label: &str, bytes: &[u8]) {
    println!("\n╔══ {} ({} bytes) ══╗", label, bytes.len());
    for (i, chunk) in bytes.chunks(16).enumerate() {
        print!("  {:04x}  ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        // padding
        for _ in 0..(16 - chunk.len()) {
            print!("   ");
        }
        print!(" │");
        for b in chunk {
            let c = if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!("│");
    }
    println!("╚{:═<width$}╝", "", width = label.len() + 22);
}

// ──────────────────────────────────────────────────────────────
// § 8 – main
// ──────────────────────────────────────────────────────────────

fn main() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Tiny x86-64 JIT Assembler  (V8 CSA style)  ");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // ── Build & inspect the code ──────────────────────────────
    // Skip for now, just run the JIT

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut asm_inspect = Assembler::new();
        use reg::*;
        asm_inspect.push_rbp();
        asm_inspect.mov_rbp_rsp();
        asm_inspect.xor_r64_r64(rax, rax);
        asm_inspect.xor_r64_r64(rcx, rcx);
        let ls = asm_inspect.new_label();
        let dn = asm_inspect.new_label();
        asm_inspect.bind(&ls);
        asm_inspect.cmp_r64_r64(rcx, rsi);
        asm_inspect.jge(&dn);
        asm_inspect.mov_r64_mem_base_idx8(r10, rdi, rcx);
        asm_inspect.add_r64_r64(rax, r10);
        asm_inspect.inc_r64(rcx);
        asm_inspect.jmp(&ls);
        asm_inspect.bind(&dn);
        asm_inspect.pop_rbp();
        asm_inspect.ret();
        let bytes = asm_inspect.into_bytes();
        hexdump("sum_array JIT stub", &bytes);

        // Disassembly annotation (manual, since we know the layout)
        println!("\nAnnotated disassembly:");
        println!("  +00  55              push   rbp");
        println!("  +01  48 89 ec        mov    rbp, rsp");
        println!("  +04  48 33 c0        xor    rax, rax        ; acc = 0");
        println!("  +07  48 33 c9        xor    rcx, rcx        ; i   = 0");
        println!("  +0a  [loop_start]");
        println!("  +0a  48 3b ce        cmp    rcx, rsi        ; i vs len");
        println!("  +0d  0f 8d ..        jge    done");
        println!("  +13  4e 8b 14 cf     mov    r10, [rdi+rcx*8]; load arr[i]");
        println!("  +17  4c 03 d0        add    rax, r10        ; acc += arr[i]");
        println!("  +1a  49 ff c2        inc    r10 (rcx via rex)");
        println!("  +1d  e9 ..           jmp    loop_start");
        println!("  +22  [done]");
        println!("  +22  5d              pop    rbp");
        println!("  +23  c3              ret");
    }

    // ── JIT compile & execute ─────────────────────────────────
    println!("\n[*] JIT compiling sum_array stub …");
    let jit = build_sum_array();
    println!("[*] Code mapped at: {:p}", jit._buf.as_fn_ptr());

    let arrays: Vec<(&str, Vec<i64>)> = vec![
        ("empty", vec![]),
        ("single element", vec![42]),
        ("1..=10", (1..=10).collect()),
        ("powers of two", vec![1, 2, 4, 8, 16, 32, 64, 128]),
        ("negative values", vec![-5, -3, 0, 3, 5]),
        ("large array", (1..=1000).collect()),
    ];

    println!("\n┌─────────────────────┬───────────────┬───────────────┬───────┐");
    println!("│ Array               │ JIT result    │ Rust stdlib   │  OK?  │");
    println!("├─────────────────────┼───────────────┼───────────────┼───────┤");

    for (name, arr) in &arrays {
        let jit_result = unsafe { (jit.get())(arr.as_ptr(), arr.len() as i64) };
        let rust_result: i64 = arr.iter().sum();
        let ok = jit_result == rust_result;
        println!(
            "│ {:<19} │ {:>13} │ {:>13} │  {}   │",
            name,
            jit_result,
            rust_result,
            if ok { "✓" } else { "✗ FAIL" }
        );
    }

    println!("└─────────────────────┴───────────────┴───────────────┴───────┘");

    // ── Jump to program start (simulate "restart") ───────────
    println!("\n[*] Jumping back to program start via function pointer …");
    // We model "jump to program start" by storing the entry address and
    // making a tail-call, exactly as a JIT engine would do.
    static START_FN: std::sync::OnceLock<unsafe extern "C" fn(*const i64, i64) -> i64> =
        std::sync::OnceLock::new();

    // Record first time only
    let fn_ptr = jit.get();
    START_FN.get_or_init(|| fn_ptr);

    // Re-execute the JIT stub from its entry point (the "jump to start")
    let demo = vec![100, 200, 300, 400, 500, 500];
    let result = unsafe { (*START_FN.get().unwrap())(demo.as_ptr(), demo.len() as i64) };
    println!("[*] Re-executed from entry: sum({:?}) = {}", demo, result);
    assert_eq!(result, 2000, "re-entry result mismatch");

    println!("\njit return 42\n");
    let jit_return = build_return_const();

    let jit_result = unsafe { (jit_return.get())() };
    assert_eq!(jit_result, 42, "jit return mismatch");

    println!("\njit add 10\n");
    let jit_return = build_const_add();

    let jit_result = unsafe { (jit_return.get())(10) };
    assert_eq!(jit_result, 20, "jit return mismatch");

    let jit_factorial = build_factorial();
    let n = 5;
    let jit_result = unsafe { (jit_factorial.get())(n) };
    
    // 5! = 5 * 4 * 3 * 2 * 1 = 120
    assert_eq!(jit_result, 120, "阶乘计算错误！"); 
    
    let n2 = 10;
    let jit_result2 = unsafe { (jit_factorial.get())(n2) };
    assert_eq!(jit_result2, 3628800, "10! 计算错误！");

    println!("\n[✓] All tests passed. JIT stub verified.");
}
