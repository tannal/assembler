// ============================================================
//  src/arch/arm64.rs  —  AArch64 汇编器
//
//  AAPCS64 调用约定（Linux / macOS / Windows on ARM64）:
//    参数: x0-x7    返回: x0
//    Caller-saved: x0-x18
//    Callee-saved: x19-x28, x29(fp), x30(lr)
//
//  所有指令均为 4 字节固定宽度，小端序。
// ============================================================

use super::{Arch, ArchAssembler, CallingConvention, Label, LabelTable, PatchSite, PatchWidth};

// ──────────────────────────────────────────────────────────────
// § 1  寄存器
// ──────────────────────────────────────────────────────────────

/// AArch64 64-bit 整数寄存器 X0-X30 + XZR(31) / SP
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XReg(pub u8); // 0-30 = X0-X30; 31 = XZR / SP (上下文相关)

#[allow(non_upper_case_globals)]
pub mod reg {
    use super::XReg;
    pub const x0:  XReg = XReg(0);
    pub const x1:  XReg = XReg(1);
    pub const x2:  XReg = XReg(2);
    pub const x3:  XReg = XReg(3);
    pub const x4:  XReg = XReg(4);
    pub const x5:  XReg = XReg(5);
    pub const x6:  XReg = XReg(6);
    pub const x7:  XReg = XReg(7);
    pub const x8:  XReg = XReg(8);
    pub const x9:  XReg = XReg(9);
    pub const x10: XReg = XReg(10);
    pub const x11: XReg = XReg(11);
    pub const x12: XReg = XReg(12);
    pub const x13: XReg = XReg(13);
    pub const x14: XReg = XReg(14);
    pub const x15: XReg = XReg(15);
    pub const x16: XReg = XReg(16);
    pub const x17: XReg = XReg(17);
    pub const x18: XReg = XReg(18);
    pub const x19: XReg = XReg(19);
    pub const x20: XReg = XReg(20);
    pub const x21: XReg = XReg(21);
    pub const x22: XReg = XReg(22);
    pub const x23: XReg = XReg(23);
    pub const x24: XReg = XReg(24);
    pub const x25: XReg = XReg(25);
    pub const x26: XReg = XReg(26);
    pub const x27: XReg = XReg(27);
    pub const x28: XReg = XReg(28);
    pub const fp:  XReg = XReg(29); // frame pointer
    pub const lr:  XReg = XReg(30); // link register
    pub const xzr: XReg = XReg(31); // zero register（指令上下文）
    pub const sp:  XReg = XReg(31); // stack pointer（指令上下文）
}

// ──────────────────────────────────────────────────────────────
// § 2  调用约定
// ──────────────────────────────────────────────────────────────

pub struct Aapcs64;
impl CallingConvention for Aapcs64 {
    fn int_param_regs() -> &'static [u8]  { &[0,1,2,3,4,5,6,7] }
    fn int_return_reg() -> u8             { 0 }
    fn caller_saved()   -> &'static [u8]  { &[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18] }
    fn callee_saved()   -> &'static [u8]  { &[19,20,21,22,23,24,25,26,27,28,29,30] }
}

// ──────────────────────────────────────────────────────────────
// § 3  Arm64Assembler
// ──────────────────────────────────────────────────────────────

pub struct Arm64Assembler {
    buf:    Vec<u8>,
    labels: LabelTable,
}

impl Arm64Assembler {
    pub fn new() -> Self {
        Arm64Assembler { buf: Vec::with_capacity(256), labels: LabelTable::new() }
    }

    // ── 内部：emit 4 字节指令字（小端）────────────────────────

    #[inline]
    fn emit_insn(&mut self, insn: u32) {
        self.buf.extend_from_slice(&insn.to_le_bytes());
    }

    // ── 回填工具 ─────────────────────────────────────────────

    /// 为 B / BL（26-bit imm）回填
    fn patch_b26(&mut self, site: usize, target: usize) {
        let off = (target as i64 - site as i64) / 4;
        assert!((-0x200_0000..0x200_0000).contains(&off), "B/BL target out of range");
        let mut insn = u32::from_le_bytes(self.buf[site..site+4].try_into().unwrap());
        insn = (insn & 0xFC00_0000) | ((off as u32) & 0x03FF_FFFF);
        self.buf[site..site+4].copy_from_slice(&insn.to_le_bytes());
    }

    /// 为 B.cond / CBZ / CBNZ（19-bit imm）回填
    fn patch_imm19(&mut self, site: usize, target: usize) {
        let off = (target as i64 - site as i64) / 4;
        assert!((-0x4_0000..0x4_0000).contains(&off), "B.cond target out of range");
        let mut insn = u32::from_le_bytes(self.buf[site..site+4].try_into().unwrap());
        insn = (insn & 0xFF00_001F) | (((off as u32) & 0x0007_FFFF) << 5);
        self.buf[site..site+4].copy_from_slice(&insn.to_le_bytes());
    }

    fn emit_branch_target(&mut self, label: &Label, base_insn: u32, width: PatchWidth) {
        let site = self.buf.len();
        let patch = PatchSite { offset: site, width };
        // 先 emit 基础指令（偏移为 0）
        self.emit_insn(base_insn);
        match self.labels.add_patch_site(label, patch) {
            Some(bound) => {
                // 后向引用：立即回填
                match width {
                    PatchWidth::ArmBranch26  => self.patch_b26(site, bound),
                    PatchWidth::Aarch64Imm19 => self.patch_imm19(site, bound),
                    _ => unreachable!(),
                }
            }
            None => {} // 前向引用，已登记，bind 时回填
        }
    }

    // ──────────────────────────────────────────────────────────
    // 公开指令集 API
    // ──────────────────────────────────────────────────────────

    // ── 栈帧 ─────────────────────────────────────────────────

    /// STP x29, x30, [sp, #-16]!   （push fp & lr）
    pub fn stp_fp_lr_pre(&mut self) {
        // Encoding: opc=10 V=0 L=0 imm7=-1 (0x7F) Rt2=lr Rn=sp Rt=fp
        // STP X (64-bit): 1010_1001_1011_1110_0111_1111_1111_1101 = 0xA9BF7BFD
        self.emit_insn(0xA9BF7BFD);
    }

    /// LDP x29, x30, [sp], #16   （pop fp & lr）
    pub fn ldp_fp_lr_post(&mut self) {
        self.emit_insn(0xA8C17BFD);
    }

    /// MOV fp, sp  (ADD X29, SP, #0)
    pub fn mov_fp_sp(&mut self) {
        // ADD Xd, Xn, #0  => 0x91000000 | (Xn<<5) | Xd
        let insn = 0x9100_0000u32 | (reg::sp.0 as u32) << 5 | reg::fp.0 as u32;
        self.emit_insn(insn);
    }

    // ── 数据移动 ─────────────────────────────────────────────

    /// MOV Xd, Xn  (ORR Xd, XZR, Xn)
    pub fn mov_reg(&mut self, dst: XReg, src: XReg) {
        // ORR (shifted register): sf=1 opc=01 shift=00 N=0 Rm Imm6=0 Rn=31 Rd
        // 0xAA00_03E0 | (Rm<<16) | Rd
        let insn = 0xAA00_03E0u32 | ((src.0 as u32) << 16) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// MOV Xd, #imm16 (MOVZ, hw=0)  — 仅 16-bit 立即数
    pub fn movz(&mut self, dst: XReg, imm16: u16) {
        // MOVZ: sf=1 opc=10 hw=00 imm16 Rd
        // 0xD280_0000 | (imm16 << 5) | Rd
        let insn = 0xD280_0000u32 | ((imm16 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// MOV Xd, imm64  —  通过最多 4 条 MOVZ/MOVK 拼装任意 64-bit 立即数
    pub fn mov_imm64(&mut self, dst: XReg, mut imm: u64) {
        let mut first = true;
        for hw in 0u32..4 {
            let chunk = (imm & 0xFFFF) as u16;
            imm >>= 16;
            if chunk != 0 || (first && imm == 0) {
                if first {
                    // MOVZ
                    let insn = 0xD280_0000u32 | (hw << 21) | ((chunk as u32) << 5) | dst.0 as u32;
                    self.emit_insn(insn);
                    first = false;
                } else {
                    // MOVK
                    let insn = 0xF280_0000u32 | (hw << 21) | ((chunk as u32) << 5) | dst.0 as u32;
                    self.emit_insn(insn);
                }
            }
        }
        if first {
            // imm == 0
            let insn = 0xD280_0000u32 | dst.0 as u32;
            self.emit_insn(insn);
        }
    }

    /// LDR Xd, [Xbase, Xidx, LSL #3]
    pub fn ldr_reg_base_idx_lsl3(&mut self, dst: XReg, base: XReg, idx: XReg) {
        // LDR (register): size=11 V=0 opc=01 Rm option=011 S=1 Rn Rt
        // 0xF868_6800 | (Rm<<16) | (Rn<<5) | Rt
        let insn = 0xF868_6800u32
            | ((idx.0 as u32) << 16)
            | ((base.0 as u32) << 5)
            | dst.0 as u32;
        self.emit_insn(insn);
    }

    // ── 算术 ─────────────────────────────────────────────────

    /// ADD Xd, Xn, Xm
    pub fn add_reg(&mut self, dst: XReg, lhs: XReg, rhs: XReg) {
        // ADD (shifted register, LSL#0): sf=1 S=0 shift=00
        // 0x8B00_0000 | (Rm<<16) | (Rn<<5) | Rd
        let insn = 0x8B00_0000u32 | ((rhs.0 as u32) << 16) | ((lhs.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// ADD Xd, Xn, #imm12 (no shift)
    pub fn add_imm12(&mut self, dst: XReg, src: XReg, imm: u16) {
        assert!(imm < 4096, "imm12 overflow");
        // 0x9100_0000 | (imm12<<10) | (Rn<<5) | Rd
        let insn = 0x9100_0000u32 | ((imm as u32) << 10) | ((src.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// SUB Xd, Xn, Xm
    pub fn sub_reg(&mut self, dst: XReg, lhs: XReg, rhs: XReg) {
        let insn = 0xCB00_0000u32 | ((rhs.0 as u32) << 16) | ((lhs.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// SUB Xd, Xn, #imm12
    pub fn sub_imm12(&mut self, dst: XReg, src: XReg, imm: u16) {
        assert!(imm < 4096, "imm12 overflow");
        let insn = 0xD100_0000u32 | ((imm as u32) << 10) | ((src.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    /// MUL Xd, Xn, Xm  (alias: MADD Xd, Xn, Xm, XZR)
    pub fn mul_reg(&mut self, dst: XReg, lhs: XReg, rhs: XReg) {
        // MADD: 0x9B00_7C00 | (Rm<<16) | (Ra<<10) | (Rn<<5) | Rd,  Ra=31=XZR
        let insn = 0x9B00_7C00u32 | ((rhs.0 as u32) << 16) | ((lhs.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    // ── 比较 ─────────────────────────────────────────────────

    /// CMP Xn, Xm  (SUBS XZR, Xn, Xm)
    pub fn cmp_reg(&mut self, lhs: XReg, rhs: XReg) {
        // SUBS (shifted reg, set flags): sf=1 S=1  0xEB00_001F | (Rm<<16) | (Rn<<5)
        let insn = 0xEB00_001Fu32 | ((rhs.0 as u32) << 16) | ((lhs.0 as u32) << 5);
        self.emit_insn(insn);
    }

    /// CMP Xn, #imm12  (SUBS XZR, Xn, #imm)
    pub fn cmp_imm12(&mut self, reg: XReg, imm: u16) {
        assert!(imm < 4096, "imm12 overflow");
        let insn = 0xF100_001Fu32 | ((imm as u32) << 10) | ((reg.0 as u32) << 5);
        self.emit_insn(insn);
    }

    // ── 跳转 ─────────────────────────────────────────────────

    /// B <label>（无条件跳转）
    pub fn b(&mut self, lbl: &Label) {
        self.emit_branch_target(lbl, 0x1400_0000, PatchWidth::ArmBranch26);
    }

    /// B.EQ / B.NE / B.LT / B.GE / B.LE / B.GT 等条件跳转
    ///  cond: 0=EQ 1=NE 2=HS 3=LO 4=MI 5=PL 6=VS 7=VC 8=HI 9=LS 10=GE 11=LT 12=GT 13=LE
    pub fn b_cond(&mut self, cond: u8, lbl: &Label) {
        // B.cond: 0x5400_0000 | (imm19<<5) | cond
        let base = 0x5400_0000u32 | cond as u32;
        self.emit_branch_target(lbl, base, PatchWidth::Aarch64Imm19);
    }

    pub fn beq(&mut self, lbl: &Label)  { self.b_cond(0,  lbl); }
    pub fn bne(&mut self, lbl: &Label)  { self.b_cond(1,  lbl); }
    pub fn bge(&mut self, lbl: &Label)  { self.b_cond(10, lbl); }
    pub fn blt(&mut self, lbl: &Label)  { self.b_cond(11, lbl); }
    pub fn bgt(&mut self, lbl: &Label)  { self.b_cond(12, lbl); }
    pub fn ble(&mut self, lbl: &Label)  { self.b_cond(13, lbl); }

    /// CBZ Xt, <label>  (Compare and Branch if Zero)
    pub fn cbz(&mut self, reg: XReg, lbl: &Label) {
        // CBZ: 0xB400_0000 | (imm19<<5) | Xt
        let base = 0xB400_0000u32 | reg.0 as u32;
        self.emit_branch_target(lbl, base, PatchWidth::Aarch64Imm19);
    }

    /// CBNZ Xt, <label>
    pub fn cbnz(&mut self, reg: XReg, lbl: &Label) {
        let base = 0xB500_0000u32 | reg.0 as u32;
        self.emit_branch_target(lbl, base, PatchWidth::Aarch64Imm19);
    }

    // ── 逻辑 ─────────────────────────────────────────────────

    /// EOR Xd, Xn, Xm  (XOR)
    pub fn eor_reg(&mut self, dst: XReg, lhs: XReg, rhs: XReg) {
        let insn = 0xCA00_0000u32 | ((rhs.0 as u32) << 16) | ((lhs.0 as u32) << 5) | dst.0 as u32;
        self.emit_insn(insn);
    }

    // ── NOP ──────────────────────────────────────────────────

    pub fn nop(&mut self) { self.emit_insn(0xD503_201F); }

    // ── 直接代理（让 Backend 不需要 use ArchAssembler）──────────
    pub fn new_label(&mut self) -> Label { self.labels.new_label() }
    pub fn bind(&mut self, label: &Label) {
        let pos = self.buf.len();
        let sites = self.labels.bind(label, pos);
        for site in sites {
            match site.width {
                PatchWidth::ArmBranch26  => self.patch_b26(site.offset, pos),
                PatchWidth::Aarch64Imm19 => self.patch_imm19(site.offset, pos),
                _ => unreachable!(),
            }
        }
    }
    pub fn ret(&mut self) { self.emit_insn(0xD65F_03C0); }

    // ── 内存写入（store）─────────────────────────────────────

    /// STR Xt, [Xbase, Xidx, LSL #3]   (store 64-bit)
    pub fn str_reg_base_idx_lsl3(&mut self, src: XReg, base: XReg, idx: XReg) {
        // STR (register): size=11 V=0 opc=00 Rm option=011 S=1 Rn Rt
        // 0xF828_6800 | (Rm<<16) | (Rn<<5) | Rt
        let insn = 0xF828_6800u32
            | ((idx.0  as u32) << 16)
            | ((base.0 as u32) << 5)
            | src.0 as u32;
        self.emit_insn(insn);
    }

    // ── 栈操作（pair push/pop）───────────────────────────────

    /// STP Xa, Xb, [sp, #-16]!   (push 任意寄存器对)
    pub fn stp_pre(&mut self, ra: XReg, rb: XReg) {
        // STP X (pre-index): opc=10 V=0 L=0 imm7=-1(0x7F) Rt2 Rn=sp Rt
        // 0xA9BF_0000 | (Rb<<10) | (sp<<5) | Ra
        let insn = 0xA9BF_0000u32
            | ((rb.0 as u32) << 10)
            | (31u32 << 5)   // sp = 31
            | ra.0 as u32;
        self.emit_insn(insn);
    }

    /// LDP Xa, Xb, [sp], #16   (pop 任意寄存器对)
    pub fn ldp_post(&mut self, ra: XReg, rb: XReg) {
        // LDP X (post-index): opc=10 V=0 L=1 imm7=+1(0x01) Rt2 Rn=sp Rt
        // 0xA8C1_0000 | (Rb<<10) | (sp<<5) | Ra
        let insn = 0xA8C1_0000u32
            | ((rb.0 as u32) << 10)
            | (31u32 << 5)
            | ra.0 as u32;
        self.emit_insn(insn);
    }

    // ── 函数调用 ─────────────────────────────────────────────

    /// BL <label>   (Branch with Link，保存 pc+4 → lr)
    pub fn bl(&mut self, lbl: &Label) {
        // BL: 0x9400_0000 | imm26
        self.emit_branch_target(lbl, 0x9400_0000, PatchWidth::ArmBranch26);
    }
}

// ──────────────────────────────────────────────────────────────
// § 4  ArchAssembler 实现
// ──────────────────────────────────────────────────────────────

impl ArchAssembler for Arm64Assembler {
    fn new_label(&mut self) -> Label { self.labels.new_label() }

    fn bind(&mut self, label: &Label) {
        let pos = self.buf.len();
        let sites = self.labels.bind(label, pos);
        for site in sites {
            match site.width {
                PatchWidth::ArmBranch26  => self.patch_b26(site.offset, pos),
                PatchWidth::Aarch64Imm19 => self.patch_imm19(site.offset, pos),
                _ => unreachable!("unexpected patch width for AArch64"),
            }
        }
    }

    fn current_offset(&self) -> usize { self.buf.len() }

    fn into_bytes(self) -> Vec<u8> {
        self.labels.assert_all_bound();
        self.buf
    }

    /// RET  (RET X30 = 0xD65F03C0)
    fn ret(&mut self) { self.emit_insn(0xD65F_03C0); }

    fn arch(&self) -> Arch { Arch::Aarch64 }
}
