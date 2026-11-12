use crate::{extract_declarations_and_body, SsaContextEnvelope, SsaVariable};
use omni_verify::LlvmVerifier;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const DEFAULT_IR_SYSTEM_PROMPT: &str = "You are the OmniScript LLVM IR Compiler Backend.\nSynthesize valid LLVM IR basic blocks matching the exact target ABI, register context, and intent.\nOutput ONLY raw LLVM IR instructions without markdown formatting, code fences, or conversational prose.";

const DIALECT_CLASSIFIER_SYSTEM_PROMPT: &str = "You are OmniScript Polyglot Dialect Classifier. Identify the programming language ecosystem for this import. Output strictly one lowercase word: java, python, rust, go, javascript, c, csharp, lua, or zig.";

// Parametric pools for combinatorial SSA register, variable, function, and struct diversity
const SLICE_NAMES: &[&str] = &["items", "records", "frames", "packets", "samples", "tokens", "weights", "batches"];
const MAP_NAMES: &[&str] = &["metrics", "scores", "headers", "cache", "ledger", "lookup", "table", "session"];
const CHAN_NAMES: &[&str] = &["ch", "work_ch", "events_ch", "tx_chan", "results_ch", "pipe_ch"];
const SCALAR_NAMES: &[&str] = &["val", "acc", "delta", "count", "limit", "offset", "mask", "epoch", "quota", "stride"];
const FLOAT_NAMES: &[&str] = &["mass", "vel", "gain", "alpha", "theta", "energy", "ratio", "sigma", "temp_k"];
const STR_NAMES: &[&str] = &["msg", "prefix", "label", "route", "host", "token_str", "err_msg", "payload"];
const FN_NAMES: &[&str] = &[
    "transform_stream", "process_batch", "compute_digest", "dispatch_frame",
    "evaluate_metric", "step_pipeline", "decode_packet", "run_worker",
];
const STRUCT_SPECS: &[(&str, &[&str])] = &[
    ("Node", &["data", "next"]),
    ("PacketHeader", &["seq_id", "payload_len", "flags"]),
    ("TelemetrySample", &["timestamp_ns", "sensor_id", "reading_bits"]),
    ("AccountState", &["balance", "nonce", "tier"]),
    ("CipherContext", &["key_hi", "key_lo", "rounds", "iv_ptr"]),
    ("PublicKey", &["n", "e"]),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub task_id: String,
    pub domain: String,
    pub action_directive: String,
    pub idiom_hint: String,
    pub prompt: String,
    pub completion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    pub verified: bool,
    pub token_estimate: usize,
}

/// Deterministic SplitMix64 PRNG so dataset synthesis is 100% reproducible without external crates.
#[derive(Clone, Copy)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: usize, hi_exclusive: usize) -> usize {
        if hi_exclusive <= lo {
            return lo;
        }
        lo + (self.next_u64() as usize % (hi_exclusive - lo))
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let idx = self.range(0, items.len());
        &items[idx]
    }
}

struct GeneratedScenario {
    domain: &'static str,
    envelope: SsaContextEnvelope,
    harness_preamble: String,
    extra_decls: Vec<String>,
    completion: String,
    broken_completion: Option<String>,
    diagnostic_hint: Option<String>,
}

pub struct SyntheticDatasetGenerator {
    verifier: LlvmVerifier,
}

impl SyntheticDatasetGenerator {
    pub fn new() -> Result<Self, omni_verify::VerifyError> {
        let verifier = LlvmVerifier::new()?;
        Ok(Self { verifier })
    }

    /// Generates a Clang-verified training dataset spanning 20 parametric 64-bit / IEEE-754 `double` /
    /// `%struct.*` GEP / `setjmp` / C FFI `declare` / `<compiler_diagnostic>` self-repair families,
    /// plus polyglot dialect-classification tasks when `count >= 100`.
    pub fn generate_verified_dataset(&self, count: usize) -> Vec<DatasetRecord> {
        let mut rng = SplitMix64::new(0x0D15_7111_2026_64B1);
        let dialect_budget = if count >= 100 { (count / 20).max(20) } else { 0 };
        let ir_target = count.saturating_sub(dialect_budget);

        let mut scenarios = Vec::with_capacity(ir_target);
        for i in 0..ir_target {
            let family = i % 20;
            // Rotate repair phase across cycles of 20 so ALL 20 families receive ~25% self-repair coverage,
            // while ALL samples with a `broken_completion` retain `rejected_completion` for ORPO!
            let emit_as_repair = count >= 20 && ((i + (i / 20)) % 4 == 3);
            let scen = self.synthesize_parametric_scenario(i, family, emit_as_repair, &mut rng);
            scenarios.push((i, emit_as_repair, scen));
        }

        // Batch-verify with Clang (64 isolated functions per translation unit, with per-item fallback)
        let verified_flags = self.verify_scenarios_batched(&scenarios);

        let mut records = Vec::with_capacity(count);
        for ((idx, is_repair, scen), ok) in scenarios.into_iter().zip(verified_flags.into_iter()) {
            if !ok {
                continue;
            }
            let fallback_diag = format!(
                "LLVM IR verification error: expected explicit 64-bit SSA instructions starting at %t_{} without markdown fences or C-style casts",
                scen.envelope.next_ssa_idx
            );
            let prompt = if is_repair {
                let diag = scen.diagnostic_hint.as_deref().unwrap_or(&fallback_diag);
                scen.envelope.to_prompt_with_feedback(Some(diag))
            } else {
                scen.envelope.to_prompt_micro_tags()
            };

            let domain_str = if is_repair {
                format!("{}_self_repair", scen.domain)
            } else {
                scen.domain.to_string()
            };
            let task_id = format!("{}_{:05}", domain_str, idx + 1);
            let completion = scen.completion;
            let token_estimate = (prompt.len() + completion.len()) / 4;

            records.push(DatasetRecord {
                task_id,
                domain: domain_str,
                action_directive: scen
                    .envelope
                    .action_directive
                    .unwrap_or_else(|| "LOWER_ANONYMOUS_INTENT".to_string()),
                idiom_hint: scen
                    .envelope
                    .idiom_hint
                    .unwrap_or_else(|| "Standard".to_string()),
                prompt,
                completion,
                rejected_completion: scen.broken_completion,
                system_prompt: None,
                verified: true,
                token_estimate,
            });
        }

        if dialect_budget > 0 {
            let dialect_recs = Self::generate_dialect_classification_records(dialect_budget, &mut rng);
            records.extend(dialect_recs);
        }

        records
    }

    fn build_single_harness_fn(fn_sym: &str, ret_ty: &str, scen: &GeneratedScenario) -> (Vec<String>, Vec<String>, String) {
        let (dyn_decls, body) = extract_declarations_and_body(&scen.completion);
        let mut all_decls = scen.extra_decls.clone();
        for d in dyn_decls {
            if !all_decls.contains(&d) {
                all_decls.push(d);
            }
        }
        let structs = scen.envelope.struct_definitions.clone();
        let default_ret = if ret_ty == "i64" { "  ret i64 0\n" } else { "  ret i32 0\n" };
        let has_term = body.lines().any(|l| {
            let t = l.trim();
            t.starts_with("ret ") || t == "unreachable"
        });

        let mut fn_ir = format!("define {} @{}() {{\nentry:\n", ret_ty, fn_sym);
        fn_ir.push_str(&scen.harness_preamble);
        fn_ir.push_str(&body);
        if !body.ends_with('\n') {
            fn_ir.push('\n');
        }
        if !has_term {
            fn_ir.push_str(default_ret);
        }
        fn_ir.push_str("}\n");
        (structs, all_decls, fn_ir)
    }

    fn assemble_module_ir(structs: &[String], decls: &[String], fns_ir: &str) -> String {
        let mut ir = String::with_capacity(fns_ir.len() + 2048);
        ir.push_str(&format!("target triple = \"{}\"\n\n", omni_runtime::host_target_triple()));
        ir.push_str("declare i8* @malloc(i64)\n");
        ir.push_str("declare void @free(i8*)\n");
        ir.push_str("declare i32 @printf(i8*, ...)\n");
        ir.push_str(omni_runtime::runtime_llvm_declarations());
        ir.push('\n');

        let runtime_decls = omni_runtime::runtime_llvm_declarations();
        let mut seen_structs = HashSet::new();
        for s in structs {
            let st = s.trim();
            if st.starts_with("%struct.") && seen_structs.insert(st.to_string()) {
                ir.push_str(st);
                ir.push('\n');
            }
        }
        if !seen_structs.is_empty() {
            ir.push('\n');
        }

        let mut seen_decls = HashSet::new();
        for d in decls {
            let dt = d.trim();
            if dt.is_empty() {
                continue;
            }
            let sym = dt
                .find('@')
                .and_then(|at| dt[at..].find('(').map(|p| &dt[at..at + p]))
                .unwrap_or("");
            if !sym.is_empty()
                && (runtime_decls.contains(sym)
                    || sym == "@malloc"
                    || sym == "@free"
                    || sym == "@printf")
            {
                continue;
            }
            if seen_decls.insert(dt.to_string()) {
                ir.push_str(dt);
                ir.push('\n');
            }
        }
        ir.push('\n');
        ir.push_str(fns_ir);
        ir
    }

    fn verify_scenarios_batched(&self, scenarios: &[(usize, bool, GeneratedScenario)]) -> Vec<bool> {
        let mut results = vec![false; scenarios.len()];
        const BATCH_SIZE: usize = 64;

        for (chunk_idx, chunk) in scenarios.chunks(BATCH_SIZE).enumerate() {
            let base = chunk_idx * BATCH_SIZE;
            let mut batch_structs = Vec::new();
            let mut batch_decls = Vec::new();
            let mut batch_fns = String::new();

            for (offset, (_, _, scen)) in chunk.iter().enumerate() {
                let fn_sym = format!("harness_{}", base + offset);
                let ret_ty = if scen.envelope.return_type == "i64" { "i64" } else { "i32" };
                let (st, dc, f_ir) = Self::build_single_harness_fn(&fn_sym, ret_ty, scen);
                batch_structs.extend(st);
                batch_decls.extend(dc);
                batch_fns.push_str(&f_ir);
                batch_fns.push('\n');
            }

            let batch_mod = Self::assemble_module_ir(&batch_structs, &batch_decls, &batch_fns);
            if self.verifier.verify_ir(&batch_mod).is_ok() {
                for offset in 0..chunk.len() {
                    results[base + offset] = true;
                }
            } else {
                // Pinpoint individual items if any single generated block failed verification
                for (offset, (_, _, scen)) in chunk.iter().enumerate() {
                    let ret_ty = if scen.envelope.return_type == "i64" { "i64" } else { "i32" };
                    let (st, dc, f_ir) = Self::build_single_harness_fn("harness_single", ret_ty, scen);
                    let single_mod = Self::assemble_module_ir(&st, &dc, &f_ir);
                    results[base + offset] = self.verifier.verify_ir(&single_mod).is_ok();
                }
            }
        }

        results
    }

    #[allow(clippy::too_many_lines)]
    fn synthesize_parametric_scenario(
        &self,
        seq_idx: usize,
        family: usize,
        _for_repair: bool,
        rng: &mut SplitMix64,
    ) -> GeneratedScenario {
        let target_abi = omni_runtime::host_target_triple().to_string();
        let fn_name = (*rng.pick(FN_NAMES)).to_string();
        let ret_ty = if rng.range(0, 4) == 0 { "i32" } else { "i64" };
        let ssa = rng.range(8, 1600);
        let slot_suf = rng.range(1, 300);
        let anchor = Some(100 + seq_idx);

        match family {
            // 0: 64-bit Slice Index Multiply & Append
            0 => {
                let s_name = *rng.pick(SLICE_NAMES);
                let k_name = *rng.pick(SCALAR_NAMES);
                let s_reg = format!("%slice_var_{}_{}", s_name, slot_suf);
                let k_reg = format!("%var_{}_{}", k_name, slot_suf + 1);
                let idx = rng.range(0, 4);
                let intent = format!("{0}.append({0}[{1}] * {2})", s_name, idx, k_name);

                let preamble = format!(
                    "  {s_reg} = alloca i8*\n  %init_s = call i8* @__omni_slice_create(i64 8)\n  store i8* %init_s, i8** {s_reg}\n  call void @__omni_slice_append(i8* %init_s, i64 42)\n  {k_reg} = alloca i64\n  store i64 1000000, i64* {k_reg}\n"
                );
                let r0 = ssa;
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{r0} = load i8*, i8** {s_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{r0}, i64 {idx})\n  %t_{} = load i64, i64* {k_reg}\n  %t_{} = mul i64 %t_{}, %t_{}\n  call void @__omni_slice_append(i8* %t_{r0}, i64 %t_{})\n",
                    r0 + 1, r0 + 2, r0 + 3, r0 + 1, r0 + 2, r0 + 3
                );
                let broken = format!(
                    "  %t_{r0} = load i8*, i8** {s_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{r0}, i64 {idx})\n  %t_{} = load i32, i32* {k_reg}\n  %t_{} = mul i64 %t_{}, %t_{}\n  call void @__omni_slice_append(i8* %t_{r0}, i64 %t_{})\n",
                    r0 + 1, r0 + 2, r0 + 3, r0 + 1, r0 + 2, r0 + 3
                );
                let diag = format!(
                    "LLVM IR syntax error: '{k_reg}' defined with type 'i64*' but expected 'i32*'"
                );

                GeneratedScenario {
                    domain: "collections_transform_64",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: s_name.to_string(), llvm_name: s_reg, type_hint: "i8* (omni_slice)".to_string() },
                            SsaVariable { name: k_name.to_string(), llvm_name: k_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("PythonListTransform".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("python".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 1: 64-bit Arithmetic Beyond 32-Bit Boundary (`> i32::MAX`)
            1 => {
                let dst = *rng.pick(SCALAR_NAMES);
                let src = SLICE_NAMES[rng.range(0, SLICE_NAMES.len())];
                let dst_reg = format!("%var_{}_{}", dst, slot_suf);
                let src_reg = format!("%var_{}_{}", src, slot_suf + 1);
                let big_const: i64 = 4_294_967_296 + (rng.range(100, 900_000) as i64) * 1000;
                let (op_sym, llvm_op) = *rng.pick(&[("+", "add"), ("-", "sub"), ("*", "mul")]);
                let intent = format!("{dst} = {src} {op_sym} {big_const}");

                let preamble = format!(
                    "  {dst_reg} = alloca i64\n  store i64 0, i64* {dst_reg}\n  {src_reg} = alloca i64\n  store i64 5000000000, i64* {src_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i64, i64* {src_reg}\n  %t_{} = {llvm_op} i64 %t_{ssa}, {big_const}\n  store i64 %t_{}, i64* {dst_reg}\n",
                    ssa + 1, ssa + 1
                );
                let broken = format!(
                    "  %t_{ssa} = load i64, i64* {src_reg}\n  %t_{} = {llvm_op} i64 %t_{ssa}, {big_const}\n  store i64 %t_{}, i64* %var_{dst}\n",
                    ssa + 1, ssa + 1
                );
                let diag = format!("LLVM IR syntax error: use of undefined value '%var_{dst}' (active symbol is '{dst_reg}')");

                GeneratedScenario {
                    domain: "i64_large_scalar",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: dst.to_string(), llvm_name: dst_reg.clone(), type_hint: "i64".to_string() },
                            SsaVariable { name: src.to_string(), llvm_name: src_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("LargeScalar64".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {dst_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("rust".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 2: IEEE-754 `double` Floating-Point Kernel (`fadd` / `fmul` / `fdiv`)
            2 => {
                let f1 = *rng.pick(FLOAT_NAMES);
                let f2 = "scale_f";
                let dst = "out_f";
                let r1 = format!("%f64_var_{}_{}", f1, slot_suf);
                let r2 = format!("%f64_var_{}_{}", f2, slot_suf + 1);
                let rdst = format!("%f64_var_{}_{}", dst, slot_suf + 2);
                let coeff = (rng.range(1, 95) as f64) * 0.25;
                let coeff_hex = format!("0x{:016X}", coeff.to_bits());
                let intent = format!("{dst} = ({f1} * {f2}) + {coeff}");

                let preamble = format!(
                    "  {r1} = alloca double\n  store double 0x4004000000000000, double* {r1}\n  {r2} = alloca double\n  store double 0x4010000000000000, double* {r2}\n  {rdst} = alloca double\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load double, double* {r1}\n  %t_{} = load double, double* {r2}\n  %t_{} = fmul double %t_{ssa}, %t_{}\n  %t_{} = fadd double %t_{}, {coeff_hex}\n  store double %t_{}, double* {rdst}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2, ssa + 3
                );
                let broken = format!(
                    "  %t_{ssa} = load double, double* {r1}\n  %t_{} = load double, double* {r2}\n  %t_{} = mul double %t_{ssa}, %t_{}\n  store double %t_{}, double* {rdst}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 2
                );
                let diag = "LLVM IR syntax error: invalid operand type 'double' for integer instruction 'mul' (use 'fmul double')".to_string();

                GeneratedScenario {
                    domain: "ieee754_double_math",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: f1.to_string(), llvm_name: r1, type_hint: "double".to_string() },
                            SsaVariable { name: f2.to_string(), llvm_name: r2, type_hint: "double".to_string() },
                            SsaVariable { name: dst.to_string(), llvm_name: rdst, type_hint: "double".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("Float64Kernel".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("c".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 3: Mixed `i64` -> `double` Promotion (`sitofp` + `fptosi`)
            3 => {
                let f_var = *rng.pick(FLOAT_NAMES);
                let i_var = *rng.pick(SCALAR_NAMES);
                let out_var = "quantized";
                let f_reg = format!("%f64_var_{}_{}", f_var, slot_suf);
                let i_reg = format!("%var_{}_{}", i_var, slot_suf + 1);
                let out_reg = format!("%var_{}_{}", out_var, slot_suf + 2);
                let intent = format!("{out_var} = int({f_var} * float({i_var}))");

                let preamble = format!(
                    "  {f_reg} = alloca double\n  store double 0x400921FB54442D18, double* {f_reg}\n  {i_reg} = alloca i64\n  store i64 12, i64* {i_reg}\n  {out_reg} = alloca i64\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load double, double* {f_reg}\n  %t_{} = load i64, i64* {i_reg}\n  %t_{} = sitofp i64 %t_{} to double\n  %t_{} = fmul double %t_{ssa}, %t_{}\n  %t_{} = fptosi double %t_{} to i64\n  store i64 %t_{}, i64* {out_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2, ssa + 4, ssa + 3, ssa + 4
                );
                let broken = format!(
                    "  %t_{ssa} = load double, double* {f_reg}\n  %t_{} = load i64, i64* {i_reg}\n  %t_{} = fmul double %t_{ssa}, %t_{}\n  store double %t_{}, i64* {out_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 2
                );
                let diag = format!(
                    "LLVM IR syntax error: '%t_{}' defined with type 'i64' but expected 'double' (use 'sitofp i64 to double')",
                    ssa + 1
                );

                GeneratedScenario {
                    domain: "mixed_int_float_cast",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: f_var.to_string(), llvm_name: f_reg, type_hint: "double".to_string() },
                            SsaVariable { name: i_var.to_string(), llvm_name: i_reg, type_hint: "i64".to_string() },
                            SsaVariable { name: out_var.to_string(), llvm_name: out_reg.clone(), type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("FloatIntPromotion".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {out_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("python".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 4: IEEE-754 `fcmp` + `select` Threshold Gate
            4 => {
                let f_var = *rng.pick(FLOAT_NAMES);
                let flag_var = "alert_code";
                let f_reg = format!("%f64_var_{}_{}", f_var, slot_suf);
                let flag_reg = format!("%var_{}_{}", flag_var, slot_suf + 1);
                let thresh = (rng.range(10, 500) as f64) * 0.5;
                let thresh_hex = format!("0x{:016X}", thresh.to_bits());
                let hi_code = rng.range(100, 900);
                let intent = format!("{flag_var} = {f_var} > {thresh} ? {hi_code} : 0");

                let preamble = format!(
                    "  {f_reg} = alloca double\n  store double {thresh_hex}, double* {f_reg}\n  {flag_reg} = alloca i64\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load double, double* {f_reg}\n  %t_{} = fcmp ogt double %t_{ssa}, {thresh_hex}\n  %t_{} = select i1 %t_{}, i64 {hi_code}, i64 0\n  store i64 %t_{}, i64* {flag_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 2
                );
                let broken = format!(
                    "  %t_{ssa} = load double, double* {f_reg}\n  %t_{} = icmp sgt double %t_{ssa}, {thresh_hex}\n  %t_{} = select i1 %t_{}, i64 {hi_code}, i64 0\n  store i64 %t_{}, i64* {flag_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 2
                );
                let diag = "LLVM IR syntax error: icmp requires integer or pointer operands, got 'double' (use 'fcmp ogt double')".to_string();

                GeneratedScenario {
                    domain: "fcmp_ternary_select",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: f_var.to_string(), llvm_name: f_reg, type_hint: "double".to_string() },
                            SsaVariable { name: flag_var.to_string(), llvm_name: flag_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("TernaryFloatCompare".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("javascript".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 5: Monomorphized `%struct.*` GEP Field Read + Update
            5 => {
                let (cls_name, fields) = *rng.pick(STRUCT_SPECS);
                let f_idx = rng.range(0, fields.len());
                let f_name = fields[f_idx];
                let inst_var = "obj";
                let inst_reg = format!("%var_{}_{}", inst_var, slot_suf);
                let inc_val = rng.range(1, 2048);
                let type_body = vec!["i64"; fields.len()].join(", ");
                let s_def = format!("%struct.{cls_name} = type {{ {type_body} }}");
                let intent = format!("{inst_var}.{f_name} = {inst_var}.{f_name} + {inc_val}");

                let preamble = format!(
                    "  %s_alloc = alloca %struct.{cls_name}\n  {inst_reg} = alloca %struct.{cls_name}*\n  store %struct.{cls_name}* %s_alloc, %struct.{cls_name}** {inst_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load %struct.{cls_name}*, %struct.{cls_name}** {inst_reg}\n  %t_{} = getelementptr inbounds %struct.{cls_name}, %struct.{cls_name}* %t_{ssa}, i32 0, i32 {f_idx}\n  %t_{} = load i64, i64* %t_{}\n  %t_{} = add i64 %t_{}, {inc_val}\n  store i64 %t_{}, i64* %t_{}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2, ssa + 3, ssa + 1
                );
                let broken = format!(
                    "  %t_{ssa} = load %struct.{cls_name}*, %struct.{cls_name}** {inst_reg}\n  %t_{} = getelementptr inbounds %struct.{cls_name}, %struct.{cls_name}* %t_{ssa}, i32 {f_idx}\n  store i64 {inc_val}, i64* %t_{}\n",
                    ssa + 1, ssa + 1
                );
                let diag = format!(
                    "LLVM IR syntax error: '%t_{}' defined with type '%struct.{cls_name}*' but expected 'i64*' (missing leading 'i32 0' struct GEP index)",
                    ssa + 1
                );

                GeneratedScenario {
                    domain: "struct_gep_mutation",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![SsaVariable {
                            name: inst_var.to_string(),
                            llvm_name: inst_reg,
                            type_hint: format!("%struct.{cls_name}*"),
                        }],
                        struct_definitions: vec![s_def],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("StructFieldGEP".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_POLYGLOT_SYNTAX".to_string()),
                        ecosystem_context: Some("java".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 6: Devirtualized 64-Bit Method Call (`@__omni_method_<Cls>_<m>(i8*, i64 x 5)`)
            6 => {
                let (cls_name, fields) = *rng.pick(STRUCT_SPECS);
                let method_name = *rng.pick(&["compute", "advance", "checksum", "validate"]);
                let inst_reg = format!("%var_inst_{}", slot_suf);
                let dst_reg = format!("%var_out_{}", slot_suf + 1);
                let arg_lit = rng.range(2, 512);
                let type_body = vec!["i64"; fields.len()].join(", ");
                let s_def = format!("%struct.{cls_name} = type {{ {type_body} }}");
                let intent = format!("res = inst.{method_name}({arg_lit})");

                let preamble = format!(
                    "  %s_alloc = alloca %struct.{cls_name}\n  {inst_reg} = alloca %struct.{cls_name}*\n  store %struct.{cls_name}* %s_alloc, %struct.{cls_name}** {inst_reg}\n  {dst_reg} = alloca i64\n"
                );
                let extra_decl = format!(
                    "declare i64 @__omni_method_{cls_name}_{method_name}(i8*, i64, i64, i64, i64, i64)"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  declare i64 @__omni_method_{cls_name}_{method_name}(i8*, i64, i64, i64, i64, i64)\n  %t_{ssa} = load %struct.{cls_name}*, %struct.{cls_name}** {inst_reg}\n  %t_{} = bitcast %struct.{cls_name}* %t_{ssa} to i8*\n  %t_{} = call i64 @__omni_method_{cls_name}_{method_name}(i8* %t_{}, i64 {arg_lit}, i64 0, i64 0, i64 0, i64 0)\n  store i64 %t_{}, i64* {dst_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 2
                );

                GeneratedScenario {
                    domain: "method_dispatch_64",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: "inst".to_string(), llvm_name: inst_reg, type_hint: format!("%struct.{cls_name}*") },
                            SsaVariable { name: "res".to_string(), llvm_name: dst_reg.clone(), type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![s_def],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("DevirtualizedMethodCall".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {dst_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("java".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![extra_decl],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 7: Carved Polyglot Function Call (`i64 @__omni_fn_<callee>(i64 x 6)`)
            7 => {
                let callee = *rng.pick(&["xxhash_mix", "adler_step", "varint_zig", "crc_fold", "mod_pow"]);
                let arg_var = *rng.pick(SCALAR_NAMES);
                let dst_var = "out_val";
                let a_reg = format!("%var_{}_{}", arg_var, slot_suf);
                let d_reg = format!("%var_{}_{}", dst_var, slot_suf + 1);
                let salt = rng.range(7, 9999);
                let intent = format!("{dst_var} = {callee}({arg_var}, {salt})");

                let preamble = format!(
                    "  {a_reg} = alloca i64\n  store i64 256, i64* {a_reg}\n  {d_reg} = alloca i64\n"
                );
                let extra_decl = format!("declare i64 @__omni_fn_{callee}(i64, i64, i64, i64, i64, i64)");
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i64, i64* {a_reg}\n  %t_{} = call i64 @__omni_fn_{callee}(i64 %t_{ssa}, i64 {salt}, i64 0, i64 0, i64 0, i64 0)\n  store i64 %t_{}, i64* {d_reg}\n",
                    ssa + 1, ssa + 1
                );
                let broken = format!(
                    "  %t_{ssa} = load i64, i64* {a_reg}\n  %t_{} = call i32 @__omni_fn_{callee}(i64 %t_{ssa}, i64 {salt}, i64 0, i64 0)\n  store i32 %t_{}, i64* {d_reg}\n",
                    ssa + 1, ssa + 1
                );
                let diag = format!(
                    "LLVM IR syntax error: '@__omni_fn_{callee}' defined with type 'i64 (i64, i64, i64, i64, i64, i64)*' (6-slot 64-bit ABI)"
                );

                GeneratedScenario {
                    domain: "carved_fn_abi64",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: arg_var.to_string(), llvm_name: a_reg, type_hint: "i64".to_string() },
                            SsaVariable { name: dst_var.to_string(), llvm_name: d_reg.clone(), type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ForeignModuleAssignment".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {d_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("go".to_string()),
                        lifetime_context: None,
                        import_chain: vec![format!("__OMNI_CARVED_FN__:{callee}")],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![extra_decl],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 8: Foreign Module Random / Crypto Range (`RandomNumberGenerator.GetInt32` / `random.randint`)
            8 => {
                let dst = *rng.pick(&["rand", "nonce", "jitter", "sample_id", "ticket"]);
                let d_reg = format!("%var_{}_{}", dst, slot_suf);
                let lo = rng.range(0, 20);
                let hi = rng.range(50, 10_000);
                let (eco, intent) = match seq_idx % 3 {
                    0 => ("csharp (module: System.Security.Cryptography)", format!("{dst} = RandomNumberGenerator.GetInt32({lo}, {hi})")),
                    1 => ("python (module: random)", format!("{dst} = random.randint({lo}, {hi})")),
                    _ => ("java (module: java.util.Random)", format!("{dst} = rng.nextInt({hi})")),
                };
                let eff_lo = if seq_idx % 3 == 2 { 0 } else { lo };

                let preamble = format!("  {d_reg} = alloca i64\n");
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = call i64 @__omni_random_range(i64 {eff_lo}, i64 {hi})\n  store i64 %t_{ssa}, i64* {d_reg}\n"
                );
                let broken = format!(
                    "  %t_{ssa} = call i64 @__omni_random_range((int), i64 {hi})\n  store i64 %t_{ssa}, i64* {d_reg}\n"
                );
                let diag = "LLVM IR syntax error: expected type token, got C-style cast '(int)' (use explicit 'i64 <val>')".to_string();

                GeneratedScenario {
                    domain: "foreign_random_range",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![SsaVariable {
                            name: dst.to_string(),
                            llvm_name: d_reg.clone(),
                            type_hint: "i64".to_string(),
                        }],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ForeignModuleAssignment".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {d_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some(eco.to_string()),
                        lifetime_context: None,
                        import_chain: vec![eco.to_string()],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 9: Open-World C ABI `declare` Hoisting (`strlen` / `atol` / `labs`)
            9 => {
                let s_var = *rng.pick(STR_NAMES);
                let d_var = *rng.pick(SCALAR_NAMES);
                let s_reg = format!("%str_var_{}_{}", s_var, slot_suf);
                let d_reg = format!("%var_{}_{}", d_var, slot_suf + 1);
                let (c_fn, intent) = if seq_idx % 2 == 0 {
                    ("strlen", format!("{d_var} = libc.strlen({s_var})"))
                } else {
                    ("atol", format!("{d_var} = libc.atol({s_var})"))
                };

                let preamble = format!(
                    "  {s_reg} = alloca i8*\n  %empty_s = call i8* @__omni_int_to_str(i64 12345)\n  store i8* %empty_s, i8** {s_reg}\n  {d_reg} = alloca i64\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  declare i64 @{c_fn}(i8*)\n  %t_{ssa} = load i8*, i8** {s_reg}\n  %t_{} = call i64 @{c_fn}(i8* %t_{ssa})\n  store i64 %t_{}, i64* {d_reg}\n",
                    ssa + 1, ssa + 1
                );
                let broken = format!(
                    "  %t_{ssa} = load i8*, i8** {s_reg}\n  %t_{} = call i64 @{c_fn}(i8* %t_{ssa})\n  store i64 %t_{}, i64* {d_reg}\n",
                    ssa + 1, ssa + 1
                );
                let diag = format!("LLVM IR syntax error: use of undefined value '@{c_fn}' (emit 'declare i64 @{c_fn}(i8*)' for external C ABI symbol)");

                GeneratedScenario {
                    domain: "open_world_c_declare",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: s_var.to_string(), llvm_name: s_reg, type_hint: "i8* (string)".to_string() },
                            SsaVariable { name: d_var.to_string(), llvm_name: d_reg.clone(), type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ForeignCDeclare".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {d_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("c (module: string.h)".to_string()),
                        lifetime_context: None,
                        import_chain: vec!["c (module: string.h)".to_string()],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 10: Drilled Native Shared-Object Spine (`zlib` `compressBound` / `crc32`)
            10 => {
                let in_var = *rng.pick(SCALAR_NAMES);
                let out_var = "bound_bytes";
                let in_reg = format!("%var_{}_{}", in_var, slot_suf);
                let out_reg = format!("%var_{}_{}", out_var, slot_suf + 1);
                let intent = format!("{out_var} = zlib.compressBound({in_var})");

                let preamble = format!(
                    "  {in_reg} = alloca i64\n  store i64 4096, i64* {in_reg}\n  {out_reg} = alloca i64\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  declare i64 @compressBound(i64)\n  %t_{ssa} = load i64, i64* {in_reg}\n  %t_{} = call i64 @compressBound(i64 %t_{ssa})\n  store i64 %t_{}, i64* {out_reg}\n",
                    ssa + 1, ssa + 1
                );

                GeneratedScenario {
                    domain: "drilled_so_ffi",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: in_var.to_string(), llvm_name: in_reg, type_hint: "i64".to_string() },
                            SsaVariable { name: out_var.to_string(), llvm_name: out_reg.clone(), type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ForeignModuleAssignment".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some(format!("store 64-bit scalar result to {out_reg}")),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("c (module: zlib)".to_string()),
                        lifetime_context: None,
                        import_chain: vec![
                            "__OMNI_DRILLED_MECH__:ELF dynsym libz.so -> uLong compressBound(uLong sourceLen)".to_string(),
                        ],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 11: Go MPMC Channel Send (`ch <- sample + bias`)
            11 => {
                let ch_name = *rng.pick(CHAN_NAMES);
                let s_name = *rng.pick(SCALAR_NAMES);
                let ch_reg = format!("%chan_{}_{}", ch_name, slot_suf);
                let s_reg = format!("%var_{}_{}", s_name, slot_suf + 1);
                let bias = rng.range(10, 5000);
                let intent = format!("{ch_name} <- {s_name} + {bias}");

                let preamble = format!(
                    "  {ch_reg} = alloca i8*\n  %init_c = call i8* @__omni_chan_make(i64 8, i64 8)\n  store i8* %init_c, i8** {ch_reg}\n  {s_reg} = alloca i64\n  store i64 50, i64* {s_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {ch_reg}\n  %t_{} = load i64, i64* {s_reg}\n  %t_{} = add i64 %t_{}, {bias}\n  %t_{} = call i1 @__omni_chan_send_int(i8* %t_{ssa}, i64 %t_{})\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2
                );

                GeneratedScenario {
                    domain: "channel_send_64",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: ch_name.to_string(), llvm_name: ch_reg, type_hint: "i8* (omni_chan)".to_string() },
                            SsaVariable { name: s_name.to_string(), llvm_name: s_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("GoChannelSend".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: Some("M:N Fiber".to_string()),
                        yield_expectation: Some("i1".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("go".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 12: Go MPMC Channel Receive (`res := <-ch`)
            12 => {
                let ch_name = *rng.pick(CHAN_NAMES);
                let dst = *rng.pick(SCALAR_NAMES);
                let ch_reg = format!("%chan_{}_{}", ch_name, slot_suf);
                let dst_reg = format!("%var_{}_{}", dst, slot_suf + 1);
                let intent = format!("{dst} := <-{ch_name}");

                let preamble = format!(
                    "  {ch_reg} = alloca i8*\n  %init_c = call i8* @__omni_chan_make(i64 4, i64 8)\n  store i8* %init_c, i8** {ch_reg}\n  call i1 @__omni_chan_send_int(i8* %init_c, i64 777)\n  {dst_reg} = alloca i64\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {ch_reg}\n  %t_{} = call i64 @__omni_chan_recv_int(i8* %t_{ssa})\n  store i64 %t_{}, i64* {dst_reg}\n",
                    ssa + 1, ssa + 1
                );

                GeneratedScenario {
                    domain: "channel_recv_64",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: ch_name.to_string(), llvm_name: ch_reg, type_hint: "i8* (omni_chan)".to_string() },
                            SsaVariable { name: dst.to_string(), llvm_name: dst_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("GoChannelRecv".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: Some("M:N Fiber".to_string()),
                        yield_expectation: Some("i64".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("go".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 13: Zero-GC Hash Map Update & Lookup (`@__omni_map_set_int` / `@__omni_map_get_int`)
            13 => {
                let m_name = *rng.pick(MAP_NAMES);
                let k_name = "key_id";
                let v_name = *rng.pick(SCALAR_NAMES);
                let m_reg = format!("%map_var_{}_{}", m_name, slot_suf);
                let k_reg = format!("%var_{}_{}", k_name, slot_suf + 1);
                let v_reg = format!("%var_{}_{}", v_name, slot_suf + 2);
                let intent = format!("{m_name}[{k_name}] = {m_name}.get({k_name}, 0) + {v_name}");

                let preamble = format!(
                    "  {m_reg} = alloca i8*\n  %init_m = call i8* @__omni_map_create(i64 16)\n  store i8* %init_m, i8** {m_reg}\n  {k_reg} = alloca i64\n  store i64 42, i64* {k_reg}\n  {v_reg} = alloca i64\n  store i64 100, i64* {v_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {m_reg}\n  %t_{} = load i64, i64* {k_reg}\n  %t_{} = call i64 @__omni_map_get_int(i8* %t_{ssa}, i64 %t_{})\n  %t_{} = load i64, i64* {v_reg}\n  %t_{} = add i64 %t_{}, %t_{}\n  call void @__omni_map_set_int(i8* %t_{ssa}, i64 %t_{}, i64 %t_{})\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 4, ssa + 2, ssa + 3, ssa + 1, ssa + 4
                );

                GeneratedScenario {
                    domain: "map_read_modify_write",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: m_name.to_string(), llvm_name: m_reg, type_hint: "i8* (omni_map)".to_string() },
                            SsaVariable { name: k_name.to_string(), llvm_name: k_reg, type_hint: "i64".to_string() },
                            SsaVariable { name: v_name.to_string(), llvm_name: v_reg, type_hint: "i64".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("MapAccumulate".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("python".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 14: Zig-Style Scope Arena Allocation (`@__omni_arena_alloc`)
            14 => {
                let buf_bytes = (1usize << rng.range(6, 14)) as i64;
                let align_bytes = if seq_idx % 2 == 0 { 16 } else { 32 };
                let a_reg = format!("%arena_ptr_{}", slot_suf);
                let out_reg = format!("%buf_ptr_{}", slot_suf + 1);
                let intent = format!("packet_buf = arena.alloc({buf_bytes}, align={align_bytes})");

                let preamble = format!(
                    "  {a_reg} = alloca i8*\n  %init_a = call i8* @__omni_arena_create(i64 65536)\n  store i8* %init_a, i8** {a_reg}\n  {out_reg} = alloca i8*\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {a_reg}\n  %t_{} = call i8* @__omni_arena_alloc(i8* %t_{ssa}, i64 {buf_bytes}, i64 {align_bytes})\n  store i8* %t_{}, i8** {out_reg}\n",
                    ssa + 1, ssa + 1
                );

                GeneratedScenario {
                    domain: "arena_aligned_alloc",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: "arena".to_string(), llvm_name: a_reg, type_hint: "i8* (omni_arena)".to_string() },
                            SsaVariable { name: "packet_buf".to_string(), llvm_name: out_reg, type_hint: "i8*".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ZigArenaAlloc".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("i8*".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("zig".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 15: Native String Formatting & Concatenation (`@__omni_float_to_str` + `@__omni_str_concat`)
            15 => {
                let pfx = *rng.pick(STR_NAMES);
                let f_var = *rng.pick(FLOAT_NAMES);
                let out_s = "formatted";
                let p_reg = format!("%str_var_{}_{}", pfx, slot_suf);
                let f_reg = format!("%f64_var_{}_{}", f_var, slot_suf + 1);
                let o_reg = format!("%str_var_{}_{}", out_s, slot_suf + 2);
                let intent = format!("{out_s} = {pfx} + str({f_var})");

                let preamble = format!(
                    "  {p_reg} = alloca i8*\n  %init_p = call i8* @__omni_int_to_str(i64 1)\n  store i8* %init_p, i8** {p_reg}\n  {f_reg} = alloca double\n  store double 0x4040400000000000, double* {f_reg}\n  {o_reg} = alloca i8*\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {p_reg}\n  %t_{} = load double, double* {f_reg}\n  %t_{} = call i8* @__omni_float_to_str(double %t_{})\n  %t_{} = call i8* @__omni_str_concat(i8* %t_{ssa}, i8* %t_{})\n  store i8* %t_{}, i8** {o_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2, ssa + 3
                );

                GeneratedScenario {
                    domain: "string_float_concat",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: pfx.to_string(), llvm_name: p_reg, type_hint: "i8* (string)".to_string() },
                            SsaVariable { name: f_var.to_string(), llvm_name: f_reg, type_hint: "double".to_string() },
                            SsaVariable { name: out_s.to_string(), llvm_name: o_reg, type_hint: "i8* (string)".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("StringFloatFormat".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("i8*".to_string()),
                        action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                        ecosystem_context: Some("python".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 16: Stack-Unwinding Exception Throw (`@__omni_throw` / `@__omni_rethrow`)
            16 => {
                let err_var = *rng.pick(STR_NAMES);
                let err_reg = format!("%str_var_{}_{}", err_var, slot_suf);
                let intent = format!("raise ValueError({err_var})");

                let preamble = format!(
                    "  {err_reg} = alloca i8*\n  %e_init = call i8* @__omni_int_to_str(i64 400)\n  store i8* %e_init, i8** {err_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {err_reg}\n  call void @__omni_throw(i64 1, i8* null, i8* %t_{ssa})\n  unreachable\n"
                );

                GeneratedScenario {
                    domain: "exception_unwind_throw",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![SsaVariable {
                            name: err_var.to_string(),
                            llvm_name: err_reg,
                            type_hint: "i8* (string)".to_string(),
                        }],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("ExceptionThrow".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("unreachable".to_string()),
                        action_directive: Some("LOWER_POLYGLOT_SYNTAX".to_string()),
                        ecosystem_context: Some("python".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 17: Go `(int64, error)` Tuple Unpacking (`@__omni_slice_get` + `inttoptr`)
            17 => {
                let tup_reg = format!("%tup_ptr_{}", slot_suf);
                let q_reg = format!("%ivar_quot_{}", slot_suf + 1);
                let err_reg = format!("%ptr_err_{}", slot_suf + 2);
                let intent = "quot, err := tup".to_string();

                let preamble = format!(
                    "  {tup_reg} = alloca i8*\n  %ts = call i8* @__omni_slice_create(i64 2)\n  call void @__omni_slice_append(i8* %ts, i64 9000000000)\n  call void @__omni_slice_append(i8* %ts, i64 0)\n  store i8* %ts, i8** {tup_reg}\n  {q_reg} = alloca i64\n  {err_reg} = alloca i8*\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i8*, i8** {tup_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{ssa}, i64 0)\n  store i64 %t_{}, i64* {q_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{ssa}, i64 1)\n  %t_{} = inttoptr i64 %t_{} to i8*\n  store i8* %t_{}, i8** {err_reg}\n",
                    ssa + 1, ssa + 1, ssa + 2, ssa + 3, ssa + 2, ssa + 3
                );
                let broken = format!(
                    "  %t_{ssa} = load i8*, i8** {tup_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{ssa}, i64 0)\n  store i64 %t_{}, i64* {q_reg}\n  %t_{} = call i64 @__omni_slice_get(i8* %t_{ssa}, i64 1)\n  store i64 %t_{}, i8** {err_reg}\n",
                    ssa + 1, ssa + 1, ssa + 2, ssa + 2
                );
                let diag = format!(
                    "LLVM IR syntax error: '%t_{}' defined with type 'i64' but expected 'i8*' when storing to '{err_reg}' (use 'inttoptr i64 to i8*')",
                    ssa + 2
                );

                GeneratedScenario {
                    domain: "go_multi_return_unpack",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![
                            SsaVariable { name: "tup".to_string(), llvm_name: tup_reg, type_hint: "i8* (omni_slice)".to_string() },
                            SsaVariable { name: "quot".to_string(), llvm_name: q_reg, type_hint: "i64".to_string() },
                            SsaVariable { name: "err".to_string(), llvm_name: err_reg, type_hint: "i8* (string)".to_string() },
                        ],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("GoTupleUnpack".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_POLYGLOT_SYNTAX".to_string()),
                        ecosystem_context: Some("go".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: Some(broken),
                    diagnostic_hint: Some(diag),
                }
            }

            // 18: 64-Bit Bitwise Hash Mix (`xor` / `lshr` / `mul`)
            18 => {
                let h_var = *rng.pick(&["hash", "state", "crc", "digest", "lane"]);
                let h_reg = format!("%var_{}_{}", h_var, slot_suf);
                let shift = rng.range(13, 37);
                let prime: u64 = 0x9E37_79B1_85EB_CA87;
                let prime_i64 = prime as i64;
                let intent = format!("{h_var} = ({h_var} ^ ({h_var} >> {shift})) * 0x9E3779B185EBCA87");

                let preamble = format!(
                    "  {h_reg} = alloca i64\n  store i64 88172645463325252, i64* {h_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  %t_{ssa} = load i64, i64* {h_reg}\n  %t_{} = lshr i64 %t_{ssa}, {shift}\n  %t_{} = xor i64 %t_{ssa}, %t_{}\n  %t_{} = mul i64 %t_{}, {prime_i64}\n  store i64 %t_{}, i64* {h_reg}\n",
                    ssa + 1, ssa + 2, ssa + 1, ssa + 3, ssa + 2, ssa + 3
                );

                GeneratedScenario {
                    domain: "bitwise_64_mix",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![SsaVariable {
                            name: h_var.to_string(),
                            llvm_name: h_reg,
                            type_hint: "i64".to_string(),
                        }],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("BitwiseKernel64".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: None,
                        yield_expectation: Some("i64".to_string()),
                        action_directive: Some("SYNTHESIZE_ANONYMOUS_BLOCK".to_string()),
                        ecosystem_context: Some("rust".to_string()),
                        lifetime_context: None,
                        import_chain: vec![],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }

            // 19: Cooperative Fiber Sleep & Print Telemetry (`@__omni_sleep_ms` + `@__omni_print_float`)
            _ => {
                let f_var = *rng.pick(FLOAT_NAMES);
                let f_reg = format!("%f64_var_{}_{}", f_var, slot_suf);
                let ms = rng.range(5, 250);
                let intent = format!("time.sleep_ms({ms}); print({f_var})");

                let preamble = format!(
                    "  {f_reg} = alloca double\n  store double 0x4029000000000000, double* {f_reg}\n"
                );
                let completion = format!(
                    "  ; >>> [Synthesized: {intent}]\n  call void @__omni_sleep_ms(i64 {ms})\n  %t_{ssa} = load double, double* {f_reg}\n  call void @__omni_print_float(double %t_{ssa})\n"
                );

                GeneratedScenario {
                    domain: "fiber_sleep_and_float_print",
                    envelope: SsaContextEnvelope {
                        target_abi,
                        enclosing_fn: fn_name,
                        return_type: ret_ty.to_string(),
                        next_ssa_idx: ssa,
                        active_symbols: vec![SsaVariable {
                            name: f_var.to_string(),
                            llvm_name: f_reg,
                            type_hint: "double".to_string(),
                        }],
                        struct_definitions: vec![],
                        raw_intent: intent.clone(),
                        idiom_hint: Some("FiberTelemetry".to_string()),
                        anchor_id: anchor,
                        physical_source: Some(intent),
                        concurrency_mode: Some("M:N Fiber".to_string()),
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: Some("python (module: time)".to_string()),
                        lifetime_context: None,
                        import_chain: vec!["python (module: time)".to_string()],
                    },
                    harness_preamble: preamble,
                    extra_decls: vec![],
                    completion,
                    broken_completion: None,
                    diagnostic_hint: None,
                }
            }
        }
    }

    fn generate_dialect_classification_records(count: usize, rng: &mut SplitMix64) -> Vec<DatasetRecord> {
        const DIALECT_CASES: &[(&str, &str, &str)] = &[
            ("python", "import rsa\nimport socket\ndef main():\n    pub, priv_k = rsa.newkeys(1024)", "rsa"),
            ("python", "import varint\ndef encode_id(n):\n    return varint.encode(n)", "varint"),
            ("java", "package com.omni;\nimport java.util.Random;\npublic class Service { int port = 8080; }", "java.util.Random"),
            ("java", "import lombok.Data;\n@Data\npublic class UserProfile { String name; }", "lombok.Data"),
            ("rust", "use adler::Adler32;\nfn digest(x: i64) -> i64 { x ^ 0xff }", "adler"),
            ("go", "func compute(s string) uint64 {\n    return xxhash.Sum64String(s)\n}", "github.com/cespare/xxhash"),
            ("javascript", "const crc = CRC32.str('payload');\nconsole.log(crc);", "crc-32"),
            ("c", "int bound = compressBound(1024);\nprintf(\"%d\\n\", bound);", "zlib.h"),
            ("csharp", "var x = RandomNumberGenerator.GetInt32(0, 100);\nvar evens = items.Where(n => n % 2 == 0);", "System.Security.Cryptography"),
            ("lua", "local msg = tostring(42)\nif msg ~= '0' then print(msg) end", "luafilesystem"),
            ("zig", "const buf = arena.alloc(4096, 16);", "std.mem"),
        ];

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let (dialect, ctx, imp) = DIALECT_CASES[(i + rng.range(0, 3)) % DIALECT_CASES.len()];
            let nonce = rng.range(10, 9999);
            let prompt = format!(
                "Context:\n{ctx}\n// build_seq={nonce}\n\nImport:\n{imp}\n\nDialect:"
            );
            let completion = dialect.to_string();
            let token_estimate = (prompt.len() + completion.len()) / 4;
            out.push(DatasetRecord {
                task_id: format!("dialect_classify_{:05}", i + 1),
                domain: "polyglot_dialect_classifier".to_string(),
                action_directive: "CLASSIFY_IMPORT_DIALECT".to_string(),
                idiom_hint: dialect.to_string(),
                prompt,
                completion,
                rejected_completion: None,
                system_prompt: Some(DIALECT_CLASSIFIER_SYSTEM_PROMPT.to_string()),
                verified: true,
                token_estimate,
            });
        }
        out
    }

    /// Writes verified records to a JSONL file
    pub fn write_jsonl(records: &[DatasetRecord], path: &Path) -> anyhow::Result<usize> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let mut written = 0;
        for r in records {
            let line = serde_json::to_string(r)?;
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            written += 1;
        }

        writer.flush()?;
        Ok(written)
    }

    /// Exports a ChatML calibration corpus (`imatrix_calib.txt`) for `llama-imatrix` activation profiling
    /// prior to `Q4_K_M` / `Q5_K_M` GGUF quantization.
    pub fn write_imatrix_calibration(
        records: &[DatasetRecord],
        path: &Path,
        max_samples: usize,
    ) -> anyhow::Result<usize> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let step = (records.len() / max_samples.max(1)).max(1);
        let mut count = 0;

        for r in records.iter().step_by(step).take(max_samples) {
            let sys = r.system_prompt.as_deref().unwrap_or(DEFAULT_IR_SYSTEM_PROMPT);
            write!(
                writer,
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n{}\n<|im_end|>\n\n",
                sys,
                r.prompt.trim(),
                r.completion.trim()
            )?;
            count += 1;
        }
        writer.flush()?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_dataset_generation_and_verification() {
        let generator = SyntheticDatasetGenerator::new().expect("Verifier init failed");
        let records = generator.generate_verified_dataset(24);

        assert_eq!(records.len(), 24);
        let mut repair_seen = 0;
        for r in &records {
            assert!(r.verified);
            assert!(r.prompt.contains("<omni_ir_task>"));
            assert!(r.completion.contains("; >>> [Synthesized:"));
            if r.prompt.contains("<compiler_diagnostic>") {
                repair_seen += 1;
            }
        }
        assert!(repair_seen >= 4, "Expected Clang diagnostic self-repair samples in corpus");

        let out_path = std::env::temp_dir().join(format!("test_synthetic_dataset_{}.jsonl", std::process::id()));
        let written = SyntheticDatasetGenerator::write_jsonl(&records, &out_path)
            .expect("Failed to write jsonl");
        assert_eq!(written, 24);
        assert!(out_path.exists());
        let _ = std::fs::remove_file(&out_path);
    }
}
