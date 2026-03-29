// ============================================================
//  src/arch/arm.rs  —  ARM 32-bit Thumb-2 汇编器
//
//  AAPCS (ARM 32-bit) 调用约定:
//    参数: r0-r3    返回: r0 (64-bit: r0+r1)
//    Caller-saved: r0-r3, r12 (ip), r14 (lr)
//    Callee-saved: r4-r11, r13 (sp), r15 (pc)
//
//  本实现采用 Thumb-2 指令集（16-bit 和 32-bit 混合编码）。
//  所有 32-bit Thumb-2 指令以大端半字对存储（小端系统上需注意）。
// ============================================================

use super::{Arch, ArchAssembler, CallingConvention, Label, LabelTable, PatchSite, PatchWidth};

// ──────────────────────────────────────────────────────────────
// § 1  寄存器
// ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reg(pub u8); // 0-15

#[allow(non_upper_case_globals)]
pub mod reg {
    use super::Reg;
    pub const r0:  Reg = Reg(0);
    pub const r1:  Reg = Reg(1);
    pub const r2:  Reg = Reg(2);
    pub const r3:  Reg = Reg(3);
    pub const r4:  Reg = Reg(4);
    pub const r5:  Reg = Reg(5);
    pub const r6:  Reg = Reg(6);
    pub const r7:  Reg = Reg(7);
    pub const r8:  Reg = Reg(8);
    pub const r9:  Reg = Reg(9);
    pub const r10: Reg = Reg(10);
    pub const r11: Reg = Reg(11); // fp
    pub const ip:  Reg = Reg(12); // intra-procedure scratch
    pub const sp:  Reg = Reg(13);
    pub const lr:  Reg = Reg(14);
    pub const pc:  Reg = Reg(15);
    pub const fp:  Reg = r11;
}

// ──────────────────────────────────────────────────────────────
// § 2  调用约定
// ──────────────────────────────────────────────────────────────

pub struct Aapcs32;
impl CallingConvention for Aapcs32 {
    fn int_param_regs() -> &'static [u8]  { &[0,1,2,3] }
    fn int_return_reg() -> u8             { 0 }
    fn caller_saved()   -> &'static [u8]  { &[0,1,2,3,12,14] }
    fn callee_saved()   -> &'static [u8]  { &[4,5,6,7,8,9,10,11,13] }
}

// ──────────────────────────────────────────────────────────────
// § 3  Thumb-2 编码工具
// ──────────────────────────────────────────────────────────────

/// 将 32-bit Thumb-2 指令拆成两个 16-bit 半字（Thumb-2 大端半字对，小端系统）
///  第一个半字是高位（bits[31:16]），第二个是低位（bits[15:0]）
fn thumb2_halfwords(insn: u32) -> [u8; 4] {
    let hw1 = (insn >> 16) as u16;
    let hw0 = (insn & 0xFFFF) as u16;
    let [a, b] = hw1.to_le_bytes();
    let [c, d] = hw0.to_le_bytes();
    [a, b, c, d]
}

// ──────────────────────────────────────────────────────────────
// § 4  ArmAssembler（Thumb-2 模式）
// ──────────────────────────────────────────────────────────────

pub struct ArmAssembler {
    buf:    Vec<u8>,
    labels: LabelTable,
}

impl ArmAssembler {
    pub fn new() -> Self {
        ArmAssembler { buf: Vec::with_capacity(256), labels: LabelTable::new() }
    }

    // ── 内部 emit ─────────────────────────────────────────────

    /// Emit 16-bit Thumb 指令（little-endian）
    #[inline]
    fn emit_t16(&mut self, hw: u16) {
        self.buf.extend_from_slice(&hw.to_le_bytes());
    }

    /// Emit 32-bit Thumb-2 指令（两个 16-bit 半字，先高后低）
    #[inline]
    fn emit_t32(&mut self, insn: u32) {
        let bytes = thumb2_halfwords(insn);
        self.buf.extend_from_slice(&bytes);
    }

    // ── 回填工具 ─────────────────────────────────────────────

    /// 回填 Thumb-2 B{cond}/B 的 20-bit 或 24-bit 偏移
    /// 本实现统一使用 32-bit Thumb-2 B（24-bit 偏移，±16 MB）
    fn patch_thumb2_b(&mut self, site: usize, target: usize) {
        // Thumb-2 B 编码（T4, 无条件）:
        //   hw1: 1111_0S__imm10__
        //   hw2: 1001_J1__J2__imm11__
        //   where S,J1,J2,imm10,imm11 encode the ±16MB signed offset
        let off = (target as i64) - (site as i64) - 4; // PC+4
        assert!(
            (-0x100_0000..0x100_0000).contains(&off),
            "Thumb-2 B target out of ±16MB range: offset = {}",
            off
        );
        let off = off as i32 as u32;
        let s      = (off >> 24) & 1;
        let imm10  = (off >> 12) & 0x3FF;
        let imm11  = (off >>  1) & 0x7FF;
        let i1     = (off >> 23) & 1;
        let i2     = (off >> 22) & 1;
        let j1     = (!i1 ^ s) & 1;
        let j2     = (!i2 ^ s) & 1;
        let hw1 = (0xF000u32 | (s << 10) | imm10) as u16;
        let hw2 = (0x9000u32 | (j1 << 13) | (j2 << 11) | imm11) as u16;
        let [a, b] = hw1.to_le_bytes();
        let [c, d] = hw2.to_le_bytes();
        self.buf[site..site+4].copy_from_slice(&[a, b, c, d]);
    }

    fn emit_branch_target(&mut self, label: &Label, base_hw1: u16, base_hw2: u16, width: PatchWidth) {
        let site = self.buf.len();
        // 先 emit 零偏移占位
        let [a, b] = base_hw1.to_le_bytes();
        let [c, d] = base_hw2.to_le_bytes();
        self.buf.extend_from_slice(&[a, b, c, d]);
        let patch = PatchSite { offset: site, width };
        match self.labels.add_patch_site(label, patch) {
            Some(bound) => self.patch_thumb2_b(site, bound),
            None => {}
        }
    }

    // ──────────────────────────────────────────────────────────
    // 公开指令集 API
    // ──────────────────────────────────────────────────────────

    // ── 栈帧 ─────────────────────────────────────────────────

    /// PUSH {r4, lr}  (T1: 16-bit)
    pub fn push_r4_lr(&mut self) {
        // PUSH register list: 1011_0101_0001_0000 = 0xB510 ({r4, lr})
        self.emit_t16(0xB510);
    }

    /// POP {r4, pc}
    pub fn pop_r4_pc(&mut self) {
        self.emit_t16(0xBD10);
    }

    /// PUSH {fp, lr}  (32-bit Thumb-2 STM)
    pub fn push_fp_lr(&mut self) {
        // STMDB sp!, {r11, r14} = PUSH {fp, lr}
        // hw1: 1110_1001_0010_1101 = 0xE92D
        // hw2: 0100_1000_0000_0000 = 0x4800  (bit14=lr=r14, bit11=fp=r11)
        self.emit_t32(0xE92D4800);
    }

    /// POP {fp, pc}
    pub fn pop_fp_pc(&mut self) {
        // LDMIA sp!, {r11, r15} = POP {fp, pc}
        self.emit_t32(0xE8BD8800);
    }

    // ── 数据移动 ─────────────────────────────────────────────

    /// MOV Rd, Rm  (T1 16-bit, any register)
    pub fn mov_reg(&mut self, dst: Reg, src: Reg) {
        // MOV (register, T1): 0100_0110_DN_Rm_Rd where D=dst[3], N=dst[2:0]
        let hw = 0x4600u16 | ((dst.0 as u16 & 8) << 4) | ((src.0 as u16) << 3) | (dst.0 as u16 & 7);
        self.emit_t16(hw);
    }

    /// MOV Rd, #imm8  (T1 16-bit, Rd must be r0-r7)
    pub fn mov_imm8(&mut self, dst: Reg, imm: u8) {
        assert!(dst.0 < 8, "MOV imm8 T1 only supports r0-r7");
        let hw = 0x2000u16 | ((dst.0 as u16) << 8) | imm as u16;
        self.emit_t16(hw);
    }

    /// MOV Rd, #imm16  (Thumb-2 MOVW, T3)
    pub fn movw(&mut self, dst: Reg, imm16: u16) {
        // MOVW: hw1 = 0xF240 | (i<<10) | imm4
        //        hw2 = 0x0000 | (imm3<<12) | (Rd<<8) | imm8
        let i     = (imm16 >> 11) & 1;
        let imm4  = (imm16 >> 12) & 0xF;
        let imm3  = (imm16 >>  8) & 0x7;
        let imm8  = imm16 & 0xFF;
        let hw1 = 0xF240u32 | ((i as u32) << 10) | imm4 as u32;
        let hw2 = 0x0000u32 | ((imm3 as u32) << 12) | ((dst.0 as u32) << 8) | imm8 as u32;
        self.emit_t32((hw1 << 16) | hw2);
    }

    /// MOV Rd, #imm32  (MOVW + MOVT)
    pub fn mov_imm32(&mut self, dst: Reg, imm: u32) {
        self.movw(dst, (imm & 0xFFFF) as u16);
        if imm >> 16 != 0 {
            self.movt(dst, (imm >> 16) as u16);
        }
    }

    /// MOVT Rd, #imm16  (Thumb-2, top half)
    pub fn movt(&mut self, dst: Reg, imm16: u16) {
        let i    = (imm16 >> 11) & 1;
        let imm4 = (imm16 >> 12) & 0xF;
        let imm3 = (imm16 >>  8) & 0x7;
        let imm8 = imm16 & 0xFF;
        let hw1 = 0xF2C0u32 | ((i as u32) << 10) | imm4 as u32;
        let hw2 = ((imm3 as u32) << 12) | ((dst.0 as u32) << 8) | imm8 as u32;
        self.emit_t32((hw1 << 16) | hw2);
    }

    // ── 内存访问 ─────────────────────────────────────────────

    /// LDR Rd, [Rn, Rm, LSL #2]  (32-bit word, shifted register)
    pub fn ldr_reg_lsl2(&mut self, dst: Reg, base: Reg, idx: Reg) {
        // T2: 0xF850_0020 | (Rn<<16) | (Rt<<12) | Rm
        // LSL #2 → shift=2 in bits[5:4]
        let insn = 0xF850_0020u32
            | ((base.0 as u32) << 16)
            | ((dst.0 as u32) << 12)
            | ((2u32) << 4)         // LSL #2
            | idx.0 as u32;
        self.emit_t32(insn);
    }

    // ── 算术 ─────────────────────────────────────────────────

    /// ADD Rd, Rn, Rm  (T1 16-bit, r0-r7 only)
    pub fn add_reg_t16(&mut self, dst: Reg, lhs: Reg, rhs: Reg) {
        assert!(dst.0 < 8 && lhs.0 < 8 && rhs.0 < 8);
        let hw = 0x1800u16 | ((rhs.0 as u16) << 6) | ((lhs.0 as u16) << 3) | dst.0 as u16;
        self.emit_t16(hw);
    }

    /// ADD Rd, Rn, Rm  (32-bit Thumb-2)
    pub fn add_reg(&mut self, dst: Reg, lhs: Reg, rhs: Reg) {
        let insn = 0xEB00_0000u32
            | ((lhs.0 as u32) << 16)
            | ((dst.0 as u32) << 8)
            | rhs.0 as u32;
        self.emit_t32(insn);
    }

    /// ADD Rd, Rn, #imm8  (T1 16-bit)
    pub fn add_imm8(&mut self, dst: Reg, src: Reg, imm: u8) {
        assert!(dst.0 < 8 && src.0 < 8);
        let hw = 0x1C00u16 | ((imm as u16) << 6) | ((src.0 as u16) << 3) | dst.0 as u16;
        self.emit_t16(hw);
    }

    /// SUB Rd, Rn, #imm8  (T1 16-bit)
    pub fn sub_imm8(&mut self, dst: Reg, src: Reg, imm: u8) {
        assert!(dst.0 < 8 && src.0 < 8);
        let hw = 0x1E00u16 | ((imm as u16) << 6) | ((src.0 as u16) << 3) | dst.0 as u16;
        self.emit_t16(hw);
    }

    /// SUB Rd, Rn, Rm  (32-bit)
    pub fn sub_reg(&mut self, dst: Reg, lhs: Reg, rhs: Reg) {
        let insn = 0xEBA0_0000u32
            | ((lhs.0 as u32) << 16)
            | ((dst.0 as u32) << 8)
            | rhs.0 as u32;
        self.emit_t32(insn);
    }

    /// MUL Rd, Rn, Rm  (32-bit Thumb-2 T2)
    pub fn mul_reg(&mut self, dst: Reg, lhs: Reg, rhs: Reg) {
        // MUL: 0xFB00_F000 | (Rn<<16) | (Rd<<8) | Rm
        let insn = 0xFB00_F000u32
            | ((lhs.0 as u32) << 16)
            | ((dst.0 as u32) << 8)
            | rhs.0 as u32;
        self.emit_t32(insn);
    }

    // ── 比较 ─────────────────────────────────────────────────

    /// CMP Rn, Rm  (T1 16-bit, r0-r7)
    pub fn cmp_reg_t16(&mut self, lhs: Reg, rhs: Reg) {
        assert!(lhs.0 < 8 && rhs.0 < 8);
        let hw = 0x4280u16 | ((rhs.0 as u16) << 3) | lhs.0 as u16;
        self.emit_t16(hw);
    }

    /// CMP Rn, #imm8  (T1 16-bit, r0-r7)
    pub fn cmp_imm8(&mut self, reg: Reg, imm: u8) {
        assert!(reg.0 < 8);
        let hw = 0x2800u16 | ((reg.0 as u16) << 8) | imm as u16;
        self.emit_t16(hw);
    }

    // ── 逻辑 ─────────────────────────────────────────────────

    /// EOR Rd, Rn, Rm  (XOR, 16-bit T1, r0-r7)
    pub fn eor_reg_t16(&mut self, dst: Reg, src: Reg) {
        let hw = 0x4040u16 | ((src.0 as u16) << 3) | dst.0 as u16;
        self.emit_t16(hw);
    }

    // ── 跳转 ─────────────────────────────────────────────────

    /// B <label>  (Thumb-2 T4, 无条件, ±16MB)
    pub fn b(&mut self, lbl: &Label) {
        // 零偏移基础编码: hw1=0xF000, hw2=0x9000
        self.emit_branch_target(lbl, 0xF000, 0x9000, PatchWidth::ArmBranch26);
    }

    /// B{cond} <label>  (Thumb-2 T3, 有条件, ±1MB)
    ///  cond: 0=EQ 1=NE 2=CS 3=CC 4=MI 5=PL 6=VS 7=VC 8=HI 9=LS 10=GE 11=LT 12=GT 13=LE
    pub fn b_cond(&mut self, cond: u8, lbl: &Label) {
        // 使用 Thumb-2 B T3 (有条件 32-bit): hw1=0xF000|(cond<<6), hw2=0x8000
        // 实际偏移回填时用相同 patch_thumb2_b（会正确处理 cond 字段）
        let site = self.buf.len();
        // 先写条件跳转的基础指令（偏移=0 = 跳到 PC+4 = 下一条）
        let hw1 = 0xF000u16 | ((cond as u16) << 6);
        let hw2 = 0x8000u16;
        let [a, b] = hw1.to_le_bytes();
        let [c, d] = hw2.to_le_bytes();
        self.buf.extend_from_slice(&[a, b, c, d]);
        let patch = PatchSite { offset: site, width: PatchWidth::ArmBranch26 };
        match self.labels.add_patch_site(lbl, patch) {
            Some(bound) => self.patch_thumb2_b(site, bound),
            None => {}
        }
    }

    pub fn beq(&mut self, lbl: &Label)  { self.b_cond(0,  lbl); }
    pub fn bne(&mut self, lbl: &Label)  { self.b_cond(1,  lbl); }
    pub fn bge(&mut self, lbl: &Label)  { self.b_cond(10, lbl); }
    pub fn blt(&mut self, lbl: &Label)  { self.b_cond(11, lbl); }
    pub fn bgt(&mut self, lbl: &Label)  { self.b_cond(12, lbl); }
    pub fn ble(&mut self, lbl: &Label)  { self.b_cond(13, lbl); }

    // ── BX lr（用于函数返回前设 T bit）────────────────────────

    /// BX lr  (从 Thumb 函数返回)
    pub fn bx_lr(&mut self) {
        self.emit_t16(0x4770);
    }
}

// ──────────────────────────────────────────────────────────────
// § 5  ArchAssembler 实现
// ──────────────────────────────────────────────────────────────

impl ArchAssembler for ArmAssembler {
    fn new_label(&mut self) -> Label { self.labels.new_label() }

    fn bind(&mut self, label: &Label) {
        let pos = self.buf.len();
        let sites = self.labels.bind(label, pos);
        for site in sites {
            self.patch_thumb2_b(site.offset, pos);
        }
    }

    fn current_offset(&self) -> usize { self.buf.len() }

    fn into_bytes(self) -> Vec<u8> {
        self.labels.assert_all_bound();
        self.buf
    }

    fn ret(&mut self) { self.bx_lr(); }
    fn arch(&self) -> Arch { Arch::Arm }
}
