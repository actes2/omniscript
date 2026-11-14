use std::collections::{HashMap, HashSet};

use super::chip::RpChipProfile;
use super::runtime_fw::{
    emit_bare_metal_runtime, emit_boot_header, SramDataInit, SRAM_USER_GLOBALS_OFF,
};
use super::thumb_enc::{Cond, ThumbAssembler};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerType {
    Void,
    Int(u32), // bit width: 1, 8, 16, 32, 64
    Float32,
    Float64,
    Ptr(Box<LowerType>),
    Array(usize, Box<LowerType>),
    Struct(String), // `%struct.Foo` or `%enum.Bar` (without leading `%`)
    AnonStruct(Vec<LowerType>),
}

impl LowerType {
    pub fn is_64bit_scalar(&self) -> bool {
        matches!(self, LowerType::Int(64) | LowerType::Float64)
    }

    pub fn byte_size(&self, structs: &HashMap<String, Vec<LowerType>>) -> u32 {
        match self {
            LowerType::Void => 0,
            LowerType::Int(0..=8) => 1,
            LowerType::Int(9..=16) => 2,
            LowerType::Int(17..=32) | LowerType::Float32 | LowerType::Ptr(_) => 4,
            LowerType::Int(_) | LowerType::Float64 => 8,
            LowerType::Array(n, elem) => (*n as u32) * elem.byte_size(structs),
            LowerType::Struct(name) => {
                if let Some(fields) = structs.get(name) {
                    struct_layout(fields, structs).0
                } else {
                    8
                }
            }
            LowerType::AnonStruct(fields) => struct_layout(fields, structs).0,
        }
    }

    pub fn byte_align(&self, structs: &HashMap<String, Vec<LowerType>>) -> u32 {
        match self {
            LowerType::Void | LowerType::Int(0..=8) => 1,
            LowerType::Int(9..=16) => 2,
            LowerType::Int(17..=32) | LowerType::Float32 | LowerType::Ptr(_) => 4,
            LowerType::Int(_) | LowerType::Float64 => 8,
            LowerType::Array(_, elem) => elem.byte_align(structs),
            LowerType::Struct(name) => {
                if let Some(fields) = structs.get(name) {
                    struct_layout(fields, structs).1
                } else {
                    8
                }
            }
            LowerType::AnonStruct(fields) => struct_layout(fields, structs).1,
        }
    }
}

/// Returns `(total_size, max_align, field_offsets)` for a struct definition on 32-bit AAPCS32.
pub fn struct_layout_offsets(
    fields: &[LowerType],
    structs: &HashMap<String, Vec<LowerType>>,
) -> (u32, u32, Vec<u32>) {
    let mut off = 0u32;
    let mut max_align = 1u32;
    let mut offsets = Vec::with_capacity(fields.len());
    for f in fields {
        let a = f.byte_align(structs).max(1);
        let sz = f.byte_size(structs);
        max_align = max_align.max(a);
        off = (off + (a - 1)) & !(a - 1);
        offsets.push(off);
        off += sz;
    }
    let total = (off + (max_align - 1)) & !(max_align - 1);
    (total.max(1), max_align, offsets)
}

fn struct_layout(fields: &[LowerType], structs: &HashMap<String, Vec<LowerType>>) -> (u32, u32) {
    let (sz, al, _) = struct_layout_offsets(fields, structs);
    (sz, al)
}

#[derive(Debug, Clone)]
struct GlobalConst {
    name: String,
    is_constant: bool,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct IrParam {
    ty: LowerType,
    name: String, // e.g. `%a0`
}

#[derive(Debug, Clone)]
struct PhiIncoming {
    val: String,
    pred_label: String,
}

#[derive(Debug, Clone)]
struct PhiNode {
    dst: String,
    ty: LowerType,
    incomings: Vec<PhiIncoming>,
}

#[derive(Debug, Clone)]
struct IrBasicBlock {
    label: String,
    phis: Vec<PhiNode>,
    insts: Vec<String>,
}

#[derive(Debug, Clone)]
struct IrFunction {
    name: String,
    #[allow(dead_code)]
    ret_ty: LowerType,
    params: Vec<IrParam>,
    blocks: Vec<IrBasicBlock>,
}

#[derive(Debug, Default)]
struct ParsedIrModule {
    structs: HashMap<String, Vec<LowerType>>,
    globals: Vec<GlobalConst>,
    declared_syms: HashSet<String>,
    functions: Vec<IrFunction>,
}

/// Pure-Rust SSA syntax & CFG validator for Cortex-M targets (zero `clang` dependency!).
pub fn validate_ir_for_cortex_m(ir: &str) -> Result<(), (usize, String)> {
    let parsed = parse_llvm_ir_module(ir)?;
    for f in &parsed.functions {
        if f.blocks.is_empty() {
            return Err((1, format!("function @{} has no basic blocks", f.name)));
        }
        let block_names: HashSet<&str> = f.blocks.iter().map(|b| b.label.as_str()).collect();
        for b in &f.blocks {
            let last = b.insts.last().map(|s| s.trim()).unwrap_or("");
            let is_term = last.starts_with("ret ")
                || last == "ret void"
                || last.starts_with("br ")
                || last.starts_with("switch ")
                || last == "unreachable";
            if !is_term {
                return Err((
                    1,
                    format!(
                        "basic block '{}' in @{} does not end with a terminator (last='{}')",
                        b.label, f.name, last
                    ),
                ));
            }
            if let Some(rest) = last.strip_prefix("br ") {
                for part in split_top_commas(rest) {
                    if let Some(lbl) = part.trim().strip_prefix("label %") {
                        let clean = lbl.trim();
                        if !block_names.contains(clean) {
                            return Err((
                                1,
                                format!(
                                    "branch in @{} references undefined basic block '%{}'",
                                    f.name, clean
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Lower a complete LLVM IR module + freestanding bare-metal runtime into a linked Flash image
/// (`0x1000_0000` XIP VMA) for `RP2040` (`ARMv6-M`) or `RP2350` (`ARMv8-M.main`).
pub fn compile_llvm_ir_to_flash_image(
    ir: &str,
    chip: &RpChipProfile,
) -> Result<(Vec<u8>, ThumbAssembler), String> {
    let module = parse_llvm_ir_module(ir).map_err(|(ln, msg)| format!("line {}: {}", ln, msg))?;

    let mut asm = ThumbAssembler::new(chip.has_thumb2);

    // 1. Emit RP2040 `boot2`+VTOR or RP2350 VTOR+`PICOBIN` header
    emit_boot_header(&mut asm, chip);

    // Partition globals into read-only Flash `.rodata` vs mutable SRAM `.data`
    let mut sram_globals: HashMap<String, u32> = HashMap::new();
    let mut sram_init_blob: Vec<u8> = Vec::new();
    let mut rodata_globals: Vec<&GlobalConst> = Vec::new();

    for g in &module.globals {
        if g.is_constant {
            rodata_globals.push(g);
        } else {
            while (sram_init_blob.len() & 7) != 0 {
                sram_init_blob.push(0);
            }
            let addr = chip.sram_base + SRAM_USER_GLOBALS_OFF + (sram_init_blob.len() as u32);
            sram_globals.insert(g.name.clone(), addr);
            sram_init_blob.extend_from_slice(&g.bytes);
            if g.bytes.is_empty() {
                sram_init_blob.extend_from_slice(&[0u8; 8]);
            }
        }
    }

    let data_init = if !sram_init_blob.is_empty() {
        Some(SramDataInit {
            flash_lma_sym: "__data_flash_lma".to_string(),
            sram_vma: chip.sram_base + SRAM_USER_GLOBALS_OFF,
            size_bytes: sram_init_blob.len() as u32,
        })
    } else {
        None
    };

    // 2. Emit `Reset_Handler` and freestanding Cortex-M runtime
    emit_bare_metal_runtime(&mut asm, chip, data_init.as_ref())?;

    // Collect set of all function names (both runtime + user IR functions) so `@fn` knows Thumb bit `| 1`
    let mut all_fn_names: HashSet<String> = asm.symbols.keys().cloned().collect();
    for f in &module.functions {
        all_fn_names.insert(f.name.clone());
    }

    // 3. Lower every `define` function in the LLVM IR module!
    for func in &module.functions {
        // Skip if a `define weak` collides with an already-defined user or core runtime symbol
        if asm.symbols.contains_key(&func.name) {
            continue;
        }
        lower_function(
            &mut asm,
            chip,
            func,
            &module.structs,
            &sram_globals,
            &all_fn_names,
        )?;
    }

    // 4. Emit any unresolved `declare` external functions as safe zero-returning stubs so custom prototypes never fail link
    let referenced_syms: Vec<String> = asm
        .global_relocs
        .iter()
        .map(|r| r.symbol.clone())
        .collect();
    for sym in referenced_syms {
        if !asm.symbols.contains_key(&sym)
            && sym != "__data_flash_lma"
            && !rodata_globals.iter().any(|g| g.name == sym)
        {
            asm.begin_function(&sym);
            asm.emit_movs_imm8(0, 0);
            asm.emit_movs_imm8(1, 0);
            asm.emit_bx(14);
            asm.end_function(chip.flash_base)?;
        }
    }

    // 5. Append Flash `.rodata` string literals & `.data` initialization image
    asm.align4_zero();
    for g in rodata_globals {
        asm.align4_zero();
        asm.symbols.insert(g.name.clone(), asm.pos());
        asm.code.extend_from_slice(&g.bytes);
        if g.bytes.is_empty() {
            asm.code.push(0);
        }
    }

    if !sram_init_blob.is_empty() {
        asm.align4_zero();
        asm.symbols
            .insert("__data_flash_lma".to_string(), asm.pos());
        asm.code.extend_from_slice(&sram_init_blob);
    }
    asm.align4_zero();

    // 6. Patch all `BL`, `MOVW`/`MOVT`, and literal-pool / Vector-Table `Abs32` relocations!
    asm.link_global_relocs(chip.flash_base)?;

    let bytes = asm.code.clone();
    Ok((bytes, asm))
}

// =============================================================================
// Function Lowering Engine (LLVM SSA -> ARMv6-M / ARMv8-M.main Thumb)
// =============================================================================

struct FnLowerCtx<'a> {
    chip: &'a RpChipProfile,
    structs: &'a HashMap<String, Vec<LowerType>>,
    sram_globals: &'a HashMap<String, u32>,
    all_fn_names: &'a HashSet<String>,
    ssa_slots: HashMap<String, u32>,      // `%vreg` -> `[sp + off]` (8-byte slot)
    alloca_storage: HashMap<String, u32>, // `%ptr` -> `[sp + data_off]`
    phi_tmp_base: u32,
    frame_bytes: u32, // 8-byte aligned (`(20 + frame_bytes) % 8 == 0`)
    bb_labels: HashMap<String, usize>,
    blocks_by_name: HashMap<String, IrBasicBlock>,
}

impl<'a> FnLowerCtx<'a> {
    fn get_slot(&self, reg: &str) -> Option<u32> {
        self.ssa_slots.get(reg).copied()
    }

    /// Load low 32-bit value of SSA operand (`%reg`, `@global`, literal `42`, `true`, `false`, `null`, `0x...`) into `rd`.
    fn load_op32(&self, asm: &mut ThumbAssembler, op: &str, rd: u8) -> Result<(), String> {
        let t = op.trim();
        if let Some(&data_off) = self.alloca_storage.get(t) {
            asm.emit_add_reg_sp(rd, data_off);
            return Ok(());
        }
        if let Some(&slot) = self.ssa_slots.get(t) {
            asm.emit_ldr_sp(rd, slot);
            return Ok(());
        }
        if let Some(sym) = t.strip_prefix('@') {
            if let Some(&sram_addr) = self.sram_globals.get(sym) {
                asm.emit_load_u32(rd, sram_addr);
            } else {
                let is_fn = self.all_fn_names.contains(sym);
                asm.emit_load_sym_addr(rd, sym, is_fn, 0);
            }
            return Ok(());
        }
        if let Some(inline_gep) = t.strip_prefix("getelementptr") {
            return self.eval_inline_const_gep(asm, inline_gep, rd);
        }
        if let Some(inner) = strip_const_cast_wrapper(t) {
            return self.load_op32(asm, inner, rd);
        }
        let val64 = parse_ir_const64(t)?;
        asm.emit_load_u32(rd, val64 as u32);
        Ok(())
    }

    /// Load full 64-bit value of operand into `(rd_lo, rd_hi)` respecting `ty` sign/zero extension if narrower.
    fn load_op64(
        &self,
        asm: &mut ThumbAssembler,
        op: &str,
        ty: &LowerType,
        rd_lo: u8,
        rd_hi: u8,
    ) -> Result<(), String> {
        let t = op.trim();
        if let Some(&data_off) = self.alloca_storage.get(t) {
            asm.emit_add_reg_sp(rd_lo, data_off);
            asm.emit_movs_imm8(rd_hi, 0);
            return Ok(());
        }
        if let Some(&slot) = self.ssa_slots.get(t) {
            asm.emit_ldr_sp(rd_lo, slot);
            if ty.is_64bit_scalar() {
                asm.emit_ldr_sp(rd_hi, slot + 4);
            } else {
                asm.emit_movs_imm8(rd_hi, 0);
            }
            return Ok(());
        }
        if t.starts_with('@') || t.starts_with("getelementptr") || strip_const_cast_wrapper(t).is_some() {
            self.load_op32(asm, t, rd_lo)?;
            asm.emit_movs_imm8(rd_hi, 0);
            return Ok(());
        }
        let val64 = parse_ir_const64(t)?;
        asm.emit_load_u32(rd_lo, (val64 & 0xFFFF_FFFF) as u32);
        asm.emit_load_u32(rd_hi, ((val64 as u64) >> 32) as u32);
        Ok(())
    }

    fn store_ssa32(&self, asm: &mut ThumbAssembler, dst: &str, rs: u8) {
        if let Some(&slot) = self.ssa_slots.get(dst) {
            asm.emit_str_sp(rs, slot);
            let zero_reg = if rs == 3 { 2 } else { 3 };
            asm.emit_movs_imm8(zero_reg, 0);
            asm.emit_str_sp(zero_reg, slot + 4);
        }
    }

    fn store_ssa64(&self, asm: &mut ThumbAssembler, dst: &str, rs_lo: u8, rs_hi: u8) {
        if let Some(&slot) = self.ssa_slots.get(dst) {
            asm.emit_str_sp(rs_lo, slot);
            asm.emit_str_sp(rs_hi, slot + 4);
        }
    }

    fn eval_inline_const_gep(
        &self,
        asm: &mut ThumbAssembler,
        rest: &str,
        rd: u8,
    ) -> Result<(), String> {
        // e.g. ` inbounds ([14 x i8], [14 x i8]* @.str.0, i64 0, i64 0)`
        let trimmed = rest.trim().trim_start_matches("inbounds").trim();
        let inner = trimmed
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(trimmed);
        let parts = split_top_commas(inner);
        if parts.len() < 2 {
            return Err(format!("malformed inline GEP: '{}'", rest));
        }
        let (_, ptr_op) = split_type_and_val(&parts[1])?;
        self.load_op32(asm, ptr_op, rd)?;
        // Constant zero indices add 0; handle non-zero byte offset if present
        let mut cur_ty = parse_ir_type(parts[0].trim());
        let mut const_add: i64 = 0;
        for (i, idx_part) in parts[2..].iter().enumerate() {
            let (_, idx_val_s) = split_type_and_val(idx_part)?;
            let idx = parse_ir_const64(idx_val_s).unwrap_or(0);
            if i == 0 {
                const_add += idx * (cur_ty.byte_size(self.structs) as i64);
            } else {
                match cur_ty.clone() {
                    LowerType::Array(_, elem) => {
                        const_add += idx * (elem.byte_size(self.structs) as i64);
                        cur_ty = *elem;
                    }
                    LowerType::Struct(sname) => {
                        if let Some(flds) = self.structs.get(&sname) {
                            let (_, _, offs) = struct_layout_offsets(flds, self.structs);
                            let fi = (idx.max(0) as usize).min(flds.len().saturating_sub(1));
                            if let Some(&fo) = offs.get(fi) {
                                const_add += fo as i64;
                                cur_ty = flds[fi].clone();
                            }
                        }
                    }
                    _ => {
                        const_add += idx * (cur_ty.byte_size(self.structs) as i64);
                    }
                }
            }
        }
        if const_add != 0 {
            let scratch = if rd == 2 { 3 } else { 2 };
            asm.emit_load_u32(scratch, const_add as u32);
            asm.emit_adds_reg(rd, rd, scratch);
        }
        Ok(())
    }
}

fn lower_function(
    asm: &mut ThumbAssembler,
    chip: &RpChipProfile,
    func: &IrFunction,
    structs: &HashMap<String, Vec<LowerType>>,
    sram_globals: &HashMap<String, u32>,
    all_fn_names: &HashSet<String>,
) -> Result<(), String> {
    // 1. Pre-scan stack layout:
    //    `[sp + 0 .. max_out_args)` = outgoing call stack area
    //    `[sp + max_out_args .. )`  = `alloca` inline buffers + SSA 8-byte slots + phi parallel-copy scratch
    let mut max_out_words: u32 = 8; // minimum 32 bytes outgoing call arg space
    let mut ssa_names: Vec<String> = Vec::new();
    let mut ssa_seen: HashSet<String> = HashSet::new();
    let mut alloca_sizes: Vec<(String, u32)> = Vec::new();
    let mut max_phis_in_block: usize = 0;

    for p in &func.params {
        if ssa_seen.insert(p.name.clone()) {
            ssa_names.push(p.name.clone());
        }
    }

    for b in &func.blocks {
        max_phis_in_block = max_phis_in_block.max(b.phis.len());
        for phi in &b.phis {
            if ssa_seen.insert(phi.dst.clone()) {
                ssa_names.push(phi.dst.clone());
            }
        }
        for line in &b.insts {
            if let Some((lhs, rhs)) = split_ssa_assign(line) {
                if let Some(alloca_rest) = rhs.strip_prefix("alloca ") {
                    let sz = compute_alloca_bytes(alloca_rest, structs);
                    alloca_sizes.push((lhs.to_string(), (sz + 7) & !7));
                } else if ssa_seen.insert(lhs.to_string()) {
                    ssa_names.push(lhs.to_string());
                }
                if rhs.contains("call ") {
                    max_out_words = max_out_words.max(estimate_call_stack_words(rhs));
                }
            } else if line.contains("call ") {
                max_out_words = max_out_words.max(estimate_call_stack_words(line));
            }
        }
    }

    let mut cur_off = ((max_out_words * 4) + 7) & !7;
    let mut alloca_storage = HashMap::new();
    for (aname, asz) in alloca_sizes {
        alloca_storage.insert(aname, cur_off);
        cur_off += asz.max(8);
    }

    let mut ssa_slots = HashMap::new();
    for sname in ssa_names {
        if !alloca_storage.contains_key(&sname) {
            ssa_slots.insert(sname, cur_off);
            cur_off += 8;
        }
    }

    let phi_tmp_base = cur_off;
    cur_off += (max_phis_in_block as u32) * 8;

    // `push {r4, r5, r6, r7, lr}` pushes 20 bytes (`4 mod 8`).
    // Choose `frame_bytes >= cur_off` with `frame_bytes % 8 == 4` so `(20 + frame_bytes) % 8 == 0`!
    let mut frame_bytes = (cur_off + 3) & !3;
    if (20 + frame_bytes) % 8 != 0 {
        frame_bytes += 4;
    }

    asm.begin_function(&func.name);
    let mut bb_labels = HashMap::new();
    let mut blocks_by_name = HashMap::new();
    for b in &func.blocks {
        bb_labels.insert(b.label.clone(), asm.new_label());
        blocks_by_name.insert(b.label.clone(), b.clone());
    }

    let ctx = FnLowerCtx {
        chip,
        structs,
        sram_globals,
        all_fn_names,
        ssa_slots,
        alloca_storage,
        phi_tmp_base,
        frame_bytes,
        bb_labels,
        blocks_by_name,
    };

    // Prologue: save `r4..r7, lr`, allocate frame, spill AAPCS32 incoming arguments into SSA slots!
    asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    if ctx.frame_bytes > 0 {
        asm.emit_sub_sp(ctx.frame_bytes);
    }

    // Spill incoming AAPCS32 parameters (`r0..r3` + `[caller_sp + ...]`)
    let caller_sp_off = 20 + ctx.frame_bytes;
    let mut word_idx: u32 = 0;
    // Note: save r0..r3 into SSA slots before clobbering any of r0..r3!
    // Since `emit_str_sp` only touches `r6`/`r7` if `slot > 1020`, `r0..r3` remain completely intact!
    for p in &func.params {
        if p.ty.is_64bit_scalar() {
            if (word_idx & 1) != 0 {
                word_idx += 1;
            }
            if let Some(slot) = ctx.get_slot(&p.name) {
                if word_idx < 4 {
                    asm.emit_str_sp(word_idx as u8, slot);
                    asm.emit_str_sp((word_idx + 1) as u8, slot + 4);
                } else {
                    let stk = caller_sp_off + (word_idx - 4) * 4;
                    asm.emit_ldr_sp(4, stk);
                    asm.emit_ldr_sp(5, stk + 4);
                    asm.emit_str_sp(4, slot);
                    asm.emit_str_sp(5, slot + 4);
                }
            }
            word_idx += 2;
        } else {
            if let Some(slot) = ctx.get_slot(&p.name) {
                if word_idx < 4 {
                    asm.emit_str_sp(word_idx as u8, slot);
                } else {
                    let stk = caller_sp_off + (word_idx - 4) * 4;
                    asm.emit_ldr_sp(4, stk);
                    asm.emit_str_sp(4, slot);
                }
                asm.emit_movs_imm8(5, 0);
                asm.emit_str_sp(5, slot + 4);
            }
            word_idx += 1;
        }
    }

    // Lower each basic block in order
    for b in &func.blocks {
        let lbl = ctx.bb_labels[&b.label];
        asm.bind_label(lbl);
        for inst in &b.insts {
            lower_instruction(asm, &ctx, &b.label, inst)?;
        }
    }

    asm.end_function(chip.flash_base)
}

fn emit_phi_moves_for_edge(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    from_bb: &str,
    to_bb: &str,
) -> Result<(), String> {
    let Some(target_block) = ctx.blocks_by_name.get(to_bb) else {
        return Ok(());
    };
    if target_block.phis.is_empty() {
        return Ok(());
    }

    let mut copies: Vec<(&PhiNode, &str)> = Vec::new();
    for phi in &target_block.phis {
        if let Some(inc) = phi.incomings.iter().find(|i| i.pred_label == from_bb) {
            copies.push((phi, inc.val.as_str()));
        }
    }
    if copies.is_empty() {
        return Ok(());
    }

    if copies.len() == 1 {
        let (phi, val) = copies[0];
        if phi.ty.is_64bit_scalar() {
            ctx.load_op64(asm, val, &phi.ty, 0, 1)?;
            ctx.store_ssa64(asm, &phi.dst, 0, 1);
        } else {
            ctx.load_op32(asm, val, 0)?;
            ctx.store_ssa32(asm, &phi.dst, 0);
        }
        return Ok(());
    }

    // Parallel-copy via `phi_tmp_base` scratch slots
    for (idx, (phi, val)) in copies.iter().enumerate() {
        let tmp_off = ctx.phi_tmp_base + (idx as u32) * 8;
        if phi.ty.is_64bit_scalar() {
            ctx.load_op64(asm, val, &phi.ty, 0, 1)?;
            asm.emit_str_sp(0, tmp_off);
            asm.emit_str_sp(1, tmp_off + 4);
        } else {
            ctx.load_op32(asm, val, 0)?;
            asm.emit_str_sp(0, tmp_off);
        }
    }
    for (idx, (phi, _)) in copies.iter().enumerate() {
        let tmp_off = ctx.phi_tmp_base + (idx as u32) * 8;
        if phi.ty.is_64bit_scalar() {
            asm.emit_ldr_sp(0, tmp_off);
            asm.emit_ldr_sp(1, tmp_off + 4);
            ctx.store_ssa64(asm, &phi.dst, 0, 1);
        } else {
            asm.emit_ldr_sp(0, tmp_off);
            ctx.store_ssa32(asm, &phi.dst, 0);
        }
    }
    Ok(())
}

fn lower_instruction(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    cur_bb: &str,
    raw_inst: &str,
) -> Result<(), String> {
    let inst = strip_trailing_ir_metadata(raw_inst.trim());
    if inst.is_empty() {
        return Ok(());
    }

    // --- Terminators & Void Instructions ---
    if inst == "ret void" {
        if ctx.frame_bytes > 0 {
            asm.emit_add_sp(ctx.frame_bytes);
        }
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        return Ok(());
    }
    if let Some(rest) = inst.strip_prefix("ret ") {
        let (ty, val) = split_type_and_val(rest)?;
        if matches!(ty, LowerType::Void) {
            if ctx.frame_bytes > 0 {
                asm.emit_add_sp(ctx.frame_bytes);
            }
            asm.emit_pop((1 << 4) | (1 << 5) | (1 << 7) | (1 << 6), true);
            return Ok(());
        }
        if ty.is_64bit_scalar() {
            ctx.load_op64(asm, val, &ty, 0, 1)?;
        } else {
            ctx.load_op32(asm, val, 0)?;
            asm.emit_movs_imm8(1, 0);
        }
        if ctx.frame_bytes > 0 {
            asm.emit_add_sp(ctx.frame_bytes);
        }
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        return Ok(());
    }

    if inst == "unreachable" {
        let spn = asm.new_label();
        asm.bind_label(spn);
        asm.emit_wfi();
        asm.emit_b_label_short(spn);
        return Ok(());
    }

    if let Some(rest) = inst.strip_prefix("br ") {
        let parts = split_top_commas(rest);
        if parts.len() == 1 {
            let dst_name = parts[0]
                .trim()
                .strip_prefix("label %")
                .ok_or_else(|| format!("malformed unconditional br: '{}'", inst))?
                .trim();
            emit_phi_moves_for_edge(asm, ctx, cur_bb, dst_name)?;
            let lbl = *ctx
                .bb_labels
                .get(dst_name)
                .ok_or_else(|| format!("unknown label '{}'", dst_name))?;
            asm.emit_jump_bb(lbl);
            return Ok(());
        } else if parts.len() == 3 {
            let (_, cond_val) = split_type_and_val(&parts[0])?;
            let t_name = parts[1]
                .trim()
                .strip_prefix("label %")
                .ok_or_else(|| format!("malformed br true label: '{}'", inst))?
                .trim();
            let f_name = parts[2]
                .trim()
                .strip_prefix("label %")
                .ok_or_else(|| format!("malformed br false label: '{}'", inst))?
                .trim();
            let t_lbl = *ctx
                .bb_labels
                .get(t_name)
                .ok_or_else(|| format!("unknown label '{}'", t_name))?;
            let f_lbl = *ctx
                .bb_labels
                .get(f_name)
                .ok_or_else(|| format!("unknown label '{}'", f_name))?;

            ctx.load_op32(asm, cond_val, 0)?;
            asm.emit_movs_imm8(1, 1);
            asm.emit_ands(0, 1);
            asm.emit_cmp_imm8(0, 0);

            let false_edge = asm.new_label();
            asm.emit_cond_jump_bb(Cond::Eq, false_edge);
            emit_phi_moves_for_edge(asm, ctx, cur_bb, t_name)?;
            asm.emit_jump_bb(t_lbl);

            asm.bind_label(false_edge);
            emit_phi_moves_for_edge(asm, ctx, cur_bb, f_name)?;
            asm.emit_jump_bb(f_lbl);
            return Ok(());
        }
    }

    if let Some(rest) = inst.strip_prefix("switch ") {
        return lower_switch(asm, ctx, cur_bb, rest);
    }

    if let Some(rest) = inst.strip_prefix("store ") {
        let rest_clean = rest.trim_start_matches("volatile ").trim();
        let parts = split_top_commas(rest_clean);
        if parts.len() < 2 {
            return Err(format!("malformed store: '{}'", inst));
        }
        let (val_ty, val_op) = split_type_and_val(&parts[0])?;
        let (_, ptr_op) = split_type_and_val(&parts[1])?;
        ctx.load_op32(asm, ptr_op, 2)?; // r2 = dst_addr
        match val_ty {
            LowerType::Int(1) | LowerType::Int(8) => {
                ctx.load_op32(asm, val_op, 0)?;
                asm.emit_strb_imm(0, 2, 0);
            }
            LowerType::Int(16) => {
                ctx.load_op32(asm, val_op, 0)?;
                asm.emit_strh_imm(0, 2, 0);
            }
            LowerType::Int(64) | LowerType::Float64 => {
                ctx.load_op64(asm, val_op, &val_ty, 0, 1)?;
                asm.emit_str_imm(0, 2, 0);
                asm.emit_str_imm(1, 2, 4);
            }
            _ => {
                ctx.load_op32(asm, val_op, 0)?;
                asm.emit_str_imm(0, 2, 0);
            }
        }
        return Ok(());
    }

    if inst.starts_with("call ") || inst.starts_with("tail call ") {
        let call_body = inst
            .trim_start_matches("tail ")
            .trim_start_matches("call ")
            .trim();
        lower_call(asm, ctx, None, call_body)?;
        return Ok(());
    }

    // --- Value-Producing SSA Instructions (`%dst = <op> ...`) ---
    let Some((dst, rhs_raw)) = split_ssa_assign(inst) else {
        return Ok(());
    };
    let rhs = rhs_raw.trim();

    if rhs.starts_with("alloca ") {
        // Storage slot is statically allocated in `[sp + data_off]` and materialized on demand!
        return Ok(());
    }

    if rhs.starts_with("call ") || rhs.starts_with("tail call ") || rhs.starts_with("musttail call ") {
        let call_body = rhs
            .trim_start_matches("musttail ")
            .trim_start_matches("tail ")
            .trim_start_matches("call ")
            .trim();
        lower_call(asm, ctx, Some(dst), call_body)?;
        return Ok(());
    }

    if let Some(rest) = rhs.strip_prefix("load ") {
        let rest_clean = rest.trim_start_matches("volatile ").trim();
        let parts = split_top_commas(rest_clean);
        if parts.len() < 2 {
            return Err(format!("malformed load: '{}'", inst));
        }
        let val_ty = parse_ir_type(parts[0].trim());
        let (_, ptr_op) = split_type_and_val(&parts[1])?;
        ctx.load_op32(asm, ptr_op, 2)?; // r2 = src_addr
        match val_ty {
            LowerType::Int(1) => {
                asm.emit_ldrb_imm(0, 2, 0);
                asm.emit_movs_imm8(1, 1);
                asm.emit_ands(0, 1);
                ctx.store_ssa32(asm, dst, 0);
            }
            LowerType::Int(8) => {
                asm.emit_ldrb_imm(0, 2, 0);
                ctx.store_ssa32(asm, dst, 0);
            }
            LowerType::Int(16) => {
                asm.emit_ldrh_imm(0, 2, 0);
                ctx.store_ssa32(asm, dst, 0);
            }
            LowerType::Int(64) | LowerType::Float64 => {
                asm.emit_ldr_imm(0, 2, 0);
                asm.emit_ldr_imm(1, 2, 4);
                ctx.store_ssa64(asm, dst, 0, 1);
            }
            _ => {
                asm.emit_ldr_imm(0, 2, 0);
                ctx.store_ssa32(asm, dst, 0);
            }
        }
        return Ok(());
    }

    if let Some(rest) = rhs.strip_prefix("getelementptr ") {
        lower_gep(asm, ctx, dst, rest)?;
        return Ok(());
    }

    if let Some(rest) = rhs.strip_prefix("icmp ") {
        lower_icmp(asm, ctx, dst, rest)?;
        return Ok(());
    }

    if let Some(rest) = rhs.strip_prefix("select ") {
        let parts = split_top_commas(rest);
        if parts.len() == 3 {
            let (_, cond_op) = split_type_and_val(&parts[0])?;
            let (v_ty, t_op) = split_type_and_val(&parts[1])?;
            let (_, f_op) = split_type_and_val(&parts[2])?;
            ctx.load_op32(asm, cond_op, 0)?;
            asm.emit_movs_imm8(1, 1);
            asm.emit_ands(0, 1);
            asm.emit_cmp_imm8(0, 0);
            let f_lbl = asm.new_label();
            let end_lbl = asm.new_label();
            asm.emit_bcond_label_short(Cond::Eq, f_lbl);
            if v_ty.is_64bit_scalar() {
                ctx.load_op64(asm, t_op, &v_ty, 0, 1)?;
                asm.emit_b_label_short(end_lbl);
                asm.bind_label(f_lbl);
                ctx.load_op64(asm, f_op, &v_ty, 0, 1)?;
                asm.bind_label(end_lbl);
                ctx.store_ssa64(asm, dst, 0, 1);
            } else {
                ctx.load_op32(asm, t_op, 0)?;
                asm.emit_b_label_short(end_lbl);
                asm.bind_label(f_lbl);
                ctx.load_op32(asm, f_op, 0)?;
                asm.bind_label(end_lbl);
                ctx.store_ssa32(asm, dst, 0);
            }
            return Ok(());
        }
    }

    // Cast operations (`zext`, `sext`, `trunc`, `bitcast`, `ptrtoint`, `inttoptr`, `fptosi`, `sitofp`)
    for cast_kw in [
        "zext ", "sext ", "trunc ", "bitcast ", "ptrtoint ", "inttoptr ", "fptosi ", "fptoui ",
        "sitofp ", "uitofp ", "fpext ", "fptrunc ", "freeze ",
    ] {
        if let Some(rest) = rhs.strip_prefix(cast_kw) {
            lower_cast(asm, ctx, dst, cast_kw.trim(), rest)?;
            return Ok(());
        }
    }

    // Binary integer & bitwise operations
    let first_tok = rhs.split_whitespace().next().unwrap_or("");
    if matches!(
        first_tok,
        "add"
            | "sub"
            | "mul"
            | "sdiv"
            | "udiv"
            | "srem"
            | "urem"
            | "and"
            | "or"
            | "xor"
            | "shl"
            | "lshr"
            | "ashr"
            | "fadd"
            | "fsub"
            | "fmul"
            | "fdiv"
            | "frem"
    ) {
        let rest = &rhs[first_tok.len()..].trim();
        lower_binop(asm, ctx, dst, first_tok, rest)?;
        return Ok(());
    }

    if let Some(rest) = rhs.strip_prefix("fneg ") {
        let (ty, op) = split_type_and_val(rest)?;
        ctx.load_op64(asm, op, &ty, 0, 1)?;
        asm.emit_load_u32(2, 0x8000_0000);
        asm.emit_eors(1, 2);
        ctx.store_ssa64(asm, dst, 0, 1);
        return Ok(());
    }

    if rhs.starts_with("fcmp ") {
        asm.emit_movs_imm8(0, 0);
        ctx.store_ssa32(asm, dst, 0);
        return Ok(());
    }

    Ok(())
}

fn lower_gep(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    dst: &str,
    rest: &str,
) -> Result<(), String> {
    let clean = rest.trim().trim_start_matches("inbounds").trim();
    let parts = split_top_commas(clean);
    if parts.len() < 2 {
        return Err(format!("malformed GEP: '{}'", rest));
    }
    let mut cur_ty = parse_ir_type(parts[0].trim());
    let (_, base_op) = split_type_and_val(&parts[1])?;
    ctx.load_op32(asm, base_op, 4)?; // r4 = accumulated address

    for (i, idx_part) in parts[2..].iter().enumerate() {
        let (_, idx_op) = split_type_and_val(idx_part)?;
        if i == 0 {
            let elem_sz = cur_ty.byte_size(ctx.structs);
            add_scaled_index(asm, ctx, 4, idx_op, elem_sz)?;
        } else {
            match cur_ty.clone() {
                LowerType::Array(_, elem) => {
                    let elem_sz = elem.byte_size(ctx.structs);
                    add_scaled_index(asm, ctx, 4, idx_op, elem_sz)?;
                    cur_ty = *elem;
                }
                LowerType::Struct(sname) => {
                    let f_idx = parse_ir_const64(idx_op).unwrap_or(0).max(0) as usize;
                    if let Some(flds) = ctx.structs.get(&sname) {
                        let (_, _, offs) = struct_layout_offsets(flds, ctx.structs);
                        if let Some(&fo) = offs.get(f_idx) {
                            if fo != 0 {
                                asm.emit_load_u32(2, fo);
                                asm.emit_adds_reg(4, 4, 2);
                            }
                            cur_ty = flds[f_idx].clone();
                        }
                    } else {
                        let fo = (f_idx as u32) * 8;
                        if fo != 0 {
                            asm.emit_load_u32(2, fo);
                            asm.emit_adds_reg(4, 4, 2);
                        }
                        cur_ty = LowerType::Int(64);
                    }
                }
                LowerType::AnonStruct(flds) => {
                    let f_idx = parse_ir_const64(idx_op).unwrap_or(0).max(0) as usize;
                    let (_, _, offs) = struct_layout_offsets(&flds, ctx.structs);
                    if let Some(&fo) = offs.get(f_idx) {
                        if fo != 0 {
                            asm.emit_load_u32(2, fo);
                            asm.emit_adds_reg(4, 4, 2);
                        }
                        cur_ty = flds[f_idx].clone();
                    }
                }
                other => {
                    let sz = other.byte_size(ctx.structs).max(1);
                    add_scaled_index(asm, ctx, 4, idx_op, sz)?;
                }
            }
        }
    }

    ctx.store_ssa32(asm, dst, 4);
    Ok(())
}

fn add_scaled_index(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    acc_reg: u8,
    idx_op: &str,
    elem_sz: u32,
) -> Result<(), String> {
    if elem_sz == 0 {
        return Ok(());
    }
    if let Ok(c) = parse_ir_const64(idx_op) {
        let byte_delta = (c as i32).wrapping_mul(elem_sz as i32);
        if byte_delta == 0 {
            return Ok(());
        }
        asm.emit_load_u32(2, byte_delta as u32);
        asm.emit_adds_reg(acc_reg, acc_reg, 2);
        return Ok(());
    }
    ctx.load_op32(asm, idx_op, 2)?;
    match elem_sz {
        1 => {}
        2 => asm.emit_lsls_imm(2, 2, 1),
        4 => asm.emit_lsls_imm(2, 2, 2),
        8 => asm.emit_lsls_imm(2, 2, 3),
        other => {
            asm.emit_load_u32(3, other);
            asm.emit_muls(2, 3);
        }
    }
    asm.emit_adds_reg(acc_reg, acc_reg, 2);
    Ok(())
}

fn lower_binop(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    dst: &str,
    op: &str,
    rest: &str,
) -> Result<(), String> {
    // Strip optional flags (`nsw`, `nuw`, `exact`, `fast`, `disjoint`)
    let mut rem = rest.trim();
    for flag in ["nsw ", "nuw ", "exact ", "disjoint ", "fast "] {
        while let Some(r) = rem.strip_prefix(flag) {
            rem = r.trim();
        }
    }
    let parts = split_top_commas(rem);
    if parts.len() != 2 {
        return Err(format!("malformed binop '{} {}'", op, rest));
    }
    let (ty, lhs_op) = split_type_and_val(&parts[0])?;
    let rhs_op = parts[1].trim();

    if ty.is_64bit_scalar() {
        ctx.load_op64(asm, lhs_op, &ty, 0, 1)?;
        ctx.load_op64(asm, rhs_op, &ty, 2, 3)?;
        match op {
            "add" | "fadd" => {
                asm.emit_adds_reg(0, 0, 2);
                asm.emit_adcs(1, 3);
            }
            "sub" | "fsub" => {
                asm.emit_subs_reg(0, 0, 2);
                asm.emit_sbcs(1, 3);
            }
            "and" => {
                asm.emit_ands(0, 2);
                asm.emit_ands(1, 3);
            }
            "or" => {
                asm.emit_orrs(0, 2);
                asm.emit_orrs(1, 3);
            }
            "xor" => {
                asm.emit_eors(0, 2);
                asm.emit_eors(1, 3);
            }
            "mul" | "fmul" => asm.emit_bl_sym("__omni_mul64"),
            "sdiv" | "fdiv" => asm.emit_bl_sym("__omni_sdiv64"),
            "udiv" => asm.emit_bl_sym("__omni_udiv64"),
            "srem" | "frem" => asm.emit_bl_sym("__omni_srem64"),
            "urem" => asm.emit_bl_sym("__omni_urem64"),
            "shl" => asm.emit_bl_sym("__omni_shl64"),
            "lshr" => asm.emit_bl_sym("__omni_lshr64"),
            "ashr" => asm.emit_bl_sym("__omni_ashr64"),
            _ => {}
        }
        ctx.store_ssa64(asm, dst, 0, 1);
    } else {
        ctx.load_op32(asm, lhs_op, 0)?;
        ctx.load_op32(asm, rhs_op, 2)?;
        match op {
            "add" => asm.emit_adds_reg(0, 0, 2),
            "sub" => asm.emit_subs_reg(0, 0, 2),
            "mul" => asm.emit_muls(0, 2),
            "and" => asm.emit_ands(0, 2),
            "or" => asm.emit_orrs(0, 2),
            "xor" => asm.emit_eors(0, 2),
            "shl" => asm.emit_lsls_reg(0, 2),
            "lshr" => asm.emit_lsrs_reg(0, 2),
            "ashr" => asm.emit_asrs_reg(0, 2),
            "sdiv" => {
                if ctx.chip.has_thumb2 {
                    asm.emit_thumb2_sdiv(0, 0, 2);
                } else {
                    asm.emit_asrs_imm(1, 0, 31);
                    asm.emit_asrs_imm(3, 2, 31);
                    asm.emit_bl_sym("__omni_sdiv64");
                }
            }
            "udiv" => {
                if ctx.chip.has_thumb2 {
                    asm.emit_thumb2_udiv(0, 0, 2);
                } else {
                    asm.emit_movs_imm8(1, 0);
                    asm.emit_movs_imm8(3, 0);
                    asm.emit_bl_sym("__omni_udiv64");
                }
            }
            "srem" => {
                asm.emit_asrs_imm(1, 0, 31);
                asm.emit_asrs_imm(3, 2, 31);
                asm.emit_bl_sym("__omni_srem64");
            }
            "urem" => {
                asm.emit_movs_imm8(1, 0);
                asm.emit_movs_imm8(3, 0);
                asm.emit_bl_sym("__omni_urem64");
            }
            _ => {}
        }
        // Mask narrow integer widths (`i1`, `i8`, `i16`)
        match ty {
            LowerType::Int(1) => {
                asm.emit_movs_imm8(1, 1);
                asm.emit_ands(0, 1);
            }
            LowerType::Int(8) => asm.emit_uxtb(0, 0),
            LowerType::Int(16) => asm.emit_uxth(0, 0),
            _ => {}
        }
        ctx.store_ssa32(asm, dst, 0);
    }
    Ok(())
}

fn lower_icmp(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    dst: &str,
    rest: &str,
) -> Result<(), String> {
    let mut it = rest.trim().splitn(2, char::is_whitespace);
    let pred = it.next().unwrap_or("").trim();
    let ops_str = it.next().unwrap_or("").trim();
    let parts = split_top_commas(ops_str);
    if parts.len() != 2 {
        return Err(format!("malformed icmp: '{}'", rest));
    }
    let (ty, lhs_op) = split_type_and_val(&parts[0])?;
    let rhs_op = parts[1].trim();

    if ty.is_64bit_scalar() {
        match pred {
            "eq" | "ne" => {
                ctx.load_op64(asm, lhs_op, &ty, 0, 1)?;
                ctx.load_op64(asm, rhs_op, &ty, 2, 3)?;
                asm.emit_eors(0, 2);
                asm.emit_eors(1, 3);
                asm.emit_orrs(0, 1);
                let is_true = asm.new_label();
                let done = asm.new_label();
                asm.emit_cmp_imm8(0, 0);
                let c = if pred == "eq" { Cond::Eq } else { Cond::Ne };
                asm.emit_bcond_label_short(c, is_true);
                asm.emit_movs_imm8(0, 0);
                asm.emit_b_label_short(done);
                asm.bind_label(is_true);
                asm.emit_movs_imm8(0, 1);
                asm.bind_label(done);
                ctx.store_ssa32(asm, dst, 0);
            }
            // Exact 2-instruction 64-bit `subs + sbcs` reduction for all 8 signed/unsigned orderings!
            _ => {
                let (swap_ops, cond) = match pred {
                    "ult" => (false, Cond::Cc), // a <u b
                    "uge" => (false, Cond::Cs), // !(a <u b)
                    "ugt" => (true, Cond::Cc),  // b <u a
                    "ule" => (true, Cond::Cs),  // !(b <u a)
                    "slt" => (false, Cond::Lt), // a <s b
                    "sge" => (false, Cond::Ge), // !(a <s b)
                    "sgt" => (true, Cond::Lt),  // b <s a
                    "sle" => (true, Cond::Ge),  // !(b <s a)
                    other => return Err(format!("unknown icmp predicate '{}'", other)),
                };
                if swap_ops {
                    ctx.load_op64(asm, rhs_op, &ty, 0, 1)?;
                    ctx.load_op64(asm, lhs_op, &ty, 2, 3)?;
                } else {
                    ctx.load_op64(asm, lhs_op, &ty, 0, 1)?;
                    ctx.load_op64(asm, rhs_op, &ty, 2, 3)?;
                }
                asm.emit_subs_reg(0, 0, 2);
                asm.emit_sbcs(1, 3);
                let tr = asm.new_label();
                let dn = asm.new_label();
                asm.emit_bcond_label_short(cond, tr);
                asm.emit_movs_imm8(0, 0);
                asm.emit_b_label_short(dn);
                asm.bind_label(tr);
                asm.emit_movs_imm8(0, 1);
                asm.bind_label(dn);
                ctx.store_ssa32(asm, dst, 0);
            }
        }
    } else {
        ctx.load_op32(asm, lhs_op, 0)?;
        ctx.load_op32(asm, rhs_op, 2)?;
        // Sign-extend or zero-extend narrow widths before 32-bit `CMP`
        let is_signed = matches!(pred, "slt" | "sle" | "sgt" | "sge");
        match ty {
            LowerType::Int(1) => {
                asm.emit_movs_imm8(3, 1);
                asm.emit_ands(0, 3);
                asm.emit_ands(2, 3);
            }
            LowerType::Int(8) => {
                if is_signed {
                    asm.emit_sxtb(0, 0);
                    asm.emit_sxtb(2, 2);
                } else {
                    asm.emit_uxtb(0, 0);
                    asm.emit_uxtb(2, 2);
                }
            }
            LowerType::Int(16) => {
                if is_signed {
                    asm.emit_sxth(0, 0);
                    asm.emit_sxth(2, 2);
                } else {
                    asm.emit_uxth(0, 0);
                    asm.emit_uxth(2, 2);
                }
            }
            _ => {}
        }
        asm.emit_cmp_reg(0, 2);
        let cond = match pred {
            "eq" => Cond::Eq,
            "ne" => Cond::Ne,
            "ult" => Cond::Cc,
            "ule" => Cond::Ls,
            "ugt" => Cond::Hi,
            "uge" => Cond::Cs,
            "slt" => Cond::Lt,
            "sle" => Cond::Le,
            "sgt" => Cond::Gt,
            "sge" => Cond::Ge,
            other => return Err(format!("unknown icmp predicate '{}'", other)),
        };
        let tr = asm.new_label();
        let dn = asm.new_label();
        asm.emit_bcond_label_short(cond, tr);
        asm.emit_movs_imm8(0, 0);
        asm.emit_b_label_short(dn);
        asm.bind_label(tr);
        asm.emit_movs_imm8(0, 1);
        asm.bind_label(dn);
        ctx.store_ssa32(asm, dst, 0);
    }
    Ok(())
}

fn lower_cast(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    dst: &str,
    cast_kind: &str,
    rest: &str,
) -> Result<(), String> {
    if cast_kind == "freeze" {
        let (ty, op) = split_type_and_val(rest)?;
        if ty.is_64bit_scalar() {
            ctx.load_op64(asm, op, &ty, 0, 1)?;
            ctx.store_ssa64(asm, dst, 0, 1);
        } else {
            ctx.load_op32(asm, op, 0)?;
            ctx.store_ssa32(asm, dst, 0);
        }
        return Ok(());
    }
    let Some(to_idx) = rest.rfind(" to ") else {
        return Err(format!("malformed cast: '{} {}'", cast_kind, rest));
    };
    let src_part = &rest[..to_idx];
    let dst_ty = parse_ir_type(rest[to_idx + 4..].trim());
    let (src_ty, src_op) = split_type_and_val(src_part)?;

    match cast_kind {
        "zext" | "ptrtoint" | "uitofp" => {
            if src_ty.is_64bit_scalar() {
                ctx.load_op64(asm, src_op, &src_ty, 0, 1)?;
            } else {
                ctx.load_op32(asm, src_op, 0)?;
                match src_ty {
                    LowerType::Int(1) => {
                        asm.emit_movs_imm8(1, 1);
                        asm.emit_ands(0, 1);
                    }
                    LowerType::Int(8) => asm.emit_uxtb(0, 0),
                    LowerType::Int(16) => asm.emit_uxth(0, 0),
                    _ => {}
                }
                asm.emit_movs_imm8(1, 0);
            }
            if dst_ty.is_64bit_scalar() {
                ctx.store_ssa64(asm, dst, 0, 1);
            } else {
                ctx.store_ssa32(asm, dst, 0);
            }
        }
        "sext" | "sitofp" => {
            if src_ty.is_64bit_scalar() {
                ctx.load_op64(asm, src_op, &src_ty, 0, 1)?;
            } else {
                ctx.load_op32(asm, src_op, 0)?;
                match src_ty {
                    LowerType::Int(1) => {
                        asm.emit_lsls_imm(0, 0, 31);
                        asm.emit_asrs_imm(0, 0, 31);
                    }
                    LowerType::Int(8) => asm.emit_sxtb(0, 0),
                    LowerType::Int(16) => asm.emit_sxth(0, 0),
                    _ => {}
                }
                asm.emit_asrs_imm(1, 0, 31);
            }
            if dst_ty.is_64bit_scalar() {
                ctx.store_ssa64(asm, dst, 0, 1);
            } else {
                ctx.store_ssa32(asm, dst, 0);
            }
        }
        _ => {
            // `trunc`, `inttoptr`, `bitcast`, `fptosi`, etc.
            if src_ty.is_64bit_scalar() && dst_ty.is_64bit_scalar() {
                ctx.load_op64(asm, src_op, &src_ty, 0, 1)?;
                ctx.store_ssa64(asm, dst, 0, 1);
            } else {
                ctx.load_op32(asm, src_op, 0)?;
                match dst_ty {
                    LowerType::Int(1) => {
                        asm.emit_movs_imm8(1, 1);
                        asm.emit_ands(0, 1);
                    }
                    LowerType::Int(8) => asm.emit_uxtb(0, 0),
                    LowerType::Int(16) => asm.emit_uxth(0, 0),
                    _ => {}
                }
                if dst_ty.is_64bit_scalar() {
                    asm.emit_movs_imm8(1, 0);
                    ctx.store_ssa64(asm, dst, 0, 1);
                } else {
                    ctx.store_ssa32(asm, dst, 0);
                }
            }
        }
    }
    Ok(())
}

fn lower_call(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    dst: Option<&str>,
    call_body: &str,
) -> Result<(), String> {
    // Strip calling-convention / return-attribute tokens (`fastcc`, `ccc`, `noundef`, `signext`, `zeroext`, `noalias`)
    let mut s = call_body.trim();
    for attr in [
        "fastcc ",
        "ccc ",
        "coldcc ",
        "noundef ",
        "signext ",
        "zeroext ",
        "noalias ",
        "nonnull ",
    ] {
        while let Some(r) = s.strip_prefix(attr) {
            s = r.trim();
        }
    }

    // Find the argument list `(...)` of the call (the LAST top-level `(...)` on the line!)
    let (head, args_str) = split_call_head_and_args(s)?;
    // `head` is e.g. `i64 @__omni_fn_foo` or `i32 (i8*, ...) @printf` or `i64 %fptr_2`
    let (ret_ty_str, callee_tok) = split_callee_from_ret_type(head)?;
    let ret_ty = parse_ir_type(ret_ty_str);

    let arg_items = if args_str.trim().is_empty() {
        Vec::new()
    } else {
        split_top_commas(args_str)
    };

    // Pass 1: Stage stack arguments (`word_idx >= 4`) into `[sp + (word_idx - 4)*4]`
    // and collect register assignments (`r0..r3`) so loading operands never clobbers `r0..r3`!
    let mut word_idx: u32 = 0;
    let mut reg_loads: Vec<(u8, Option<u8>, LowerType, String)> = Vec::new();

    for raw_arg in &arg_items {
        let (aty, aval) = split_arg_type_and_val(raw_arg)?;
        if aty.is_64bit_scalar() {
            if (word_idx & 1) != 0 {
                word_idx += 1;
            }
            if word_idx < 4 {
                reg_loads.push((
                    word_idx as u8,
                    Some((word_idx + 1) as u8),
                    aty,
                    aval.to_string(),
                ));
            } else {
                let stk_off = (word_idx - 4) * 4;
                ctx.load_op64(asm, aval, &aty, 0, 1)?;
                asm.emit_str_sp(0, stk_off);
                asm.emit_str_sp(1, stk_off + 4);
            }
            word_idx += 2;
        } else {
            if word_idx < 4 {
                reg_loads.push((word_idx as u8, None, aty, aval.to_string()));
            } else {
                let stk_off = (word_idx - 4) * 4;
                ctx.load_op32(asm, aval, 0)?;
                asm.emit_str_sp(0, stk_off);
            }
            word_idx += 1;
        }
    }

    // If indirect call (`%fn_ptr`), load target into `r4` BEFORE filling `r0..r3`!
    let indirect_callee = !callee_tok.starts_with('@');
    if indirect_callee {
        ctx.load_op32(asm, callee_tok, 4)?;
        // Ensure Thumb bit 0 is set (`orr r4, #1`) for `BLX r4` on Cortex-M!
        asm.emit_movs_imm8(5, 1);
        asm.emit_orrs(4, 5);
    }

    // Pass 2: Load register arguments `r0..r3` in reverse order (or via `r5`/`r6` scratch) so `r0..r3` don't clobber each other!
    // Wait: `load_op32` / `load_op64` only uses the target `rd` (and `r6`/`r7` if `slot > 1020`, or `r2`/`r3` for inline GEP).
    // Loading in reverse (`r2:r3` first, then `r0:r1`) guarantees inline GEP's `r2`/`r3` scratch for `arg0`?
    // Wait: if `arg0` uses inline GEP, `eval_inline_const_gep` with `const_add == 0` (`c"...\00", 0, 0`) uses ZERO scratch registers!
    // Even safer: stage register args into `r0..r3` via stack or load into `r5`/`r6` and move:
    // Actually, pushing each register arg or loading via `r5` (`r5` is saved in prologue and free!) is 100% clobber-proof!
    for (r_lo, r_hi_opt, aty, aval) in reg_loads.into_iter().rev() {
        if let Some(r_hi) = r_hi_opt {
            ctx.load_op64(asm, &aval, &aty, 5, 6)?;
            asm.emit_movs_reg(r_lo, 5);
            asm.emit_movs_reg(r_hi, 6);
        } else {
            ctx.load_op32(asm, &aval, 5)?;
            asm.emit_movs_reg(r_lo, 5);
        }
    }

    // Emit `BL sym` or `BLX r4`
    if indirect_callee {
        asm.emit_blx_reg(4);
    } else {
        let sym = callee_tok.trim_start_matches('@');
        let mapped_sym = match sym {
            "llvm.pow.f64" => "__omni_ipow64",
            other => other,
        };
        asm.emit_bl_sym(mapped_sym);
    }

    if let Some(dst_reg) = dst {
        if ret_ty.is_64bit_scalar() {
            ctx.store_ssa64(asm, dst_reg, 0, 1);
        } else if !matches!(ret_ty, LowerType::Void) {
            ctx.store_ssa32(asm, dst_reg, 0);
        }
    }
    Ok(())
}

fn lower_switch(
    asm: &mut ThumbAssembler,
    ctx: &FnLowerCtx,
    cur_bb: &str,
    rest: &str,
) -> Result<(), String> {
    // `i64 %val, label %def [ i64 0, label %c0 i64 1, label %c1 ]`
    let Some(brk_open) = rest.find('[') else {
        return Err(format!("malformed switch: '{}'", rest));
    };
    let brk_close = rest.rfind(']').unwrap_or(rest.len());
    let header = &rest[..brk_open];
    let cases_body = &rest[brk_open + 1..brk_close];

    let h_parts = split_top_commas(header);
    if h_parts.len() < 2 {
        return Err(format!("malformed switch header: '{}'", header));
    }
    let (val_ty, val_op) = split_type_and_val(&h_parts[0])?;
    let def_name = h_parts[1]
        .trim()
        .strip_prefix("label %")
        .ok_or_else(|| format!("malformed switch default: '{}'", header))?
        .trim();

    if val_ty.is_64bit_scalar() {
        ctx.load_op64(asm, val_op, &val_ty, 4, 5)?;
    } else {
        ctx.load_op32(asm, val_op, 4)?;
        asm.emit_movs_imm8(5, 0);
    }

    // Parse pairs `<ty> <const>, label %target`
    let toks: Vec<&str> = cases_body.split_whitespace().collect();
    let mut i = 0;
    while i + 3 < toks.len() {
        let c_val_raw = toks[i + 1].trim_end_matches(',');
        let lbl_tok = toks[i + 3].trim_end_matches(',');
        if let Some(target_name) = lbl_tok.strip_prefix('%') {
            let c_val = parse_ir_const64(c_val_raw)?;
            asm.emit_load_u32(2, (c_val & 0xFFFF_FFFF) as u32);
            asm.emit_cmp_reg(4, 2);
            let next_case = asm.new_label();
            asm.emit_bcond_label_short(Cond::Ne, next_case);
            if val_ty.is_64bit_scalar() {
                asm.emit_load_u32(3, ((c_val as u64) >> 32) as u32);
                asm.emit_cmp_reg(5, 3);
                asm.emit_bcond_label_short(Cond::Ne, next_case);
            }
            emit_phi_moves_for_edge(asm, ctx, cur_bb, target_name)?;
            let t_lbl = *ctx
                .bb_labels
                .get(target_name)
                .ok_or_else(|| format!("unknown switch target '{}'", target_name))?;
            asm.emit_jump_bb(t_lbl);
            asm.bind_label(next_case);
        }
        i += 4;
    }

    emit_phi_moves_for_edge(asm, ctx, cur_bb, def_name)?;
    let def_lbl = *ctx
        .bb_labels
        .get(def_name)
        .ok_or_else(|| format!("unknown switch default '{}'", def_name))?;
    asm.emit_jump_bb(def_lbl);
    Ok(())
}

// =============================================================================
// Lightweight Pure-Rust LLVM IR Text Parser
// =============================================================================

fn parse_llvm_ir_module(ir: &str) -> Result<ParsedIrModule, (usize, String)> {
    let mut out = ParsedIrModule::default();
    let lines: Vec<&str> = ir.lines().collect();
    let mut idx = 0usize;

    while idx < lines.len() {
        let lineno = idx + 1;
        let line = strip_ir_comment(lines[idx]).trim();
        if line.is_empty()
            || line.starts_with("source_filename")
            || line.starts_with("target ")
            || line.starts_with("attributes ")
            || line.starts_with('!')
        {
            idx += 1;
            continue;
        }

        // Struct / Enum type alias: `%struct.Foo = type { i64, i64 }`
        if line.starts_with('%') && line.contains("= type ") {
            if let Some(eq_pos) = line.find('=') {
                let sname = line[..eq_pos].trim().trim_start_matches('%').to_string();
                let rhs = line[eq_pos + 1..].trim().trim_start_matches("type").trim();
                if let Some(inner) = rhs.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let fields = split_top_commas(inner)
                        .into_iter()
                        .filter(|p| !p.trim().is_empty())
                        .map(|p| parse_ir_type(p.trim()))
                        .collect();
                    out.structs.insert(sname, fields);
                }
            }
            idx += 1;
            continue;
        }

        // External declaration: `declare ... @name(...)`
        if line.starts_with("declare ") {
            if let Some(at_pos) = line.find('@') {
                let after = &line[at_pos + 1..];
                let end = after.find('(').unwrap_or(after.len());
                out.declared_syms.insert(after[..end].trim().to_string());
            }
            idx += 1;
            continue;
        }

        // Global constant / variable: `@.str.0 = private unnamed_addr constant [12 x i8] c"hello\00", align 1`
        if line.starts_with('@') && line.contains('=') {
            if let Some(g) = parse_global_line(line) {
                out.globals.push(g);
            }
            idx += 1;
            continue;
        }

        // Function definition: `define ... @name(...) {`
        if line.starts_with("define ") {
            let mut header_acc = line.to_string();
            while !header_acc.contains('{') && idx + 1 < lines.len() {
                idx += 1;
                header_acc.push(' ');
                header_acc.push_str(strip_ir_comment(lines[idx]).trim());
            }
            let (fn_name, ret_ty, params) =
                parse_define_header(&header_acc).map_err(|e| (lineno, e))?;

            let mut body_lines: Vec<(usize, String)> = Vec::new();
            let mut brace_depth = header_acc.chars().filter(|&c| c == '{').count() as i32
                - header_acc.chars().filter(|&c| c == '}').count() as i32;

            while brace_depth > 0 && idx + 1 < lines.len() {
                idx += 1;
                let bline = strip_ir_comment(lines[idx]).trim().to_string();
                brace_depth += bline.chars().filter(|&c| c == '{').count() as i32;
                brace_depth -= bline.chars().filter(|&c| c == '}').count() as i32;
                if brace_depth > 0 || (brace_depth == 0 && bline != "}") {
                    let cleaned = bline.trim_end_matches('}').trim().to_string();
                    if !cleaned.is_empty() {
                        body_lines.push((idx + 1, cleaned));
                    }
                }
            }
            if brace_depth != 0 {
                return Err((lineno, format!("unclosed brace in define @{}", fn_name)));
            }

            let blocks = parse_function_blocks(&body_lines).map_err(|e| (lineno, e))?;
            out.functions.push(IrFunction {
                name: fn_name,
                ret_ty,
                params,
                blocks,
            });
            idx += 1;
            continue;
        }

        idx += 1;
    }

    Ok(out)
}

fn parse_define_header(hdr: &str) -> Result<(String, LowerType, Vec<IrParam>), String> {
    let before_brace = hdr.split('{').next().unwrap_or(hdr).trim();
    let at_pos = before_brace
        .find('@')
        .ok_or_else(|| format!("missing @ in define header: '{}'", hdr))?;
    let prefix = before_brace[..at_pos].trim();
    // Strip `define`, linkage (`weak`, `internal`, `private`, `dso_local`, `linkonce_odr`), attrs
    let mut ret_tokens: Vec<&str> = Vec::new();
    for tok in prefix.split_whitespace().skip(1) {
        if matches!(
            tok,
            "weak"
                | "weak_odr"
                | "linkonce"
                | "linkonce_odr"
                | "internal"
                | "private"
                | "dso_local"
                | "hidden"
                | "noundef"
                | "signext"
                | "zeroext"
                | "noalias"
                | "fastcc"
                | "ccc"
        ) {
            continue;
        }
        ret_tokens.push(tok);
    }
    let ret_ty = parse_ir_type(&ret_tokens.join(" "));

    let after_at = &before_brace[at_pos + 1..];
    let open_p = after_at
        .find('(')
        .ok_or_else(|| format!("missing '(' in define header: '{}'", hdr))?;
    let close_p = after_at
        .rfind(')')
        .ok_or_else(|| format!("missing ')' in define header: '{}'", hdr))?;
    let fn_name = after_at[..open_p].trim().trim_matches('"').to_string();
    let params_str = after_at[open_p + 1..close_p].trim();

    let mut params = Vec::new();
    if !params_str.is_empty() {
        for (p_idx, part) in split_top_commas(params_str).into_iter().enumerate() {
            let pt = part.trim();
            if pt == "..." || pt.is_empty() {
                continue;
            }
            let (ty, name) = parse_param_decl(pt, p_idx);
            params.push(IrParam { ty, name });
        }
    }
    Ok((fn_name, ret_ty, params))
}

fn parse_param_decl(decl: &str, idx: usize) -> (LowerType, String) {
    let toks: Vec<&str> = decl
        .split_whitespace()
        .filter(|t| {
            !matches!(
                *t,
                "noundef" | "nonnull" | "nocapture" | "readonly" | "signext" | "zeroext" | "noalias"
            )
        })
        .collect();
    if let Some(last) = toks.last() {
        if last.starts_with('%') {
            let ty_s = toks[..toks.len() - 1].join(" ");
            return (parse_ir_type(&ty_s), (*last).to_string());
        }
    }
    (parse_ir_type(&toks.join(" ")), format!("%{}", idx))
}

fn parse_function_blocks(lines: &[(usize, String)]) -> Result<Vec<IrBasicBlock>, String> {
    let mut blocks: Vec<IrBasicBlock> = Vec::new();
    let mut cur_label = "entry".to_string();
    let mut cur_phis: Vec<PhiNode> = Vec::new();
    let mut cur_insts: Vec<String> = Vec::new();
    let mut started = false;

    let mut i = 0usize;
    while i < lines.len() {
        let mut line = lines[i].1.clone();
        // Join multi-line `switch ... [ ... ]` blocks cleanly
        if line.starts_with("switch ") && line.contains('[') && !line.contains(']') {
            while !line.contains(']') && i + 1 < lines.len() {
                i += 1;
                line.push(' ');
                line.push_str(&lines[i].1);
            }
        }

        if let Some(lbl) = extract_block_label(&line) {
            if started && (!cur_insts.is_empty() || !cur_phis.is_empty()) {
                blocks.push(IrBasicBlock {
                    label: cur_label,
                    phis: std::mem::take(&mut cur_phis),
                    insts: std::mem::take(&mut cur_insts),
                });
            }
            cur_label = lbl;
            started = true;
            i += 1;
            continue;
        }

        started = true;
        if let Some((dst, rhs)) = split_ssa_assign(&line) {
            if let Some(phi_rest) = rhs.strip_prefix("phi ") {
                let phi = parse_phi_node(dst, phi_rest)?;
                cur_phis.push(phi);
                i += 1;
                continue;
            }
        }
        cur_insts.push(line);
        i += 1;
    }

    if started && (!cur_insts.is_empty() || !cur_phis.is_empty()) {
        blocks.push(IrBasicBlock {
            label: cur_label,
            phis: cur_phis,
            insts: cur_insts,
        });
    }
    Ok(blocks)
}

fn extract_block_label(line: &str) -> Option<String> {
    // e.g. `entry:` or `loop.cond_1:` (no `=` before `:`)
    let colon = line.find(':')?;
    if line[..colon].contains(' ') || line[..colon].contains('=') {
        return None;
    }
    Some(line[..colon].trim().trim_start_matches('%').to_string())
}

fn parse_phi_node(dst: &str, phi_rest: &str) -> Result<PhiNode, String> {
    let brk = phi_rest
        .find('[')
        .ok_or_else(|| format!("malformed phi: '{}'", phi_rest))?;
    let ty = parse_ir_type(phi_rest[..brk].trim());
    let mut incomings = Vec::new();
    let mut rem = &phi_rest[brk..];
    while let Some(open) = rem.find('[') {
        let after = &rem[open + 1..];
        let Some(close) = after.find(']') else {
            break;
        };
        let inside = &after[..close];
        let parts = split_top_commas(inside);
        if parts.len() == 2 {
            let val = parts[0].trim().to_string();
            let pred_label = parts[1].trim().trim_start_matches('%').to_string();
            incomings.push(PhiIncoming { val, pred_label });
        }
        rem = &after[close + 1..];
    }
    Ok(PhiNode {
        dst: dst.to_string(),
        ty,
        incomings,
    })
}

fn parse_global_line(line: &str) -> Option<GlobalConst> {
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().trim_start_matches('@').to_string();
    let rhs = line[eq_pos + 1..].trim();
    let is_constant = rhs.contains(" constant ");

    // String constant `c"...\00"`
    if let Some(c_idx) = rhs.find("c\"") {
        let after = &rhs[c_idx + 2..];
        let bytes = unescape_llvm_c_string(after);
        return Some(GlobalConst {
            name,
            is_constant,
            bytes,
        });
    }

    // Scalar or zeroinitializer global
    let stripped_align = rhs.split(", align").next().unwrap_or(rhs).trim();
    if stripped_align.ends_with("zeroinitializer") {
        return Some(GlobalConst {
            name,
            is_constant,
            bytes: vec![0u8; 8],
        });
    }

    let last_tok = stripped_align.split_whitespace().last().unwrap_or("0");
    let v = parse_ir_const64(last_tok).unwrap_or(0);
    Some(GlobalConst {
        name,
        is_constant,
        bytes: v.to_le_bytes().to_vec(),
    })
}

fn unescape_llvm_c_string(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            break;
        }
        if b[i] == b'\\' && i + 2 < b.len() {
            if let (Some(h1), Some(h2)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn parse_ir_type(raw: &str) -> LowerType {
    let t = raw.trim();
    if t.is_empty() || t == "void" {
        return LowerType::Void;
    }
    if let Some(pointee) = t.strip_suffix('*') {
        return LowerType::Ptr(Box::new(parse_ir_type(pointee)));
    }
    if t == "ptr" {
        return LowerType::Ptr(Box::new(LowerType::Int(8)));
    }
    if t == "double" {
        return LowerType::Float64;
    }
    if t == "float" {
        return LowerType::Float32;
    }
    if let Some(bits_s) = t.strip_prefix('i') {
        if let Ok(bits) = bits_s.parse::<u32>() {
            return LowerType::Int(bits);
        }
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if let Some(x_pos) = inner.find(" x ") {
            let cnt = inner[..x_pos].trim().parse::<usize>().unwrap_or(1);
            let elem = parse_ir_type(inner[x_pos + 3..].trim());
            return LowerType::Array(cnt, Box::new(elem));
        }
    }
    if let Some(inner) = t.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let fields = split_top_commas(inner)
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| parse_ir_type(s.trim()))
            .collect();
        return LowerType::AnonStruct(fields);
    }
    if let Some(sname) = t.strip_prefix('%') {
        return LowerType::Struct(sname.trim().to_string());
    }
    LowerType::Int(64)
}

fn compute_alloca_bytes(alloca_rest: &str, structs: &HashMap<String, Vec<LowerType>>) -> u32 {
    let parts = split_top_commas(alloca_rest);
    let ty = parse_ir_type(parts[0].trim());
    let mut sz = ty.byte_size(structs).max(4);
    if parts.len() >= 2 && !parts[1].trim().starts_with("align") {
        if let Ok((_, cnt_s)) = split_type_and_val(&parts[1]) {
            if let Ok(cnt) = parse_ir_const64(cnt_s) {
                if cnt > 1 {
                    sz = sz.saturating_mul(cnt as u32);
                }
            }
        }
    }
    sz
}

fn estimate_call_stack_words(line: &str) -> u32 {
    let commas = line.chars().filter(|&c| c == ',').count() as u32;
    (commas + 2) * 2
}

fn strip_ir_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if bytes[i] == b';' && !in_str {
            return &line[..i];
        }
        i += 1;
    }
    line
}

fn strip_trailing_ir_metadata(inst: &str) -> &str {
    if let Some(idx) = inst.find(", !") {
        inst[..idx].trim()
    } else {
        inst
    }
}

fn split_ssa_assign(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if !t.starts_with('%') {
        return None;
    }
    let eq = t.find('=')?;
    let lhs = t[..eq].trim();
    if lhs.contains(' ') {
        return None;
    }
    Some((lhs, t[eq + 1..].trim()))
}

fn split_top_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut start = 0usize;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let c = bytes[i];
        if c == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if !in_str {
            match c {
                b'(' | b'[' | b'{' | b'<' => depth += 1,
                b')' | b']' | b'}' | b'>' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(s[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    out.push(s[start..].trim().to_string());
    out
}

fn split_type_and_val(s: &str) -> Result<(LowerType, &str), String> {
    let t = s.trim();
    if t == "void" {
        return Ok((LowerType::Void, ""));
    }
    // Handle inline constant expressions like `i8* getelementptr inbounds (...)` or `i8* bitcast (...)`
    for kw in [" getelementptr", " bitcast", " inttoptr", " ptrtoint"] {
        if let Some(pos) = t.find(kw) {
            let ty_str = t[..pos].trim();
            let val_str = t[pos + 1..].trim();
            return Ok((parse_ir_type(ty_str), val_str));
        }
    }

    // Walk forward past the LLVM type (`i64`, `[14 x i8]*`, `%struct.Foo**`, `i64 (i64)*`)
    let bytes = t.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'{' | b'(' | b'<' => depth += 1,
            b']' | b'}' | b')' | b'>' => {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    while i < bytes.len() && (bytes[i] == b'*' || bytes[i] == b' ') {
                        if bytes[i] == b'*' {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            b' ' | b'\t' if depth == 0 => {
                // Check if next token is `(` of a function pointer type e.g. `void (i8*)* %fn`
                let rest_trim = t[i..].trim_start();
                if rest_trim.starts_with('(') {
                    // Continue consuming function pointer signature
                } else {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let ty_s = t[..i].trim();
    let val_s = t[i..].trim();
    Ok((parse_ir_type(ty_s), val_s))
}

fn split_arg_type_and_val(s: &str) -> Result<(LowerType, &str), String> {
    let (ty, mut val) = split_type_and_val(s)?;
    for attr in [
        "noundef ",
        "nonnull ",
        "nocapture ",
        "readonly ",
        "signext ",
        "zeroext ",
        "noalias ",
    ] {
        while let Some(r) = val.strip_prefix(attr) {
            val = r.trim();
        }
    }
    Ok((ty, val))
}

fn split_call_head_and_args(s: &str) -> Result<(&str, &str), String> {
    let close = s
        .rfind(')')
        .ok_or_else(|| format!("missing ')' in call: '{}'", s))?;
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut open = None;
    let mut idx = close as isize;
    while idx >= 0 {
        match bytes[idx as usize] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(idx as usize);
                    break;
                }
            }
            _ => {}
        }
        idx -= 1;
    }
    let op = open.ok_or_else(|| format!("unbalanced '(' in call: '{}'", s))?;
    Ok((s[..op].trim(), &s[op + 1..close]))
}

fn split_callee_from_ret_type<'a>(head: &'a str) -> Result<(&'a str, &'a str), String> {
    // `head` ends with `@callee` or `%fptr`!
    let last_ws = head
        .rfind(char::is_whitespace)
        .ok_or_else(|| format!("malformed call head: '{}'", head))?;
    let mut ret_part = head[..last_ws].trim();
    let callee = head[last_ws + 1..].trim();
    // Strip optional variadic prototype e.g. `i32 (i8*, ...)` -> `i32`
    if let Some(p_open) = ret_part.find('(') {
        if !ret_part.ends_with('*') {
            ret_part = ret_part[..p_open].trim();
        }
    }
    Ok((ret_part, callee))
}

fn strip_const_cast_wrapper(s: &str) -> Option<&str> {
    for kw in ["bitcast", "inttoptr", "ptrtoint"] {
        if let Some(rest) = s.strip_prefix(kw) {
            let inner = rest.trim().strip_prefix('(')?.strip_suffix(')')?;
            let to_idx = inner.rfind(" to ")?;
            let (_, val) = split_type_and_val(&inner[..to_idx]).ok()?;
            return Some(val);
        }
    }
    None
}

fn parse_ir_const64(raw: &str) -> Result<i64, String> {
    let s = raw.trim();
    match s {
        "true" => return Ok(1),
        "false" | "null" | "zeroinitializer" | "undef" | "poison" => return Ok(0),
        _ => {}
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        let u = u64::from_str_radix(hex, 16)
            .map_err(|_| format!("invalid hex literal '{}'", s))?;
        return Ok(u as i64);
    }
    if let Ok(v) = s.parse::<i64>() {
        return Ok(v);
    }
    if let Ok(u) = s.parse::<u64>() {
        return Ok(u as i64);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(f.to_bits() as i64);
    }
    Err(format!("unsupported IR operand '{}'", s))
}
