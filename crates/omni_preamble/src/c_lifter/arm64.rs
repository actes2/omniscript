use super::binfmt::{is_ext_sentinel, BinaryArch, ParsedBinaryImage};
use super::ext_abi::{next_t, ExternalDeclTracker, FLAG_C, FLAG_N, FLAG_V, FLAG_Z};
use super::x86_64::LiftedModuleOutput;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

/// Lift all reachable `AArch64` (`arm64`) functions starting from `root_vmas` into LLVM IR.
pub fn lift_aarch64_functions(
    img: &ParsedBinaryImage,
    mod_id: &str,
    root_vmas: &[u64],
) -> Result<LiftedModuleOutput, String> {
    let mut worklist: VecDeque<u64> = root_vmas.iter().copied().collect();
    let mut visited_fns: BTreeSet<u64> = BTreeSet::new();
    let mut ext_tracker = ExternalDeclTracker::default();
    let mut ir_functions = String::new();
    let mut total_instructions = 0usize;
    let mut max_static_stack_alloc = 0u64;

    while let Some(fn_vma) = worklist.pop_front() {
        if !visited_fns.insert(fn_vma) {
            continue;
        }
        if !img.is_exec_vma(fn_vma) {
            continue;
        }
        let mut discovered_callees = Vec::new();
        let (fn_ir, insn_cnt, stack_sub) = lift_single_aarch64_fn(
            img,
            mod_id,
            fn_vma,
            &mut ext_tracker,
            &mut discovered_callees,
        )?;
        ir_functions.push_str(&fn_ir);
        ir_functions.push('\n');
        total_instructions += insn_cnt;
        max_static_stack_alloc = max_static_stack_alloc.max(stack_sub);

        for callee in discovered_callees {
            if !visited_fns.contains(&callee) && img.is_exec_vma(callee) {
                worklist.push_back(callee);
            }
        }
    }

    emit_arm64_indirect_call_helper(
        &mut ir_functions,
        img,
        mod_id,
        &visited_fns,
        &mut ext_tracker,
    );
    if ext_tracker.uses_dynamic_stack_probe {
        max_static_stack_alloc = max_static_stack_alloc.max(65_536);
    }

    Ok(LiftedModuleOutput {
        ir_functions,
        lifted_fn_vmas: visited_fns,
        total_instructions,
        max_static_stack_alloc,
        ext_tracker,
    })
}

fn emit_arm64_indirect_call_helper(
    out: &mut String,
    img: &ParsedBinaryImage,
    mod_id: &str,
    lifted_fns: &BTreeSet<u64>,
    ext_tracker: &mut ExternalDeclTracker,
) {
    let img_len = img.image_bytes.len().max(16);
    let img_glob = format!("__omni_lifted_img_{}", mod_id);
    let mut tmp = 0usize;
    let _ = writeln!(
        out,
        "define internal void @__omni_indirect_call_{id}(%OmniCpuState* nocapture %st, i64 %target) nounwind {{
entry:
  %img_base = ptrtoint [{len} x i8]* @{glob} to i64
  %rel_host = sub i64 %target, %img_base
  %is_host_fn = icmp ult i64 %rel_host, {len}
  %as_vma = add i64 %rel_host, {base}
  %norm_target = select i1 %is_host_fn, i64 %as_vma, i64 %target
  switch i64 %norm_target, label %sw_default [",
        id = mod_id,
        len = img_len,
        glob = img_glob,
        base = img.base_vma,
    );

    let mut sorted_exts: Vec<(u64, String)> = img
        .ext_sentinels
        .iter()
        .map(|(&k, v)| (k, v.clone()))
        .collect();
    sorted_exts.sort_by_key(|(k, _)| *k);
    for (sentinel, _) in &sorted_exts {
        let _ = writeln!(out, "    i64 {}, label %ext_{:x}", sentinel, sentinel);
    }
    for &fvma in lifted_fns {
        let _ = writeln!(out, "    i64 {}, label %fn_{:x}", fvma, fvma);
    }
    out.push_str("  ]\n");

    for (sentinel, sym_name) in &sorted_exts {
        let _ = writeln!(out, "ext_{:x}:", sentinel);
        ext_tracker.emit_external_call(
            out,
            &mut tmp,
            BinaryArch::Aarch64,
            img.abi,
            mod_id,
            sym_name,
        );
        out.push_str("  ret void\n");
    }
    for &fvma in lifted_fns {
        let _ = writeln!(out, "fn_{:x}:", fvma);
        let _ = writeln!(
            out,
            "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)\n  ret void",
            mod_id, fvma
        );
    }
    out.push_str("sw_default:\n  ret void\n}\n");
}

fn is_other_arm64_fn(img: &ParsedBinaryImage, entry_vma: u64, target_vma: u64) -> bool {
    if target_vma == entry_vma {
        return false;
    }
    if img.plt_stubs.contains_key(&target_vma) {
        return true;
    }
    if let Some(nm) = img.addr_to_sym.get(&target_vma) {
        if !nm.starts_with("LBB") && !nm.starts_with(".L") && !nm.starts_with("ltmp") {
            return true;
        }
    }
    false
}

/// Recover ARM64 `switch` jump tables (`adr xBase, table; ldrb/ldrh/ldrsw wOff, [xBase, xIdx...]; add xTarget, xBase, xOff; br xTarget`).
fn recover_arm64_jump_table(
    img: &ParsedBinaryImage,
    entry_vma: u64,
    recent_adrs: &[u64],
    hard_upper: u64,
) -> Vec<u64> {
    let mut out = Vec::new();
    for &tab_vma in recent_adrs.iter().rev() {
        if !img.is_mapped_vma(tab_vma) {
            continue;
        }
        // 1. Try 32-bit signed word offset table (`target = tab_vma + i32 * 4` or `tab_vma + i32`)
        for scale in [1i64, 4i64] {
            let mut hits = Vec::new();
            for i in 0..256u64 {
                let Some(rel) = img.read_i32_le(tab_vma + i * 4) else {
                    break;
                };
                let cand = (tab_vma as i64).wrapping_add((rel as i64) * scale) as u64;
                if (cand & 3) == 0 && cand >= entry_vma && cand < hard_upper && img.is_exec_vma(cand) {
                    hits.push(cand);
                } else {
                    break;
                }
            }
            if hits.len() >= 2 {
                out.extend(hits);
                return out;
            }
        }
        // 2. Try Apple Clang compressed 8-bit or 16-bit unsigned/signed instruction-offset jump table (`adr xBase, anchor; ldrb wOff, [tab]; add xTarget, xBase, wOff, lsl #2`)
        if recent_adrs.len() >= 2 {
            let anchor = recent_adrs[recent_adrs.len() - 1];
            let mut hits8 = Vec::new();
            for i in 0..128u64 {
                let Some(b) = img.read_bytes(tab_vma + i, 1) else {
                    break;
                };
                let cand = anchor.wrapping_add((b[0] as u64) * 4);
                if cand >= entry_vma && cand < hard_upper && img.is_exec_vma(cand) {
                    hits8.push(cand);
                } else {
                    break;
                }
            }
            if hits8.len() >= 2 {
                out.extend(hits8);
                return out;
            }
        }
    }
    out
}

fn lift_single_aarch64_fn(
    img: &ParsedBinaryImage,
    mod_id: &str,
    entry_vma: u64,
    ext_tracker: &mut ExternalDeclTracker,
    discovered_callees: &mut Vec<u64>,
) -> Result<(String, usize, u64), String> {
    if let Some(ext_sym) = img.plt_stubs.get(&entry_vma) {
        let mut out = String::new();
        let mut tmp = 0usize;
        let _ = writeln!(
            out,
            "define internal void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* nocapture %st) nounwind {{\nentry:",
            mod_id, entry_vma
        );
        ext_tracker.emit_external_call(
            &mut out,
            &mut tmp,
            BinaryArch::Aarch64,
            img.abi,
            mod_id,
            ext_sym,
        );
        out.push_str("  ret void\n}\n");
        return Ok((out, 1, 0));
    }

    let exec_end = img
        .exec_ranges
        .iter()
        .find(|&&(lo, hi)| entry_vma >= lo && entry_vma < hi)
        .map(|&(_, hi)| hi)
        .ok_or_else(|| format!("ARM64 entry 0x{:x} outside executable segment", entry_vma))?;
    let hard_upper = exec_end.min(entry_vma.saturating_add(0x4_0000));

    let mut leaders: BTreeSet<u64> = BTreeSet::new();
    let mut q: VecDeque<u64> = VecDeque::new();
    leaders.insert(entry_vma);
    q.push_back(entry_vma);

    let mut insns: BTreeMap<u64, u32> = BTreeMap::new();
    let mut recent_adrs: Vec<u64> = Vec::new();
    let mut tail_trampolines: BTreeSet<u64> = BTreeSet::new();
    let mut max_stack_sub = 0u64;

    while let Some(start_pc) = q.pop_front() {
        let mut pc = start_pc;
        let mut guard = 0usize;
        while pc + 4 <= hard_upper && guard < 16384 {
            guard += 1;
            if pc != start_pc && leaders.contains(&pc) {
                break;
            }
            if insns.contains_key(&pc) {
                break;
            }
            let Some(w) = img.read_u32_le(pc) else {
                break;
            };
            insns.insert(pc, w);
            let next_pc = pc + 4;

            // Track `SUB sp, sp, #imm` (`1101_0001_00...` Rd=31, Rn=31) or `SUB sp, sp, Xm` (`___chkstk_darwin`)
            if (w & 0xFF00_03FF) == 0xD100_03FF {
                let sh = ((w >> 22) & 1) * 12;
                let imm12 = ((w >> 10) & 0xFFF) as u64;
                max_stack_sub = max_stack_sub.saturating_add(imm12 << sh);
            } else if (w & 0xFFE0_03FF) == 0xCB20_03FF {
                max_stack_sub = max_stack_sub.max(65_536);
            }

            // Track `ADR` / `ADRP` (+ following `ADD`) for jump-table & callback discovery
            if (w & 0x1F00_0000) == 0x1000_0000 {
                let is_adrp = (w >> 31) != 0;
                let immlo = ((w >> 29) & 3) as i64;
                let immhi = (((w >> 5) & 0x7FFFF) as i32) << 13 >> 11;
                let raw = (immhi as i64) | immlo;
                let addr = if is_adrp {
                    ((pc & !0xFFF) as i64).wrapping_add(raw << 12) as u64
                } else {
                    (pc as i64).wrapping_add(raw) as u64
                };
                recent_adrs.push(addr);
                if recent_adrs.len() > 8 {
                    recent_adrs.remove(0);
                }
                if !is_adrp && img.is_exec_vma(addr) && !img.plt_stubs.contains_key(&addr) {
                    discovered_callees.push(addr);
                }
            } else if (w & 0xFF80_0000) == 0x9100_0000 {
                // `ADD Xd, Xn, #imm12` right after `ADRP`: refine last `ADRP` address!
                if let Some(&last_page) = recent_adrs.last() {
                    if (last_page & 0xFFF) == 0 {
                        let imm12 = ((w >> 10) & 0xFFF) as u64;
                        let full = last_page + imm12;
                        recent_adrs.push(full);
                        if recent_adrs.len() > 8 {
                            recent_adrs.remove(0);
                        }
                        if img.is_exec_vma(full) && !img.plt_stubs.contains_key(&full) {
                            discovered_callees.push(full);
                        }
                    }
                }
            }

            // Control-flow analysis on `w`
            // 1. `B imm26` (`000101...`) or `BL imm26` (`100101...`)
            if (w & 0x7C00_0000) == 0x1400_0000 {
                let is_bl = (w >> 31) != 0;
                let imm26 = (((w & 0x03FF_FFFF) as i32) << 6) >> 4;
                let target = (pc as i64).wrapping_add(imm26 as i64) as u64;
                if is_bl {
                    if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                        discovered_callees.push(target);
                    }
                    pc = next_pc;
                    continue;
                } else {
                    // Unconditional `B target`
                    if is_other_arm64_fn(img, entry_vma, target) {
                        if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                            discovered_callees.push(target);
                        }
                    } else if target >= img.base_vma && target < hard_upper && img.is_exec_vma(target) {
                        if leaders.insert(target) {
                            q.push_back(target);
                        }
                    }
                    break;
                }
            }

            // 2. `B.cond` (`01010100_imm19_0_cond`)
            if (w & 0xFF00_0010) == 0x5400_0000 {
                let imm19 = ((((w >> 5) & 0x7FFFF) as i32) << 13) >> 11;
                let target = (pc as i64).wrapping_add(imm19 as i64) as u64;
                if is_other_arm64_fn(img, entry_vma, target) {
                    tail_trampolines.insert(target);
                    if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                        discovered_callees.push(target);
                    }
                } else if target >= img.base_vma && target < hard_upper && img.is_exec_vma(target) {
                    if leaders.insert(target) {
                        q.push_back(target);
                    }
                }
                if leaders.insert(next_pc) {
                    q.push_back(next_pc);
                }
                break;
            }

            // 3. `CBZ` / `CBNZ` (`x011010x_imm19_Rt`)
            if (w & 0x7E00_0000) == 0x3400_0000 {
                let imm19 = ((((w >> 5) & 0x7FFFF) as i32) << 13) >> 11;
                let target = (pc as i64).wrapping_add(imm19 as i64) as u64;
                if is_other_arm64_fn(img, entry_vma, target) {
                    tail_trampolines.insert(target);
                    if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                        discovered_callees.push(target);
                    }
                } else if target >= img.base_vma && target < hard_upper && img.is_exec_vma(target) {
                    if leaders.insert(target) {
                        q.push_back(target);
                    }
                }
                if leaders.insert(next_pc) {
                    q.push_back(next_pc);
                }
                break;
            }

            // 4. `TBZ` / `TBNZ` (`x011011x_b40_imm14_Rt`)
            if (w & 0x7E00_0000) == 0x3600_0000 {
                let imm14 = ((((w >> 5) & 0x3FFF) as i32) << 18) >> 16;
                let target = (pc as i64).wrapping_add(imm14 as i64) as u64;
                if is_other_arm64_fn(img, entry_vma, target) {
                    tail_trampolines.insert(target);
                    if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                        discovered_callees.push(target);
                    }
                } else if target >= img.base_vma && target < hard_upper && img.is_exec_vma(target) {
                    if leaders.insert(target) {
                        q.push_back(target);
                    }
                }
                if leaders.insert(next_pc) {
                    q.push_back(next_pc);
                }
                break;
            }

            // 5. `BR` / `BLR` / `RET` / `RETAA` / `RETAB` (`1101011_opc_11111_...`)
            if (w & 0xFE00_0000) == 0xD600_0000 {
                let opc = (w >> 21) & 0xF;
                if opc == 1 || opc == 9 {
                    // `BLR` / `BLRAA`: call & fall through!
                    pc = next_pc;
                    continue;
                }
                if opc == 0 || opc == 8 {
                    // `BR Rn` -> recover jump table targets!
                    let jt = recover_arm64_jump_table(img, entry_vma, &recent_adrs, hard_upper);
                    for t in jt {
                        if leaders.insert(t) {
                            q.push_back(t);
                        }
                    }
                }
                // `RET` (`opc == 2` or `RETAA`/`RETAB` `opc == 10`) or `BR` or `BRK`
                break;
            }

            // `BRK` / `UDF` trap (`D4200000` or `00000000`)
            if (w & 0xFFE0_001F) == 0xD420_0000 || w == 0 {
                break;
            }

            pc = next_pc;
        }
    }

    let sorted_leaders: Vec<u64> = leaders.iter().copied().collect();
    let mut out = String::new();
    let mut tmp = 0usize;
    let img_len = img.image_bytes.len().max(16);
    let img_glob = format!("__omni_lifted_img_{}", mod_id);

    let _ = writeln!(
        out,
        "define internal void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* nocapture %st) nounwind {{",
        mod_id, entry_vma
    );
    let _ = writeln!(out, "entry:\n  br label %bb_{:x}", entry_vma);

    for (b_idx, &bb_start) in sorted_leaders.iter().enumerate() {
        let next_leader = sorted_leaders.get(b_idx + 1).copied().unwrap_or(hard_upper);
        let _ = writeln!(out, "bb_{:x}:", bb_start);
        let mut pc = bb_start;
        let mut terminated = false;

        while pc < next_leader {
            let Some(&w) = insns.get(&pc) else {
                break;
            };
            let next_pc = pc + 4;
            terminated = emit_arm64_insn(
                &mut out,
                &mut tmp,
                img,
                mod_id,
                &img_glob,
                img_len,
                entry_vma,
                pc,
                next_pc,
                w,
                &sorted_leaders,
                &tail_trampolines,
                ext_tracker,
            )?;
            pc = next_pc;
            if terminated {
                break;
            }
        }

        if !terminated {
            if leaders.contains(&pc) {
                let _ = writeln!(out, "  br label %bb_{:x}", pc);
            } else {
                out.push_str("  ret void\n");
            }
        }
    }

    for &tc_target in &tail_trampolines {
        let _ = writeln!(out, "tail_tramp_{:x}:", tc_target);
        if let Some(ext_sym) = img.plt_stubs.get(&tc_target) {
            ext_tracker.emit_external_call(
                &mut out,
                &mut tmp,
                BinaryArch::Aarch64,
                img.abi,
                mod_id,
                ext_sym,
            );
        } else {
            let _ = writeln!(
                out,
                "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)",
                mod_id, tc_target
            );
        }
        out.push_str("  ret void\n");
    }

    out.push_str("}\n");
    Ok((out, insns.len(), max_stack_sub))
}

// ============================================================================
// ARM64 Architectural Helpers (`DecodeBitMasks`, Register Access, Conditions)
// ============================================================================

/// Exact ARMv8-A `DecodeBitMasks(immN, imms, immr)` returning `(wmask, tmask)`.
pub fn decode_arm64_bit_masks(sf: u32, imm_n: u32, imms: u32, immr: u32) -> Option<(u64, u64)> {
    let combined = (imm_n << 6) | ((!imms) & 0x3F);
    if combined == 0 {
        return None;
    }
    let len = 31 - combined.leading_zeros();
    if len < 1 || (sf == 0 && len >= 6) {
        return None;
    }
    let levels = (1u32 << len) - 1;
    let s = imms & levels;
    let r = immr & levels;
    let diff = s.wrapping_sub(r) & levels;
    let esize = 1u32 << len;

    let welem: u64 = if s + 1 == 64 {
        u64::MAX
    } else {
        (1u64 << (s + 1)) - 1
    };
    let telem: u64 = if diff + 1 == 64 {
        u64::MAX
    } else {
        (1u64 << (diff + 1)) - 1
    };

    let r_mod = r % esize;
    let w_rot = if r_mod == 0 {
        welem
    } else if esize == 64 {
        welem.rotate_right(r_mod)
    } else {
        let mask_e = (1u64 << esize) - 1;
        ((welem >> r_mod) | (welem << (esize - r_mod))) & mask_e
    };

    let mut wmask = 0u64;
    let mut tmask = 0u64;
    let total_bits = if sf == 1 { 64 } else { 32 };
    let mut pos = 0u32;
    while pos < total_bits {
        wmask |= w_rot << pos;
        tmask |= telem << pos;
        pos += esize;
    }
    Some((wmask, tmask))
}

fn emit_read_arm_reg(
    out: &mut String,
    tmp: &mut usize,
    reg: u32,
    sf: u32,
    allow_sp: bool,
) -> String {
    if reg == 31 && !allow_sp {
        return "0".to_string();
    }
    let p = next_t(tmp);
    let v64 = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  {} = load i64, i64* {}, align 8",
        p, reg, v64, p
    );
    if sf == 1 {
        v64
    } else {
        let v32 = next_t(tmp);
        let _ = writeln!(out, "  {} = trunc i64 {} to i32", v32, v64);
        v32
    }
}

fn emit_write_arm_reg(
    out: &mut String,
    tmp: &mut usize,
    reg: u32,
    sf: u32,
    allow_sp: bool,
    val: &str,
) {
    if reg == 31 && !allow_sp {
        return; // Writes to XZR/WZR are discarded!
    }
    let v64 = if sf == 1 {
        val.to_string()
    } else {
        // Writing W0..W30 zero-extends bits [63:32] to 64 bits!
        let z = next_t(tmp);
        let _ = writeln!(out, "  {} = zext i32 {} to i64", z, val);
        z
    };
    let p = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  store i64 {}, i64* {}, align 8",
        p, reg, v64, p
    );
}

fn emit_read_neon(out: &mut String, tmp: &mut usize, vreg: u32) -> String {
    let p = next_t(tmp);
    let v = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 {}\n  {} = load <16 x i8>, <16 x i8>* {}, align 16",
        p,
        vreg & 31,
        v,
        p
    );
    v
}

fn emit_write_neon(out: &mut String, tmp: &mut usize, vreg: u32, val_v16i8: &str) {
    let p = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 {}\n  store <16 x i8> {}, <16 x i8>* {}, align 16",
        p,
        vreg & 31,
        val_v16i8,
        p
    );
}

fn emit_store_flag(out: &mut String, tmp: &mut usize, flag_idx: u8, cond_i1: &str) {
    let z = next_t(tmp);
    let p = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = zext i1 {} to i8\n  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 2, i32 {}\n  store i8 {}, i8* {}, align 1",
        z, cond_i1, p, flag_idx, z, p
    );
}

fn emit_load_flag(out: &mut String, tmp: &mut usize, flag_idx: u8) -> String {
    let p = next_t(tmp);
    let b = next_t(tmp);
    let c = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 2, i32 {}\n  {} = load i8, i8* {}, align 1\n  {} = icmp ne i8 {}, 0",
        p, flag_idx, b, p, c, b
    );
    c
}

fn emit_eval_arm_cond(out: &mut String, tmp: &mut usize, cond: u32) -> String {
    let base = cond >> 1;
    let raw = match base {
        0 => emit_load_flag(out, tmp, FLAG_Z), // EQ / NE
        1 => emit_load_flag(out, tmp, FLAG_C), // CS(HS) / CC(LO)
        2 => emit_load_flag(out, tmp, FLAG_N), // MI / PL
        3 => emit_load_flag(out, tmp, FLAG_V), // VS / VC
        4 => {
            // HI (`C == 1 && Z == 0`) / LS
            let c = emit_load_flag(out, tmp, FLAG_C);
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let nz = next_t(tmp);
            let r = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = xor i1 {}, true\n  {} = and i1 {}, {}",
                nz, z, r, c, nz
            );
            r
        }
        5 => {
            // GE (`N == V`) / LT (`N != V`)
            let n = emit_load_flag(out, tmp, FLAG_N);
            let v = emit_load_flag(out, tmp, FLAG_V);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = icmp eq i1 {}, {}", r, n, v);
            r
        }
        6 => {
            // GT (`Z == 0 && N == V`) / LE
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let n = emit_load_flag(out, tmp, FLAG_N);
            let v = emit_load_flag(out, tmp, FLAG_V);
            let nz = next_t(tmp);
            let eq = next_t(tmp);
            let r = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = xor i1 {}, true\n  {} = icmp eq i1 {}, {}\n  {} = and i1 {}, {}",
                nz, z, eq, n, v, r, nz, eq
            );
            r
        }
        _ => return "true".to_string(), // AL / NV
    };
    if (cond & 1) == 1 && cond < 14 {
        let inv = next_t(tmp);
        let _ = writeln!(out, "  {} = xor i1 {}, true", inv, raw);
        inv
    } else {
        raw
    }
}

fn emit_shift_reg(
    out: &mut String,
    tmp: &mut usize,
    val: &str,
    sf: u32,
    shift_type: u32,
    amount: u32,
) -> String {
    if amount == 0 {
        return val.to_string();
    }
    let ty = if sf == 1 { "i64" } else { "i32" };
    let bits = if sf == 1 { 64 } else { 32 };
    let amt = amount & (bits - 1);
    let r = next_t(tmp);
    match shift_type & 3 {
        0 => {
            let _ = writeln!(out, "  {} = shl {} {}, {}", r, ty, val, amt);
        }
        1 => {
            let _ = writeln!(out, "  {} = lshr {} {}, {}", r, ty, val, amt);
        }
        2 => {
            let _ = writeln!(out, "  {} = ashr {} {}, {}", r, ty, val, amt);
        }
        _ => {
            let p1 = next_t(tmp);
            let p2 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = lshr {} {}, {}\n  {} = shl {} {}, {}\n  {} = or {} {}, {}",
                p1,
                ty,
                val,
                amt,
                p2,
                ty,
                val,
                bits - amt,
                r,
                ty,
                p1,
                p2
            );
        }
    }
    r
}

fn emit_extend_reg(
    out: &mut String,
    tmp: &mut usize,
    rm_val64: &str,
    sf: u32,
    option: u32,
    shift: u32,
) -> String {
    let ty = if sf == 1 { "i64" } else { "i32" };
    // option: 0=UXTB, 1=UXTH, 2=UXTW, 3=UXTX, 4=SXTB, 5=SXTH, 6=SXTW, 7=SXTX
    let ext = match option & 7 {
        0 => {
            let b = next_t(tmp);
            let z = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = trunc i64 {} to i8\n  {} = zext i8 {} to {}",
                b, rm_val64, z, b, ty
            );
            z
        }
        1 => {
            let h = next_t(tmp);
            let z = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = trunc i64 {} to i16\n  {} = zext i16 {} to {}",
                h, rm_val64, z, h, ty
            );
            z
        }
        2 => {
            let w = next_t(tmp);
            if sf == 1 {
                let z = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = trunc i64 {} to i32\n  {} = zext i32 {} to i64",
                    w, rm_val64, z, w
                );
                z
            } else {
                let _ = writeln!(out, "  {} = trunc i64 {} to i32", w, rm_val64);
                w
            }
        }
        4 => {
            let b = next_t(tmp);
            let s = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = trunc i64 {} to i8\n  {} = sext i8 {} to {}",
                b, rm_val64, s, b, ty
            );
            s
        }
        5 => {
            let h = next_t(tmp);
            let s = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = trunc i64 {} to i16\n  {} = sext i16 {} to {}",
                h, rm_val64, s, h, ty
            );
            s
        }
        6 => {
            let w = next_t(tmp);
            if sf == 1 {
                let s = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = trunc i64 {} to i32\n  {} = sext i32 {} to i64",
                    w, rm_val64, s, w
                );
                s
            } else {
                let _ = writeln!(out, "  {} = trunc i64 {} to i32", w, rm_val64);
                w
            }
        }
        _ => {
            // UXTX / SXTX
            if sf == 1 {
                rm_val64.to_string()
            } else {
                let w = next_t(tmp);
                let _ = writeln!(out, "  {} = trunc i64 {} to i32", w, rm_val64);
                w
            }
        }
    };
    if shift > 0 {
        let sh = next_t(tmp);
        let _ = writeln!(out, "  {} = shl {} {}, {}", sh, ty, ext, shift.min(4));
        sh
    } else {
        ext
    }
}

fn emit_add_sub_flags(
    out: &mut String,
    tmp: &mut usize,
    sf: u32,
    is_sub: bool,
    a: &str,
    b: &str,
    set_flags: bool,
) -> String {
    let ty = if sf == 1 { "i64" } else { "i32" };
    let res = next_t(tmp);
    if is_sub {
        let _ = writeln!(out, "  {} = sub {} {}, {}", res, ty, a, b);
    } else {
        let _ = writeln!(out, "  {} = add {} {}, {}", res, ty, a, b);
    }
    if set_flags {
        let z = next_t(tmp);
        let n = next_t(tmp);
        let c = next_t(tmp);
        let x1 = next_t(tmp);
        let x2 = next_t(tmp);
        let ovm = next_t(tmp);
        let v = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = icmp eq {} {}, 0\n  {} = icmp slt {} {}, 0",
            z, ty, res, n, ty, res
        );
        if is_sub {
            // ARM64 SUBS/CMP: C = 1 when `a >= b` (NO borrow)!
            let _ = writeln!(
                out,
                "  {} = icmp uge {} {}, {}\n  {} = xor {} {}, {}\n  {} = xor {} {}, {}\n  {} = and {} {}, {}\n  {} = icmp slt {} {}, 0",
                c, ty, a, b, x1, ty, a, b, x2, ty, a, res, ovm, ty, x1, x2, v, ty, ovm
            );
        } else {
            // ARM64 ADDS/CMN: C = 1 on unsigned carry (`res < a`)!
            let _ = writeln!(
                out,
                "  {} = icmp ult {} {}, {}\n  {} = xor {} {}, {}\n  {} = xor {} {}, {}\n  {} = and {} {}, {}\n  {} = icmp slt {} {}, 0",
                c, ty, res, a, x1, ty, a, res, x2, ty, b, res, ovm, ty, x1, x2, v, ty, ovm
            );
        }
        emit_store_flag(out, tmp, FLAG_Z, &z);
        emit_store_flag(out, tmp, FLAG_N, &n);
        emit_store_flag(out, tmp, FLAG_C, &c);
        emit_store_flag(out, tmp, FLAG_V, &v);
    }
    res
}

// ============================================================================
// Core AArch64 32-bit Instruction Decoder & Emitter
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn emit_arm64_insn(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    entry_vma: u64,
    pc: u64,
    next_pc: u64,
    w: u32,
    sorted_leaders: &[u64],
    tail_trampolines: &BTreeSet<u64>,
    ext_tracker: &mut ExternalDeclTracker,
) -> Result<bool, String> {
    // 0. System Hints (`NOP`, `BTI`, `PACIASP`, `AUTIASP`, `YIELD`, `DMB`, `DSB`, `ISB`, `PRFM`)
    if (w & 0xFFFF_F01F) == 0xD503_201F
        || (w & 0xFFFF_F01F) == 0xD503_301F
        || (w & 0xFF00_0000) == 0xF900_0000 && ((w >> 22) & 3) == 2 && ((w >> 30) & 3) == 3
    {
        return Ok(false);
    }
    // Barriers (`DMB`/`DSB`/`ISB` = `D5033xxx`) & `MRS TPIDR_EL0` (`D53BD04x`)
    if (w & 0xFFFF_F000) == 0xD503_3000 {
        return Ok(false);
    }
    if (w & 0xFFFF_FFE0) == 0xD53B_D040 || (w & 0xFFFF_FFE0) == 0xD53B_D060 {
        let rt = w & 31;
        emit_write_arm_reg(out, tmp, rt, 1, false, "0");
        return Ok(false);
    }

    let op0 = (w >> 25) & 0xF;

    // ========================================================================
    // Group 1: Data Processing - Immediate (`op0` in `1000` | `1001`, i.e. 8 | 9)
    // ========================================================================
    if op0 == 8 || op0 == 9 {
        let sub_op = (w >> 23) & 7;
        match sub_op {
            0 | 1 => {
                // `ADR` (`op=0`) / `ADRP` (`op=1`)
                let is_adrp = (w >> 31) != 0;
                let rd = w & 31;
                let immlo = ((w >> 29) & 3) as i64;
                let immhi = (((w >> 5) & 0x7FFFF) as i32) << 13 >> 11;
                let raw = (immhi as i64) | immlo;
                let target_vma = if is_adrp {
                    ((pc & !0xFFF) as i64).wrapping_add(raw << 12) as u64
                } else {
                    (pc as i64).wrapping_add(raw) as u64
                };
                // Materialize as real host pointer into `@__omni_lifted_img_<mod_id>` if inside mapped image!
                // Note: for `ADRP` (`4 KB` page), compute `host_ptr_of_pc_page + page_delta` OR if `target_vma` is within `[base_vma - 4096 .. base_vma + img_len + 4096]`,
                //Wait: think carefully about `ADRP x8, page` followed by `ADD x8, x8, #pageoff` OR `LDR x8, [x8, #pageoff]`!!
                // Look! On real hardware, `@__omni_lifted_img_<mod_id>` is aligned to 16 bytes (NOT necessarily 4096 bytes!),
                // so `(pc & !0xFFF) + (raw << 12) + (#imm12_from_linker)` equals the exact `symbol_vma` ONLY in VMA space (`target_vma + imm12 = sym_vma`)!
                // If `ADRP` converted `target_vma` to `host_img_base + (target_vma - img.base_vma)` right away, let's check:
                // Suppose `img.base_vma = 0x1000_0000` (which IS 4096-byte aligned: `0x1000_0000 & 0xFFF == 0`!!)!
                // Wait: check `target_vma + imm12`!
                // `host_val = host_img_base + (target_vma - 0x1000_0000)`!
                // Then the next instruction executes `ADD x8, x8, #imm12` (`x8 = host_val + imm12`)!
                // Look at `host_val + imm12`:
                // `host_img_base + (target_vma - 0x1000_0000) + imm12`
                // `= host_img_base + ((target_vma + imm12) - 0x1000_0000)`
                // `= host_img_base + (sym_vma - img.base_vma)`!!
                // IT CANCELLED OUT EXACTLY regardless of `@__omni_lifted_img`'s host alignment as long as `img.base_vma` is 4096-byte aligned (and `0x1000_0000` IS `0x1000 * 0x10000` = 4096-aligned!), EVEN IF `target_vma` is 1 page below `img.base_vma` (`signed_off = target_vma as i64 - img.base_vma as i64`)!!
                let signed_off = (target_vma as i64).wrapping_sub(img.base_vma as i64);
                if signed_off >= -4096 && signed_off <= (img_len as i64) + 4096 {
                    let base_i = next_t(tmp);
                    let res = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = ptrtoint [{} x i8]* @{} to i64\n  {} = add i64 {}, {}",
                        base_i, img_len, img_glob, res, base_i, signed_off
                    );
                    emit_write_arm_reg(out, tmp, rd, 1, false, &res);
                } else {
                    emit_write_arm_reg(out, tmp, rd, 1, false, &format!("{}", target_vma as i64));
                }
                return Ok(false);
            }
            2 | 3 => {
                // Add/subtract (immediate): `sf_op_S_100010_sh_imm12_Rn_Rd`
                let sf = (w >> 31) & 1;
                let is_sub = ((w >> 30) & 1) != 0;
                let set_flags = ((w >> 29) & 1) != 0;
                let sh = ((w >> 22) & 1) * 12;
                let imm12 = ((w >> 10) & 0xFFF) as u64;
                let rn = (w >> 5) & 31;
                let rd = w & 31;
                let imm_val = format!("{}", imm12 << sh);
                let a = emit_read_arm_reg(out, tmp, rn, sf, true);
                let res = emit_add_sub_flags(out, tmp, sf, is_sub, &a, &imm_val, set_flags);
                emit_write_arm_reg(out, tmp, rd, sf, !set_flags, &res);
                return Ok(false);
            }
            4 => {
                // Logical (immediate): `sf_opc_100100_N_immr_imms_Rn_Rd`
                let sf = (w >> 31) & 1;
                let opc = (w >> 29) & 3;
                let imm_n = (w >> 22) & 1;
                let immr = (w >> 16) & 0x3F;
                let imms = (w >> 10) & 0x3F;
                let rn = (w >> 5) & 31;
                let rd = w & 31;
                let (wmask, _) = decode_arm64_bit_masks(sf, imm_n, imms, immr)
                    .ok_or_else(|| format!("Invalid ARM64 logical bitmask 0x{:08x}", w))?;
                let ty = if sf == 1 { "i64" } else { "i32" };
                let imm_s = if sf == 1 {
                    format!("{}", wmask as i64)
                } else {
                    format!("{}", (wmask as u32) as i32)
                };
                let a = emit_read_arm_reg(out, tmp, rn, sf, false);
                let op_str = match opc {
                    0 | 3 => "and",
                    1 => "or",
                    _ => "xor",
                };
                let res = next_t(tmp);
                let _ = writeln!(out, "  {} = {} {} {}, {}", res, op_str, ty, a, imm_s);
                let set_flags = opc == 3;
                if set_flags {
                    let z = next_t(tmp);
                    let n = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = icmp eq {} {}, 0\n  {} = icmp slt {} {}, 0",
                        z, ty, res, n, ty, res
                    );
                    emit_store_flag(out, tmp, FLAG_Z, &z);
                    emit_store_flag(out, tmp, FLAG_N, &n);
                    emit_store_flag(out, tmp, FLAG_C, "false");
                    emit_store_flag(out, tmp, FLAG_V, "false");
                }
                emit_write_arm_reg(out, tmp, rd, sf, !set_flags, &res);
                return Ok(false);
            }
            5 => {
                // Move wide (immediate): `MOVN` (`opc=0`), `MOVZ` (`opc=2`), `MOVK` (`opc=3`)
                let sf = (w >> 31) & 1;
                let opc = (w >> 29) & 3;
                let hw = ((w >> 21) & 3) * 16;
                let imm16 = ((w >> 5) & 0xFFFF) as u64;
                let rd = w & 31;
                match opc {
                    0 => {
                        // MOVN
                        let v = !(imm16 << hw);
                        let s = if sf == 1 {
                            format!("{}", v as i64)
                        } else {
                            format!("{}", (v as u32) as i32)
                        };
                        emit_write_arm_reg(out, tmp, rd, sf, false, &s);
                    }
                    2 => {
                        // MOVZ
                        let v = imm16 << hw;
                        let s = if sf == 1 {
                            format!("{}", v as i64)
                        } else {
                            format!("{}", (v as u32) as i32)
                        };
                        emit_write_arm_reg(out, tmp, rd, sf, false, &s);
                    }
                    3 => {
                        // MOVK
                        let ty = if sf == 1 { "i64" } else { "i32" };
                        let old = emit_read_arm_reg(out, tmp, rd, sf, false);
                        let clear_m = !(0xFFFFu64 << hw);
                        let set_v = imm16 << hw;
                        let cm_s = if sf == 1 {
                            format!("{}", clear_m as i64)
                        } else {
                            format!("{}", (clear_m as u32) as i32)
                        };
                        let sv_s = if sf == 1 {
                            format!("{}", set_v as i64)
                        } else {
                            format!("{}", (set_v as u32) as i32)
                        };
                        let m1 = next_t(tmp);
                        let res = next_t(tmp);
                        let _ = writeln!(
                            out,
                            "  {} = and {} {}, {}\n  {} = or {} {}, {}",
                            m1, ty, old, cm_s, res, ty, m1, sv_s
                        );
                        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
                    }
                    _ => {}
                }
                return Ok(false);
            }
            6 => {
                // Bitfield (`SBFM` opc=0, `BFM` opc=1, `UBFM` opc=2)
                let sf = (w >> 31) & 1;
                let opc = (w >> 29) & 3;
                let imm_n = (w >> 22) & 1;
                let immr = (w >> 16) & 0x3F;
                let imms = (w >> 10) & 0x3F;
                let rn = (w >> 5) & 31;
                let rd = w & 31;
                let ty = if sf == 1 { "i64" } else { "i32" };
                let bits = if sf == 1 { 64 } else { 32 };
                let src = emit_read_arm_reg(out, tmp, rn, sf, false);

                if opc == 2 {
                    // `UBFM`: covers `LSL` (`imms != 63/31 && imms + 1 == immr`), `LSR` (`imms == 63/31`), `UXTB`, `UXTH`, `UBFX`, `UBFIZ`!
                    if imms == bits - 1 {
                        // LSR src, #immr
                        let r = emit_shift_reg(out, tmp, &src, sf, 1, immr);
                        emit_write_arm_reg(out, tmp, rd, sf, false, &r);
                        return Ok(false);
                    }
                    let (wmask, tmask) = decode_arm64_bit_masks(sf, imm_n, imms, immr)
                        .ok_or("Invalid UBFM mask")?;
                    let rot = emit_shift_reg(out, tmp, &src, sf, 3, immr);
                    let mask = wmask & tmask;
                    let ms = if sf == 1 {
                        format!("{}", mask as i64)
                    } else {
                        format!("{}", (mask as u32) as i32)
                    };
                    let res = next_t(tmp);
                    let _ = writeln!(out, "  {} = and {} {}, {}", res, ty, rot, ms);
                    emit_write_arm_reg(out, tmp, rd, sf, false, &res);
                    return Ok(false);
                } else if opc == 0 {
                    // `SBFM`: covers `ASR` (`imms == bits - 1`), `SXTB`, `SXTH`, `SXTW`, `SBFX`, `SBFIZ`!
                    if imms == bits - 1 {
                        let r = emit_shift_reg(out, tmp, &src, sf, 2, immr);
                        emit_write_arm_reg(out, tmp, rd, sf, false, &r);
                        return Ok(false);
                    }
                    if imms >= immr {
                        // `SBFX`: extract bits `[imms:immr]` and sign-extend from bit `imms - immr`!
                        let shl_amt = (bits - 1) - imms;
                        let ashr_amt = shl_amt + immr;
                        let s1 = emit_shift_reg(out, tmp, &src, sf, 0, shl_amt);
                        let s2 = emit_shift_reg(out, tmp, &s1, sf, 2, ashr_amt);
                        emit_write_arm_reg(out, tmp, rd, sf, false, &s2);
                    } else {
                        // `SBFIZ`: sign-extend low `imms + 1` bits, then shift left by `bits - immr`!
                        let shl_ext = (bits - 1) - imms;
                        let s1 = emit_shift_reg(out, tmp, &src, sf, 0, shl_ext);
                        let s2 = emit_shift_reg(out, tmp, &s1, sf, 2, shl_ext);
                        let s3 = emit_shift_reg(out, tmp, &s2, sf, 0, bits - immr);
                        emit_write_arm_reg(out, tmp, rd, sf, false, &s3);
                    }
                    return Ok(false);
                } else {
                    // `BFM` (`BFI` / `BFXIL`): `dst = (dst & ~(wmask & tmask)) | (ROR(src, immr) & (wmask & tmask))`
                    let (wmask, tmask) = decode_arm64_bit_masks(sf, imm_n, imms, immr)
                        .ok_or("Invalid BFM mask")?;
                    let mask = wmask & tmask;
                    let dst = emit_read_arm_reg(out, tmp, rd, sf, false);
                    let rot = emit_shift_reg(out, tmp, &src, sf, 3, immr);
                    let ms = if sf == 1 {
                        format!("{}", mask as i64)
                    } else {
                        format!("{}", (mask as u32) as i32)
                    };
                    let inv_ms = if sf == 1 {
                        format!("{}", (!mask) as i64)
                    } else {
                        format!("{}", (!(mask as u32)) as i32)
                    };
                    let p1 = next_t(tmp);
                    let p2 = next_t(tmp);
                    let res = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = and {} {}, {}\n  {} = and {} {}, {}\n  {} = or {} {}, {}",
                        p1, ty, dst, inv_ms, p2, ty, rot, ms, res, ty, p1, p2
                    );
                    emit_write_arm_reg(out, tmp, rd, sf, false, &res);
                    return Ok(false);
                }
            }
            7 => {
                // `EXTR` (`ROR` when `Rn == Rm`): `Rd = ((Rn:Rm) >> imms)[bits-1:0]`
                let sf = (w >> 31) & 1;
                let rm = (w >> 16) & 31;
                let imms = (w >> 10) & 0x3F;
                let rn = (w >> 5) & 31;
                let rd = w & 31;
                let ty = if sf == 1 { "i64" } else { "i32" };
                let bits = if sf == 1 { 64 } else { 32 };
                let hi = emit_read_arm_reg(out, tmp, rn, sf, false);
                let lo = emit_read_arm_reg(out, tmp, rm, sf, false);
                if imms == 0 {
                    emit_write_arm_reg(out, tmp, rd, sf, false, &lo);
                } else {
                    let s = imms & (bits - 1);
                    let p1 = next_t(tmp);
                    let p2 = next_t(tmp);
                    let res = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = lshr {} {}, {}\n  {} = shl {} {}, {}\n  {} = or {} {}, {}",
                        p1,
                        ty,
                        lo,
                        s,
                        p2,
                        ty,
                        hi,
                        bits - s,
                        res,
                        ty,
                        p1,
                        p2
                    );
                    emit_write_arm_reg(out, tmp, rd, sf, false, &res);
                }
                return Ok(false);
            }
            _ => {}
        }
    }

    // ========================================================================
    // Group 2: Branches & System (`op0` in `1010` | `1011`, i.e. 10 | 11)
    // ========================================================================
    if op0 == 10 || op0 == 11 {
        // `B` / `BL` (`x00101_imm26`)
        if (w & 0x7C00_0000) == 0x1400_0000 {
            let is_bl = (w >> 31) != 0;
            let imm26 = (((w & 0x03FF_FFFF) as i32) << 6) >> 4;
            let target = (pc as i64).wrapping_add(imm26 as i64) as u64;
            if is_bl {
                emit_write_arm_reg(out, tmp, 30, 1, false, &format!("{}", next_pc as i64));
                if let Some(ext_sym) = img.plt_stubs.get(&target) {
                    ext_tracker.emit_external_call(
                        out,
                        tmp,
                        BinaryArch::Aarch64,
                        img.abi,
                        mod_id,
                        ext_sym,
                    );
                } else if img.is_exec_vma(target) {
                    let _ = writeln!(
                        out,
                        "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)",
                        mod_id, target
                    );
                }
                return Ok(false);
            } else {
                if let Some(ext_sym) = img.plt_stubs.get(&target) {
                    ext_tracker.emit_external_call(
                        out,
                        tmp,
                        BinaryArch::Aarch64,
                        img.abi,
                        mod_id,
                        ext_sym,
                    );
                    out.push_str("  ret void\n");
                } else if is_other_arm64_fn(img, entry_vma, target) {
                    let _ = writeln!(
                        out,
                        "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)\n  ret void",
                        mod_id, target
                    );
                } else if sorted_leaders.binary_search(&target).is_ok() {
                    let _ = writeln!(out, "  br label %bb_{:x}", target);
                } else {
                    out.push_str("  ret void\n");
                }
                return Ok(true);
            }
        }

        // `B.cond` (`01010100_imm19_0_cond`)
        if (w & 0xFF00_0010) == 0x5400_0000 {
            let cond_code = w & 0xF;
            let imm19 = ((((w >> 5) & 0x7FFFF) as i32) << 13) >> 11;
            let target = (pc as i64).wrapping_add(imm19 as i64) as u64;
            let cond = emit_eval_arm_cond(out, tmp, cond_code);
            return emit_arm_cond_br(out, tmp, &cond, target, next_pc, sorted_leaders, tail_trampolines);
        }

        // `CBZ` / `CBNZ`
        if (w & 0x7E00_0000) == 0x3400_0000 {
            let sf = (w >> 31) & 1;
            let is_nz = ((w >> 24) & 1) != 0;
            let imm19 = ((((w >> 5) & 0x7FFFF) as i32) << 13) >> 11;
            let rt = w & 31;
            let target = (pc as i64).wrapping_add(imm19 as i64) as u64;
            let ty = if sf == 1 { "i64" } else { "i32" };
            let v = emit_read_arm_reg(out, tmp, rt, sf, false);
            let cond = next_t(tmp);
            let pred = if is_nz { "ne" } else { "eq" };
            let _ = writeln!(out, "  {} = icmp {} {} {}, 0", cond, pred, ty, v);
            return emit_arm_cond_br(out, tmp, &cond, target, next_pc, sorted_leaders, tail_trampolines);
        }

        // `TBZ` / `TBNZ`
        if (w & 0x7E00_0000) == 0x3600_0000 {
            let b5 = (w >> 31) & 1;
            let b40 = (w >> 19) & 0x1F;
            let bit_num = (b5 << 5) | b40;
            let is_nz = ((w >> 24) & 1) != 0;
            let imm14 = ((((w >> 5) & 0x3FFF) as i32) << 18) >> 16;
            let rt = w & 31;
            let target = (pc as i64).wrapping_add(imm14 as i64) as u64;
            let v64 = emit_read_arm_reg(out, tmp, rt, 1, false);
            let masked = next_t(tmp);
            let cond = next_t(tmp);
            let mask_val = (1u64 << bit_num) as i64;
            let pred = if is_nz { "ne" } else { "eq" };
            let _ = writeln!(
                out,
                "  {} = and i64 {}, {}\n  {} = icmp {} i64 {}, 0",
                masked, v64, mask_val, cond, pred, masked
            );
            return emit_arm_cond_br(out, tmp, &cond, target, next_pc, sorted_leaders, tail_trampolines);
        }

        // `BR` / `BLR` / `RET`
        if (w & 0xFE00_0000) == 0xD600_0000 {
            let opc = (w >> 21) & 0xF;
            let rn = (w >> 5) & 31;
            if opc == 2 || opc == 10 {
                // `RET` / `RETAA` / `RETAB`
                out.push_str("  ret void\n");
                return Ok(true);
            }
            let target_reg = emit_read_arm_reg(out, tmp, rn, 1, false);
            if opc == 1 || opc == 9 {
                // `BLR Rn` / `BLRAA`
                emit_write_arm_reg(out, tmp, 30, 1, false, &format!("{}", next_pc as i64));
                let _ = writeln!(
                    out,
                    "  call void @__omni_indirect_call_{}(%OmniCpuState* %st, i64 {})",
                    mod_id, target_reg
                );
                return Ok(false);
            }
            if opc == 0 || opc == 8 {
                // `BR Rn`: dispatch local switch jump-table leaders, falling back to indirect tail-call!
                let img_b = next_t(tmp);
                let rel_h = next_t(tmp);
                let is_h = next_t(tmp);
                let as_vma = next_t(tmp);
                let norm = next_t(tmp);
                let def_lbl = format!("br_tail_{}", *tmp);
                let _ = writeln!(
                    out,
                    "  {} = ptrtoint [{} x i8]* @{} to i64\n  {} = sub i64 {}, {}\n  {} = icmp ult i64 {}, {}\n  {} = add i64 {}, {}\n  {} = select i1 {}, i64 {}, i64 {}\n  switch i64 {}, label %{} [",
                    img_b, img_len, img_glob, rel_h, target_reg, img_b, is_h, rel_h, img_len, as_vma, rel_h, img.base_vma, norm, is_h, as_vma, target_reg, norm, def_lbl
                );
                for &ldr in sorted_leaders {
                    let _ = writeln!(out, "    i64 {}, label %bb_{:x}", ldr, ldr);
                }
                let _ = writeln!(
                    out,
                    "  ]\n{}:\n  call void @__omni_indirect_call_{}(%OmniCpuState* %st, i64 {})\n  ret void",
                    def_lbl, mod_id, target_reg
                );
                return Ok(true);
            }
        }

        // `BRK`
        if (w & 0xFFE0_001F) == 0xD420_0000 {
            out.push_str("  ret void\n");
            return Ok(true);
        }
    }

    // ========================================================================
    // Group 3: Data Processing - Register (`(op0 & 7) == 5`, i.e. 5 | 13)
    // ========================================================================
    if (op0 & 7) == 5 {
        return emit_arm64_dp_reg(out, tmp, w, ext_tracker);
    }

    // ========================================================================
    // Group 4: Loads and Stores (`(op0 & 5) == 4`, i.e. 4 | 6 | 12 | 14)
    // ========================================================================
    if (op0 & 5) == 4 {
        return emit_arm64_load_store(out, tmp, img, mod_id, img_glob, img_len, pc, w);
    }

    // ========================================================================
    // Group 5: Data Processing - Scalar FP & Advanced SIMD (`(op0 & 7) == 7`, i.e. 7 | 15)
    // ========================================================================
    if (op0 & 7) == 7 {
        return emit_arm64_simd_fp(out, tmp, w);
    }

    Err(format!(
        "Unsupported AArch64 instruction word 0x{:08x} at PC 0x{:x}",
        w, pc
    ))
}

fn emit_arm_cond_br(
    out: &mut String,
    tmp: &mut usize,
    cond: &str,
    target: u64,
    next_pc: u64,
    sorted_leaders: &[u64],
    tail_trampolines: &BTreeSet<u64>,
) -> Result<bool, String> {
    let t_lbl = if tail_trampolines.contains(&target) {
        format!("tail_tramp_{:x}", target)
    } else if sorted_leaders.binary_search(&target).is_ok() {
        format!("bb_{:x}", target)
    } else {
        format!("bb_{:x}", next_pc)
    };
    if sorted_leaders.binary_search(&next_pc).is_ok() {
        let _ = writeln!(out, "  br i1 {}, label %{}, label %bb_{:x}", cond, t_lbl, next_pc);
    } else {
        let rl = format!("arm_ret_{}", *tmp);
        let _ = writeln!(
            out,
            "  br i1 {}, label %{}, label %{}\n{}:\n  ret void",
            cond, t_lbl, rl, rl
        );
    }
    Ok(true)
}

fn emit_arm64_dp_reg(
    out: &mut String,
    tmp: &mut usize,
    w: u32,
    ext_tracker: &mut ExternalDeclTracker,
) -> Result<bool, String> {
    let sf = (w >> 31) & 1;
    let ty = if sf == 1 { "i64" } else { "i32" };
    let bits = if sf == 1 { 64u32 } else { 32u32 };

    // 1. Logical (shifted register): `sf_opc_01010_shift_N_Rm_imm6_Rn_Rd`
    if (w & 0x1F00_0000) == 0x0A00_0000 {
        let opc = (w >> 29) & 3;
        let shift = (w >> 22) & 3;
        let inv = ((w >> 21) & 1) != 0;
        let rm = (w >> 16) & 31;
        let imm6 = (w >> 10) & 0x3F;
        let rn = (w >> 5) & 31;
        let rd = w & 31;

        let a = emit_read_arm_reg(out, tmp, rn, sf, false);
        let b_raw = emit_read_arm_reg(out, tmp, rm, sf, false);
        let b_sh = emit_shift_reg(out, tmp, &b_raw, sf, shift, imm6);
        let b = if inv {
            let nb = next_t(tmp);
            let _ = writeln!(out, "  {} = xor {} {}, -1", nb, ty, b_sh);
            nb
        } else {
            b_sh
        };
        let op_str = match opc {
            0 | 3 => "and",
            1 => "or",
            _ => "xor",
        };
        let res = next_t(tmp);
        let _ = writeln!(out, "  {} = {} {} {}, {}", res, op_str, ty, a, b);
        if opc == 3 {
            let z = next_t(tmp);
            let n = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = icmp eq {} {}, 0\n  {} = icmp slt {} {}, 0",
                z, ty, res, n, ty, res
            );
            emit_store_flag(out, tmp, FLAG_Z, &z);
            emit_store_flag(out, tmp, FLAG_N, &n);
            emit_store_flag(out, tmp, FLAG_C, "false");
            emit_store_flag(out, tmp, FLAG_V, "false");
        }
        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        return Ok(false);
    }

    // 2. Add/subtract (shifted or extended register): `sf_op_S_01011_...`
    if (w & 0x1F00_0000) == 0x0B00_0000 {
        let is_sub = ((w >> 30) & 1) != 0;
        let set_flags = ((w >> 29) & 1) != 0;
        let is_ext = ((w >> 21) & 1) != 0;
        let rm = (w >> 16) & 31;
        let rn = (w >> 5) & 31;
        let rd = w & 31;

        if is_ext {
            let option = (w >> 13) & 7;
            let imm3 = (w >> 10) & 7;
            let a = emit_read_arm_reg(out, tmp, rn, sf, true);
            let rm64 = emit_read_arm_reg(out, tmp, rm, 1, false);
            let b = emit_extend_reg(out, tmp, &rm64, sf, option, imm3);
            let res = emit_add_sub_flags(out, tmp, sf, is_sub, &a, &b, set_flags);
            emit_write_arm_reg(out, tmp, rd, sf, !set_flags, &res);
        } else {
            let shift = (w >> 22) & 3;
            let imm6 = (w >> 10) & 0x3F;
            let a = emit_read_arm_reg(out, tmp, rn, sf, false);
            let rm_v = emit_read_arm_reg(out, tmp, rm, sf, false);
            let b = emit_shift_reg(out, tmp, &rm_v, sf, shift, imm6);
            let res = emit_add_sub_flags(out, tmp, sf, is_sub, &a, &b, set_flags);
            emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        }
        return Ok(false);
    }

    // 3. Add/subtract with carry (`ADC`/`ADCS`/`SBC`/`SBCS`): `sf_op_S_11010000_Rm_000000_Rn_Rd`
    if (w & 0x1FE0_FC00) == 0x1A00_0000 {
        let is_sub = ((w >> 30) & 1) != 0;
        let set_flags = ((w >> 29) & 1) != 0;
        let rm = (w >> 16) & 31;
        let rn = (w >> 5) & 31;
        let rd = w & 31;
        let a = emit_read_arm_reg(out, tmp, rn, sf, false);
        let b_raw = emit_read_arm_reg(out, tmp, rm, sf, false);
        let c_in = emit_load_flag(out, tmp, FLAG_C);
        let res = next_t(tmp);
        if is_sub {
            // ARM64 SBC: `a + ~b + C` = `a - b - (1 - C)`!
            let not_c = next_t(tmp);
            let borrow = next_t(tmp);
            let t1 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = xor i1 {}, true\n  {} = zext i1 {} to {}\n  {} = sub {} {}, {}\n  {} = sub {} {}, {}",
                not_c, c_in, borrow, not_c, ty, t1, ty, a, b_raw, res, ty, t1, borrow
            );
        } else {
            let carry = next_t(tmp);
            let t1 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = zext i1 {} to {}\n  {} = add {} {}, {}\n  {} = add {} {}, {}",
                carry, c_in, ty, t1, ty, a, b_raw, res, ty, t1, carry
            );
        }
        if set_flags {
            let z = next_t(tmp);
            let n = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = icmp eq {} {}, 0\n  {} = icmp slt {} {}, 0",
                z, ty, res, n, ty, res
            );
            emit_store_flag(out, tmp, FLAG_Z, &z);
            emit_store_flag(out, tmp, FLAG_N, &n);
        }
        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        return Ok(false);
    }

    // 4. Conditional compare (`CCMP` / `CCMN` reg & imm): `sf_op_1_11010010_...`
    if (w & 0x1FE0_0410) == 0x1A40_0000 && ((w >> 29) & 1) == 1 {
        let is_sub = ((w >> 30) & 1) != 0; // CCMP=1, CCMN=0
        let is_imm = ((w >> 11) & 1) != 0;
        let rm_or_imm5 = (w >> 16) & 31;
        let cond_code = (w >> 12) & 0xF;
        let rn = (w >> 5) & 31;
        let nzcv = w & 0xF;

        let cond = emit_eval_arm_cond(out, tmp, cond_code);
        // Save old flags, run `emit_add_sub_flags`, then select per flag based on `cond`!
        let a = emit_read_arm_reg(out, tmp, rn, sf, false);
        let b = if is_imm {
            format!("{}", rm_or_imm5)
        } else {
            emit_read_arm_reg(out, tmp, rm_or_imm5, sf, false)
        };
        let _ = emit_add_sub_flags(out, tmp, sf, is_sub, &a, &b, true);
        let f_n = if ((nzcv >> 3) & 1) != 0 { "true" } else { "false" };
        let f_z = if ((nzcv >> 2) & 1) != 0 { "true" } else { "false" };
        let f_c = if ((nzcv >> 1) & 1) != 0 { "true" } else { "false" };
        let f_v = if (nzcv & 1) != 0 { "true" } else { "false" };
        for (fidx, fallback_b) in [(FLAG_N, f_n), (FLAG_Z, f_z), (FLAG_C, f_c), (FLAG_V, f_v)] {
            let computed = emit_load_flag(out, tmp, fidx);
            let sel = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = select i1 {}, i1 {}, i1 {}",
                sel, cond, computed, fallback_b
            );
            emit_store_flag(out, tmp, fidx, &sel);
        }
        return Ok(false);
    }

    // 5. Conditional select (`CSEL`, `CSINC`, `CSINV`, `CSNEG`): `sf_op_0_11010100_Rm_cond_0_o2_Rn_Rd`
    if (w & 0x1FE0_0800) == 0x1A80_0000 && ((w >> 29) & 1) == 0 {
        let op = (w >> 30) & 1;
        let o2 = (w >> 10) & 1;
        let rm = (w >> 16) & 31;
        let cond_code = (w >> 12) & 0xF;
        let rn = (w >> 5) & 31;
        let rd = w & 31;

        let cond = emit_eval_arm_cond(out, tmp, cond_code);
        let t_val = emit_read_arm_reg(out, tmp, rn, sf, false);
        let m_raw = emit_read_arm_reg(out, tmp, rm, sf, false);
        let f_val = match (op, o2) {
            (0, 0) => m_raw, // CSEL
            (0, 1) => {
                // CSINC
                let r = next_t(tmp);
                let _ = writeln!(out, "  {} = add {} {}, 1", r, ty, m_raw);
                r
            }
            (1, 0) => {
                // CSINV
                let r = next_t(tmp);
                let _ = writeln!(out, "  {} = xor {} {}, -1", r, ty, m_raw);
                r
            }
            _ => {
                // CSNEG
                let r = next_t(tmp);
                let _ = writeln!(out, "  {} = sub {} 0, {}", r, ty, m_raw);
                r
            }
        };
        let res = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = select i1 {}, {} {}, {} {}",
            res, cond, ty, t_val, ty, f_val
        );
        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        return Ok(false);
    }

    // 6. Data-processing (2 source: `UDIV`, `SDIV`, `LSLV`, `LSRV`, `ASRV`, `RORV`): `sf_00_11010110_Rm_opcode_Rn_Rd`
    if (w & 0x5FE0_0000) == 0x1AC0_0000 {
        let rm = (w >> 16) & 31;
        let opcode = (w >> 10) & 0x3F;
        let rn = (w >> 5) & 31;
        let rd = w & 31;
        let a = emit_read_arm_reg(out, tmp, rn, sf, false);
        let b = emit_read_arm_reg(out, tmp, rm, sf, false);
        let res = next_t(tmp);
        match opcode {
            2 | 3 => {
                // UDIV (2) / SDIV (3) — ARM64 division by 0 returns 0 (does NOT trap)!
                let is_z = next_t(tmp);
                let safe_b = next_t(tmp);
                let q = next_t(tmp);
                let div_op = if opcode == 3 { "sdiv" } else { "udiv" };
                let _ = writeln!(
                    out,
                    "  {} = icmp eq {} {}, 0\n  {} = select i1 {}, {} 1, {} {}\n  {} = {} {} {}, {}\n  {} = select i1 {}, {} 0, {} {}",
                    is_z, ty, b, safe_b, is_z, ty, ty, b, q, div_op, ty, a, safe_b, res, is_z, ty, ty, q
                );
            }
            8 | 9 | 10 | 11 => {
                let cnt = next_t(tmp);
                let _ = writeln!(out, "  {} = and {} {}, {}", cnt, ty, b, bits - 1);
                match opcode {
                    8 => {
                        let _ = writeln!(out, "  {} = shl {} {}, {}", res, ty, a, cnt);
                    }
                    9 => {
                        let _ = writeln!(out, "  {} = lshr {} {}, {}", res, ty, a, cnt);
                    }
                    10 => {
                        let _ = writeln!(out, "  {} = ashr {} {}, {}", res, ty, a, cnt);
                    }
                    _ => {
                        let inv = next_t(tmp);
                        let inv_m = next_t(tmp);
                        let p1 = next_t(tmp);
                        let p2 = next_t(tmp);
                        let _ = writeln!(
                            out,
                            "  {} = sub {} {}, {}\n  {} = and {} {}, {}\n  {} = lshr {} {}, {}\n  {} = shl {} {}, {}\n  {} = or {} {}, {}",
                            inv,
                            ty,
                            bits,
                            cnt,
                            inv_m,
                            ty,
                            inv,
                            bits - 1,
                            p1,
                            ty,
                            a,
                            cnt,
                            p2,
                            ty,
                            a,
                            inv_m,
                            res,
                            ty,
                            p1,
                            p2
                        );
                    }
                }
            }
            _ => {
                let _ = writeln!(out, "  {} = add {} {}, 0", res, ty, a);
            }
        }
        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        return Ok(false);
    }

    // 7. Data-processing (1 source: `RBIT`, `REV16`, `REV32`, `REV`, `CLZ`): `sf_10_11010110_00000_opcode_Rn_Rd`
    if (w & 0x5FFF_0000) == 0x5AC0_0000 {
        let opcode = (w >> 10) & 0x3F;
        let rn = (w >> 5) & 31;
        let rd = w & 31;
        let a = emit_read_arm_reg(out, tmp, rn, sf, false);
        let res = next_t(tmp);
        match opcode {
            0 => {
                // RBIT
                ext_tracker
                    .needed_decls
                    .insert(format!("declare {t} @llvm.bitreverse.{t}({t})", t = ty));
                let _ = writeln!(
                    out,
                    "  {} = call {} @llvm.bitreverse.{}({} {})",
                    res, ty, ty, ty, a
                );
            }
            2 | 3 => {
                // REV32 / REV64 (`bswap`)
                if sf == 1 && opcode == 2 {
                    // REV32 on Xd: swap bytes within each 32-bit half!
                    ext_tracker
                        .needed_decls
                        .insert("declare <2 x i32> @llvm.bswap.v2i32(<2 x i32>)".to_string());
                    let v = next_t(tmp);
                    let s = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = bitcast i64 {} to <2 x i32>\n  {} = call <2 x i32> @llvm.bswap.v2i32(<2 x i32> {})\n  {} = bitcast <2 x i32> {} to i64",
                        v, a, s, v, res, s
                    );
                } else {
                    ext_tracker
                        .needed_decls
                        .insert(format!("declare {t} @llvm.bswap.{t}({t})", t = ty));
                    let _ = writeln!(out, "  {} = call {} @llvm.bswap.{}({} {})", res, ty, ty, ty, a);
                }
            }
            1 => {
                // REV16
                let vty = if sf == 1 { "<4 x i16>" } else { "<2 x i16>" };
                ext_tracker
                    .needed_decls
                    .insert(format!("declare {vt} @llvm.bswap.{s}({vt})", vt = vty, s = if sf == 1 { "v4i16" } else { "v2i16" }));
                let v = next_t(tmp);
                let s = next_t(tmp);
                let intr = if sf == 1 { "v4i16" } else { "v2i16" };
                let _ = writeln!(
                    out,
                    "  {} = bitcast {} {} to {}\n  {} = call {} @llvm.bswap.{}({} {})\n  {} = bitcast {} {} to {}",
                    v, ty, a, vty, s, vty, intr, vty, v, res, vty, s, ty
                );
            }
            4 => {
                // CLZ
                ext_tracker
                    .needed_decls
                    .insert(format!("declare {t} @llvm.ctlz.{t}({t}, i1)", t = ty));
                let _ = writeln!(
                    out,
                    "  {} = call {} @llvm.ctlz.{}({} {}, i1 false)",
                    res, ty, ty, ty, a
                );
            }
            _ => {
                let _ = writeln!(out, "  {} = add {} {}, 0", res, ty, a);
            }
        }
        emit_write_arm_reg(out, tmp, rd, sf, false, &res);
        return Ok(false);
    }

    // 8. Data-processing (3 source: `MADD`, `MSUB`, `SMADDL`, `SMSUBL`, `SMULH`, `UMADDL`, `UMSUBL`, `UMULH`): `sf_00_11011_op31_Rm_o0_Ra_Rn_Rd`
    if (w & 0x7F00_0000) == 0x1B00_0000 {
        let op31 = (w >> 21) & 7;
        let rm = (w >> 16) & 31;
        let o0 = (w >> 15) & 1;
        let ra = (w >> 10) & 31;
        let rn = (w >> 5) & 31;
        let rd = w & 31;

        match op31 {
            0 => {
                // MADD (o0=0) / MSUB (o0=1)
                let n = emit_read_arm_reg(out, tmp, rn, sf, false);
                let m = emit_read_arm_reg(out, tmp, rm, sf, false);
                let a = emit_read_arm_reg(out, tmp, ra, sf, false);
                let prod = next_t(tmp);
                let res = next_t(tmp);
                let add_sub = if o0 == 1 { "sub" } else { "add" };
                let _ = writeln!(
                    out,
                    "  {} = mul {} {}, {}\n  {} = {} {} {}, {}",
                    prod, ty, n, m, res, add_sub, ty, a, prod
                );
                emit_write_arm_reg(out, tmp, rd, sf, false, &res);
            }
            1 | 5 => {
                // SMADDL/SMSUBL (1) or UMADDL/UMSUBL (5): 64 = 64 +/- (32 * 32)
                let ext = if op31 == 1 { "sext" } else { "zext" };
                let n32 = emit_read_arm_reg(out, tmp, rn, 0, false);
                let m32 = emit_read_arm_reg(out, tmp, rm, 0, false);
                let a64 = emit_read_arm_reg(out, tmp, ra, 1, false);
                let n64 = next_t(tmp);
                let m64 = next_t(tmp);
                let prod = next_t(tmp);
                let res = next_t(tmp);
                let add_sub = if o0 == 1 { "sub" } else { "add" };
                let _ = writeln!(
                    out,
                    "  {} = {} i32 {} to i64\n  {} = {} i32 {} to i64\n  {} = mul i64 {}, {}\n  {} = {} i64 {}, {}",
                    n64, ext, n32, m64, ext, m32, prod, n64, m64, res, add_sub, a64, prod
                );
                emit_write_arm_reg(out, tmp, rd, 1, false, &res);
            }
            2 | 6 => {
                // SMULH (2) / UMULH (6): high 64 bits of 64x64 multiply!
                let ext = if op31 == 2 { "sext" } else { "zext" };
                let n64 = emit_read_arm_reg(out, tmp, rn, 1, false);
                let m64 = emit_read_arm_reg(out, tmp, rm, 1, false);
                let n128 = next_t(tmp);
                let m128 = next_t(tmp);
                let p128 = next_t(tmp);
                let sh = next_t(tmp);
                let res = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = {} i64 {} to i128\n  {} = {} i64 {} to i128\n  {} = mul i128 {}, {}\n  {} = lshr i128 {}, 64\n  {} = trunc i128 {} to i64",
                    n128, ext, n64, m128, ext, m64, p128, n128, m128, sh, p128, res, sh
                );
                emit_write_arm_reg(out, tmp, rd, 1, false, &res);
            }
            _ => {}
        }
        return Ok(false);
    }

    Err(format!("Unsupported ARM64 DP-register instruction 0x{:08x}", w))
}

#[allow(clippy::too_many_arguments)]
fn emit_arm64_load_store(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    pc: u64,
    w: u32,
) -> Result<bool, String> {
    let is_simd = ((w >> 26) & 1) != 0;

    // 1. Load literal (PC-relative): `opc_011_V_00_imm19_Rt`
    if (w & 0x3B00_0000) == 0x1800_0000 {
        let opc = (w >> 30) & 3;
        let imm19 = ((((w >> 5) & 0x7FFFF) as i32) << 13) >> 11;
        let rt = w & 31;
        let target_vma = (pc as i64).wrapping_add(imm19 as i64) as u64;
        let ea = if target_vma >= img.base_vma && (target_vma - img.base_vma) < img_len as u64 {
            let off = target_vma - img.base_vma;
            let p = next_t(tmp);
            let i = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 {}\n  {} = ptrtoint i8* {} to i64",
                p, img_len, img_len, img_glob, off, i, p
            );
            i
        } else {
            format!("{}", target_vma as i64)
        };
        let p8 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
            p8, mod_id, ea
        );
        if is_simd {
            let bytes = match opc {
                0 => 4,
                1 => 8,
                _ => 16,
            };
            emit_neon_mem_load(out, tmp, &p8, rt, bytes);
        } else {
            match opc {
                0 => {
                    let pt = next_t(tmp);
                    let v = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = bitcast i8* {} to i32*\n  {} = load i32, i32* {}, align 1",
                        pt, p8, v, pt
                    );
                    emit_write_arm_reg(out, tmp, rt, 0, false, &v);
                }
                1 => {
                    let pt = next_t(tmp);
                    let v = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = bitcast i8* {} to i64*\n  {} = load i64, i64* {}, align 1",
                        pt, p8, v, pt
                    );
                    emit_write_arm_reg(out, tmp, rt, 1, false, &v);
                }
                2 => {
                    // LDRSW literal
                    let pt = next_t(tmp);
                    let v32 = next_t(tmp);
                    let v64 = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = bitcast i8* {} to i32*\n  {} = load i32, i32* {}, align 1\n  {} = sext i32 {} to i64",
                        pt, p8, v32, pt, v64, v32
                    );
                    emit_write_arm_reg(out, tmp, rt, 1, false, &v64);
                }
                _ => {} // PRFM
            }
        }
        return Ok(false);
    }

    // 2. Load/Store Pair (`LDP` / `STP` / `LDNP` / `STNP` / `LDPSW`): `opc_101_V_mode_L_imm7_Rt2_Rn_Rt`
    if (w & 0x3A00_0000) == 0x2800_0000 {
        let opc = (w >> 30) & 3;
        let mode = (w >> 23) & 7; // 0=no-alloc, 1=post-idx, 2=signed-offset, 3=pre-idx
        let is_load = ((w >> 22) & 1) != 0;
        let imm7_raw = ((w >> 15) & 0x7F) as i32;
        let imm7 = (imm7_raw << 25) >> 25; // sign-extend 7-bit
        let rt2 = (w >> 10) & 31;
        let rn = (w >> 5) & 31;
        let rt = w & 31;

        let elem_bytes: i64 = if is_simd {
            match opc {
                0 => 4,
                1 => 8,
                _ => 16,
            }
        } else if opc == 2 {
            8
        } else {
            4
        };
        let byte_off = (imm7 as i64) * elem_bytes;
        let base_addr = emit_read_arm_reg(out, tmp, rn, 1, true);
        let addr_with_off = if byte_off != 0 {
            let a = next_t(tmp);
            let _ = writeln!(out, "  {} = add i64 {}, {}", a, base_addr, byte_off);
            a
        } else {
            base_addr.clone()
        };
        let eff_addr = if mode == 1 {
            // Post-index: access at `base_addr`, write back `addr_with_off`!
            base_addr
        } else {
            addr_with_off.clone()
        };
        let addr2 = next_t(tmp);
        let p1 = next_t(tmp);
        let p2 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = add i64 {}, {}\n  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
            addr2, eff_addr, elem_bytes, p1, mod_id, eff_addr, p2, mod_id, addr2
        );

        if is_simd {
            if is_load {
                emit_neon_mem_load(out, tmp, &p1, rt, elem_bytes as usize);
                emit_neon_mem_load(out, tmp, &p2, rt2, elem_bytes as usize);
            } else {
                emit_neon_mem_store(out, tmp, &p1, rt, elem_bytes as usize);
                emit_neon_mem_store(out, tmp, &p2, rt2, elem_bytes as usize);
            }
        } else if is_load {
            if opc == 2 {
                let q1 = next_t(tmp);
                let q2 = next_t(tmp);
                let v1 = next_t(tmp);
                let v2 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to i64*\n  {} = bitcast i8* {} to i64*\n  {} = load i64, i64* {}, align 1\n  {} = load i64, i64* {}, align 1",
                    q1, p1, q2, p2, v1, q1, v2, q2
                );
                emit_write_arm_reg(out, tmp, rt, 1, false, &v1);
                emit_write_arm_reg(out, tmp, rt2, 1, false, &v2);
            } else if opc == 1 {
                // LDPSW
                let q1 = next_t(tmp);
                let q2 = next_t(tmp);
                let w1 = next_t(tmp);
                let w2 = next_t(tmp);
                let v1 = next_t(tmp);
                let v2 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to i32*\n  {} = bitcast i8* {} to i32*\n  {} = load i32, i32* {}, align 1\n  {} = load i32, i32* {}, align 1\n  {} = sext i32 {} to i64\n  {} = sext i32 {} to i64",
                    q1, p1, q2, p2, w1, q1, w2, q2, v1, w1, v2, w2
                );
                emit_write_arm_reg(out, tmp, rt, 1, false, &v1);
                emit_write_arm_reg(out, tmp, rt2, 1, false, &v2);
            } else {
                let q1 = next_t(tmp);
                let q2 = next_t(tmp);
                let v1 = next_t(tmp);
                let v2 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to i32*\n  {} = bitcast i8* {} to i32*\n  {} = load i32, i32* {}, align 1\n  {} = load i32, i32* {}, align 1",
                    q1, p1, q2, p2, v1, q1, v2, q2
                );
                emit_write_arm_reg(out, tmp, rt, 0, false, &v1);
                emit_write_arm_reg(out, tmp, rt2, 0, false, &v2);
            }
        } else {
            let sf = if opc == 2 { 1 } else { 0 };
            let ty = if sf == 1 { "i64" } else { "i32" };
            let v1 = emit_read_arm_reg(out, tmp, rt, sf, false);
            let v2 = emit_read_arm_reg(out, tmp, rt2, sf, false);
            let q1 = next_t(tmp);
            let q2 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to {}*\n  {} = bitcast i8* {} to {}*\n  store {} {}, {}* {}, align 1\n  store {} {}, {}* {}, align 1",
                q1, p1, ty, q2, p2, ty, ty, v1, ty, q1, ty, v2, ty, q2
            );
        }

        if mode == 1 || mode == 3 {
            emit_write_arm_reg(out, tmp, rn, 1, true, &addr_with_off);
        }
        return Ok(false);
    }

    // 3. Load-Acquire / Store-Release (`LDAR`, `LDARB`, `LDARH`, `LDAXR`, `STLR`, `STLRB`, `STLRH`, `STLXR`): `size_001000_...`
    if (w & 0x3F00_0000) == 0x0800_0000 {
        let size = (w >> 30) & 3;
        let o2 = (w >> 23) & 1;
        let is_load = ((w >> 22) & 1) != 0;
        let o1 = (w >> 21) & 1;
        let rs = (w >> 16) & 31;
        let rn = (w >> 5) & 31;
        let rt = w & 31;
        let base_addr = emit_read_arm_reg(out, tmp, rn, 1, true);
        let p8 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
            p8, mod_id, base_addr
        );
        let (ty, sf) = match size {
            0 => ("i8", 0),
            1 => ("i16", 0),
            2 => ("i32", 0),
            _ => ("i64", 1),
        };
        let pt = next_t(tmp);
        let _ = writeln!(out, "  {} = bitcast i8* {} to {}*", pt, p8, ty);
        if is_load {
            let v = next_t(tmp);
            let _ = writeln!(out, "  {} = load {}, {}* {}, align 1", v, ty, ty, pt);
            let wv = if size < 2 {
                let z = next_t(tmp);
                let _ = writeln!(out, "  {} = zext {} {} to i32", z, ty, v);
                z
            } else {
                v
            };
            emit_write_arm_reg(out, tmp, rt, sf, false, &wv);
        } else {
            let rv = emit_read_arm_reg(out, tmp, rt, sf, false);
            let sv = if size < 2 {
                let tr = next_t(tmp);
                let _ = writeln!(out, "  {} = trunc i32 {} to {}", tr, rv, ty);
                tr
            } else {
                rv
            };
            let _ = writeln!(out, "  store {} {}, {}* {}, align 1", ty, sv, ty, pt);
            // Exclusive store (`STLXR` / `STXR`, `o2 == 0 && o1 == 0`): write `0` (success!) to status register `Ws`!
            if o2 == 0 && o1 == 0 {
                emit_write_arm_reg(out, tmp, rs, 0, false, "0");
            }
        }
        return Ok(false);
    }

    // 4. Single Register Load/Store (`size_111_V_0_...` unsigned offset, pre/post/unscaled, and register offset!)
    if (w & 0x3A00_0000) == 0x3800_0000 {
        let size = (w >> 30) & 3;
        let opc = (w >> 22) & 3;
        let is_uoff = ((w >> 24) & 1) != 0;
        let rn = (w >> 5) & 31;
        let rt = w & 31;

        let log2_bytes = if is_simd && (opc & 2) != 0 && size == 0 {
            4u32 // 128-bit `Q` register (`size=00, opc=1x`)
        } else {
            size
        };
        let elem_bytes = 1usize << log2_bytes;

        let base_addr = emit_read_arm_reg(out, tmp, rn, 1, true);
        let mut writeback_addr: Option<String> = None;

        let eff_addr = if is_uoff {
            let imm12 = ((w >> 10) & 0xFFF) as u64;
            let byte_off = imm12 << log2_bytes;
            if byte_off == 0 {
                base_addr
            } else {
                let a = next_t(tmp);
                let _ = writeln!(out, "  {} = add i64 {}, {}", a, base_addr, byte_off);
                a
            }
        } else if ((w >> 21) & 1) == 1 && ((w >> 10) & 3) == 2 {
            // Register offset: `LDR/STR Rt, [Xn, Rm{, extend {#amount}}]`
            let rm = (w >> 16) & 31;
            let option = (w >> 13) & 7;
            let s_bit = (w >> 12) & 1;
            let shift_amt = if s_bit == 1 { log2_bytes } else { 0 };
            let rm64 = emit_read_arm_reg(out, tmp, rm, 1, false);
            let idx64 = emit_extend_reg(out, tmp, &rm64, 1, option, shift_amt);
            let a = next_t(tmp);
            let _ = writeln!(out, "  {} = add i64 {}, {}", a, base_addr, idx64);
            a
        } else {
            // Unscaled (`00`), Post-index (`01`), or Pre-index (`11`) 9-bit signed immediate!
            let idx_mode = (w >> 10) & 3;
            let imm9_raw = ((w >> 12) & 0x1FF) as i32;
            let imm9 = ((imm9_raw << 23) >> 23) as i64;
            let sum = if imm9 != 0 {
                let a = next_t(tmp);
                let _ = writeln!(out, "  {} = add i64 {}, {}", a, base_addr, imm9);
                a
            } else {
                base_addr.clone()
            };
            match idx_mode {
                1 => {
                    writeback_addr = Some(sum);
                    base_addr
                }
                3 => {
                    writeback_addr = Some(sum.clone());
                    sum
                }
                _ => sum,
            }
        };

        let p8 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
            p8, mod_id, eff_addr
        );

        if is_simd {
            let is_load = (opc & 1) != 0;
            if is_load {
                emit_neon_mem_load(out, tmp, &p8, rt, elem_bytes);
            } else {
                emit_neon_mem_store(out, tmp, &p8, rt, elem_bytes);
            }
        } else {
            // GP Load/Store:
            // `opc == 0`: STR (8/16/32/64)
            // `opc == 1`: LDR unsigned (8/16/32/64)
            // `opc == 2`: LDRSB/LDRSH/LDRSW to 64-bit `Xt` (note: size=3, opc=2 is PRFM no-op!)
            // `opc == 3`: LDRSB/LDRSH to 32-bit `Wt`
            let mem_ty = match elem_bytes {
                1 => "i8",
                2 => "i16",
                4 => "i32",
                _ => "i64",
            };
            if opc == 0 {
                let sf = if elem_bytes == 8 { 1 } else { 0 };
                let rv = emit_read_arm_reg(out, tmp, rt, sf, false);
                let sv = if elem_bytes < 4 {
                    let tr = next_t(tmp);
                    let _ = writeln!(out, "  {} = trunc i32 {} to {}", tr, rv, mem_ty);
                    tr
                } else {
                    rv
                };
                let pt = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to {}*\n  store {} {}, {}* {}, align 1",
                    pt, p8, mem_ty, mem_ty, sv, mem_ty, pt
                );
            } else if opc == 1 {
                let pt = next_t(tmp);
                let mv = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to {}*\n  {} = load {}, {}* {}, align 1",
                    pt, p8, mem_ty, mv, mem_ty, mem_ty, pt
                );
                if elem_bytes == 8 {
                    emit_write_arm_reg(out, tmp, rt, 1, false, &mv);
                } else if elem_bytes == 4 {
                    emit_write_arm_reg(out, tmp, rt, 0, false, &mv);
                } else {
                    let z32 = next_t(tmp);
                    let _ = writeln!(out, "  {} = zext {} {} to i32", z32, mem_ty, mv);
                    emit_write_arm_reg(out, tmp, rt, 0, false, &z32);
                }
            } else if opc == 2 && elem_bytes <= 4 {
                let pt = next_t(tmp);
                let mv = next_t(tmp);
                let sx64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to {}*\n  {} = load {}, {}* {}, align 1\n  {} = sext {} {} to i64",
                    pt, p8, mem_ty, mv, mem_ty, mem_ty, pt, sx64, mem_ty, mv
                );
                emit_write_arm_reg(out, tmp, rt, 1, false, &sx64);
            } else if opc == 3 && elem_bytes <= 2 {
                let pt = next_t(tmp);
                let mv = next_t(tmp);
                let sx32 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast i8* {} to {}*\n  {} = load {}, {}* {}, align 1\n  {} = sext {} {} to i32",
                    pt, p8, mem_ty, mv, mem_ty, mem_ty, pt, sx32, mem_ty, mv
                );
                emit_write_arm_reg(out, tmp, rt, 0, false, &sx32);
            }
        }

        if let Some(wb) = writeback_addr {
            emit_write_arm_reg(out, tmp, rn, 1, true, &wb);
        }
        return Ok(false);
    }

    // 5. NEON Post-index / single-structure `LD1` / `ST1` (`0Q001100...` / `0Q001101...`)
    if (w & 0xBF00_0000) == 0x0C00_0000 {
        let q = (w >> 30) & 1;
        let is_post = ((w >> 23) & 1) != 0;
        let is_load = ((w >> 22) & 1) != 0;
        let rm = (w >> 16) & 31;
        let opcode = (w >> 12) & 0xF;
        let rn = (w >> 5) & 31;
        let rt = w & 31;
        let nregs: usize = match opcode {
            7 => 1,
            10 => 2,
            6 => 3,
            2 => 4,
            _ => 1,
        };
        let reg_bytes = if q == 1 { 16usize } else { 8usize };
        let base_addr = emit_read_arm_reg(out, tmp, rn, 1, true);
        for k in 0..nregs {
            let ea = if k == 0 {
                base_addr.clone()
            } else {
                let a = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = add i64 {}, {}",
                    a,
                    base_addr,
                    (k * reg_bytes) as u64
                );
                a
            };
            let p8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
                p8, mod_id, ea
            );
            let vr = (rt + k as u32) & 31;
            if is_load {
                emit_neon_mem_load(out, tmp, &p8, vr, reg_bytes);
            } else {
                emit_neon_mem_store(out, tmp, &p8, vr, reg_bytes);
            }
        }
        if is_post {
            let inc = if rm == 31 {
                format!("{}", (nregs * reg_bytes) as u64)
            } else {
                emit_read_arm_reg(out, tmp, rm, 1, false)
            };
            let wb = next_t(tmp);
            let _ = writeln!(out, "  {} = add i64 {}, {}", wb, base_addr, inc);
            emit_write_arm_reg(out, tmp, rn, 1, true, &wb);
        }
        return Ok(false);
    }

    Err(format!(
        "Unsupported ARM64 Load/Store word 0x{:08x} at PC 0x{:x}",
        w, pc
    ))
}

fn emit_neon_mem_load(out: &mut String, tmp: &mut usize, p8: &str, rt: u32, bytes: usize) {
    match bytes {
        16 => {
            let pv = next_t(tmp);
            let v = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to <16 x i8>*\n  {} = load <16 x i8>, <16 x i8>* {}, align 1",
                pv, p8, v, pv
            );
            emit_write_neon(out, tmp, rt, &v);
        }
        8 => {
            let pt = next_t(tmp);
            let lo = next_t(tmp);
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to i64*\n  {} = load i64, i64* {}, align 1\n  {} = insertelement <2 x i64> zeroinitializer, i64 {}, i32 0\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                pt, p8, lo, pt, ins, lo, v8, ins
            );
            emit_write_neon(out, tmp, rt, &v8);
        }
        4 => {
            let pt = next_t(tmp);
            let lo = next_t(tmp);
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to i32*\n  {} = load i32, i32* {}, align 1\n  {} = insertelement <4 x i32> zeroinitializer, i32 {}, i32 0\n  {} = bitcast <4 x i32> {} to <16 x i8>",
                pt, p8, lo, pt, ins, lo, v8, ins
            );
            emit_write_neon(out, tmp, rt, &v8);
        }
        2 => {
            let pt = next_t(tmp);
            let lo = next_t(tmp);
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to i16*\n  {} = load i16, i16* {}, align 1\n  {} = insertelement <8 x i16> zeroinitializer, i16 {}, i32 0\n  {} = bitcast <8 x i16> {} to <16 x i8>",
                pt, p8, lo, pt, ins, lo, v8, ins
            );
            emit_write_neon(out, tmp, rt, &v8);
        }
        _ => {
            let lo = next_t(tmp);
            let ins = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = load i8, i8* {}, align 1\n  {} = insertelement <16 x i8> zeroinitializer, i8 {}, i32 0",
                lo, p8, ins, lo
            );
            emit_write_neon(out, tmp, rt, &ins);
        }
    }
}

fn emit_neon_mem_store(out: &mut String, tmp: &mut usize, p8: &str, rt: u32, bytes: usize) {
    let v = emit_read_neon(out, tmp, rt);
    match bytes {
        16 => {
            let pv = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast i8* {} to <16 x i8>*\n  store <16 x i8> {}, <16 x i8>* {}, align 1",
                pv, p8, v, pv
            );
        }
        8 => {
            let bc = next_t(tmp);
            let e0 = next_t(tmp);
            let pt = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0\n  {} = bitcast i8* {} to i64*\n  store i64 {}, i64* {}, align 1",
                bc, v, e0, bc, pt, p8, e0, pt
            );
        }
        4 => {
            let bc = next_t(tmp);
            let e0 = next_t(tmp);
            let pt = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = extractelement <4 x i32> {}, i32 0\n  {} = bitcast i8* {} to i32*\n  store i32 {}, i32* {}, align 1",
                bc, v, e0, bc, pt, p8, e0, pt
            );
        }
        2 => {
            let bc = next_t(tmp);
            let e0 = next_t(tmp);
            let pt = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <8 x i16>\n  {} = extractelement <8 x i16> {}, i32 0\n  {} = bitcast i8* {} to i16*\n  store i16 {}, i16* {}, align 1",
                bc, v, e0, bc, pt, p8, e0, pt
            );
        }
        _ => {
            let e0 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = extractelement <16 x i8> {}, i32 0\n  store i8 {}, i8* {}, align 1",
                e0, v, e0, p8
            );
        }
    }
}

/// Scalar FP & Advanced SIMD (`NEON`) instructions used by C libraries (`MOVI`, `EOR`, `ORR`, `AND`, `BIC`, `DUP`, `UMOV`, `ADD`, `SUB`, `CMEQ`, `FMOV`, `FADD`, `FSUB`, `FMUL`, `FDIV`, `FCMP`, `SCVTF`, `FCVTZS`).
fn emit_arm64_simd_fp(out: &mut String, tmp: &mut usize, w: u32) -> Result<bool, String> {
    let rd = w & 31;
    let rn = (w >> 5) & 31;
    let rm = (w >> 16) & 31;

    // 1. `MOVI` / `MVNI` (`0_Q_op_0111100000_a_b_c_cmode_01_d_e_f_g_h_Rd`)
    if (w & 0x9F80_0400) == 0x0F00_0400 {
        let q = (w >> 30) & 1;
        let op = (w >> 29) & 1;
        let cmode = (w >> 12) & 0xF;
        let abc = (w >> 16) & 7;
        let defgh = (w >> 5) & 0x1F;
        let imm8 = ((abc << 5) | defgh) as u8;

        let mut bytes16 = [0u8; 16];
        let limit = if q == 1 { 16 } else { 8 };
        if cmode == 14 && op == 0 {
            for k in 0..limit {
                bytes16[k] = imm8;
            }
        } else if cmode == 14 && op == 1 {
            // 64-bit mask per bit of imm8
            for b in 0..8 {
                let fill = if ((imm8 >> b) & 1) != 0 { 0xFF } else { 0x00 };
                bytes16[b] = fill;
                if q == 1 {
                    bytes16[8 + b] = fill;
                }
            }
        } else if (cmode & 9) == 0 {
            // 32-bit shifted immediate
            let sh = ((cmode >> 1) & 3) * 8;
            let mut val32 = (imm8 as u32) << sh;
            if op == 1 {
                val32 = !val32;
            }
            let vb = val32.to_le_bytes();
            for k in (0..limit).step_by(4) {
                bytes16[k..k + 4].copy_from_slice(&vb);
            }
        }
        let lit = bytes16
            .iter()
            .map(|&b| format!("i8 {}", b as i8))
            .collect::<Vec<_>>()
            .join(", ");
        emit_write_neon(out, tmp, rd, &format!("<{}>", lit));
        return Ok(false);
    }

    // 2. NEON Bitwise (`AND` `0Q001110_00_1_Rm_000111_Rn_Rd`, `BIC`, `ORR`/`MOV`, `ORN`, `EOR`, `BSL`, `BIT`, `BIF`)
    if (w & 0x9F20_FC00) == 0x0E20_1C00 {
        let q = (w >> 30) & 1;
        let u = (w >> 29) & 1;
        let size = (w >> 22) & 3;
        let a = emit_read_neon(out, tmp, rn);
        let b = emit_read_neon(out, tmp, rm);
        let res = next_t(tmp);
        match (u, size) {
            (0, 0) => {
                let _ = writeln!(out, "  {} = and <16 x i8> {}, {}", res, a, b);
            }
            (0, 1) => {
                // BIC: `a & ~b`
                let nb = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = xor <16 x i8> {}, <i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1>\n  {} = and <16 x i8> {}, {}",
                    nb, b, res, a, nb
                );
            }
            (0, 2) => {
                let _ = writeln!(out, "  {} = or <16 x i8> {}, {}", res, a, b);
            }
            (1, 0) => {
                if rn == rm {
                    emit_write_neon(out, tmp, rd, "zeroinitializer");
                    return Ok(false);
                }
                let _ = writeln!(out, "  {} = xor <16 x i8> {}, {}", res, a, b);
            }
            (1, 1) => {
                // BSL: `(rd & a) | (~rd & b)`
                let d = emit_read_neon(out, tmp, rd);
                let nd = next_t(tmp);
                let p1 = next_t(tmp);
                let p2 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = xor <16 x i8> {}, <i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1>\n  {} = and <16 x i8> {}, {}\n  {} = and <16 x i8> {}, {}\n  {} = or <16 x i8> {}, {}",
                    nd, d, p1, d, a, p2, nd, b, res, p1, p2
                );
            }
            _ => {
                let _ = writeln!(out, "  {} = or <16 x i8> {}, {}", res, a, b);
            }
        }
        let final_v = if q == 0 {
            let bc = next_t(tmp);
            let z = next_t(tmp);
            let r8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = insertelement <2 x i64> {}, i64 0, i32 1\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                bc, res, z, bc, r8, z
            );
            r8
        } else {
            res
        };
        emit_write_neon(out, tmp, rd, &final_v);
        return Ok(false);
    }

    // 3. NEON `DUP` (general) / `INS` (general) / `UMOV` / `SMOV`: `0_Q_001110000_imm5_0_op4_1_Rn_Rd`
    if (w & 0xBFE0_8400) == 0x0E00_0400 {
        let q = (w >> 30) & 1;
        let imm5 = (w >> 16) & 0x1F;
        let op4 = (w >> 11) & 0xF;
        if imm5 == 0 {
            return Ok(false);
        }
        let tz = imm5.trailing_zeros(); // 0=byte, 1=half, 2=word, 3=dword
        let lane = (imm5 >> (tz + 1)) as u32;
        let (ety, vty, lanes) = match tz {
            0 => ("i8", "<16 x i8>", 16usize),
            1 => ("i16", "<8 x i16>", 8),
            2 => ("i32", "<4 x i32>", 4),
            _ => ("i64", "<2 x i64>", 2),
        };
        match op4 {
            1 => {
                // DUP Vd.<T>, Wn/Xn
                let gp64 = emit_read_arm_reg(out, tmp, rn, 1, false);
                let el = if tz == 3 {
                    gp64
                } else {
                    let tr = next_t(tmp);
                    let _ = writeln!(out, "  {} = trunc i64 {} to {}", tr, gp64, ety);
                    tr
                };
                let ins0 = next_t(tmp);
                let splat = next_t(tmp);
                let mask_s = (0..lanes)
                    .map(|k| {
                        if q == 0 && k >= lanes / 2 {
                            format!("i32 {}", lanes) // zeroinitializer
                        } else {
                            "i32 0".to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = shufflevector {} {}, {} zeroinitializer, <{} x i32> <{}>",
                    ins0, vty, ety, el, splat, vty, ins0, vty, lanes, mask_s
                );
                let v8 = if tz == 0 {
                    splat
                } else {
                    let b = next_t(tmp);
                    let _ = writeln!(out, "  {} = bitcast {} {} to <16 x i8>", b, vty, splat);
                    b
                };
                emit_write_neon(out, tmp, rd, &v8);
                return Ok(false);
            }
            3 => {
                // INS Vd.<Ts>[lane], Wn/Xn
                let gp64 = emit_read_arm_reg(out, tmp, rn, 1, false);
                let el = if tz == 3 {
                    gp64
                } else {
                    let tr = next_t(tmp);
                    let _ = writeln!(out, "  {} = trunc i64 {} to {}", tr, gp64, ety);
                    tr
                };
                let cur = emit_read_neon(out, tmp, rd);
                let cv = if tz == 0 {
                    cur
                } else {
                    let b = next_t(tmp);
                    let _ = writeln!(out, "  {} = bitcast <16 x i8> {} to {}", b, cur, vty);
                    b
                };
                let upd = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = insertelement {} {}, {} {}, i32 {}",
                    upd, vty, cv, ety, el, lane
                );
                let v8 = if tz == 0 {
                    upd
                } else {
                    let b = next_t(tmp);
                    let _ = writeln!(out, "  {} = bitcast {} {} to <16 x i8>", b, vty, upd);
                    b
                };
                emit_write_neon(out, tmp, rd, &v8);
                return Ok(false);
            }
            5 | 7 => {
                // SMOV (5) / UMOV (7): `Wd/Xd = Vn.<Ts>[lane]`
                let sv = emit_read_neon(out, tmp, rn);
                let cv = if tz == 0 {
                    sv
                } else {
                    let b = next_t(tmp);
                    let _ = writeln!(out, "  {} = bitcast <16 x i8> {} to {}", b, sv, vty);
                    b
                };
                let ex = next_t(tmp);
                let _ = writeln!(out, "  {} = extractelement {} {}, i32 {}", ex, vty, cv, lane);
                let sf_out = if q == 1 { 1 } else { 0 };
                let out_ty = if sf_out == 1 { "i64" } else { "i32" };
                let ext_op = if op4 == 5 { "sext" } else { "zext" };
                let final_gp = if ety == out_ty {
                    ex
                } else {
                    let e = next_t(tmp);
                    let _ = writeln!(out, "  {} = {} {} {} to {}", e, ext_op, ety, ex, out_ty);
                    e
                };
                emit_write_arm_reg(out, tmp, rd, sf_out, false, &final_gp);
                return Ok(false);
            }
            _ => {}
        }
    }

    // 4. NEON Vector Integer `ADD`/`SUB` (`0_Q_U_01110_size_1_Rm_100001_Rn_Rd`) & `CMEQ`/`CMGT`/`CMHI`
    if (w & 0x9F20_FC00) == 0x0E20_8400 || (w & 0x9F20_FC00) == 0x0E20_8C00 {
        let u = (w >> 29) & 1;
        let size = (w >> 22) & 3;
        let is_cmp_eq = (w & 0x9F20_FC00) == 0x0E20_8C00 && u == 1;
        let vty = match size {
            0 => "<16 x i8>",
            1 => "<8 x i16>",
            2 => "<4 x i32>",
            _ => "<2 x i64>",
        };
        let bty = match size {
            0 => "<16 x i1>",
            1 => "<8 x i1>",
            2 => "<4 x i1>",
            _ => "<2 x i1>",
        };
        let a = emit_read_neon(out, tmp, rn);
        let b = emit_read_neon(out, tmp, rm);
        let ca = next_t(tmp);
        let cb = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}",
            ca, a, vty, cb, b, vty
        );
        if is_cmp_eq {
            let cmp = next_t(tmp);
            let sx = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = icmp eq {} {}, {}\n  {} = sext {} {} to {}\n  {} = bitcast {} {} to <16 x i8>",
                cmp, vty, ca, cb, sx, bty, cmp, vty, v8, vty, sx
            );
            emit_write_neon(out, tmp, rd, &v8);
        } else {
            let op = if u == 1 { "sub" } else { "add" };
            let r = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = {} {} {}, {}\n  {} = bitcast {} {} to <16 x i8>",
                r, op, vty, ca, cb, v8, vty, r
            );
            emit_write_neon(out, tmp, rd, &v8);
        }
        return Ok(false);
    }

    // 5. Scalar FP <-> GP Conversions & `FMOV` (`sf_00_11110_ftype_1_rmode_opcode_000000_Rn_Rd`)
    if (w & 0x7F20_03C0) == 0x1E20_0000 && (w & 0x0000_FC00) == 0 {
        let sf = (w >> 31) & 1;
        let ftype = (w >> 22) & 3; // 0=float, 1=double
        let rmode = (w >> 19) & 3;
        let opcode = (w >> 16) & 7;
        let (fty, vty, ibits) = if ftype == 1 {
            ("double", "<2 x double>", "i64")
        } else {
            ("float", "<4 x float>", "i32")
        };
        let gpty = if sf == 1 { "i64" } else { "i32" };

        match (rmode, opcode) {
            (0, 6) => {
                // FMOV Wd/Xd, Sn/Dn
                let sv = emit_read_neon(out, tmp, rn);
                let ivty = if ftype == 1 { "<2 x i64>" } else { "<4 x i32>" };
                let bc = next_t(tmp);
                let e0 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                    bc, sv, ivty, e0, ivty, bc
                );
                emit_write_arm_reg(out, tmp, rd, sf, false, &e0);
                return Ok(false);
            }
            (0, 7) => {
                // FMOV Sd/Dd, Wn/Xn
                let gv = emit_read_arm_reg(out, tmp, rn, sf, false);
                let ivty = if ftype == 1 { "<2 x i64>" } else { "<4 x i32>" };
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                    ins, ivty, ibits, gv, v8, ivty, ins
                );
                emit_write_neon(out, tmp, rd, &v8);
                return Ok(false);
            }
            (0, 2) | (0, 3) => {
                // SCVTF (2) / UCVTF (3)
                let gv = emit_read_arm_reg(out, tmp, rn, sf, false);
                let cvt = if opcode == 2 { "sitofp" } else { "uitofp" };
                let fp = next_t(tmp);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = {} {} {} to {}\n  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                    fp, cvt, gpty, gv, fty, ins, vty, fty, fp, v8, vty, ins
                );
                emit_write_neon(out, tmp, rd, &v8);
                return Ok(false);
            }
            (_, 0) | (_, 1) => {
                // FCVTZS / FCVTZU
                let sv = emit_read_neon(out, tmp, rn);
                let bc = next_t(tmp);
                let f0 = next_t(tmp);
                let iv = next_t(tmp);
                let cvt = if opcode == 0 { "fptosi" } else { "fptoui" };
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0\n  {} = {} {} {} to {}",
                    bc, sv, vty, f0, vty, bc, iv, cvt, fty, f0, gpty
                );
                emit_write_arm_reg(out, tmp, rd, sf, false, &iv);
                return Ok(false);
            }
            _ => {}
        }
    }

    // 6. Scalar FP 2-source (`FMUL`, `FDIV`, `FADD`, `FSUB`): `000_11110_ftype_1_Rm_op_10_Rn_Rd`
    if (w & 0xFF20_0C00) == 0x1E20_0800 {
        let ftype = (w >> 22) & 3;
        let op = (w >> 12) & 0xF;
        let (fty, vty) = if ftype == 1 {
            ("double", "<2 x double>")
        } else {
            ("float", "<4 x float>")
        };
        let fop = match op {
            0 => "fmul",
            1 => "fdiv",
            2 => "fadd",
            3 => "fsub",
            _ => "fadd",
        };
        let a = emit_read_neon(out, tmp, rn);
        let b = emit_read_neon(out, tmp, rm);
        let ca = next_t(tmp);
        let cb = next_t(tmp);
        let a0 = next_t(tmp);
        let b0 = next_t(tmp);
        let res = next_t(tmp);
        let ins = next_t(tmp);
        let v8 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0\n  {} = extractelement {} {}, i32 0\n  {} = {} {} {}, {}\n  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
            ca, a, vty, cb, b, vty, a0, vty, ca, b0, vty, cb, res, fop, fty, a0, b0, ins, vty, fty, res, v8, vty, ins
        );
        emit_write_neon(out, tmp, rd, &v8);
        return Ok(false);
    }

    // 7. Scalar FP `FCMP` (`000_11110_ftype_1_Rm_00_1000_Rn_00_opc_000`)
    if (w & 0xFF20_FC07) == 0x1E20_2000 {
        let ftype = (w >> 22) & 3;
        let cmp_zero = ((w >> 3) & 1) != 0;
        let (fty, vty) = if ftype == 1 {
            ("double", "<2 x double>")
        } else {
            ("float", "<4 x float>")
        };
        let a = emit_read_neon(out, tmp, rn);
        let ca = next_t(tmp);
        let a0 = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
            ca, a, vty, a0, vty, ca
        );
        let b0 = if cmp_zero {
            "0.0".to_string()
        } else {
            let b = emit_read_neon(out, tmp, rm);
            let cb = next_t(tmp);
            let ex = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                cb, b, vty, ex, vty, cb
            );
            ex
        };
        let z = next_t(tmp);
        let n = next_t(tmp);
        let c = next_t(tmp);
        let v = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = fcmp oeq {} {}, {}\n  {} = fcmp olt {} {}, {}\n  {} = fcmp uge {} {}, {}\n  {} = fcmp uno {} {}, {}",
            z, fty, a0, b0, n, fty, a0, b0, c, fty, a0, b0, v, fty, a0, b0
        );
        emit_store_flag(out, tmp, FLAG_Z, &z);
        emit_store_flag(out, tmp, FLAG_N, &n);
        emit_store_flag(out, tmp, FLAG_C, &c);
        emit_store_flag(out, tmp, FLAG_V, &v);
        return Ok(false);
    }

    // 8. Scalar FP `FMOV` immediate / register (`000_11110_ftype_1_000000_10000_Rn_Rd` or `..._imm8_100_00000_Rd`)
    if (w & 0xFF3F_FC00) == 0x1E20_4000 {
        let v = emit_read_neon(out, tmp, rn);
        emit_write_neon(out, tmp, rd, &v);
        return Ok(false);
    }
    if (w & 0xFF20_1FE0) == 0x1E20_1000 {
        let ftype = (w >> 22) & 3;
        let imm8 = ((w >> 13) & 0xFF) as u8;
        let val_f64 = decode_vfp_imm8(imm8);
        if ftype == 1 {
            let bits = val_f64.to_bits() as i64;
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = insertelement <2 x i64> zeroinitializer, i64 {}, i32 0\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                ins, bits, v8, ins
            );
            emit_write_neon(out, tmp, rd, &v8);
        } else {
            let bits = (val_f64 as f32).to_bits() as i32;
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = insertelement <4 x i32> zeroinitializer, i32 {}, i32 0\n  {} = bitcast <4 x i32> {} to <16 x i8>",
                ins, bits, v8, ins
            );
            emit_write_neon(out, tmp, rd, &v8);
        }
        return Ok(false);
    }

    // Graceful no-op for benign SIMD hint/rev/ext/tbl if unused, or report unsupported if needed:
    // Also support `EXT Vd.16B, Vn.16B, Vm.16B, #imm4` (`0Q101110_00_0_Rm_0_imm4_0_Rn_Rd`) & `REV64` (`0Q001110_size_10000_0000_10_Rn_Rd`)!
    if (w & 0xBF20_8400) == 0x2E00_0000 {
        let imm4 = ((w >> 11) & 0xF) as usize;
        let a = emit_read_neon(out, tmp, rn);
        let b = emit_read_neon(out, tmp, rm);
        let mask_s = (0..16)
            .map(|k| format!("i32 {}", imm4 + k))
            .collect::<Vec<_>>()
            .join(", ");
        let sh = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = shufflevector <16 x i8> {}, <16 x i8> {}, <16 x i32> <{}>",
            sh, a, b, mask_s
        );
        emit_write_neon(out, tmp, rd, &sh);
        return Ok(false);
    }
    if (w & 0xBF3F_FC00) == 0x0E20_0800 {
        // REV64 Vd.<T>, Vn.<T> (`size` in bits [23:22])
        let size = (w >> 22) & 3;
        let a = emit_read_neon(out, tmp, rn);
        let elem_sz = 1usize << size;
        let mut mask = Vec::with_capacity(16);
        for blk in [0usize, 8usize] {
            let elems_in_64 = 8 / elem_sz;
            for e in (0..elems_in_64).rev() {
                for b in 0..elem_sz {
                    mask.push(format!("i32 {}", blk + e * elem_sz + b));
                }
            }
        }
        let sh = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = shufflevector <16 x i8> {}, <16 x i8> poison, <16 x i32> <{}>",
            sh,
            a,
            mask.join(", ")
        );
        emit_write_neon(out, tmp, rd, &sh);
        return Ok(false);
    }

    let _ = is_ext_sentinel(0);
    Err(format!("Unsupported ARM64 SIMD/FP word 0x{:08x}", w))
}

fn decode_vfp_imm8(imm8: u8) -> f64 {
    let sign = ((imm8 >> 7) & 1) as u64;
    let not_b6 = (((imm8 >> 6) & 1) ^ 1) as u64;
    let rep = if not_b6 == 0 { 0xFFu64 } else { 0u64 };
    let exp = (not_b6 << 10) | ((rep & 0xFF) << 2) | (((imm8 >> 4) & 3) as u64);
    let frac = ((imm8 & 0xF) as u64) << 48;
    f64::from_bits((sign << 63) | (exp << 52) | frac)
}
