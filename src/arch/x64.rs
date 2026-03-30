// ============================================================
//  src/arch/x64.rs  —  x86-64 汇编器
//
//  System V AMD64 ABI (Linux / macOS):
//    params: rdi rsi rdx rcx r8 r9   return: rax
//  Microsoft x64 ABI (Windows):
//    params: rcx rdx r8  r9          return: rax
// ============================================================

use super::{Arch, ArchAssembler, CallingConvention, Label, LabelTable, PatchSite, PatchWidth};

// ──────────────────────────────────────────────────────────────
// § 1  寄存器
// ──────────────────────────────────────────────────────────────

/// x86-64 64-bit 通用寄存器（REX 编号 0-15）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reg64(pub u8);

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
    pub const r8:  Reg64 = Reg64(8);
    pub const r9:  Reg64 = Reg64(9);
    pub const r10: Reg64 = Reg64(10);
    pub const r11: Reg64 = Reg64(11);
    pub const r12: Reg64 = Reg64(12);
    pub const r13: Reg64 = Reg64(13);
    pub const r14: Reg64 = Reg64(14);
    pub const r15: Reg64 = Reg64(15);
}

// ──────────────────────────────────────────────────────────────
// § 2  调用约定
// ──────────────────────────────────────────────────────────────

pub struct SysVAmd64;
impl CallingConvention for SysVAmd64 {
    fn int_param_regs() -> &'static [u8]  { &[7, 6, 2, 1, 8, 9] } // rdi rsi rdx rcx r8 r9
    fn int_return_reg() -> u8             { 0 }                     // rax
    fn caller_saved()   -> &'static [u8]  { &[0,1,2,6,7,8,9,10,11] }
    fn callee_saved()   -> &'static [u8]  { &[3,4,5,12,13,14,15] }
}

pub struct MsX64;
impl CallingConvention for MsX64 {
    fn int_param_regs() -> &'static [u8]  { &[1, 2, 8, 9] }        // rcx rdx r8 r9
    fn int_return_reg() -> u8             { 0 }                     // rax
    fn caller_saved()   -> &'static [u8]  { &[0,1,2,8,9,10,11] }
    fn callee_saved()   -> &'static [u8]  { &[3,4,5,6,7,12,13,14,15] }
}

// ──────────────────────────────────────────────────────────────
// § 3  X64Assembler
// ──────────────────────────────────────────────────────────────

pub struct X64Assembler {
    buf:    Vec<u8>,
    labels: LabelTable,
}

impl X64Assembler {
    pub fn new() -> Self {
        X64Assembler {
            buf:    Vec::with_capacity(256),
            labels: LabelTable::new(),
        }
    }

    // ── 底层 emit 工具 ────────────────────────────────────────

    #[inline] fn emit1(&mut self, b: u8)      { self.buf.push(b); }
    #[inline] fn emit2(&mut self, a: u8, b: u8) { self.buf.push(a); self.buf.push(b); }

    /// 将字节数组直接写入缓冲区
    #[inline]
    pub fn emit_array(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    #[inline]
    fn emit_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    fn emit_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    // ── REX 前缀 ─────────────────────────────────────────────

    /// REX.W（强制 64-bit）+ 可选 R / B 扩展位
    #[inline]
    fn rex_w(&mut self, r: u8, b: u8) {
        self.emit1(
            0x48
                | (if r & 8 != 0 { 0x04 } else { 0 })
                | (if b & 8 != 0 { 0x01 } else { 0 }),
        );
    }

    /// REX（可选 W/R/X/B），REX.W=0 时若前缀为 0x40 则省略
    /// 用于 32-bit 操作数（如 MOVSX、部分 SSE 指令）
    #[allow(dead_code)]
    #[inline]
    fn rex_opt(&mut self, w: bool, r: u8, x: u8, b: u8) {
        let byte = 0x40u8
            | (if w        { 0x08 } else { 0 })
            | (if r & 8!=0 { 0x04 } else { 0 })
            | (if x & 8!=0 { 0x02 } else { 0 })
            | (if b & 8!=0 { 0x01 } else { 0 });
        if byte != 0x40 { self.emit1(byte); }
    }

    // ── ModRM ─────────────────────────────────────────────────

    /// ModRM: mod=11 (reg-reg)
    #[inline]
    fn modrm_rr(&mut self, reg: u8, rm: u8) {
        self.emit1(0xC0 | ((reg & 7) << 3) | (rm & 7));
    }

    // ── rel32 跳转回填 ────────────────────────────────────────

    fn emit_rel32_for(&mut self, label: &Label) {
        let site = self.buf.len();
        let patch = PatchSite { offset: site, width: PatchWidth::Rel32Le };
        match self.labels.add_patch_site(label, patch) {
            Some(bound) => {
                // 后向引用：立即计算偏移
                let rel32 = (bound as i64 - (site as i64 + 4)) as i32;
                self.emit_i32(rel32);
            }
            None => {
                // 前向引用：先占位 0
                self.emit_i32(0);
            }
        }
    }

    fn patch_rel32(&mut self, site: usize, target: usize) {
        let rel32 = (target as i64 - (site as i64 + 4)) as i32;
        let bytes = rel32.to_le_bytes();
        self.buf[site..site + 4].copy_from_slice(&bytes);
    }

    // ──────────────────────────────────────────────────────────
    // 公开指令集 API
    // ──────────────────────────────────────────────────────────

    // ── 栈帧 ─────────────────────────────────────────────────

    pub fn push_rbp(&mut self)   { self.emit1(0x55); }
    pub fn pop_rbp(&mut self)    { self.emit1(0x5D); }

    /// MOV rbp, rsp
    pub fn mov_rbp_rsp(&mut self) {
        self.rex_w(reg::rbp.0, reg::rsp.0);
        self.emit1(0x89);
        self.modrm_rr(reg::rsp.0, reg::rbp.0);
    }

    // ── 数据移动 ─────────────────────────────────────────────

    /// MOV r64, imm64
    pub fn mov_r64_imm64(&mut self, dst: Reg64, imm: i64) {
        self.rex_w(0, dst.0);
        self.emit1(0xB8 | (dst.0 & 7));
        self.emit_i64(imm);
    }

    /// MOV r64, r64
    pub fn mov_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(src.0, dst.0);
        self.emit1(0x89);
        self.modrm_rr(src.0, dst.0);
    }

    /// MOV dst, [base + index*8]
    pub fn mov_r64_mem_base_idx8(&mut self, dst: Reg64, base: Reg64, idx: Reg64) {
        let rex = 0x48
            | (if dst.0 & 8 != 0 { 0x04 } else { 0 })
            | (if idx.0 & 8 != 0 { 0x02 } else { 0 })
            | (if base.0 & 8 != 0 { 0x01 } else { 0 });
        self.emit1(rex);
        self.emit1(0x8B);
        self.emit1((dst.0 & 7) << 3 | 0x04);
        self.emit1((3 << 6) | ((idx.0 & 7) << 3) | (base.0 & 7));
    }

    // ── 算术 ─────────────────────────────────────────────────

    /// XOR r64, r64
    pub fn xor_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(dst.0, src.0);
        self.emit1(0x33);
        self.modrm_rr(dst.0, src.0);
    }

    /// ADD r64, r64
    pub fn add_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(src.0, dst.0);
        self.emit1(0x01);
        self.modrm_rr(src.0, dst.0);
    }

    /// ADD r64, imm32 (sign-extended)
    pub fn add_r64_imm32(&mut self, dst: Reg64, imm: i32) {
        self.rex_w(0, dst.0);
        if (-128..=127).contains(&imm) {
            self.emit1(0x83);
            self.modrm_rr(0, dst.0);
            self.emit1(imm as u8);
        } else {
            self.emit1(0x81);
            self.modrm_rr(0, dst.0);
            self.emit_i32(imm);
        }
    }

    /// SUB r64, r64
    pub fn sub_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        // 0x29: SUB r/m64, r64  (reg=src, rm=dst)
        self.rex_w(src.0, dst.0);
        self.emit1(0x29);
        self.modrm_rr(src.0, dst.0);
    }

    /// PUSH r12  (41 54)
    pub fn push_r12(&mut self) { self.emit2(0x41, 0x54); }

    /// POP r12   (41 5C)
    pub fn pop_r12(&mut self)  { self.emit2(0x41, 0x5C); }

    /// SUB r64, imm32
    pub fn sub_r64_imm32(&mut self, dst: Reg64, imm: i32) {
        self.rex_w(0, dst.0);
        if (-128..=127).contains(&imm) {
            self.emit1(0x83);
            self.modrm_rr(5, dst.0);
            self.emit1(imm as u8);
        } else {
            self.emit1(0x81);
            self.modrm_rr(5, dst.0);
            self.emit_i32(imm);
        }
    }

    /// IMUL r64, r64  (0F AF /r, REX.W)
    pub fn imul_r64_r64(&mut self, dst: Reg64, src: Reg64) {
        self.rex_w(dst.0, src.0);
        self.emit2(0x0F, 0xAF);
        self.modrm_rr(dst.0, src.0);
    }

    /// INC r64
    pub fn inc_r64(&mut self, r: Reg64) {
        self.rex_w(0, r.0);
        self.emit1(0xFF);
        self.modrm_rr(0, r.0);
    }

    // ── 比较 ─────────────────────────────────────────────────

    /// CMP r64, r64  —  flags = lhs − rhs
    /// Opcode 0x3B: CMP r64, r/m64  → reg field = lhs, r/m field = rhs
    pub fn cmp_r64_r64(&mut self, lhs: Reg64, rhs: Reg64) {
        // REX.W + REX.R(lhs extension) + REX.B(rhs extension)
        self.rex_w(lhs.0, rhs.0);
        self.emit1(0x3B);
        // ModRM: reg = lhs (reg operand), rm = rhs (r/m operand)
        self.modrm_rr(lhs.0, rhs.0);
    }

    /// CMP r64, imm32
    pub fn cmp_r64_imm32(&mut self, reg: Reg64, imm: i32) {
        self.rex_w(0, reg.0);
        self.emit1(0x81);
        self.modrm_rr(7, reg.0);
        self.emit_i32(imm);
    }

    pub fn cmp_reg_imm(&mut self, reg: Reg64, imm: i64) {
        let r_idx = reg.0; // 获取寄存器编号 (0-7, 或处理 REX.B)

        if imm >= -128 && imm <= 127 {
            // --- CMP r/m64, imm8 ---
            // REX.W (0x48) | Opcode (0x83) | ModR/M (0xF8 + reg) | imm8
            self.buf.push(0x48);
            self.buf.push(0x83);
            self.buf.push(0xF8 | r_idx); 
            self.buf.push(imm as u8);
        } else if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
            // --- CMP r/m64, imm32 ---
            // REX.W (0x48) | Opcode (0x81) | ModR/M (0xF8 + reg) | imm32 (LE)
            self.buf.push(0x48);
            self.buf.push(0x81);
            self.buf.push(0xF8 | r_idx);
            self.buf.extend_from_slice(&(imm as i32).to_le_bytes());
        } else {
            // --- 超过 32 位：必须中转 ---
            let scratch = reg::r11; // 假设 R11 是内部 Scratch
            self.mov_r64_imm64(scratch, imm);
            self.cmp_r64_r64(reg, scratch);
        }
    }

    // ── 栈操作（任意寄存器）──────────────────────────────────

    /// PUSH r64  (r0-r7: 50+rd; r8-r15: REX.B + 50+(rd&7))
    pub fn push_r64(&mut self, r: Reg64) {
        if r.0 >= 8 { self.emit1(0x41); }
        self.emit1(0x50 | (r.0 & 7));
    }

    /// POP r64
    pub fn pop_r64(&mut self, r: Reg64) {
        if r.0 >= 8 { self.emit1(0x41); }
        self.emit1(0x58 | (r.0 & 7));
    }

    // ── 内存写入（store）─────────────────────────────────────

    /// MOV [base + index*8], src  (store 64-bit, SIB scale=3)
    pub fn mov_mem_base_idx8_r64(&mut self, base: Reg64, idx: Reg64, src: Reg64) {
        // REX.W + REX.R(src) + REX.X(idx) + REX.B(base)
        let rex = 0x48
            | (if src.0  & 8 != 0 { 0x04 } else { 0 })
            | (if idx.0  & 8 != 0 { 0x02 } else { 0 })
            | (if base.0 & 8 != 0 { 0x01 } else { 0 });
        self.emit1(rex);
        self.emit1(0x89); // MOV r/m64, r64
        // ModRM: mod=00 reg=src&7 rm=100(SIB)
        self.emit1((src.0 & 7) << 3 | 0x04);
        // SIB: scale=3(×8) index=idx&7 base=base&7
        self.emit1((3 << 6) | ((idx.0 & 7) << 3) | (base.0 & 7));
    }
    
    pub fn store_mem(&mut self, base: Reg64, offset: i64, src: Reg64) {

        // 指令格式: MOV r/m64, r64 (Opcode 0x89)
        // REX 颜色处理 (处理 R8-R15)
        let mut rex = 0x48; // 基础 REX.W (64-bit)
        if src.0 > 7 { rex |= 0x04; } // REX.R
        if base.0 > 7 { rex |= 0x01; } // REX.B
        self.emit1(rex);

        self.emit1(0x89);

        let src_enc = (src.0 & 7) << 3;
        let base_enc = base.0 & 7;

        if offset == 0 && base_enc != 5 && base_enc != 4 {
            // [reg] 模式 (注意: RBP 和 RSP 有特殊处理，这里简化处理)
            self.emit1(0x00 | src_enc | base_enc);
        } else if offset >= -128 && offset <= 127 {
            // [reg + disp8] 模式
            // 特殊情况：如果基址是 RSP (4)，必须发射 SIB 字节
            if base_enc == 4 {
                self.emit1(0x40 | src_enc | 0x04);
                self.emit1(0x24); // SIB: [rsp]
            } else {
                self.emit1(0x40 | src_enc | base_enc);
            }
            self.emit1(offset as u8);
        } else {
            // [reg + disp32] 模式
            if base_enc == 4 {
                self.emit1(0x80 | src_enc | 0x04);
                self.emit1(0x24);
            } else {
                self.emit1(0x80 | src_enc | base_enc);
            }
            self.emit_array(&offset.to_le_bytes());
        }
    }

    // ── 跳转 ─────────────────────────────────────────────────

    pub fn jmp(&mut self, lbl: &Label) { self.emit1(0xE9); self.emit_rel32_for(lbl); }
    pub fn jz (&mut self, lbl: &Label) { self.emit2(0x0F, 0x84); self.emit_rel32_for(lbl); }
    pub fn jnz(&mut self, lbl: &Label) { self.emit2(0x0F, 0x85); self.emit_rel32_for(lbl); }
    pub fn je (&mut self, lbl: &Label) { self.jz(lbl); }
    pub fn jne(&mut self, lbl: &Label) { self.jnz(lbl); }
    pub fn jl (&mut self, lbl: &Label) { self.emit2(0x0F, 0x8C); self.emit_rel32_for(lbl); }
    pub fn jge(&mut self, lbl: &Label) { self.emit2(0x0F, 0x8D); self.emit_rel32_for(lbl); }
    pub fn jle(&mut self, lbl: &Label) { self.emit2(0x0F, 0x8E); self.emit_rel32_for(lbl); }
    pub fn jg (&mut self, lbl: &Label) { self.emit2(0x0F, 0x8F); self.emit_rel32_for(lbl); }

    // ── 函数调用 ─────────────────────────────────────────────

    /// CALL rel32  (E8 cd)
    pub fn call_label(&mut self, lbl: &Label) {
        self.emit1(0xE8);
        self.emit_rel32_for(lbl);
    }

    pub fn call_r64(&mut self, reg: Reg64) {
        let reg_code = reg.0;
        
        // 1. 处理 REX 前缀 (针对 R8-R15)
        if reg_code >= 8 {
            self.buf.push(0x41); // REX.B
        }

        // 2. Opcode
        self.buf.push(0xFF);

        // 3. ModRM 字节
        // Mod: 11 (寄存器模式)
        // Reg/Opcode: 010 (十进制 2, 间接调用专用)
        // R/M: reg_code & 7
        let mod_rm = 0xC0 | (0x02 << 3) | (reg_code & 7);
        self.buf.push(mod_rm);
    }
}

// ──────────────────────────────────────────────────────────────
// § 4  ArchAssembler 实现
// ──────────────────────────────────────────────────────────────

impl ArchAssembler for X64Assembler {
    fn new_label(&mut self) -> Label {
        self.labels.new_label()
    }

    fn bind(&mut self, label: &Label) {
        let pos = self.buf.len();
        let sites = self.labels.bind(label, pos);
        for site in sites {
            self.patch_rel32(site.offset, pos);
        }
    }

    fn current_offset(&self) -> usize { self.buf.len() }

    fn into_bytes(self) -> Vec<u8> {
        self.labels.assert_all_bound();
        self.buf
    }

    fn ret(&mut self) { self.emit1(0xC3); }
    fn arch(&self) -> Arch { Arch::X86_64 }
}


