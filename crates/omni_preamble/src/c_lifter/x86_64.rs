use super::binfmt::{is_ext_sentinel, BinaryArch, ParsedBinaryImage};
use super::ext_abi::{next_t, ExternalDeclTracker, FLAG_C, FLAG_N, FLAG_V, FLAG_Z};
use iced_x86::{ConditionCode, Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

pub struct LiftedModuleOutput {
    pub ir_functions: String,
    pub lifted_fn_vmas: BTreeSet<u64>,
    pub total_instructions: usize,
    pub max_static_stack_alloc: u64,
    pub ext_tracker: ExternalDeclTracker,
}

/// Lift all reachable `x86_64` functions starting from `root_vmas` into LLVM IR.
pub fn lift_x86_64_functions(
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

    // Pre-collect sorted known function symbols to bound linear fallthroughs cleanly
    let mut sorted_sym_vmas: Vec<u64> = img
        .addr_to_sym
        .keys()
        .copied()
        .filter(|&v| img.is_exec_vma(v))
        .collect();
    sorted_sym_vmas.sort_unstable();
    sorted_sym_vmas.dedup();

    while let Some(fn_vma) = worklist.pop_front() {
        if !visited_fns.insert(fn_vma) {
            continue;
        }
        if !img.is_exec_vma(fn_vma) {
            continue;
        }
        let mut discovered_callees = Vec::new();
        let (fn_ir, insn_cnt, stack_sub) = lift_single_x86_64_fn(
            img,
            mod_id,
            fn_vma,
            &sorted_sym_vmas,
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

    // Emit `@__omni_indirect_call_<mod_id>` dispatcher covering all external sentinels & lifted internal functions
    emit_x86_indirect_call_helper(
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

fn emit_x86_indirect_call_helper(
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
            BinaryArch::X86_64,
            img.abi,
            mod_id,
            sym_name,
        );
        out.push_str("  ret void\n");
    }
    for &fvma in lifted_fns {
        let _ = writeln!(out, "fn_{:x}:", fvma);
        emit_push_dummy_ret_addr(out, &mut tmp, mod_id);
        let _ = writeln!(
            out,
            "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)\n  ret void",
            mod_id, fvma
        );
    }
    out.push_str("sw_default:\n  ret void\n}\n");
}

fn emit_push_dummy_ret_addr(out: &mut String, tmp: &mut usize, mod_id: &str) {
    let sp_p = next_t(tmp);
    let sp_v = next_t(tmp);
    let sp_new = next_t(tmp);
    let p8 = next_t(tmp);
    let p64 = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 4",
        sp_p
    );
    let _ = writeln!(out, "  {} = load i64, i64* {}, align 8", sp_v, sp_p);
    let _ = writeln!(out, "  {} = sub i64 {}, 8", sp_new, sp_v);
    let _ = writeln!(out, "  store i64 {}, i64* {}, align 8", sp_new, sp_p);
    let _ = writeln!(
        out,
        "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
        p8, mod_id, sp_new
    );
    let _ = writeln!(out, "  {} = bitcast i8* {} to i64*", p64, p8);
    let _ = writeln!(out, "  store i64 0, i64* {}, align 1", p64);
}

fn is_external_or_other_fn(
    img: &ParsedBinaryImage,
    entry_vma: u64,
    target_vma: u64,
) -> bool {
    if target_vma == entry_vma {
        return false;
    }
    if img.plt_stubs.contains_key(&target_vma) {
        return true;
    }
    if let Some(nm) = img.addr_to_sym.get(&target_vma) {
        if !nm.starts_with(".L") && !nm.starts_with("LBB") {
            return true;
        }
    }
    false
}

fn recover_jump_table_targets(
    img: &ParsedBinaryImage,
    entry_vma: u64,
    recent_rip_leas: &[u64],
    hard_upper: u64,
) -> Vec<u64> {
    let mut targets = Vec::new();
    for &table_vma in recent_rip_leas.iter().rev() {
        if !img.is_mapped_vma(table_vma) {
            continue;
        }
        // Try PIC 32-bit signed offset table (`target = table_vma + i32_entry`)
        let mut pic_hits = Vec::new();
        for idx in 0..256u64 {
            let Some(rel32) = img.read_i32_le(table_vma + idx * 4) else {
                break;
            };
            let cand = (table_vma as i64).wrapping_add(rel32 as i64) as u64;
            if cand >= entry_vma && cand < hard_upper && img.is_exec_vma(cand) {
                pic_hits.push(cand);
            } else {
                break;
            }
        }
        if pic_hits.len() >= 2 {
            targets.extend(pic_hits);
            break;
        }

        // Try Windows PE/COFF 32-bit RVA table (`target = img.base_vma + u32_rva`)
        let mut rva_hits = Vec::new();
        for idx in 0..256u64 {
            let Some(rva32) = img.read_u32_le(table_vma + idx * 4) else {
                break;
            };
            let cand = img.base_vma.wrapping_add(rva32 as u64);
            if cand >= entry_vma && cand < hard_upper && img.is_exec_vma(cand) {
                rva_hits.push(cand);
            } else {
                break;
            }
        }
        if rva_hits.len() >= 2 {
            targets.extend(rva_hits);
            break;
        }

        // Try 64-bit absolute VMA table
        let mut abs_hits = Vec::new();
        for idx in 0..256u64 {
            let Some(cand) = img.read_u64_le(table_vma + idx * 8) else {
                break;
            };
            if cand >= entry_vma && cand < hard_upper && img.is_exec_vma(cand) {
                abs_hits.push(cand);
            } else {
                break;
            }
        }
        if abs_hits.len() >= 2 {
            targets.extend(abs_hits);
            break;
        }
    }
    targets
}

fn lift_single_x86_64_fn(
    img: &ParsedBinaryImage,
    mod_id: &str,
    entry_vma: u64,
    _sorted_sym_vmas: &[u64],
    ext_tracker: &mut ExternalDeclTracker,
    discovered_callees: &mut Vec<u64>,
) -> Result<(String, usize, u64), String> {
    // If `entry_vma` itself is a PLT / import thunk (`jmp [rip + got]`), emit direct external call!
    if let Some(ext_sym) = img.plt_stubs.get(&entry_vma) {
        let mut out = String::new();
        let mut tmp = 0usize;
        let _ = writeln!(
            out,
            "define internal void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* nocapture %st) nounwind {{\nentry:",
            mod_id, entry_vma
        );
        // Pop caller's 8-byte return address first? Wait: `ext_abi` expects RSP BEFORE return address push if called via PLT,
        // OR if entered as a standalone sub (`push_dummy_ret_addr` was pushed), adjust RSP + 8 after!
        // Better: pop the 8-byte return slot BEFORE `emit_external_call` so stack args at `[RSP]`/`[RSP+32]` align!
        let sp_p = next_t(&mut tmp);
        let sp_v = next_t(&mut tmp);
        let sp_up = next_t(&mut tmp);
        let _ = writeln!(
            out,
            "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 4\n  {} = load i64, i64* {}, align 8\n  {} = add i64 {}, 8\n  store i64 {}, i64* {}, align 8",
            sp_p, sp_v, sp_p, sp_up, sp_v, sp_up, sp_p
        );
        ext_tracker.emit_external_call(
            &mut out,
            &mut tmp,
            BinaryArch::X86_64,
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
        .ok_or_else(|| format!("Entry VMA 0x{:x} is not in an executable segment", entry_vma))?;
    let hard_upper = exec_end.min(entry_vma.saturating_add(0x4_0000));

    // Pass 1: Discover all reachable basic-block leaders & decoded instructions via worklist
    let mut leaders: BTreeSet<u64> = BTreeSet::new();
    let mut q: VecDeque<u64> = VecDeque::new();
    leaders.insert(entry_vma);
    q.push_back(entry_vma);

    let mut decoded_map: BTreeMap<u64, Instruction> = BTreeMap::new();
    let mut recent_leas: Vec<u64> = Vec::new();
    let mut tail_call_trampolines: BTreeSet<u64> = BTreeSet::new();
    let mut max_stack_sub = 0u64;

    while let Some(start_pc) = q.pop_front() {
        let mut pc = start_pc;
        let mut guard = 0usize;
        while pc < hard_upper && guard < 16384 {
            guard += 1;
            if pc != start_pc && leaders.contains(&pc) {
                break;
            }
            if decoded_map.contains_key(&pc) {
                break;
            }
            let max_bytes = (hard_upper - pc).min(16) as usize;
            let Some(bytes) = img.read_bytes(pc, max_bytes) else {
                break;
            };
            let mut dec = Decoder::with_ip(64, bytes, pc, DecoderOptions::NONE);
            let instr = dec.decode();
            if instr.is_invalid() {
                break;
            }
            let next_pc = pc + instr.len() as u64;
            decoded_map.insert(pc, instr);

            // Track static `sub rsp, imm` OR dynamic `sub rsp, <reg>` (`___chkstk_ms`) for fiber-safe stack buffer sizing
            if instr.mnemonic() == Mnemonic::Sub
                && instr.op_count() == 2
                && instr.op0_kind() == OpKind::Register
                && instr.op0_register() == Register::RSP
            {
                if matches!(
                    instr.op1_kind(),
                    OpKind::Immediate8to64 | OpKind::Immediate32to64 | OpKind::Immediate64
                ) {
                    let imm = instr.immediate64();
                    if imm < 0x100_0000 {
                        max_stack_sub = max_stack_sub.max(imm);
                    }
                } else if instr.op1_kind() == OpKind::Register {
                    max_stack_sub = max_stack_sub.max(65_536);
                }
            }

            // Track RIP-relative `LEA` for switch jump-table discovery AND function-pointer references!
            if instr.mnemonic() == Mnemonic::Lea && instr.is_ip_rel_memory_operand() {
                let target_addr = instr.ip_rel_memory_address();
                recent_leas.push(target_addr);
                if recent_leas.len() > 8 {
                    recent_leas.remove(0);
                }
                if img.is_exec_vma(target_addr) && !img.plt_stubs.contains_key(&target_addr) {
                    discovered_callees.push(target_addr);
                }
            }

            let mn = instr.mnemonic();
            if mn == Mnemonic::Call {
                if matches!(
                    instr.op0_kind(),
                    OpKind::NearBranch64 | OpKind::NearBranch32 | OpKind::NearBranch16
                ) {
                    let target = instr.near_branch_target();
                    if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                        discovered_callees.push(target);
                    }
                } else if instr.is_ip_rel_memory_operand() {
                    let got_addr = instr.ip_rel_memory_address();
                    if let Some(val) = img.read_u64_le(got_addr) {
                        if !is_ext_sentinel(val) && img.is_exec_vma(val) {
                            discovered_callees.push(val);
                        }
                    }
                }
                pc = next_pc;
                continue;
            }

            if mn == Mnemonic::Ret
                || mn == Mnemonic::Ud2
                || mn == Mnemonic::Hlt
                || mn == Mnemonic::Int3
                || (mn == Mnemonic::Int && instr.immediate8() == 0x29)
            {
                break;
            }

            if mn == Mnemonic::Jmp {
                if matches!(
                    instr.op0_kind(),
                    OpKind::NearBranch64 | OpKind::NearBranch32 | OpKind::NearBranch16
                ) {
                    let target = instr.near_branch_target();
                    if is_external_or_other_fn(img, entry_vma, target) {
                        if !img.plt_stubs.contains_key(&target) && img.is_exec_vma(target) {
                            discovered_callees.push(target);
                        }
                    } else if target >= img.base_vma && target < hard_upper && img.is_exec_vma(target) {
                        if leaders.insert(target) {
                            q.push_back(target);
                        }
                    }
                } else if instr.is_ip_rel_memory_operand() {
                    let got_addr = instr.ip_rel_memory_address();
                    if let Some(val) = img.read_u64_le(got_addr) {
                        if !is_ext_sentinel(val) && img.is_exec_vma(val) {
                            discovered_callees.push(val);
                        }
                    }
                } else {
                    // Indirect register/indexed jump -> recover switch jump table!
                    let jt_targets =
                        recover_jump_table_targets(img, entry_vma, &recent_leas, hard_upper);
                    for jt in jt_targets {
                        if leaders.insert(jt) {
                            q.push_back(jt);
                        }
                    }
                }
                break;
            }

            if instr.condition_code() != ConditionCode::None
                && matches!(
                    instr.op0_kind(),
                    OpKind::NearBranch64 | OpKind::NearBranch32 | OpKind::NearBranch16
                )
            {
                let target = instr.near_branch_target();
                if is_external_or_other_fn(img, entry_vma, target) {
                    tail_call_trampolines.insert(target);
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

            pc = next_pc;
        }
    }

    // Ensure every instruction whose fallthrough hits another leader is split cleanly at basic-block boundaries
    let sorted_leaders: Vec<u64> = leaders.iter().copied().collect();

    // Pass 2: Emit LLVM IR for `@__omni_lifted_<mod_id>_sub_<entry_vma>`
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
            let Some(&instr) = decoded_map.get(&pc) else {
                break;
            };
            let next_pc = pc + instr.len() as u64;
            terminated = emit_x86_instr(
                &mut out,
                &mut tmp,
                img,
                mod_id,
                &img_glob,
                img_len,
                entry_vma,
                pc,
                next_pc,
                &instr,
                &sorted_leaders,
                &tail_call_trampolines,
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

    // Emit conditional tail-branch trampolines if any
    for &tc_target in &tail_call_trampolines {
        let _ = writeln!(out, "tail_tramp_{:x}:", tc_target);
        if let Some(ext_sym) = img.plt_stubs.get(&tc_target) {
            emit_pop_ret_slot(&mut out, &mut tmp);
            ext_tracker.emit_external_call(
                &mut out,
                &mut tmp,
                BinaryArch::X86_64,
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
    Ok((out, decoded_map.len(), max_stack_sub))
}

fn emit_pop_ret_slot(out: &mut String, tmp: &mut usize) {
    let sp_p = next_t(tmp);
    let sp_v = next_t(tmp);
    let sp_up = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 4\n  {} = load i64, i64* {}, align 8\n  {} = add i64 {}, 8\n  store i64 {}, i64* {}, align 8",
        sp_p, sp_v, sp_p, sp_up, sp_v, sp_up, sp_p
    );
}

// ============================================================================
// Register & Memory Operand Helpers for x86_64
// ============================================================================

#[derive(Clone, Copy)]
enum GprSub {
    Full64,
    Low32,
    Low16,
    Low8,
    High8, // AH, CH, DH, BH
}

fn map_x86_gpr(reg: Register) -> Option<(usize, GprSub)> {
    use Register::*;
    match reg {
        RAX => Some((0, GprSub::Full64)),
        EAX => Some((0, GprSub::Low32)),
        AX => Some((0, GprSub::Low16)),
        AL => Some((0, GprSub::Low8)),
        AH => Some((0, GprSub::High8)),

        RCX => Some((1, GprSub::Full64)),
        ECX => Some((1, GprSub::Low32)),
        CX => Some((1, GprSub::Low16)),
        CL => Some((1, GprSub::Low8)),
        CH => Some((1, GprSub::High8)),

        RDX => Some((2, GprSub::Full64)),
        EDX => Some((2, GprSub::Low32)),
        DX => Some((2, GprSub::Low16)),
        DL => Some((2, GprSub::Low8)),
        DH => Some((2, GprSub::High8)),

        RBX => Some((3, GprSub::Full64)),
        EBX => Some((3, GprSub::Low32)),
        BX => Some((3, GprSub::Low16)),
        BL => Some((3, GprSub::Low8)),
        BH => Some((3, GprSub::High8)),

        RSP => Some((4, GprSub::Full64)),
        ESP => Some((4, GprSub::Low32)),
        SP => Some((4, GprSub::Low16)),
        SPL => Some((4, GprSub::Low8)),

        RBP => Some((5, GprSub::Full64)),
        EBP => Some((5, GprSub::Low32)),
        BP => Some((5, GprSub::Low16)),
        BPL => Some((5, GprSub::Low8)),

        RSI => Some((6, GprSub::Full64)),
        ESI => Some((6, GprSub::Low32)),
        SI => Some((6, GprSub::Low16)),
        SIL => Some((6, GprSub::Low8)),

        RDI => Some((7, GprSub::Full64)),
        EDI => Some((7, GprSub::Low32)),
        DI => Some((7, GprSub::Low16)),
        DIL => Some((7, GprSub::Low8)),

        R8 => Some((8, GprSub::Full64)),
        R8D => Some((8, GprSub::Low32)),
        R8W => Some((8, GprSub::Low16)),
        R8L => Some((8, GprSub::Low8)),

        R9 => Some((9, GprSub::Full64)),
        R9D => Some((9, GprSub::Low32)),
        R9W => Some((9, GprSub::Low16)),
        R9L => Some((9, GprSub::Low8)),

        R10 => Some((10, GprSub::Full64)),
        R10D => Some((10, GprSub::Low32)),
        R10W => Some((10, GprSub::Low16)),
        R10L => Some((10, GprSub::Low8)),

        R11 => Some((11, GprSub::Full64)),
        R11D => Some((11, GprSub::Low32)),
        R11W => Some((11, GprSub::Low16)),
        R11L => Some((11, GprSub::Low8)),

        R12 => Some((12, GprSub::Full64)),
        R12D => Some((12, GprSub::Low32)),
        R12W => Some((12, GprSub::Low16)),
        R12L => Some((12, GprSub::Low8)),

        R13 => Some((13, GprSub::Full64)),
        R13D => Some((13, GprSub::Low32)),
        R13W => Some((13, GprSub::Low16)),
        R13L => Some((13, GprSub::Low8)),

        R14 => Some((14, GprSub::Full64)),
        R14D => Some((14, GprSub::Low32)),
        R14W => Some((14, GprSub::Low16)),
        R14L => Some((14, GprSub::Low8)),

        R15 => Some((15, GprSub::Full64)),
        R15D => Some((15, GprSub::Low32)),
        R15W => Some((15, GprSub::Low16)),
        R15L => Some((15, GprSub::Low8)),

        _ => Option::None,
    }
}

fn map_xmm_idx(reg: Register) -> Option<usize> {
    use Register::*;
    match reg {
        XMM0 | YMM0 => Some(0),
        XMM1 | YMM1 => Some(1),
        XMM2 | YMM2 => Some(2),
        XMM3 | YMM3 => Some(3),
        XMM4 | YMM4 => Some(4),
        XMM5 | YMM5 => Some(5),
        XMM6 | YMM6 => Some(6),
        XMM7 | YMM7 => Some(7),
        XMM8 | YMM8 => Some(8),
        XMM9 | YMM9 => Some(9),
        XMM10 | YMM10 => Some(10),
        XMM11 | YMM11 => Some(11),
        XMM12 | YMM12 => Some(12),
        XMM13 | YMM13 => Some(13),
        XMM14 | YMM14 => Some(14),
        XMM15 | YMM15 => Some(15),
        _ => Option::None,
    }
}

fn bits_ty(bits: u32) -> &'static str {
    match bits {
        8 => "i8",
        16 => "i16",
        32 => "i32",
        64 => "i64",
        128 => "i128",
        _ => "i64",
    }
}

fn gpr_bits(reg: Register) -> u32 {
    match map_x86_gpr(reg) {
        Some((_, GprSub::Full64)) => 64,
        Some((_, GprSub::Low32)) => 32,
        Some((_, GprSub::Low16)) => 16,
        Some((_, GprSub::Low8 | GprSub::High8)) => 8,
        None => 64,
    }
}

fn operand_bits(instr: &Instruction, op_idx: u32) -> u32 {
    match instr.op_kind(op_idx) {
        OpKind::Register => {
            let r = instr.op_register(op_idx);
            if map_xmm_idx(r).is_some() {
                128
            } else {
                gpr_bits(r)
            }
        }
        OpKind::Memory => {
            let sz = instr.memory_size().size() as u32 * 8;
            if sz == 0 {
                64
            } else {
                sz.min(128)
            }
        }
        OpKind::Immediate8 => 8,
        OpKind::Immediate16 => 16,
        OpKind::Immediate32 => 32,
        OpKind::Immediate8to16 => 16,
        OpKind::Immediate8to32 => 32,
        OpKind::Immediate8to64 | OpKind::Immediate32to64 | OpKind::Immediate64 => 64,
        _ => 64,
    }
}

fn emit_read_gpr_slot64(out: &mut String, tmp: &mut usize, slot: usize) -> String {
    let p = next_t(tmp);
    let v = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  {} = load i64, i64* {}, align 8",
        p, slot, v, p
    );
    v
}

fn emit_write_gpr_slot64(out: &mut String, tmp: &mut usize, slot: usize, val64: &str) {
    let p = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  store i64 {}, i64* {}, align 8",
        p, slot, val64, p
    );
}

fn emit_read_gpr_sized(out: &mut String, tmp: &mut usize, reg: Register) -> (String, u32) {
    let Some((slot, sub)) = map_x86_gpr(reg) else {
        return ("0".to_string(), 64);
    };
    let v64 = emit_read_gpr_slot64(out, tmp, slot);
    match sub {
        GprSub::Full64 => (v64, 64),
        GprSub::Low32 => {
            let t = next_t(tmp);
            let _ = writeln!(out, "  {} = trunc i64 {} to i32", t, v64);
            (t, 32)
        }
        GprSub::Low16 => {
            let t = next_t(tmp);
            let _ = writeln!(out, "  {} = trunc i64 {} to i16", t, v64);
            (t, 16)
        }
        GprSub::Low8 => {
            let t = next_t(tmp);
            let _ = writeln!(out, "  {} = trunc i64 {} to i8", t, v64);
            (t, 8)
        }
        GprSub::High8 => {
            let sh = next_t(tmp);
            let t = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = lshr i64 {}, 8\n  {} = trunc i64 {} to i8",
                sh, v64, t, sh
            );
            (t, 8)
        }
    }
}

fn emit_write_gpr_sized(out: &mut String, tmp: &mut usize, reg: Register, val: &str, val_bits: u32) {
    let Some((slot, sub)) = map_x86_gpr(reg) else {
        return;
    };
    match sub {
        GprSub::Full64 => {
            let v64 = if val_bits == 64 {
                val.to_string()
            } else {
                let z = next_t(tmp);
                let _ = writeln!(out, "  {} = zext {} {} to i64", z, bits_ty(val_bits), val);
                z
            };
            emit_write_gpr_slot64(out, tmp, slot, &v64);
        }
        GprSub::Low32 => {
            // x86_64 architectural rule: writing a 32-bit GPR zero-extends to 64 bits!
            let v32 = if val_bits == 32 {
                val.to_string()
            } else if val_bits > 32 {
                let tr = next_t(tmp);
                let _ = writeln!(out, "  {} = trunc {} {} to i32", tr, bits_ty(val_bits), val);
                tr
            } else {
                let z = next_t(tmp);
                let _ = writeln!(out, "  {} = zext {} {} to i32", z, bits_ty(val_bits), val);
                z
            };
            let z64 = next_t(tmp);
            let _ = writeln!(out, "  {} = zext i32 {} to i64", z64, v32);
            emit_write_gpr_slot64(out, tmp, slot, &z64);
        }
        GprSub::Low16 => {
            let old = emit_read_gpr_slot64(out, tmp, slot);
            let masked = next_t(tmp);
            let z64 = next_t(tmp);
            let merged = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = and i64 {}, -65536\n  {} = zext {} {} to i64\n  {} = or i64 {}, {}",
                masked,
                old,
                z64,
                bits_ty(val_bits),
                val,
                merged,
                masked,
                z64
            );
            emit_write_gpr_slot64(out, tmp, slot, &merged);
        }
        GprSub::Low8 => {
            let old = emit_read_gpr_slot64(out, tmp, slot);
            let masked = next_t(tmp);
            let z64 = next_t(tmp);
            let merged = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = and i64 {}, -256\n  {} = zext {} {} to i64\n  {} = or i64 {}, {}",
                masked,
                old,
                z64,
                bits_ty(val_bits),
                val,
                merged,
                masked,
                z64
            );
            emit_write_gpr_slot64(out, tmp, slot, &merged);
        }
        GprSub::High8 => {
            let old = emit_read_gpr_slot64(out, tmp, slot);
            let masked = next_t(tmp);
            let z64 = next_t(tmp);
            let sh = next_t(tmp);
            let merged = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = and i64 {}, -65281\n  {} = zext {} {} to i64\n  {} = shl i64 {}, 8\n  {} = or i64 {}, {}",
                masked,
                old,
                z64,
                bits_ty(val_bits),
                val,
                sh,
                z64,
                merged,
                masked,
                sh
            );
            emit_write_gpr_slot64(out, tmp, slot, &merged);
        }
    }
}

fn emit_read_xmm(out: &mut String, tmp: &mut usize, reg: Register) -> String {
    let idx = map_xmm_idx(reg).unwrap_or(0);
    let p = next_t(tmp);
    let v = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 {}\n  {} = load <16 x i8>, <16 x i8>* {}, align 16",
        p, idx, v, p
    );
    v
}

fn emit_write_xmm(out: &mut String, tmp: &mut usize, reg: Register, val_v16i8: &str) {
    let idx = map_xmm_idx(reg).unwrap_or(0);
    let p = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 {}\n  store <16 x i8> {}, <16 x i8>* {}, align 16",
        p, idx, val_v16i8, p
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

/// Compute effective address (`i64` host pointer or flat address) of an instruction's memory operand!
fn emit_mem_ea(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
) -> String {
    if instr.is_ip_rel_memory_operand() {
        let target_vma = instr.ip_rel_memory_address();
        if target_vma >= img.base_vma && (target_vma - img.base_vma) < img_len as u64 {
            let off = target_vma - img.base_vma;
            let p = next_t(tmp);
            let i = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 {}\n  {} = ptrtoint i8* {} to i64",
                p, img_len, img_len, img_glob, off, i, p
            );
            return i;
        }
        return format!("{}", target_vma as i64);
    }

    let mut cur: Option<String> = None;
    let base = instr.memory_base();
    if base != Register::None {
        let (bval, bbits) = emit_read_gpr_sized(out, tmp, base);
        let b64 = if bbits == 64 {
            bval
        } else {
            let z = next_t(tmp);
            let _ = writeln!(out, "  {} = zext {} {} to i64", z, bits_ty(bbits), bval);
            z
        };
        cur = Some(b64);
    }

    let index = instr.memory_index();
    if index != Register::None {
        let (ival, ibits) = emit_read_gpr_sized(out, tmp, index);
        let i64v = if ibits == 64 {
            ival
        } else {
            let z = next_t(tmp);
            let _ = writeln!(out, "  {} = zext {} {} to i64", z, bits_ty(ibits), ival);
            z
        };
        let scale = instr.memory_index_scale();
        let scaled = if scale > 1 {
            let s = next_t(tmp);
            let _ = writeln!(out, "  {} = mul i64 {}, {}", s, i64v, scale);
            s
        } else {
            i64v
        };
        cur = Some(match cur {
            Some(prev) => {
                let a = next_t(tmp);
                let _ = writeln!(out, "  {} = add i64 {}, {}", a, prev, scaled);
                a
            }
            None => scaled,
        });
    }

    let disp = instr.memory_displacement64() as i64;
    match cur {
        Some(prev) if disp != 0 => {
            let a = next_t(tmp);
            let _ = writeln!(out, "  {} = add i64 {}, {}", a, prev, disp);
            a
        }
        Some(prev) => prev,
        None => format!("{}", disp),
    }
}

fn emit_mem_load(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
    bits: u32,
) -> String {
    // Handle Linux `fs:[0x28]` / Windows `gs:[...]` TLS stack-canary loads deterministically!
    let seg = instr.memory_segment();
    if seg == Register::FS || seg == Register::GS {
        return match bits {
            64 => "6510615555426900570".to_string(), // 0x5A5A_5A5A_A5A5_A5A5
            32 => "1515870810".to_string(),
            16 => "23130".to_string(),
            _ => "90".to_string(),
        };
    }

    let ea = emit_mem_ea(out, tmp, img, img_glob, img_len, instr);
    let p8 = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
        p8, mod_id, ea
    );
    if bits == 128 {
        let pv = next_t(tmp);
        let v = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast i8* {} to <16 x i8>*\n  {} = load <16 x i8>, <16 x i8>* {}, align 1",
            pv, p8, v, pv
        );
        v
    } else {
        let ty = bits_ty(bits);
        let pt = next_t(tmp);
        let v = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast i8* {} to {}*\n  {} = load {}, {}* {}, align 1",
            pt, p8, ty, v, ty, ty, pt
        );
        v
    }
}

fn emit_mem_store(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
    val: &str,
    bits: u32,
) {
    let seg = instr.memory_segment();
    if seg == Register::FS || seg == Register::GS {
        return;
    }
    let ea = emit_mem_ea(out, tmp, img, img_glob, img_len, instr);
    let p8 = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
        p8, mod_id, ea
    );
    if bits == 128 {
        let pv = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast i8* {} to <16 x i8>*\n  store <16 x i8> {}, <16 x i8>* {}, align 1",
            pv, p8, val, pv
        );
    } else {
        let ty = bits_ty(bits);
        let pt = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = bitcast i8* {} to {}*\n  store {} {}, {}* {}, align 1",
            pt, p8, ty, ty, val, ty, pt
        );
    }
}

fn emit_read_op(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
    op_idx: u32,
    want_bits: u32,
) -> String {
    match instr.op_kind(op_idx) {
        OpKind::Register => {
            let r = instr.op_register(op_idx);
            if map_xmm_idx(r).is_some() {
                emit_read_xmm(out, tmp, r)
            } else {
                let (v, b) = emit_read_gpr_sized(out, tmp, r);
                coerce_int_width(out, tmp, &v, b, want_bits, false)
            }
        }
        OpKind::Memory => emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, want_bits),
        OpKind::Immediate8 => {
            let v = instr.immediate8() as u64;
            format!("{}", mask_for_bits(v, want_bits) as i64)
        }
        OpKind::Immediate16 => {
            let v = instr.immediate16() as u64;
            format!("{}", mask_for_bits(v, want_bits) as i64)
        }
        OpKind::Immediate32 => {
            let v = instr.immediate32() as u64;
            format!("{}", mask_for_bits(v, want_bits) as i64)
        }
        OpKind::Immediate8to16 => {
            let v = instr.immediate8to16() as i16 as i64;
            format!("{}", v)
        }
        OpKind::Immediate8to32 => {
            let v = instr.immediate8to32() as i64;
            format!("{}", v)
        }
        OpKind::Immediate8to64 => {
            let v = instr.immediate8to64();
            format!("{}", mask_signed_for_bits(v, want_bits))
        }
        OpKind::Immediate32to64 => {
            let v = instr.immediate32to64();
            format!("{}", mask_signed_for_bits(v, want_bits))
        }
        OpKind::Immediate64 => {
            let v = instr.immediate64() as i64;
            format!("{}", v)
        }
        _ => "0".to_string(),
    }
}

fn emit_write_op(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
    op_idx: u32,
    val: &str,
    bits: u32,
) {
    match instr.op_kind(op_idx) {
        OpKind::Register => {
            let r = instr.op_register(op_idx);
            if map_xmm_idx(r).is_some() {
                emit_write_xmm(out, tmp, r, val);
            } else {
                emit_write_gpr_sized(out, tmp, r, val, bits);
            }
        }
        OpKind::Memory => {
            emit_mem_store(out, tmp, img, mod_id, img_glob, img_len, instr, val, bits);
        }
        _ => {}
    }
}

fn coerce_int_width(
    out: &mut String,
    tmp: &mut usize,
    val: &str,
    from_bits: u32,
    to_bits: u32,
    signed: bool,
) -> String {
    if from_bits == to_bits {
        return val.to_string();
    }
    let r = next_t(tmp);
    if from_bits > to_bits {
        let _ = writeln!(
            out,
            "  {} = trunc {} {} to {}",
            r,
            bits_ty(from_bits),
            val,
            bits_ty(to_bits)
        );
    } else if signed {
        let _ = writeln!(
            out,
            "  {} = sext {} {} to {}",
            r,
            bits_ty(from_bits),
            val,
            bits_ty(to_bits)
        );
    } else {
        let _ = writeln!(
            out,
            "  {} = zext {} {} to {}",
            r,
            bits_ty(from_bits),
            val,
            bits_ty(to_bits)
        );
    }
    r
}

fn mask_for_bits(v: u64, bits: u32) -> u64 {
    match bits {
        8 => v & 0xFF,
        16 => v & 0xFFFF,
        32 => v & 0xFFFF_FFFF,
        _ => v,
    }
}

fn mask_signed_for_bits(v: i64, bits: u32) -> i64 {
    match bits {
        8 => (v as i8) as i64,
        16 => (v as i16) as i64,
        32 => (v as i32) as i64,
        _ => v,
    }
}

fn emit_zf_sf(out: &mut String, tmp: &mut usize, res: &str, bits: u32) {
    let ty = bits_ty(bits);
    let zf = next_t(tmp);
    let sf = next_t(tmp);
    let _ = writeln!(
        out,
        "  {} = icmp eq {} {}, 0\n  {} = icmp slt {} {}, 0",
        zf, ty, res, sf, ty, res
    );
    emit_store_flag(out, tmp, FLAG_Z, &zf);
    emit_store_flag(out, tmp, FLAG_N, &sf);
}

fn emit_eval_cond(out: &mut String, tmp: &mut usize, cc: ConditionCode) -> String {
    match cc {
        ConditionCode::e => emit_load_flag(out, tmp, FLAG_Z),
        ConditionCode::ne => {
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = xor i1 {}, true", r, z);
            r
        }
        ConditionCode::b => emit_load_flag(out, tmp, FLAG_C),
        ConditionCode::ae => {
            let c = emit_load_flag(out, tmp, FLAG_C);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = xor i1 {}, true", r, c);
            r
        }
        ConditionCode::s => emit_load_flag(out, tmp, FLAG_N),
        ConditionCode::ns => {
            let n = emit_load_flag(out, tmp, FLAG_N);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = xor i1 {}, true", r, n);
            r
        }
        ConditionCode::o => emit_load_flag(out, tmp, FLAG_V),
        ConditionCode::no => {
            let v = emit_load_flag(out, tmp, FLAG_V);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = xor i1 {}, true", r, v);
            r
        }
        ConditionCode::be => {
            let c = emit_load_flag(out, tmp, FLAG_C);
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = or i1 {}, {}", r, c, z);
            r
        }
        ConditionCode::a => {
            let c = emit_load_flag(out, tmp, FLAG_C);
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let cz = next_t(tmp);
            let r = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = or i1 {}, {}\n  {} = xor i1 {}, true",
                cz, c, z, r, cz
            );
            r
        }
        ConditionCode::l => {
            let n = emit_load_flag(out, tmp, FLAG_N);
            let v = emit_load_flag(out, tmp, FLAG_V);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = icmp ne i1 {}, {}", r, n, v);
            r
        }
        ConditionCode::ge => {
            let n = emit_load_flag(out, tmp, FLAG_N);
            let v = emit_load_flag(out, tmp, FLAG_V);
            let r = next_t(tmp);
            let _ = writeln!(out, "  {} = icmp eq i1 {}, {}", r, n, v);
            r
        }
        ConditionCode::le => {
            let z = emit_load_flag(out, tmp, FLAG_Z);
            let n = emit_load_flag(out, tmp, FLAG_N);
            let v = emit_load_flag(out, tmp, FLAG_V);
            let nv = next_t(tmp);
            let r = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = icmp ne i1 {}, {}\n  {} = or i1 {}, {}",
                nv, n, v, r, z, nv
            );
            r
        }
        ConditionCode::g => {
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
        ConditionCode::p => "false".to_string(),
        ConditionCode::np => "true".to_string(),
        ConditionCode::None => "true".to_string(),
    }
}

// ============================================================================
// Core x86_64 Instruction Semantics -> LLVM IR
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn emit_x86_instr(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    entry_vma: u64,
    _pc: u64,
    next_pc: u64,
    instr: &Instruction,
    sorted_leaders: &[u64],
    tail_call_trampolines: &BTreeSet<u64>,
    ext_tracker: &mut ExternalDeclTracker,
) -> Result<bool, String> {
    use Mnemonic::*;
    let mn = instr.mnemonic();

    // Handle `rep movs*` / `rep stos*` string operations
    if matches!(mn, Movsb | Movsw | Movsd | Movsq) && instr.has_rep_prefix() {
        let elem_sz: u64 = match mn {
            Movsb => 1,
            Movsw => 2,
            Movsd => 4,
            Movsq => 8,
            _ => 1,
        };
        // Note: unprefixed `movsd` with XMM operands has `has_rep_prefix() == false` (F2 prefix is `repne`, handled below!)
        if instr.op_count() == 0 || instr.op0_kind() == OpKind::Memory {
            ext_tracker
                .needed_decls
                .insert("declare i8* @memcpy(i8*, i8*, i64)".to_string());
            let rdi = emit_read_gpr_slot64(out, tmp, 7);
            let rsi = emit_read_gpr_slot64(out, tmp, 6);
            let rcx = emit_read_gpr_slot64(out, tmp, 1);
            let bytes = if elem_sz > 1 {
                let b = next_t(tmp);
                let _ = writeln!(out, "  {} = mul i64 {}, {}", b, rcx, elem_sz);
                b
            } else {
                rcx
            };
            let dst_p = next_t(tmp);
            let src_p = next_t(tmp);
            let mc = next_t(tmp);
            let new_rdi = next_t(tmp);
            let new_rsi = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = call i8* @memcpy(i8* {}, i8* {}, i64 {})\n  {} = add i64 {}, {}\n  {} = add i64 {}, {}",
                dst_p, mod_id, rdi, src_p, mod_id, rsi, mc, dst_p, src_p, bytes, new_rdi, rdi, bytes, new_rsi, rsi, bytes
            );
            emit_write_gpr_slot64(out, tmp, 7, &new_rdi);
            emit_write_gpr_slot64(out, tmp, 6, &new_rsi);
            emit_write_gpr_slot64(out, tmp, 1, "0");
            return Ok(false);
        }
    }

    if matches!(mn, Stosb | Stosw | Stosd | Stosq) && instr.has_rep_prefix() {
        let elem_sz: u64 = match mn {
            Stosb => 1,
            Stosw => 2,
            Stosd => 4,
            Stosq => 8,
            _ => 1,
        };
        let rdi = emit_read_gpr_slot64(out, tmp, 7);
        let rax = emit_read_gpr_slot64(out, tmp, 0);
        let rcx = emit_read_gpr_slot64(out, tmp, 1);
        if elem_sz == 1 {
            ext_tracker
                .needed_decls
                .insert("declare i8* @memset(i8*, i32, i64)".to_string());
            let dst_p = next_t(tmp);
            let c32 = next_t(tmp);
            let ms = next_t(tmp);
            let new_rdi = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = trunc i64 {} to i32\n  {} = call i8* @memset(i8* {}, i32 {}, i64 {})\n  {} = add i64 {}, {}",
                dst_p, mod_id, rdi, c32, rax, ms, dst_p, c32, rcx, new_rdi, rdi, rcx
            );
            emit_write_gpr_slot64(out, tmp, 7, &new_rdi);
            emit_write_gpr_slot64(out, tmp, 1, "0");
            return Ok(false);
        }
    }

    match mn {
        Nop | Endbr64 | Endbr32 | Pause | Lfence | Sfence | Mfence | Prefetcht0 | Prefetcht1
        | Prefetcht2 | Prefetchnta | Prefetchw | Cld => Ok(false),

        Mov => {
            let bits = operand_bits(instr, 0);
            let v = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            emit_write_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, &v, bits);
            Ok(false)
        }

        Movzx => {
            let dst_bits = operand_bits(instr, 0);
            let src_bits = operand_bits(instr, 1);
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, src_bits);
            let ext = coerce_int_width(out, tmp, &src, src_bits, dst_bits, false);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &ext, dst_bits,
            );
            Ok(false)
        }

        Movsx | Movsxd => {
            let dst_bits = operand_bits(instr, 0);
            let src_bits = operand_bits(instr, 1);
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, src_bits);
            let ext = coerce_int_width(out, tmp, &src, src_bits, dst_bits, true);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &ext, dst_bits,
            );
            Ok(false)
        }

        Lea => {
            let dst_bits = operand_bits(instr, 0);
            let ea = emit_mem_ea(out, tmp, img, img_glob, img_len, instr);
            let val = coerce_int_width(out, tmp, &ea, 64, dst_bits, false);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &val, dst_bits,
            );
            Ok(false)
        }

        Push => {
            let bits = operand_bits(instr, 0).max(64);
            let v = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let sp = emit_read_gpr_slot64(out, tmp, 4);
            let new_sp = next_t(tmp);
            let p8 = next_t(tmp);
            let p64 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = sub i64 {}, 8\n  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = bitcast i8* {} to i64*\n  store i64 {}, i64* {}, align 1",
                new_sp, sp, p8, mod_id, new_sp, p64, p8, v, p64
            );
            emit_write_gpr_slot64(out, tmp, 4, &new_sp);
            Ok(false)
        }

        Pop => {
            let sp = emit_read_gpr_slot64(out, tmp, 4);
            let p8 = next_t(tmp);
            let p64 = next_t(tmp);
            let v = next_t(tmp);
            let new_sp = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = bitcast i8* {} to i64*\n  {} = load i64, i64* {}, align 1\n  {} = add i64 {}, 8",
                p8, mod_id, sp, p64, p8, v, p64, new_sp, sp
            );
            // Update RSP BEFORE writing destination operand in case of `pop [rsp + disp]`!
            emit_write_gpr_slot64(out, tmp, 4, &new_sp);
            let dst_bits = operand_bits(instr, 0);
            let vd = coerce_int_width(out, tmp, &v, 64, dst_bits, false);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &vd, dst_bits,
            );
            Ok(false)
        }

        Leave => {
            let rbp = emit_read_gpr_slot64(out, tmp, 5);
            let p8 = next_t(tmp);
            let p64 = next_t(tmp);
            let old_rbp = next_t(tmp);
            let new_sp = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})\n  {} = bitcast i8* {} to i64*\n  {} = load i64, i64* {}, align 1\n  {} = add i64 {}, 8",
                p8, mod_id, rbp, p64, p8, old_rbp, p64, new_sp, rbp
            );
            emit_write_gpr_slot64(out, tmp, 5, &old_rbp);
            emit_write_gpr_slot64(out, tmp, 4, &new_sp);
            Ok(false)
        }

        Xchg => {
            let bits = operand_bits(instr, 0);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            emit_write_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, &b, bits);
            emit_write_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, &a, bits);
            Ok(false)
        }

        Cbw => {
            let (al, _) = emit_read_gpr_sized(out, tmp, Register::AL);
            let ax = coerce_int_width(out, tmp, &al, 8, 16, true);
            emit_write_gpr_sized(out, tmp, Register::AX, &ax, 16);
            Ok(false)
        }
        Cwde => {
            let (ax, _) = emit_read_gpr_sized(out, tmp, Register::AX);
            let eax = coerce_int_width(out, tmp, &ax, 16, 32, true);
            emit_write_gpr_sized(out, tmp, Register::EAX, &eax, 32);
            Ok(false)
        }
        Cdqe => {
            let (eax, _) = emit_read_gpr_sized(out, tmp, Register::EAX);
            let rax = coerce_int_width(out, tmp, &eax, 32, 64, true);
            emit_write_gpr_slot64(out, tmp, 0, &rax);
            Ok(false)
        }
        Cdq => {
            let (eax, _) = emit_read_gpr_sized(out, tmp, Register::EAX);
            let edx = next_t(tmp);
            let _ = writeln!(out, "  {} = ashr i32 {}, 31", edx, eax);
            emit_write_gpr_sized(out, tmp, Register::EDX, &edx, 32);
            Ok(false)
        }
        Cqo => {
            let rax = emit_read_gpr_slot64(out, tmp, 0);
            let rdx = next_t(tmp);
            let _ = writeln!(out, "  {} = ashr i64 {}, 63", rdx, rax);
            emit_write_gpr_slot64(out, tmp, 2, &rdx);
            Ok(false)
        }

        Add | Adc => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let sum1 = next_t(tmp);
            let cf1 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = add {} {}, {}\n  {} = icmp ult {} {}, {}",
                sum1, ty, a, b, cf1, ty, sum1, a
            );
            let (res, cf_final) = if mn == Adc {
                let c_in = emit_load_flag(out, tmp, FLAG_C);
                let c_ext = next_t(tmp);
                let sum2 = next_t(tmp);
                let cf2 = next_t(tmp);
                let cfo = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = zext i1 {} to {}\n  {} = add {} {}, {}\n  {} = icmp ult {} {}, {}\n  {} = or i1 {}, {}",
                    c_ext, c_in, ty, sum2, ty, sum1, c_ext, cf2, ty, sum2, sum1, cfo, cf1, cf2
                );
                (sum2, cfo)
            } else {
                (sum1, cf1)
            };
            // Signed overflow: `((a ^ res) & (b ^ res)) < 0`
            let x1 = next_t(tmp);
            let x2 = next_t(tmp);
            let ov_m = next_t(tmp);
            let of = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = xor {} {}, {}\n  {} = xor {} {}, {}\n  {} = and {} {}, {}\n  {} = icmp slt {} {}, 0",
                x1, ty, a, res, x2, ty, b, res, ov_m, ty, x1, x2, of, ty, ov_m
            );
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_zf_sf(out, tmp, &res, bits);
            emit_store_flag(out, tmp, FLAG_C, &cf_final);
            emit_store_flag(out, tmp, FLAG_V, &of);
            Ok(false)
        }

        Sub | Sbb | Cmp => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let diff1 = next_t(tmp);
            let cf1 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = sub {} {}, {}\n  {} = icmp ult {} {}, {}",
                diff1, ty, a, b, cf1, ty, a, b
            );
            let (res, cf_final) = if mn == Sbb {
                let c_in = emit_load_flag(out, tmp, FLAG_C);
                let c_ext = next_t(tmp);
                let diff2 = next_t(tmp);
                let cf2 = next_t(tmp);
                let cfo = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = zext i1 {} to {}\n  {} = sub {} {}, {}\n  {} = icmp ult {} {}, {}\n  {} = or i1 {}, {}",
                    c_ext, c_in, ty, diff2, ty, diff1, c_ext, cf2, ty, diff1, c_ext, cfo, cf1, cf2
                );
                (diff2, cfo)
            } else {
                (diff1, cf1)
            };
            // Signed overflow for `a - b`: `((a ^ b) & (a ^ res)) < 0`
            let x1 = next_t(tmp);
            let x2 = next_t(tmp);
            let ov_m = next_t(tmp);
            let of = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = xor {} {}, {}\n  {} = xor {} {}, {}\n  {} = and {} {}, {}\n  {} = icmp slt {} {}, 0",
                x1, ty, a, b, x2, ty, a, res, ov_m, ty, x1, x2, of, ty, ov_m
            );
            if mn != Cmp {
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
                );
            }
            emit_zf_sf(out, tmp, &res, bits);
            emit_store_flag(out, tmp, FLAG_C, &cf_final);
            emit_store_flag(out, tmp, FLAG_V, &of);
            Ok(false)
        }

        Inc | Dec => {
            // Note: INC/DEC preserve CF!
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let res = next_t(tmp);
            let of = next_t(tmp);
            let min_signed: i64 = match bits {
                8 => i8::MIN as i64,
                16 => i16::MIN as i64,
                32 => i32::MIN as i64,
                _ => i64::MIN,
            };
            if mn == Inc {
                let _ = writeln!(
                    out,
                    "  {} = add {} {}, 1\n  {} = icmp eq {} {}, {}",
                    res, ty, a, of, ty, res, min_signed
                );
            } else {
                let _ = writeln!(
                    out,
                    "  {} = sub {} {}, 1\n  {} = icmp eq {} {}, {}",
                    res, ty, a, of, ty, a, min_signed
                );
            }
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_zf_sf(out, tmp, &res, bits);
            emit_store_flag(out, tmp, FLAG_V, &of);
            Ok(false)
        }

        Neg => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let res = next_t(tmp);
            let cf = next_t(tmp);
            let min_signed: i64 = match bits {
                8 => i8::MIN as i64,
                16 => i16::MIN as i64,
                32 => i32::MIN as i64,
                _ => i64::MIN,
            };
            let of = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = sub {} 0, {}\n  {} = icmp ne {} {}, 0\n  {} = icmp eq {} {}, {}",
                res, ty, a, cf, ty, a, of, ty, a, min_signed
            );
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_zf_sf(out, tmp, &res, bits);
            emit_store_flag(out, tmp, FLAG_C, &cf);
            emit_store_flag(out, tmp, FLAG_V, &of);
            Ok(false)
        }

        Not => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let res = next_t(tmp);
            let _ = writeln!(out, "  {} = xor {} {}, -1", res, ty, a);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            Ok(false)
        }

        And | Or | Xor | Test => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            // Fast idiom: `xor eax, eax` / `xor rax, rax` -> zero reg & flags
            if mn == Xor
                && instr.op0_kind() == OpKind::Register
                && instr.op1_kind() == OpKind::Register
                && instr.op0_register() == instr.op1_register()
                && map_xmm_idx(instr.op0_register()).is_none()
            {
                emit_write_gpr_sized(out, tmp, instr.op0_register(), "0", bits);
                emit_store_flag(out, tmp, FLAG_Z, "true");
                emit_store_flag(out, tmp, FLAG_N, "false");
                emit_store_flag(out, tmp, FLAG_C, "false");
                emit_store_flag(out, tmp, FLAG_V, "false");
                return Ok(false);
            }
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let llvm_op = match mn {
                And | Test => "and",
                Or => "or",
                Xor => "xor",
                _ => "and",
            };
            let res = next_t(tmp);
            let _ = writeln!(out, "  {} = {} {} {}, {}", res, llvm_op, ty, a, b);
            if mn != Test {
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
                );
            }
            emit_zf_sf(out, tmp, &res, bits);
            emit_store_flag(out, tmp, FLAG_C, "false");
            emit_store_flag(out, tmp, FLAG_V, "false");
            Ok(false)
        }

        Shl | Sal | Shr | Sar | Rol | Ror => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let mask_amt = if bits == 64 { 63 } else { 31 };
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let raw_cnt = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let cnt = next_t(tmp);
            let _ = writeln!(out, "  {} = and {} {}, {}", cnt, ty, raw_cnt, mask_amt);
            let res = next_t(tmp);
            match mn {
                Shl | Sal => {
                    let _ = writeln!(out, "  {} = shl {} {}, {}", res, ty, a, cnt);
                }
                Shr => {
                    let _ = writeln!(out, "  {} = lshr {} {}, {}", res, ty, a, cnt);
                }
                Sar => {
                    let _ = writeln!(out, "  {} = ashr {} {}, {}", res, ty, a, cnt);
                }
                Rol | Ror => {
                    let mod_bits = next_t(tmp);
                    let inv = next_t(tmp);
                    let inv_m = next_t(tmp);
                    let p1 = next_t(tmp);
                    let p2 = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = and {} {}, {}\n  {} = sub {} {}, {}\n  {} = and {} {}, {}",
                        mod_bits,
                        ty,
                        cnt,
                        bits - 1,
                        inv,
                        ty,
                        bits,
                        mod_bits,
                        inv_m,
                        ty,
                        inv,
                        bits - 1
                    );
                    if mn == Rol {
                        let _ = writeln!(
                            out,
                            "  {} = shl {} {}, {}\n  {} = lshr {} {}, {}\n  {} = or {} {}, {}",
                            p1, ty, a, mod_bits, p2, ty, a, inv_m, res, ty, p1, p2
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "  {} = lshr {} {}, {}\n  {} = shl {} {}, {}\n  {} = or {} {}, {}",
                            p1, ty, a, mod_bits, p2, ty, a, inv_m, res, ty, p1, p2
                        );
                    }
                }
                _ => {}
            }
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            if matches!(mn, Shl | Sal | Shr | Sar) {
                emit_zf_sf(out, tmp, &res, bits);
            }
            Ok(false)
        }

        Shld | Shrd => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let raw_c = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 2, bits);
            let fsh = if mn == Shld { "fshl" } else { "fshr" };
            let intr = format!("declare {t} @llvm.{f}.{t}({t}, {t}, {t})", t = ty, f = fsh);
            ext_tracker.needed_decls.insert(intr);
            let res = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call {} @llvm.{}.{}({} {}, {} {}, {} {})",
                res, ty, fsh, ty, ty, a, ty, b, ty, raw_c
            );
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_zf_sf(out, tmp, &res, bits);
            Ok(false)
        }

        Imul => {
            let nops = instr.op_count();
            if nops == 1 {
                let bits = operand_bits(instr, 0);
                let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
                if bits == 64 {
                    let rax = emit_read_gpr_slot64(out, tmp, 0);
                    let a128 = next_t(tmp);
                    let b128 = next_t(tmp);
                    let p128 = next_t(tmp);
                    let lo = next_t(tmp);
                    let hi_sh = next_t(tmp);
                    let hi = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = sext i64 {} to i128\n  {} = sext i64 {} to i128\n  {} = mul i128 {}, {}\n  {} = trunc i128 {} to i64\n  {} = lshr i128 {}, 64\n  {} = trunc i128 {} to i64",
                        a128, rax, b128, src, p128, a128, b128, lo, p128, hi_sh, p128, hi, hi_sh
                    );
                    emit_write_gpr_slot64(out, tmp, 0, &lo);
                    emit_write_gpr_slot64(out, tmp, 2, &hi);
                } else {
                    let (eax, _) = emit_read_gpr_sized(out, tmp, Register::EAX);
                    let a64 = coerce_int_width(out, tmp, &eax, 32, 64, true);
                    let b64 = coerce_int_width(out, tmp, &src, bits, 64, true);
                    let p64 = next_t(tmp);
                    let lo = next_t(tmp);
                    let hi = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = mul i64 {}, {}\n  {} = trunc i64 {} to i32\n  {} = lshr i64 {}, 32",
                        p64, a64, b64, lo, p64, hi, p64
                    );
                    emit_write_gpr_sized(out, tmp, Register::EAX, &lo, 32);
                    emit_write_gpr_slot64(out, tmp, 2, &hi);
                }
            } else if nops == 2 {
                let bits = operand_bits(instr, 0);
                let ty = bits_ty(bits);
                let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
                let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
                let res = next_t(tmp);
                let _ = writeln!(out, "  {} = mul {} {}, {}", res, ty, a, b);
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
                );
            } else {
                let bits = operand_bits(instr, 0);
                let ty = bits_ty(bits);
                let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
                let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 2, bits);
                let res = next_t(tmp);
                let _ = writeln!(out, "  {} = mul {} {}, {}", res, ty, a, b);
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
                );
            }
            Ok(false)
        }

        Mul => {
            let bits = operand_bits(instr, 0);
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            if bits == 64 {
                let rax = emit_read_gpr_slot64(out, tmp, 0);
                let a128 = next_t(tmp);
                let b128 = next_t(tmp);
                let p128 = next_t(tmp);
                let lo = next_t(tmp);
                let hi_sh = next_t(tmp);
                let hi = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = zext i64 {} to i128\n  {} = zext i64 {} to i128\n  {} = mul i128 {}, {}\n  {} = trunc i128 {} to i64\n  {} = lshr i128 {}, 64\n  {} = trunc i128 {} to i64",
                    a128, rax, b128, src, p128, a128, b128, lo, p128, hi_sh, p128, hi, hi_sh
                );
                emit_write_gpr_slot64(out, tmp, 0, &lo);
                emit_write_gpr_slot64(out, tmp, 2, &hi);
            } else {
                let (eax, _) = emit_read_gpr_sized(out, tmp, Register::EAX);
                let a64 = coerce_int_width(out, tmp, &eax, 32, 64, false);
                let b64 = coerce_int_width(out, tmp, &src, bits, 64, false);
                let p64 = next_t(tmp);
                let lo = next_t(tmp);
                let hi_sh = next_t(tmp);
                let hi = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = mul i64 {}, {}\n  {} = trunc i64 {} to i32\n  {} = lshr i64 {}, 32\n  {} = trunc i64 {} to i32",
                    p64, a64, b64, lo, p64, hi_sh, p64, hi, hi_sh
                );
                emit_write_gpr_sized(out, tmp, Register::EAX, &lo, 32);
                emit_write_gpr_sized(out, tmp, Register::EDX, &hi, 32);
            }
            Ok(false)
        }

        Div | Idiv => {
            let is_signed = mn == Idiv;
            let bits = operand_bits(instr, 0);
            let div_op = if is_signed { "sdiv" } else { "udiv" };
            let rem_op = if is_signed { "srem" } else { "urem" };
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            if bits == 64 {
                let rdx = emit_read_gpr_slot64(out, tmp, 2);
                let rax = emit_read_gpr_slot64(out, tmp, 0);
                let hi = next_t(tmp);
                let hi_sh = next_t(tmp);
                let lo = next_t(tmp);
                let num = next_t(tmp);
                let den = next_t(tmp);
                let q128 = next_t(tmp);
                let r128 = next_t(tmp);
                let q = next_t(tmp);
                let r = next_t(tmp);
                let ext = if is_signed { "sext" } else { "zext" };
                let _ = writeln!(
                    out,
                    "  {} = zext i64 {} to i128\n  {} = shl i128 {}, 64\n  {} = zext i64 {} to i128\n  {} = or i128 {}, {}\n  {} = {} i64 {} to i128\n  {} = {} i128 {}, {}\n  {} = {} i128 {}, {}\n  {} = trunc i128 {} to i64\n  {} = trunc i128 {} to i64",
                    hi, rdx, hi_sh, hi, lo, rax, num, hi_sh, lo, den, ext, src, q128, div_op, num, den, r128, rem_op, num, den, q, q128, r, r128
                );
                emit_write_gpr_slot64(out, tmp, 0, &q);
                emit_write_gpr_slot64(out, tmp, 2, &r);
            } else {
                let (edx, _) = emit_read_gpr_sized(out, tmp, Register::EDX);
                let (eax, _) = emit_read_gpr_sized(out, tmp, Register::EAX);
                let hi = next_t(tmp);
                let hi_sh = next_t(tmp);
                let lo = next_t(tmp);
                let num = next_t(tmp);
                let den = coerce_int_width(out, tmp, &src, bits, 64, is_signed);
                let q64 = next_t(tmp);
                let r64 = next_t(tmp);
                let q32 = next_t(tmp);
                let r32 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = zext i32 {} to i64\n  {} = shl i64 {}, 32\n  {} = zext i32 {} to i64\n  {} = or i64 {}, {}\n  {} = {} i64 {}, {}\n  {} = {} i64 {}, {}\n  {} = trunc i64 {} to i32\n  {} = trunc i64 {} to i32",
                    hi, edx, hi_sh, hi, lo, eax, num, hi_sh, lo, q64, div_op, num, den, r64, rem_op, num, den, q32, q64, r32, r64
                );
                emit_write_gpr_sized(out, tmp, Register::EAX, &q32, 32);
                emit_write_gpr_sized(out, tmp, Register::EDX, &r32, 32);
            }
            Ok(false)
        }

        Bswap => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            ext_tracker
                .needed_decls
                .insert(format!("declare {t} @llvm.bswap.{t}({t})", t = ty));
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let res = next_t(tmp);
            let _ = writeln!(out, "  {} = call {} @llvm.bswap.{}({} {})", res, ty, ty, ty, a);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            Ok(false)
        }

        Tzcnt | Bsf => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            ext_tracker
                .needed_decls
                .insert(format!("declare {t} @llvm.cttz.{t}({t}, i1)", t = ty));
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let res = next_t(tmp);
            let zf = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call {} @llvm.cttz.{}({} {}, i1 false)\n  {} = icmp eq {} {}, 0",
                res, ty, ty, ty, src, zf, ty, src
            );
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_store_flag(out, tmp, FLAG_Z, &zf);
            Ok(false)
        }

        Lzcnt | Bsr => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            ext_tracker
                .needed_decls
                .insert(format!("declare {t} @llvm.ctlz.{t}({t}, i1)", t = ty));
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let ctlz = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call {} @llvm.ctlz.{}({} {}, i1 false)",
                ctlz, ty, ty, ty, src
            );
            let res = if mn == Bsr {
                let r = next_t(tmp);
                let _ = writeln!(out, "  {} = xor {} {}, {}", r, ty, ctlz, bits - 1);
                r
            } else {
                ctlz
            };
            let zf = next_t(tmp);
            let _ = writeln!(out, "  {} = icmp eq {} {}, 0", zf, ty, src);
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_store_flag(out, tmp, FLAG_Z, &zf);
            Ok(false)
        }

        Popcnt => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            ext_tracker
                .needed_decls
                .insert(format!("declare {t} @llvm.ctpop.{t}({t})", t = ty));
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let res = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = call {} @llvm.ctpop.{}({} {})",
                res, ty, ty, ty, src
            );
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &res, bits,
            );
            emit_zf_sf(out, tmp, &res, bits);
            Ok(false)
        }

        Bt | Bts | Btr | Btc => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let a = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let b_raw = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let b = next_t(tmp);
            let mask = next_t(tmp);
            let bit_val = next_t(tmp);
            let cf = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = and {} {}, {}\n  {} = shl {} 1, {}\n  {} = and {} {}, {}\n  {} = icmp ne {} {}, 0",
                b,
                ty,
                b_raw,
                bits - 1,
                mask,
                ty,
                b,
                bit_val,
                ty,
                a,
                mask,
                cf,
                ty,
                bit_val
            );
            emit_store_flag(out, tmp, FLAG_C, &cf);
            if mn != Bt {
                let updated = next_t(tmp);
                match mn {
                    Bts => {
                        let _ = writeln!(out, "  {} = or {} {}, {}", updated, ty, a, mask);
                    }
                    Btr => {
                        let inv = next_t(tmp);
                        let _ = writeln!(
                            out,
                            "  {} = xor {} {}, -1\n  {} = and {} {}, {}",
                            inv, ty, mask, updated, ty, a, inv
                        );
                    }
                    Btc => {
                        let _ = writeln!(out, "  {} = xor {} {}, {}", updated, ty, a, mask);
                    }
                    _ => {}
                }
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &updated, bits,
                );
            }
            Ok(false)
        }

        // Conditional Set (`SETcc`) & Conditional Move (`CMOVcc`)
        Seta | Setae | Setb | Setbe | Sete | Setg | Setge | Setl | Setle | Setne | Setno
        | Setnp | Setns | Seto | Setp | Sets => {
            let cond = emit_eval_cond(out, tmp, instr.condition_code());
            let b8 = next_t(tmp);
            let _ = writeln!(out, "  {} = zext i1 {} to i8", b8, cond);
            emit_write_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, &b8, 8);
            Ok(false)
        }

        Cmova | Cmovae | Cmovb | Cmovbe | Cmove | Cmovg | Cmovge | Cmovl | Cmovle | Cmovne
        | Cmovno | Cmovnp | Cmovns | Cmovo | Cmovp | Cmovs => {
            let bits = operand_bits(instr, 0);
            let ty = bits_ty(bits);
            let cond = emit_eval_cond(out, tmp, instr.condition_code());
            let old = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, bits);
            let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, bits);
            let sel = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = select i1 {}, {} {}, {} {}",
                sel, cond, ty, src, ty, old
            );
            // Note: even when false on x86_64, 32-bit CMOV still zero-extends upper 32 bits!
            emit_write_op(
                out, tmp, img, mod_id, img_glob, img_len, instr, 0, &sel, bits,
            );
            Ok(false)
        }

        // Control-Flow: CALL, JMP, Jcc, RET
        Call => {
            if matches!(
                instr.op0_kind(),
                OpKind::NearBranch64 | OpKind::NearBranch32 | OpKind::NearBranch16
            ) {
                let target = instr.near_branch_target();
                if let Some(ext_sym) = img.plt_stubs.get(&target) {
                    ext_tracker.emit_external_call(
                        out,
                        tmp,
                        BinaryArch::X86_64,
                        img.abi,
                        mod_id,
                        ext_sym,
                    );
                } else if img.is_exec_vma(target) {
                    emit_push_dummy_ret_addr(out, tmp, mod_id);
                    let _ = writeln!(
                        out,
                        "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)",
                        mod_id, target
                    );
                }
            } else if instr.is_ip_rel_memory_operand() {
                let got_addr = instr.ip_rel_memory_address();
                if let Some(val) = img.read_u64_le(got_addr) {
                    if let Some(ext_sym) = img.ext_sentinels.get(&val) {
                        ext_tracker.emit_external_call(
                            out,
                            tmp,
                            BinaryArch::X86_64,
                            img.abi,
                            mod_id,
                            ext_sym,
                        );
                        return Ok(false);
                    } else if img.is_exec_vma(val) {
                        emit_push_dummy_ret_addr(out, tmp, mod_id);
                        let _ = writeln!(
                            out,
                            "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)",
                            mod_id, val
                        );
                        return Ok(false);
                    }
                }
                let dyn_t = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, 64);
                let _ = writeln!(
                    out,
                    "  call void @__omni_indirect_call_{}(%OmniCpuState* %st, i64 {})",
                    mod_id, dyn_t
                );
            } else {
                let dyn_t = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, 64);
                let _ = writeln!(
                    out,
                    "  call void @__omni_indirect_call_{}(%OmniCpuState* %st, i64 {})",
                    mod_id, dyn_t
                );
            }
            Ok(false)
        }

        Jmp => {
            if matches!(
                instr.op0_kind(),
                OpKind::NearBranch64 | OpKind::NearBranch32 | OpKind::NearBranch16
            ) {
                let target = instr.near_branch_target();
                if let Some(ext_sym) = img.plt_stubs.get(&target) {
                    // Tail-call to external function: pop caller's 8-byte return slot so stack args align, then call!
                    emit_pop_ret_slot(out, tmp);
                    ext_tracker.emit_external_call(
                        out,
                        tmp,
                        BinaryArch::X86_64,
                        img.abi,
                        mod_id,
                        ext_sym,
                    );
                    out.push_str("  ret void\n");
                } else if is_external_or_other_fn(img, entry_vma, target) {
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
            } else if instr.is_ip_rel_memory_operand() {
                let got_addr = instr.ip_rel_memory_address();
                if let Some(val) = img.read_u64_le(got_addr) {
                    if let Some(ext_sym) = img.ext_sentinels.get(&val) {
                        emit_pop_ret_slot(out, tmp);
                        ext_tracker.emit_external_call(
                            out,
                            tmp,
                            BinaryArch::X86_64,
                            img.abi,
                            mod_id,
                            ext_sym,
                        );
                        out.push_str("  ret void\n");
                        return Ok(true);
                    } else if img.is_exec_vma(val) {
                        let _ = writeln!(
                            out,
                            "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)\n  ret void",
                            mod_id, val
                        );
                        return Ok(true);
                    }
                }
                out.push_str("  ret void\n");
            } else {
                // Indirect switch jump (`jmp rax`): normalize host image pointer -> VMA offset and dispatch to any local leader!
                let dyn_t = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, 64);
                let img_b = next_t(tmp);
                let rel_h = next_t(tmp);
                let is_h = next_t(tmp);
                let as_vma = next_t(tmp);
                let norm = next_t(tmp);
                let def_lbl = format!("jmp_ret_{}", *tmp);
                let _ = writeln!(
                    out,
                    "  {} = ptrtoint [{} x i8]* @{} to i64\n  {} = sub i64 {}, {}\n  {} = icmp ult i64 {}, {}\n  {} = add i64 {}, {}\n  {} = select i1 {}, i64 {}, i64 {}\n  switch i64 {}, label %{} [",
                    img_b, img_len, img_glob, rel_h, dyn_t, img_b, is_h, rel_h, img_len, as_vma, rel_h, img.base_vma, norm, is_h, as_vma, dyn_t, norm, def_lbl
                );
                for &ldr in sorted_leaders {
                    let _ = writeln!(out, "    i64 {}, label %bb_{:x}", ldr, ldr);
                }
                let _ = writeln!(out, "  ]\n{}:", def_lbl);
                // If not a local switch target, treat as indirect tail-call!
                emit_pop_ret_slot(out, tmp);
                let _ = writeln!(
                    out,
                    "  call void @__omni_indirect_call_{}(%OmniCpuState* %st, i64 {})\n  ret void",
                    mod_id, dyn_t
                );
            }
            Ok(true)
        }

        Ja | Jae | Jb | Jbe | Je | Jg | Jge | Jl | Jle | Jne | Jno | Jnp | Jns | Jo | Jp | Js => {
            let cond = emit_eval_cond(out, tmp, instr.condition_code());
            let target = instr.near_branch_target();
            let t_lbl = if tail_call_trampolines.contains(&target) {
                format!("tail_tramp_{:x}", target)
            } else if sorted_leaders.binary_search(&target).is_ok() {
                format!("bb_{:x}", target)
            } else {
                format!("bb_{:x}", next_pc)
            };
            let f_lbl = if sorted_leaders.binary_search(&next_pc).is_ok() {
                format!("bb_{:x}", next_pc)
            } else {
                let ret_l = format!("cond_ret_{}", *tmp);
                let _ = writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}\n{}:\n  ret void",
                    cond, t_lbl, ret_l, ret_l
                );
                return Ok(true);
            };
            let _ = writeln!(out, "  br i1 {}, label %{}, label %{}", cond, t_lbl, f_lbl);
            Ok(true)
        }

        Ret => {
            let extra = if instr.op_count() > 0 {
                instr.immediate16() as u64
            } else {
                0
            };
            let sp = emit_read_gpr_slot64(out, tmp, 4);
            let new_sp = next_t(tmp);
            let _ = writeln!(out, "  {} = add i64 {}, {}", new_sp, sp, 8 + extra);
            emit_write_gpr_slot64(out, tmp, 4, &new_sp);
            out.push_str("  ret void\n");
            Ok(true)
        }

        Ud2 | Hlt | Int3 => {
            out.push_str("  ret void\n");
            Ok(true)
        }

        // Windows `__fastfail` (`int 0x29` / `CD 29`) emitted in stack-cookie failure cold paths
        Int if instr.immediate8() == 0x29 => {
            out.push_str("  ret void\n");
            Ok(true)
        }

        // ====================================================================
        // 128-bit SSE / SSE2 / SSSE3 / Scalar FP Instructions
        // ====================================================================
        _ => emit_x86_simd_or_fp(out, tmp, img, mod_id, img_glob, img_len, instr),
    }
}

fn emit_x86_simd_or_fp(
    out: &mut String,
    tmp: &mut usize,
    img: &ParsedBinaryImage,
    mod_id: &str,
    img_glob: &str,
    img_len: usize,
    instr: &Instruction,
) -> Result<bool, String> {
    use Mnemonic::*;
    let mn = instr.mnemonic();
    match mn {
        Movdqa | Movdqu | Movaps | Movups | Lddqu => {
            let v = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            emit_write_op(out, tmp, img, mod_id, img_glob, img_len, instr, 0, &v, 128);
            Ok(false)
        }

        Movq => {
            // Either `movq xmm, r/m64` (zero-extends upper 64 bits of XMM) or `movq r/m64, xmm`!
            if instr.op0_kind() == OpKind::Register && map_xmm_idx(instr.op0_register()).is_some() {
                let low64 = if instr.op1_kind() == OpKind::Register
                    && map_xmm_idx(instr.op1_register()).is_some()
                {
                    let sv = emit_read_xmm(out, tmp, instr.op1_register());
                    let bc = next_t(tmp);
                    let ex = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                        bc, sv, ex, bc
                    );
                    ex
                } else {
                    emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 64)
                };
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = insertelement <2 x i64> zeroinitializer, i64 {}, i32 0\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                    ins, low64, v8, ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let low64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                    bc, sv, low64, bc
                );
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &low64, 64,
                );
            }
            Ok(false)
        }

        Movd => {
            if instr.op0_kind() == OpKind::Register && map_xmm_idx(instr.op0_register()).is_some() {
                let low32 = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 32);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = insertelement <4 x i32> zeroinitializer, i32 {}, i32 0\n  {} = bitcast <4 x i32> {} to <16 x i8>",
                    ins, low32, v8, ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let low32 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = extractelement <4 x i32> {}, i32 0",
                    bc, sv, low32, bc
                );
                emit_write_op(
                    out, tmp, img, mod_id, img_glob, img_len, instr, 0, &low32, 32,
                );
            }
            Ok(false)
        }

        Movddup => {
            let low64 = if instr.op1_kind() == OpKind::Memory {
                emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, 64)
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let ex = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                    bc, sv, ex, bc
                );
                ex
            };
            let t0 = next_t(tmp);
            let t1 = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = insertelement <2 x i64> zeroinitializer, i64 {}, i32 0\n  {} = insertelement <2 x i64> {}, i64 {}, i32 1\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                t0, low64, t1, t0, low64, v8, t1
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Movhps | Movhpd => {
            if instr.op0_kind() == OpKind::Register && map_xmm_idx(instr.op0_register()).is_some() {
                let dst = emit_read_xmm(out, tmp, instr.op0_register());
                let m64 = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, 64);
                let bc = next_t(tmp);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = insertelement <2 x i64> {}, i64 {}, i32 1\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                    bc, dst, ins, bc, m64, v8, ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let hi64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 1",
                    bc, sv, hi64, bc
                );
                emit_mem_store(out, tmp, img, mod_id, img_glob, img_len, instr, &hi64, 64);
            }
            Ok(false)
        }

        Movlps | Movlpd => {
            if instr.op0_kind() == OpKind::Register && map_xmm_idx(instr.op0_register()).is_some() {
                let dst = emit_read_xmm(out, tmp, instr.op0_register());
                let m64 = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, 64);
                let bc = next_t(tmp);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = insertelement <2 x i64> {}, i64 {}, i32 0\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                    bc, dst, ins, bc, m64, v8, ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let lo64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                    bc, sv, lo64, bc
                );
                emit_mem_store(out, tmp, img, mod_id, img_glob, img_len, instr, &lo64, 64);
            }
            Ok(false)
        }

        Movhlps | Movlhps => {
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_xmm(out, tmp, instr.op1_register());
            let mask = if mn == Movhlps {
                "<i32 3, i32 1>"
            } else {
                "<i32 0, i32 2>"
            };
            let ba = next_t(tmp);
            let bb = next_t(tmp);
            let sh = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = shufflevector <2 x i64> {}, <2 x i64> {}, <2 x i32> {}\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                ba, a, bb, b, sh, ba, bb, mask, v8, sh
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pxor | Xorps | Xorpd | Por | Orps | Orpd | Pand | Andps | Andpd | Pandn | Andnps
        | Andnpd => {
            if matches!(mn, Pxor | Xorps | Xorpd)
                && instr.op0_kind() == OpKind::Register
                && instr.op1_kind() == OpKind::Register
                && instr.op0_register() == instr.op1_register()
            {
                emit_write_xmm(out, tmp, instr.op0_register(), "zeroinitializer");
                return Ok(false);
            }
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let res = next_t(tmp);
            match mn {
                Pxor | Xorps | Xorpd => {
                    let _ = writeln!(out, "  {} = xor <16 x i8> {}, {}", res, a, b);
                }
                Por | Orps | Orpd => {
                    let _ = writeln!(out, "  {} = or <16 x i8> {}, {}", res, a, b);
                }
                Pand | Andps | Andpd => {
                    let _ = writeln!(out, "  {} = and <16 x i8> {}, {}", res, a, b);
                }
                _ => {
                    let na = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = xor <16 x i8> {}, <i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1, i8 -1>\n  {} = and <16 x i8> {}, {}",
                        na, a, res, na, b
                    );
                }
            }
            emit_write_xmm(out, tmp, instr.op0_register(), &res);
            Ok(false)
        }

        Paddb | Paddw | Paddd | Paddq | Psubb | Psubw | Psubd | Psubq | Pmullw | Pmulld => {
            let vty = match mn {
                Paddb | Psubb => "<16 x i8>",
                Paddw | Psubw | Pmullw => "<8 x i16>",
                Paddd | Psubd | Pmulld => "<4 x i32>",
                _ => "<2 x i64>",
            };
            let op = match mn {
                Paddb | Paddw | Paddd | Paddq => "add",
                Psubb | Psubw | Psubd | Psubq => "sub",
                _ => "mul",
            };
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let ca = next_t(tmp);
            let cb = next_t(tmp);
            let r = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = {} {} {}, {}\n  {} = bitcast {} {} to <16 x i8>",
                ca, a, vty, cb, b, vty, r, op, vty, ca, cb, v8, vty, r
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pmuludq => {
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let ca = next_t(tmp);
            let cb = next_t(tmp);
            let sa = next_t(tmp);
            let sb = next_t(tmp);
            let za = next_t(tmp);
            let zb = next_t(tmp);
            let prod = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = shufflevector <4 x i32> {}, <4 x i32> poison, <2 x i32> <i32 0, i32 2>\n  {} = shufflevector <4 x i32> {}, <4 x i32> poison, <2 x i32> <i32 0, i32 2>\n  {} = zext <2 x i32> {} to <2 x i64>\n  {} = zext <2 x i32> {} to <2 x i64>\n  {} = mul <2 x i64> {}, {}\n  {} = bitcast <2 x i64> {} to <16 x i8>",
                ca, a, cb, b, sa, ca, sb, cb, za, sa, zb, sb, prod, za, zb, v8, prod
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pcmpeqb | Pcmpeqw | Pcmpeqd | Pcmpeqq | Pcmpgtb | Pcmpgtw | Pcmpgtd | Pcmpgtq => {
            let vty = match mn {
                Pcmpeqb | Pcmpgtb => "<16 x i8>",
                Pcmpeqw | Pcmpgtw => "<8 x i16>",
                Pcmpeqd | Pcmpgtd => "<4 x i32>",
                _ => "<2 x i64>",
            };
            let bty = match mn {
                Pcmpeqb | Pcmpgtb => "<16 x i1>",
                Pcmpeqw | Pcmpgtw => "<8 x i1>",
                Pcmpeqd | Pcmpgtd => "<4 x i1>",
                _ => "<2 x i1>",
            };
            let cmp_pred = if matches!(mn, Pcmpeqb | Pcmpeqw | Pcmpeqd | Pcmpeqq) {
                "eq"
            } else {
                "sgt"
            };
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let ca = next_t(tmp);
            let cb = next_t(tmp);
            let cmp = next_t(tmp);
            let sx = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = icmp {} {} {}, {}\n  {} = sext {} {} to {}\n  {} = bitcast {} {} to <16 x i8>",
                ca, a, vty, cb, b, vty, cmp, cmp_pred, vty, ca, cb, sx, bty, cmp, vty, v8, vty, sx
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pminub | Pmaxub | Pminsw | Pmaxsw => {
            let (vty, bty, pred) = match mn {
                Pminub => ("<16 x i8>", "<16 x i1>", "ult"),
                Pmaxub => ("<16 x i8>", "<16 x i1>", "ugt"),
                Pminsw => ("<8 x i16>", "<8 x i1>", "slt"),
                _ => ("<8 x i16>", "<8 x i1>", "sgt"),
            };
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let ca = next_t(tmp);
            let cb = next_t(tmp);
            let cmp = next_t(tmp);
            let sel = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = icmp {} {} {}, {}\n  {} = select {} {}, {} {}, {} {}\n  {} = bitcast {} {} to <16 x i8>",
                ca, a, vty, cb, b, vty, cmp, pred, vty, ca, cb, sel, bty, cmp, vty, ca, vty, cb, v8, vty, sel
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pshufd | Shufps => {
            let imm = instr.immediate8();
            let i0 = imm & 3;
            let i1 = (imm >> 2) & 3;
            let i2 = (imm >> 4) & 3;
            let i3 = (imm >> 6) & 3;
            if mn == Pshufd {
                let src = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
                let cs = next_t(tmp);
                let sh = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = shufflevector <4 x i32> {}, <4 x i32> poison, <4 x i32> <i32 {}, i32 {}, i32 {}, i32 {}>\n  {} = bitcast <4 x i32> {} to <16 x i8>",
                    cs, src, sh, cs, i0, i1, i2, i3, v8, sh
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let a = emit_read_xmm(out, tmp, instr.op0_register());
                let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
                let ca = next_t(tmp);
                let cb = next_t(tmp);
                let sh = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = bitcast <16 x i8> {} to <4 x i32>\n  {} = shufflevector <4 x i32> {}, <4 x i32> {}, <4 x i32> <i32 {}, i32 {}, i32 {}, i32 {}>\n  {} = bitcast <4 x i32> {} to <16 x i8>",
                    ca, a, cb, b, sh, ca, cb, i0, i1, i2 + 4, i3 + 4, v8, sh
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            }
            Ok(false)
        }

        Punpcklbw | Punpcklwd | Punpckldq | Punpcklqdq | Punpckhbw | Punpckhwd | Punpckhdq
        | Punpckhqdq => {
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let (vty, lanes, is_hi) = match mn {
                Punpcklbw => ("<16 x i8>", 16usize, false),
                Punpckhbw => ("<16 x i8>", 16, true),
                Punpcklwd => ("<8 x i16>", 8, false),
                Punpckhwd => ("<8 x i16>", 8, true),
                Punpckldq => ("<4 x i32>", 4, false),
                Punpckhdq => ("<4 x i32>", 4, true),
                Punpcklqdq => ("<2 x i64>", 2, false),
                _ => ("<2 x i64>", 2, true),
            };
            let base_i = if is_hi { lanes / 2 } else { 0 };
            let mut mask_elems = Vec::with_capacity(lanes);
            for k in 0..(lanes / 2) {
                mask_elems.push(format!("i32 {}", base_i + k));
                mask_elems.push(format!("i32 {}", lanes + base_i + k));
            }
            let mask_str = format!("<{} x i32> <{}>", lanes, mask_elems.join(", "));
            let ca = next_t(tmp);
            let cb = next_t(tmp);
            let sh = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = shufflevector {} {}, {} {}, {}\n  {} = bitcast {} {} to <16 x i8>",
                ca, a, vty, cb, b, vty, sh, vty, ca, vty, cb, mask_str, v8, vty, sh
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pslldq | Psrldq => {
            let shift_bytes = (instr.immediate8() as usize).min(16);
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let mut mask_elems = Vec::with_capacity(16);
            for k in 0..16usize {
                if mn == Pslldq {
                    if k < shift_bytes {
                        mask_elems.push("i32 16".to_string()); // zeroinitializer[0]
                    } else {
                        mask_elems.push(format!("i32 {}", k - shift_bytes));
                    }
                } else if k + shift_bytes < 16 {
                    mask_elems.push(format!("i32 {}", k + shift_bytes));
                } else {
                    mask_elems.push("i32 16".to_string());
                }
            }
            let res = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = shufflevector <16 x i8> {}, <16 x i8> zeroinitializer, <16 x i32> <{}>",
                res,
                a,
                mask_elems.join(", ")
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &res);
            Ok(false)
        }

        Psllw | Pslld | Psllq | Psrlw | Psrld | Psrlq | Psraw | Psrad => {
            let (vty, ety, lanes, ebits, op_name) = match mn {
                Psllw => ("<8 x i16>", "i16", 8usize, 16u64, "shl"),
                Pslld => ("<4 x i32>", "i32", 4, 32, "shl"),
                Psllq => ("<2 x i64>", "i64", 2, 64, "shl"),
                Psrlw => ("<8 x i16>", "i16", 8, 16, "lshr"),
                Psrld => ("<4 x i32>", "i32", 4, 32, "lshr"),
                Psrlq => ("<2 x i64>", "i64", 2, 64, "lshr"),
                Psraw => ("<8 x i16>", "i16", 8, 16, "ashr"),
                _ => ("<4 x i32>", "i32", 4, 32, "ashr"),
            };
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let ca = next_t(tmp);
            let _ = writeln!(out, "  {} = bitcast <16 x i8> {} to {}", ca, a, vty);
            let cnt64 = if instr.op1_kind() == OpKind::Immediate8 {
                format!("{}", instr.immediate8() as u64)
            } else {
                let sv = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
                let bc = next_t(tmp);
                let c0 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                    bc, sv, c0, bc
                );
                c0
            };
            let clamped = next_t(tmp);
            let is_oob = next_t(tmp);
            let safe_cnt = next_t(tmp);
            let cnt_el = next_t(tmp);
            let ins0 = next_t(tmp);
            let splat = next_t(tmp);
            let sh = next_t(tmp);
            let final_v = next_t(tmp);
            let v8 = next_t(tmp);
            let zero_mask = vec!["i32 0"; lanes].join(", ");
            let _ = writeln!(
                out,
                "  {} = icmp uge i64 {}, {}\n  {} = select i1 {}, i64 {}, i64 {}\n  {} = trunc i64 {} to {}\n  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = shufflevector {} {}, {} poison, <{} x i32> <{}>\n  {} = {} {} {}, {}",
                is_oob,
                cnt64,
                ebits,
                safe_cnt,
                is_oob,
                ebits - 1,
                cnt64,
                cnt_el,
                safe_cnt,
                ety,
                ins0,
                vty,
                ety,
                cnt_el,
                splat,
                vty,
                ins0,
                vty,
                lanes,
                zero_mask,
                sh,
                op_name,
                vty,
                ca,
                splat
            );
            let _ = clamped;
            if op_name == "ashr" {
                let _ = writeln!(out, "  {} = bitcast {} {} to <16 x i8>", v8, vty, sh);
            } else {
                let _ = writeln!(
                    out,
                    "  {} = select i1 {}, {} zeroinitializer, {} {}\n  {} = bitcast {} {} to <16 x i8>",
                    final_v, is_oob, vty, vty, sh, v8, vty, final_v
                );
            }
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Pmovmskb => {
            let sv = emit_read_xmm(out, tmp, instr.op1_register());
            let cmp = next_t(tmp);
            let bits16 = next_t(tmp);
            let z32 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = icmp slt <16 x i8> {}, zeroinitializer\n  {} = bitcast <16 x i1> {} to i16\n  {} = zext i16 {} to i32",
                cmp, sv, bits16, cmp, z32, bits16
            );
            emit_write_gpr_sized(out, tmp, instr.op0_register(), &z32, 32);
            Ok(false)
        }

        Pshufb => {
            // Byte-by-byte table lookup with bit 7 zero-masking
            let a = emit_read_xmm(out, tmp, instr.op0_register());
            let b = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, 128);
            let mut acc = "zeroinitializer".to_string();
            for lane in 0..16 {
                let ctl = next_t(tmp);
                let is_neg = next_t(tmp);
                let idx = next_t(tmp);
                let picked = next_t(tmp);
                let val = next_t(tmp);
                let next_acc = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = extractelement <16 x i8> {}, i32 {}\n  {} = icmp slt i8 {}, 0\n  {} = and i8 {}, 15\n  {} = extractelement <16 x i8> {}, i8 {}\n  {} = select i1 {}, i8 0, i8 {}\n  {} = insertelement <16 x i8> {}, i8 {}, i32 {}",
                    ctl, b, lane, is_neg, ctl, idx, ctl, picked, a, idx, val, is_neg, picked, next_acc, acc, val, lane
                );
                acc = next_acc;
            }
            emit_write_xmm(out, tmp, instr.op0_register(), &acc);
            Ok(false)
        }

        // Scalar IEEE-754 double/float SSE2 instructions (`movsd`, `movss`, `addsd`, `subsd`, `mulsd`, `divsd`, `ucomisd`, `cvtsi2sd`, `cvttsd2si`)
        Movsd | Movss => {
            let is_f64 = mn == Movsd;
            let bits = if is_f64 { 64 } else { 32 };
            let vty = if is_f64 { "<2 x i64>" } else { "<4 x i32>" };
            if instr.op0_kind() == OpKind::Memory {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let bc = next_t(tmp);
                let e0 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                    bc, sv, vty, e0, vty, bc
                );
                emit_mem_store(out, tmp, img, mod_id, img_glob, img_len, instr, &e0, bits);
            } else if instr.op1_kind() == OpKind::Memory {
                let m = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, bits);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = insertelement {} zeroinitializer, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                    ins,
                    vty,
                    bits_ty(bits),
                    m,
                    v8,
                    vty,
                    ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            } else {
                let dst = emit_read_xmm(out, tmp, instr.op0_register());
                let src = emit_read_xmm(out, tmp, instr.op1_register());
                let cd = next_t(tmp);
                let cs = next_t(tmp);
                let e0 = next_t(tmp);
                let ins = next_t(tmp);
                let v8 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0\n  {} = insertelement {} {}, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                    cd, dst, vty, cs, src, vty, e0, vty, cs, ins, vty, cd, bits_ty(bits), e0, v8, vty, ins
                );
                emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            }
            Ok(false)
        }

        Addsd | Subsd | Mulsd | Divsd | Addss | Subss | Mulss | Divss => {
            let is_f64 = matches!(mn, Addsd | Subsd | Mulsd | Divsd);
            let (fty, vty, bits) = if is_f64 {
                ("double", "<2 x double>", 64u32)
            } else {
                ("float", "<4 x float>", 32u32)
            };
            let fop = match mn {
                Addsd | Addss => "fadd",
                Subsd | Subss => "fsub",
                Mulsd | Mulss => "fmul",
                _ => "fdiv",
            };
            let dst = emit_read_xmm(out, tmp, instr.op0_register());
            let cd = next_t(tmp);
            let a0 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                cd, dst, vty, a0, vty, cd
            );
            let b0 = if instr.op1_kind() == OpKind::Memory {
                let mi = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, bits);
                let bf = next_t(tmp);
                let _ = writeln!(out, "  {} = bitcast {} {} to {}", bf, bits_ty(bits), mi, fty);
                bf
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let cs = next_t(tmp);
                let ex = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                    cs, sv, vty, ex, vty, cs
                );
                ex
            };
            let res = next_t(tmp);
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = {} {} {}, {}\n  {} = insertelement {} {}, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                res, fop, fty, a0, b0, ins, vty, cd, fty, res, v8, vty, ins
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Ucomisd | Comisd | Ucomiss | Comiss => {
            let is_f64 = matches!(mn, Ucomisd | Comisd);
            let (fty, vty, bits) = if is_f64 {
                ("double", "<2 x double>", 64u32)
            } else {
                ("float", "<4 x float>", 32u32)
            };
            let a_v = emit_read_xmm(out, tmp, instr.op0_register());
            let ca = next_t(tmp);
            let a0 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                ca, a_v, vty, a0, vty, ca
            );
            let b0 = if instr.op1_kind() == OpKind::Memory {
                let mi = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, bits);
                let bf = next_t(tmp);
                let _ = writeln!(out, "  {} = bitcast {} {} to {}", bf, bits_ty(bits), mi, fty);
                bf
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let cs = next_t(tmp);
                let ex = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                    cs, sv, vty, ex, vty, cs
                );
                ex
            };
            let zf = next_t(tmp);
            let cf = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = fcmp ueq {} {}, {}\n  {} = fcmp ult {} {}, {}",
                zf, fty, a0, b0, cf, fty, a0, b0
            );
            emit_store_flag(out, tmp, FLAG_Z, &zf);
            emit_store_flag(out, tmp, FLAG_C, &cf);
            emit_store_flag(out, tmp, FLAG_N, "false");
            emit_store_flag(out, tmp, FLAG_V, "false");
            Ok(false)
        }

        Cvtsi2sd | Cvtsi2ss => {
            let is_f64 = mn == Cvtsi2sd;
            let (fty, vty) = if is_f64 {
                ("double", "<2 x double>")
            } else {
                ("float", "<4 x float>")
            };
            let sbits = operand_bits(instr, 1);
            let sv = emit_read_op(out, tmp, img, mod_id, img_glob, img_len, instr, 1, sbits);
            let dst = emit_read_xmm(out, tmp, instr.op0_register());
            let cd = next_t(tmp);
            let fp = next_t(tmp);
            let ins = next_t(tmp);
            let v8 = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = bitcast <16 x i8> {} to {}\n  {} = sitofp {} {} to {}\n  {} = insertelement {} {}, {} {}, i32 0\n  {} = bitcast {} {} to <16 x i8>",
                cd, dst, vty, fp, bits_ty(sbits), sv, fty, ins, vty, cd, fty, fp, v8, vty, ins
            );
            emit_write_xmm(out, tmp, instr.op0_register(), &v8);
            Ok(false)
        }

        Cvttsd2si | Cvtsd2si | Cvttss2si | Cvtss2si => {
            let is_f64 = matches!(mn, Cvttsd2si | Cvtsd2si);
            let (fty, vty, sbits) = if is_f64 {
                ("double", "<2 x double>", 64u32)
            } else {
                ("float", "<4 x float>", 32u32)
            };
            let dbits = operand_bits(instr, 0);
            let f0 = if instr.op1_kind() == OpKind::Memory {
                let mi = emit_mem_load(out, tmp, img, mod_id, img_glob, img_len, instr, sbits);
                let bf = next_t(tmp);
                let _ = writeln!(out, "  {} = bitcast {} {} to {}", bf, bits_ty(sbits), mi, fty);
                bf
            } else {
                let sv = emit_read_xmm(out, tmp, instr.op1_register());
                let cs = next_t(tmp);
                let ex = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = bitcast <16 x i8> {} to {}\n  {} = extractelement {} {}, i32 0",
                    cs, sv, vty, ex, vty, cs
                );
                ex
            };
            let iv = next_t(tmp);
            let _ = writeln!(out, "  {} = fptosi {} {} to {}", iv, fty, f0, bits_ty(dbits));
            emit_write_gpr_sized(out, tmp, instr.op0_register(), &iv, dbits);
            Ok(false)
        }

        other => Err(format!(
            "Unsupported x86_64 mnemonic '{:?}' at IP 0x{:x} ({:?})",
            other,
            instr.ip(),
            instr
        )),
    }
}
