use std::collections::HashMap;

/// ARM condition codes (`0..=13`) for `B<cond>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    Eq = 0x0, // Z == 1
    Ne = 0x1, // Z == 0
    Cs = 0x2, // C == 1 (HS / unsigned >=)
    Cc = 0x3, // C == 0 (LO / unsigned <)
    Mi = 0x4, // N == 1
    Pl = 0x5, // N == 0
    Vs = 0x6, // V == 1
    Vc = 0x7, // V == 0
    Hi = 0x8, // C == 1 && Z == 0 (unsigned >)
    Ls = 0x9, // C == 0 || Z == 1 (unsigned <=)
    Ge = 0xA, // N == V (signed >=)
    Lt = 0xB, // N != V (signed <)
    Gt = 0xC, // Z == 0 && N == V (signed >)
    Le = 0xD, // Z == 1 || N != V (signed <=)
}

impl Cond {
    #[inline]
    pub const fn invert(self) -> Self {
        match self {
            Cond::Eq => Cond::Ne,
            Cond::Ne => Cond::Eq,
            Cond::Cs => Cond::Cc,
            Cond::Cc => Cond::Cs,
            Cond::Mi => Cond::Pl,
            Cond::Pl => Cond::Mi,
            Cond::Vs => Cond::Vc,
            Cond::Vc => Cond::Vs,
            Cond::Hi => Cond::Ls,
            Cond::Ls => Cond::Hi,
            Cond::Ge => Cond::Lt,
            Cond::Lt => Cond::Ge,
            Cond::Gt => Cond::Le,
            Cond::Le => Cond::Gt,
        }
    }
}

#[derive(Debug, Clone)]
enum LitValue {
    Const(u32),
    SymAddr { sym: String, thumb_bit: bool, addend: i32 },
    LocalLabelThumbAddr(usize),
}

#[derive(Debug, Clone)]
struct PendingLit {
    ldr_off: usize,
    rd: u8,
    val: LitValue,
}

#[derive(Debug, Clone)]
enum LocalBranchKind {
    Thumb1B11,   // 2-byte `0xE000` (+/- 2048 B)
    Thumb1BCond(Cond), // 2-byte `0xD000` (+/- 256 B)
    Thumb2BW,    // 4-byte `0xF000 / 0x9000` (+/- 16 MB)
}

#[derive(Debug, Clone)]
struct LocalBranchFixup {
    instr_off: usize,
    target_label: usize,
    kind: LocalBranchKind,
}

#[derive(Debug, Clone)]
pub enum GlobalRelocKind {
    Bl32, // 4-byte Thumb `BL` (+/- 16 MB)
    Abs32 { thumb_bit: bool, addend: i32 }, // 4-byte word in literal pool or vector table
    MovwMovt { rd: u8, thumb_bit: bool, addend: i32 }, // 8-byte `MOVW` + `MOVT` pair on Thumb-2
}

#[derive(Debug, Clone)]
pub struct GlobalReloc {
    pub offset: usize,
    pub symbol: String,
    pub kind: GlobalRelocKind,
}

#[derive(Debug, Clone)]
pub struct FunctionSymRecord {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

/// Pure-Rust ARMv6-M (Thumb-1, RP2040) & ARMv8-M.main (Thumb-2, RP2350) machine code emitter.
pub struct ThumbAssembler {
    pub has_thumb2: bool,
    pub code: Vec<u8>,
    pub symbols: HashMap<String, usize>,
    pub fn_records: Vec<FunctionSymRecord>,
    pub global_relocs: Vec<GlobalReloc>,
    local_labels: Vec<Option<usize>>,
    local_branches: Vec<LocalBranchFixup>,
    pending_lits: Vec<PendingLit>,
    pending_local_lit_words: Vec<(usize, usize)>, // (word_off, label_id)
    cur_fn_start: Option<(String, usize)>,
}

impl ThumbAssembler {
    pub fn new(has_thumb2: bool) -> Self {
        Self {
            has_thumb2,
            code: Vec::with_capacity(16384),
            symbols: HashMap::new(),
            fn_records: Vec::new(),
            global_relocs: Vec::new(),
            local_labels: Vec::new(),
            local_branches: Vec::new(),
            pending_lits: Vec::new(),
            pending_local_lit_words: Vec::new(),
            cur_fn_start: None,
        }
    }

    #[inline]
    pub fn pos(&self) -> usize {
        self.code.len()
    }

    pub fn align4_nop(&mut self) {
        while (self.code.len() & 3) != 0 {
            self.emit16(0xBF00); // architectural Thumb NOP
        }
    }

    pub fn align4_zero(&mut self) {
        while (self.code.len() & 3) != 0 {
            self.code.push(0);
        }
    }

    pub fn begin_function(&mut self, name: &str) {
        self.align4_nop();
        let start = self.pos();
        self.symbols.insert(name.to_string(), start);
        self.local_labels.clear();
        self.local_branches.clear();
        self.pending_lits.clear();
        self.pending_local_lit_words.clear();
        self.cur_fn_start = Some((name.to_string(), start));
    }

    pub fn end_function(&mut self, base_vma: u32) -> Result<(), String> {
        self.flush_literal_pool_inline();
        self.resolve_local_branches(base_vma)?;
        if let Some((name, start)) = self.cur_fn_start.take() {
            let size = self.pos().saturating_sub(start);
            self.fn_records.push(FunctionSymRecord {
                name,
                offset: start,
                size,
            });
        }
        Ok(())
    }

    pub fn new_label(&mut self) -> usize {
        let id = self.local_labels.len();
        self.local_labels.push(None);
        id
    }

    pub fn bind_label(&mut self, id: usize) {
        self.maybe_emit_literal_island();
        self.local_labels[id] = Some(self.pos());
    }

    #[inline]
    pub fn emit16(&mut self, hw: u16) {
        self.code.extend_from_slice(&hw.to_le_bytes());
    }

    #[inline]
    pub fn emit32_le(&mut self, w: u32) {
        self.code.extend_from_slice(&w.to_le_bytes());
    }

    #[inline]
    pub fn emit_thumb32(&mut self, hw1: u16, hw2: u16) {
        self.emit16(hw1);
        self.emit16(hw2);
    }

    fn patch16(&mut self, off: usize, hw: u16) {
        let b = hw.to_le_bytes();
        self.code[off] = b[0];
        self.code[off + 1] = b[1];
    }

    fn patch32_le(&mut self, off: usize, w: u32) {
        let b = w.to_le_bytes();
        self.code[off..off + 4].copy_from_slice(&b);
    }

    /// Check if oldest pending literal `LDR [pc, #imm8*4]` is approaching the 1020-byte Thumb-1 limit;
    /// if so, emit an inline literal pool island guarded by an unconditional `B` jump-over.
    pub fn maybe_emit_literal_island(&mut self) {
        if let Some(first) = self.pending_lits.first() {
            let dist = self.pos().saturating_sub(first.ldr_off);
            let pool_bytes = self.pending_lits.len() * 4 + 8;
            if dist + pool_bytes >= 820 {
                let jmp_off = self.pos();
                self.emit16(0xE000); // placeholder `B after_pool`
                self.flush_literal_pool_inline();
                let after = self.pos();
                let rel = (after as isize) - ((jmp_off as isize) + 4);
                let imm11 = ((rel >> 1) as i16) as u16 & 0x7FF;
                self.patch16(jmp_off, 0xE000 | imm11);
            }
        }
    }

    pub fn flush_literal_pool_inline(&mut self) {
        if self.pending_lits.is_empty() {
            return;
        }
        self.align4_nop();
        let pending = std::mem::take(&mut self.pending_lits);
        // Deduplicate pure constants within this pool to keep flash tight
        let mut const_pool_offs: HashMap<u32, usize> = HashMap::new();

        for item in pending {
            let word_off = match item.val {
                LitValue::Const(v) => {
                    if let Some(&existing_off) = const_pool_offs.get(&v) {
                        let base = (item.ldr_off + 4) & !3;
                        if existing_off >= base && (existing_off - base) <= 1020 {
                            existing_off
                        } else {
                            let off = self.pos();
                            self.emit32_le(v);
                            const_pool_offs.insert(v, off);
                            off
                        }
                    } else {
                        let off = self.pos();
                        self.emit32_le(v);
                        const_pool_offs.insert(v, off);
                        off
                    }
                }
                LitValue::SymAddr {
                    sym,
                    thumb_bit,
                    addend,
                } => {
                    let off = self.pos();
                    self.emit32_le(0);
                    self.global_relocs.push(GlobalReloc {
                        offset: off,
                        symbol: sym,
                        kind: GlobalRelocKind::Abs32 { thumb_bit, addend },
                    });
                    off
                }
                LitValue::LocalLabelThumbAddr(lbl) => {
                    let off = self.pos();
                    self.emit32_le(0);
                    self.pending_local_lit_words.push((off, lbl));
                    off
                }
            };

            let base = (item.ldr_off + 4) & !3;
            let delta = word_off
                .checked_sub(base)
                .expect("literal pool placed before LDR");
            assert!(
                delta % 4 == 0 && delta / 4 <= 255,
                "Thumb-1 literal pool offset overflow: delta={}",
                delta
            );
            let imm8 = (delta / 4) as u16;
            self.patch16(item.ldr_off, 0x4800 | ((item.rd as u16 & 7) << 8) | imm8);
        }
    }

    fn resolve_local_branches(&mut self, base_vma: u32) -> Result<(), String> {
        for &(word_off, lbl) in &self.pending_local_lit_words.clone() {
            let target_off = self
                .local_labels
                .get(lbl)
                .and_then(|x| *x)
                .ok_or_else(|| format!("unbound local label {} in literal pool", lbl))?;
            let vma = (base_vma + target_off as u32) | 1;
            self.patch32_le(word_off, vma);
        }

        let branches = std::mem::take(&mut self.local_branches);
        for br in branches {
            let target_off = self
                .local_labels
                .get(br.target_label)
                .and_then(|x| *x)
                .ok_or_else(|| format!("unbound local branch label {}", br.target_label))?;
            let rel = (target_off as i32) - (br.instr_off as i32 + 4);
            match br.kind {
                LocalBranchKind::Thumb1B11 => {
                    if !(-2048..=2046).contains(&rel) {
                        return Err(format!(
                            "Thumb-1 B11 range exceeded (rel={} bytes); use emit_b_label",
                            rel
                        ));
                    }
                    let imm11 = ((rel >> 1) as u16) & 0x7FF;
                    self.patch16(br.instr_off, 0xE000 | imm11);
                }
                LocalBranchKind::Thumb1BCond(cond) => {
                    if !(-256..=254).contains(&rel) {
                        return Err(format!(
                            "Thumb-1 B<cond> short range exceeded (rel={} bytes)",
                            rel
                        ));
                    }
                    let imm8 = ((rel >> 1) as u16) & 0xFF;
                    self.patch16(br.instr_off, 0xD000 | ((cond as u16) << 8) | imm8);
                }
                LocalBranchKind::Thumb2BW => {
                    let (hw1, hw2) = encode_thumb2_bw(rel)?;
                    let b1 = hw1.to_le_bytes();
                    let b2 = hw2.to_le_bytes();
                    self.code[br.instr_off..br.instr_off + 2].copy_from_slice(&b1);
                    self.code[br.instr_off + 2..br.instr_off + 4].copy_from_slice(&b2);
                }
            }
        }
        Ok(())
    }

    /// Resolve all symbol relocations across the finalized image (given `base_vma`, e.g. `0x1000_0000`).
    pub fn link_global_relocs(&mut self, base_vma: u32) -> Result<(), String> {
        let relocs = self.global_relocs.clone();
        for r in relocs {
            let &sym_off = self
                .symbols
                .get(&r.symbol)
                .ok_or_else(|| format!("undefined bare-metal symbol '{}'", r.symbol))?;
            let sym_vma = base_vma.wrapping_add(sym_off as u32);
            match r.kind {
                GlobalRelocKind::Bl32 => {
                    let instr_vma = base_vma.wrapping_add(r.offset as u32);
                    let rel = (sym_vma as i64 - (instr_vma as i64 + 4)) as i32;
                    let (hw1, hw2) = encode_bl_rel(rel)?;
                    self.patch16(r.offset, hw1);
                    self.patch16(r.offset + 2, hw2);
                }
                GlobalRelocKind::Abs32 { thumb_bit, addend } => {
                    let mut val = (sym_vma as i64 + addend as i64) as u32;
                    if thumb_bit {
                        val |= 1;
                    }
                    self.patch32_le(r.offset, val);
                }
                GlobalRelocKind::MovwMovt {
                    rd,
                    thumb_bit,
                    addend,
                } => {
                    let mut val = (sym_vma as i64 + addend as i64) as u32;
                    if thumb_bit {
                        val |= 1;
                    }
                    let (w1, w2) = encode_movw(rd, (val & 0xFFFF) as u16);
                    let (t1, t2) = encode_movt(rd, (val >> 16) as u16);
                    self.patch16(r.offset, w1);
                    self.patch16(r.offset + 2, w2);
                    self.patch16(r.offset + 4, t1);
                    self.patch16(r.offset + 6, t2);
                }
            }
        }
        Ok(())
    }

    // --- Core Instruction Helpers (r0..r7 safe across BOTH ARMv6-M and ARMv8-M.main) ---

    #[inline]
    pub fn emit_nop(&mut self) {
        self.emit16(0xBF00);
    }

    #[inline]
    pub fn emit_wfi(&mut self) {
        self.emit16(0xBF30);
    }

    #[inline]
    pub fn emit_movs_imm8(&mut self, rd: u8, imm8: u8) {
        debug_assert!(rd < 8);
        self.emit16(0x2000 | ((rd as u16) << 8) | (imm8 as u16));
    }

    /// Register-to-register move (`r0..r15`, does NOT clobber flags!).
    #[inline]
    pub fn emit_mov_reg(&mut self, rd: u8, rm: u8) {
        if rd == rm {
            return;
        }
        let d_hi = ((rd as u16) >> 3) & 1;
        self.emit16(0x4600 | (d_hi << 7) | (((rm as u16) & 0xF) << 3) | ((rd as u16) & 7));
    }

    #[inline]
    pub fn emit_movs_reg(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        // `LSLS Rd, Rm, #0` == `MOVS Rd, Rm`
        self.emit16(((rm as u16) << 3) | (rd as u16));
    }

    /// Materialize arbitrary 32-bit constant into `rd` (`r0..r7`).
    /// Uses `MOVS` for `0..=255`, `MOVW`/`MOVT` on RP2350, and auto-pooled `LDR [PC, #off]` on RP2040.
    pub fn emit_load_u32(&mut self, rd: u8, val: u32) {
        debug_assert!(rd < 8);
        if val <= 255 {
            self.emit_movs_imm8(rd, val as u8);
            return;
        }
        if self.has_thumb2 {
            let (w1, w2) = encode_movw(rd, (val & 0xFFFF) as u16);
            self.emit_thumb32(w1, w2);
            let hi = (val >> 16) as u16;
            if hi != 0 {
                let (t1, t2) = encode_movt(rd, hi);
                self.emit_thumb32(t1, t2);
            }
        } else {
            self.maybe_emit_literal_island();
            let ldr_off = self.pos();
            self.emit16(0x4800 | ((rd as u16) << 8));
            self.pending_lits.push(PendingLit {
                ldr_off,
                rd,
                val: LitValue::Const(val),
            });
        }
    }

    /// Load global symbol address (`sym + addend`, optionally `| 1` for Thumb function pointers) into `rd` (`r0..r7`).
    pub fn emit_load_sym_addr(&mut self, rd: u8, sym: &str, thumb_bit: bool, addend: i32) {
        debug_assert!(rd < 8);
        if self.has_thumb2 {
            let off = self.pos();
            self.emit_thumb32(0xF240, (rd as u16) << 8);
            self.emit_thumb32(0xF2C0, (rd as u16) << 8);
            self.global_relocs.push(GlobalReloc {
                offset: off,
                symbol: sym.to_string(),
                kind: GlobalRelocKind::MovwMovt {
                    rd,
                    thumb_bit,
                    addend,
                },
            });
        } else {
            self.maybe_emit_literal_island();
            let ldr_off = self.pos();
            self.emit16(0x4800 | ((rd as u16) << 8));
            self.pending_lits.push(PendingLit {
                ldr_off,
                rd,
                val: LitValue::SymAddr {
                    sym: sym.to_string(),
                    thumb_bit,
                    addend,
                },
            });
        }
    }

    #[inline]
    pub fn emit_adds_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        debug_assert!(rd < 8 && rn < 8 && rm < 8);
        self.emit16(0x1800 | ((rm as u16) << 6) | ((rn as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_subs_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        debug_assert!(rd < 8 && rn < 8 && rm < 8);
        self.emit16(0x1A00 | ((rm as u16) << 6) | ((rn as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_adds_imm8(&mut self, rdn: u8, imm8: u8) {
        debug_assert!(rdn < 8);
        if imm8 != 0 {
            self.emit16(0x3000 | ((rdn as u16) << 8) | (imm8 as u16));
        }
    }

    #[inline]
    pub fn emit_subs_imm8(&mut self, rdn: u8, imm8: u8) {
        debug_assert!(rdn < 8);
        if imm8 != 0 {
            self.emit16(0x3800 | ((rdn as u16) << 8) | (imm8 as u16));
        }
    }

    #[inline]
    pub fn emit_adcs(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4140 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_sbcs(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4180 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_rsbs_zero(&mut self, rd: u8, rn: u8) {
        debug_assert!(rd < 8 && rn < 8);
        self.emit16(0x4240 | ((rn as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_muls(&mut self, rdm: u8, rn: u8) {
        debug_assert!(rdm < 8 && rn < 8);
        self.emit16(0x4340 | ((rn as u16) << 3) | (rdm as u16));
    }

    #[inline]
    pub fn emit_ands(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4000 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_orrs(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4300 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_eors(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4040 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_bics(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4380 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_mvns(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        self.emit16(0x43C0 | ((rm as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_lsls_imm(&mut self, rd: u8, rm: u8, shift: u8) {
        debug_assert!(rd < 8 && rm < 8 && shift < 32);
        self.emit16(0x0000 | (((shift as u16) & 0x1F) << 6) | ((rm as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_lsrs_imm(&mut self, rd: u8, rm: u8, shift: u8) {
        debug_assert!(rd < 8 && rm < 8 && shift <= 32);
        if shift == 0 {
            self.emit_movs_reg(rd, rm);
        } else {
            let enc = if shift == 32 { 0 } else { shift & 0x1F };
            self.emit16(0x0800 | ((enc as u16) << 6) | ((rm as u16) << 3) | (rd as u16));
        }
    }

    #[inline]
    pub fn emit_asrs_imm(&mut self, rd: u8, rm: u8, shift: u8) {
        debug_assert!(rd < 8 && rm < 8 && shift <= 32);
        if shift == 0 {
            self.emit_movs_reg(rd, rm);
        } else {
            let enc = if shift == 32 { 0 } else { shift & 0x1F };
            self.emit16(0x1000 | ((enc as u16) << 6) | ((rm as u16) << 3) | (rd as u16));
        }
    }

    #[inline]
    pub fn emit_lsls_reg(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4080 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_lsrs_reg(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x40C0 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_asrs_reg(&mut self, rdn: u8, rm: u8) {
        debug_assert!(rdn < 8 && rm < 8);
        self.emit16(0x4100 | ((rm as u16) << 3) | (rdn as u16));
    }

    #[inline]
    pub fn emit_cmp_imm8(&mut self, rn: u8, imm8: u8) {
        debug_assert!(rn < 8);
        self.emit16(0x2800 | ((rn as u16) << 8) | (imm8 as u16));
    }

    #[inline]
    pub fn emit_cmp_reg(&mut self, rn: u8, rm: u8) {
        if rn < 8 && rm < 8 {
            self.emit16(0x4280 | ((rm as u16) << 3) | (rn as u16));
        } else {
            let n_hi = ((rn as u16) >> 3) & 1;
            self.emit16(0x4500 | (n_hi << 7) | (((rm as u16) & 0xF) << 3) | ((rn as u16) & 7));
        }
    }

    #[inline]
    pub fn emit_tst_reg(&mut self, rn: u8, rm: u8) {
        debug_assert!(rn < 8 && rm < 8);
        self.emit16(0x4200 | ((rm as u16) << 3) | (rn as u16));
    }

    #[inline]
    pub fn emit_uxtb(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        self.emit16(0xB2C0 | ((rm as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_sxtb(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        self.emit16(0xB240 | ((rm as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_uxth(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        self.emit16(0xB280 | ((rm as u16) << 3) | (rd as u16));
    }

    #[inline]
    pub fn emit_sxth(&mut self, rd: u8, rm: u8) {
        debug_assert!(rd < 8 && rm < 8);
        self.emit16(0xB200 | ((rm as u16) << 3) | (rd as u16));
    }

    // --- Memory Load / Store ---

    #[inline]
    pub fn emit_ldr_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off % 4 == 0 && byte_off / 4 < 32);
        let imm5 = (byte_off / 4) as u16;
        self.emit16(0x6800 | (imm5 << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_str_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off % 4 == 0 && byte_off / 4 < 32);
        let imm5 = (byte_off / 4) as u16;
        self.emit16(0x6000 | (imm5 << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_ldr_reg(&mut self, rt: u8, rn: u8, rm: u8) {
        debug_assert!(rt < 8 && rn < 8 && rm < 8);
        self.emit16(0x5800 | ((rm as u16) << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_str_reg(&mut self, rt: u8, rn: u8, rm: u8) {
        debug_assert!(rt < 8 && rn < 8 && rm < 8);
        self.emit16(0x5000 | ((rm as u16) << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_ldrb_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off < 32);
        self.emit16(0x7800 | ((byte_off as u16) << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_strb_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off < 32);
        self.emit16(0x7000 | ((byte_off as u16) << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_ldrh_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off % 2 == 0 && byte_off / 2 < 32);
        let imm5 = (byte_off / 2) as u16;
        self.emit16(0x8800 | (imm5 << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    #[inline]
    pub fn emit_strh_imm(&mut self, rt: u8, rn: u8, byte_off: u32) {
        debug_assert!(rt < 8 && rn < 8 && byte_off % 2 == 0 && byte_off / 2 < 32);
        let imm5 = (byte_off / 2) as u16;
        self.emit16(0x8000 | (imm5 << 6) | ((rn as u16) << 3) | (rt as u16));
    }

    /// Load 32-bit word `[sp + byte_off]` into `rt` (`r0..r7`), handling arbitrarily large stack frames
    /// using `r7` as a temporary frame address scratch if `byte_off > 1020`.
    pub fn emit_ldr_sp(&mut self, rt: u8, byte_off: u32) {
        debug_assert!(rt < 8 && byte_off % 4 == 0);
        if byte_off <= 1020 {
            let imm8 = (byte_off / 4) as u16;
            self.emit16(0x9800 | ((rt as u16) << 8) | imm8);
        } else {
            let scratch = if rt == 7 { 6 } else { 7 };
            self.emit_load_u32(scratch, byte_off);
            // `ADD scratch, SP` (`0x4468 | scratch` -> `ADD Rdn, Rm` where Rm=13(SP))
            self.emit16(0x4400 | (13u16 << 3) | (scratch as u16));
            self.emit_ldr_imm(rt, scratch, 0);
        }
    }

    /// Store 32-bit word `rt` (`r0..r7`) to `[sp + byte_off]`, handling arbitrarily large stack frames.
    pub fn emit_str_sp(&mut self, rt: u8, byte_off: u32) {
        debug_assert!(rt < 8 && byte_off % 4 == 0);
        if byte_off <= 1020 {
            let imm8 = (byte_off / 4) as u16;
            self.emit16(0x9000 | ((rt as u16) << 8) | imm8);
        } else {
            let scratch = if rt == 7 { 6 } else { 7 };
            self.emit_load_u32(scratch, byte_off);
            self.emit16(0x4400 | (13u16 << 3) | (scratch as u16));
            self.emit_str_imm(rt, scratch, 0);
        }
    }

    /// Compute `rd = sp + byte_off` (`r0..r7`).
    pub fn emit_add_reg_sp(&mut self, rd: u8, byte_off: u32) {
        debug_assert!(rd < 8);
        if byte_off % 4 == 0 && byte_off <= 1020 {
            let imm8 = (byte_off / 4) as u16;
            self.emit16(0xA800 | ((rd as u16) << 8) | imm8);
        } else {
            self.emit_load_u32(rd, byte_off);
            self.emit16(0x4400 | (13u16 << 3) | (rd as u16));
        }
    }

    pub fn emit_sub_sp(&mut self, mut bytes: u32) {
        debug_assert!(bytes % 4 == 0);
        while bytes > 0 {
            let chunk = bytes.min(508);
            let imm7 = (chunk / 4) as u16;
            self.emit16(0xB080 | imm7);
            bytes -= chunk;
        }
    }

    pub fn emit_add_sp(&mut self, mut bytes: u32) {
        debug_assert!(bytes % 4 == 0);
        while bytes > 0 {
            let chunk = bytes.min(508);
            let imm7 = (chunk / 4) as u16;
            self.emit16(0xB000 | imm7);
            bytes -= chunk;
        }
    }

    #[inline]
    pub fn emit_push(&mut self, low_mask: u8, push_lr: bool) {
        self.emit16(0xB400 | (if push_lr { 0x0100 } else { 0 }) | (low_mask as u16));
    }

    #[inline]
    pub fn emit_pop(&mut self, low_mask: u8, pop_pc: bool) {
        self.emit16(0xBC00 | (if pop_pc { 0x0100 } else { 0 }) | (low_mask as u16));
    }

    #[inline]
    pub fn emit_bx(&mut self, rm: u8) {
        self.emit16(0x4700 | (((rm as u16) & 0xF) << 3));
    }

    #[inline]
    pub fn emit_blx_reg(&mut self, rm: u8) {
        self.emit16(0x4780 | (((rm as u16) & 0xF) << 3));
    }

    /// Emit 32-bit `BL symbol` (supported on BOTH ARMv6-M RP2040 and ARMv8-M.main RP2350!).
    pub fn emit_bl_sym(&mut self, sym: &str) {
        self.maybe_emit_literal_island();
        let off = self.pos();
        self.emit_thumb32(0xF000, 0xF800);
        self.global_relocs.push(GlobalReloc {
            offset: off,
            symbol: sym.to_string(),
            kind: GlobalRelocKind::Bl32,
        });
    }

    /// Emit unconditional branch to `label` that NEVER overflows even on huge multi-KB functions!
    /// - On RP2350 (`has_thumb2`): 4-byte `B.W` (+/- 16 MB, zero register clobber).
    /// - On RP2040 (`!has_thumb2`): if target is already bound and within -2040 bytes, 2-byte `B`;
    ///   otherwise uses `r7` scratch `LDR r7, [pc, #lit]; BX r7` if far, or short `B` when small.
    pub fn emit_b_label_short(&mut self, label: usize) {
        let off = self.pos();
        self.emit16(0xE000);
        self.local_branches.push(LocalBranchFixup {
            instr_off: off,
            target_label: label,
            kind: LocalBranchKind::Thumb1B11,
        });
    }

    pub fn emit_bcond_label_short(&mut self, cond: Cond, label: usize) {
        let off = self.pos();
        self.emit16(0xD000 | ((cond as u16) << 8));
        self.local_branches.push(LocalBranchFixup {
            instr_off: off,
            target_label: label,
            kind: LocalBranchKind::Thumb1BCond(cond),
        });
    }

    /// Range-unlimited intra-function unconditional jump (safe at basic-block boundaries where `r7` is scratch).
    pub fn emit_jump_bb(&mut self, label: usize) {
        if self.has_thumb2 {
            let off = self.pos();
            self.emit_thumb32(0xF000, 0x9000);
            self.local_branches.push(LocalBranchFixup {
                instr_off: off,
                target_label: label,
                kind: LocalBranchKind::Thumb2BW,
            });
        } else {
            // If backward target is already known and within Thumb-1 `B` range (-2040 B), emit compact 2-byte `B`!
            if let Some(Some(t_off)) = self.local_labels.get(label) {
                let rel = (*t_off as isize) - (self.pos() as isize + 4);
                if rel >= -2040 && rel <= 2040 {
                    self.emit_b_label_short(label);
                    return;
                }
            }
            // Arbitrary-distance ARMv6-M basic-block jump via auto-island literal pool (`r7` scratch)
            self.maybe_emit_literal_island();
            let ldr_off = self.pos();
            self.emit16(0x4F00); // `ldr r7, [pc, #imm8*4]`
            self.pending_lits.push(PendingLit {
                ldr_off,
                rd: 7,
                val: LitValue::LocalLabelThumbAddr(label),
            });
            self.emit_bx(7);
        }
    }

    /// Range-unlimited intra-function conditional jump at basic-block boundaries.
    pub fn emit_cond_jump_bb(&mut self, cond: Cond, label: usize) {
        let skip = self.new_label();
        self.emit_bcond_label_short(cond.invert(), skip);
        self.emit_jump_bb(label);
        self.bind_label(skip);
    }

    // --- Hardware 32-bit Divide on ARMv8-M.main (`RP2350`) ---

    /// `SDIV Rd, Rn, Rm` (Thumb-2 T1: `0xFB90 | Rn`, `0xF0F0 | (Rd << 8) | Rm`)
    pub fn emit_thumb2_sdiv(&mut self, rd: u8, rn: u8, rm: u8) {
        debug_assert!(self.has_thumb2);
        self.emit_thumb32(
            0xFB90 | ((rn as u16) & 0xF),
            0xF0F0 | (((rd as u16) & 0xF) << 8) | ((rm as u16) & 0xF),
        );
    }

    /// `UDIV Rd, Rn, Rm` (Thumb-2 T1: `0xFBB0 | Rn`, `0xF0F0 | (Rd << 8) | Rm`)
    pub fn emit_thumb2_udiv(&mut self, rd: u8, rn: u8, rm: u8) {
        debug_assert!(self.has_thumb2);
        self.emit_thumb32(
            0xFBB0 | ((rn as u16) & 0xF),
            0xF0F0 | (((rd as u16) & 0xF) << 8) | ((rm as u16) & 0xF),
        );
    }
}

pub fn encode_movw(rd: u8, imm16: u16) -> (u16, u16) {
    let imm4 = (imm16 >> 12) & 0xF;
    let i = (imm16 >> 11) & 1;
    let imm3 = (imm16 >> 8) & 7;
    let imm8 = imm16 & 0xFF;
    let hw1 = 0xF240 | (i << 10) | imm4;
    let hw2 = (imm3 << 12) | (((rd as u16) & 0xF) << 8) | imm8;
    (hw1, hw2)
}

pub fn encode_movt(rd: u8, imm16: u16) -> (u16, u16) {
    let imm4 = (imm16 >> 12) & 0xF;
    let i = (imm16 >> 11) & 1;
    let imm3 = (imm16 >> 8) & 7;
    let imm8 = imm16 & 0xFF;
    let hw1 = 0xF2C0 | (i << 10) | imm4;
    let hw2 = (imm3 << 12) | (((rd as u16) & 0xF) << 8) | imm8;
    (hw1, hw2)
}

pub fn encode_bl_rel(rel: i32) -> Result<(u16, u16), String> {
    if (rel & 1) != 0 || !(-16_777_216..=16_777_214).contains(&rel) {
        return Err(format!("Thumb BL relocation out of +/-16MB range: {}", rel));
    }
    let s = ((rel >> 24) & 1) as u16;
    let i1 = ((rel >> 23) & 1) as u16;
    let i2 = ((rel >> 22) & 1) as u16;
    let j1 = (!(i1 ^ s)) & 1;
    let j2 = (!(i2 ^ s)) & 1;
    let imm10 = ((rel >> 12) & 0x3FF) as u16;
    let imm11 = ((rel >> 1) & 0x7FF) as u16;
    let hw1 = 0xF000 | (s << 10) | imm10;
    let hw2 = 0xD000 | (j1 << 13) | (j2 << 11) | imm11;
    Ok((hw1, hw2))
}

pub fn encode_thumb2_bw(rel: i32) -> Result<(u16, u16), String> {
    if (rel & 1) != 0 || !(-16_777_216..=16_777_214).contains(&rel) {
        return Err(format!("Thumb-2 B.W relocation out of +/-16MB range: {}", rel));
    }
    let s = ((rel >> 24) & 1) as u16;
    let i1 = ((rel >> 23) & 1) as u16;
    let i2 = ((rel >> 22) & 1) as u16;
    let j1 = (!(i1 ^ s)) & 1;
    let j2 = (!(i2 ^ s)) & 1;
    let imm10 = ((rel >> 12) & 0x3FF) as u16;
    let imm11 = ((rel >> 1) & 0x7FF) as u16;
    let hw1 = 0xF000 | (s << 10) | imm10;
    let hw2 = 0x9000 | (j1 << 13) | (j2 << 11) | imm11;
    Ok((hw1, hw2))
}
