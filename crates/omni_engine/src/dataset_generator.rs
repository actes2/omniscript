use crate::{SsaContextEnvelope, SsaVariable};
use omni_verify::LlvmVerifier;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub task_id: String,
    pub domain: String,
    pub action_directive: String,
    pub idiom_hint: String,
    pub prompt: String,
    pub completion: String,
    pub verified: bool,
    pub token_estimate: usize,
}

pub struct SyntheticDatasetGenerator {
    verifier: LlvmVerifier,
}

impl SyntheticDatasetGenerator {
    pub fn new() -> Result<Self, omni_verify::VerifyError> {
        let verifier = LlvmVerifier::new()?;
        Ok(Self { verifier })
    }

    /// Generates a verified synthetic training dataset across diverse systems & polyglot domains
    pub fn generate_verified_dataset(&self, count: usize) -> Vec<DatasetRecord> {
        let mut records = Vec::new();
        let templates = self.get_scenario_templates();

        for i in 0..count {
            let template_idx = i % templates.len();
            let template = &templates[template_idx];
            let task_id = format!("{}_{:04}", template.domain, i + 1);

            let prompt = template.envelope.to_prompt_micro_tags();
            let completion = template.completion.clone();

            // Construct verification harness module
            let mut harness = String::new();
            harness.push_str("target triple = \"x86_64-pc-linux-gnu\"\n\n");
            harness.push_str(omni_runtime::runtime_llvm_declarations());
            harness.push_str("\n");

            harness.push_str("define i32 @harness() {\nentry:\n");
            harness.push_str(&template.harness_preamble);
            harness.push_str(&completion);
            harness.push_str("  ret i32 0\n}\n");

            // Verify with Clang / LLVM syntax & type checker
            let is_verified = self.verifier.verify_ir(&harness).is_ok();

            if is_verified {
                let token_estimate = (prompt.len() + completion.len()) / 4;
                records.push(DatasetRecord {
                    task_id,
                    domain: template.domain.to_string(),
                    action_directive: template
                        .envelope
                        .action_directive
                        .clone()
                        .unwrap_or_else(|| "LOWER_ANONYMOUS_INTENT".to_string()),
                    idiom_hint: template
                        .envelope
                        .idiom_hint
                        .clone()
                        .unwrap_or_else(|| "Standard".to_string()),
                    prompt,
                    completion,
                    verified: true,
                    token_estimate,
                });
            }
        }

        records
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

    fn get_scenario_templates(&self) -> Vec<ScenarioTemplate> {
        vec![
            // 1. Slice / List Arithmetic Transformation
            ScenarioTemplate {
                domain: "collections_transform",
                envelope: SsaContextEnvelope {
                    target_abi: "x86_64-pc-linux-gnu".to_string(),
                    enclosing_fn: "transform_stream".to_string(),
                    return_type: "i32".to_string(),
                    next_ssa_idx: 10,
                    active_symbols: vec![
                        SsaVariable {
                            name: "items".to_string(),
                            llvm_name: "%items_ptr".to_string(),
                            type_hint: "i8* (omni_slice)".to_string(),
                        },
                        SsaVariable {
                            name: "factor".to_string(),
                            llvm_name: "%factor_val".to_string(),
                            type_hint: "i32".to_string(),
                        },
                    ],
                    struct_definitions: vec![],
                    raw_intent: "items.append(items[0] * factor)".to_string(),
                    idiom_hint: Some("PythonListTransform".to_string()),
                    anchor_id: Some(101),
                    physical_source: Some("items.append(items[0] * factor)".to_string()),
                    concurrency_mode: None,
                    yield_expectation: Some("void".to_string()),
                    action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                },
                harness_preamble: r#"  %items_ptr = alloca i8*
  %init_s = call i8* @__omni_slice_create(i64 4)
  store i8* %init_s, i8** %items_ptr
  call void @__omni_slice_append(i8* %init_s, i64 21)
  %factor_val = alloca i32
  store i32 2, i32* %factor_val
"#
                .to_string(),
                completion: r#"  ; >>> [Synthesized: items.append(items[0] * factor)]
  %t_10 = load i8*, i8** %items_ptr
  %t_11 = call i64 @__omni_slice_get(i8* %t_10, i64 0)
  %t_12 = trunc i64 %t_11 to i32
  %t_13 = load i32, i32* %factor_val
  %t_14 = mul i32 %t_12, %t_13
  %t_15 = sext i32 %t_14 to i64
  call void @__omni_slice_append(i8* %t_10, i64 %t_15)
"#
                .to_string(),
            },
            // 2. Go Channel Fan-out Pipeline
            ScenarioTemplate {
                domain: "channel_pipeline",
                envelope: SsaContextEnvelope {
                    target_abi: "x86_64-pc-linux-gnu".to_string(),
                    enclosing_fn: "producer_loop".to_string(),
                    return_type: "i32".to_string(),
                    next_ssa_idx: 20,
                    active_symbols: vec![
                        SsaVariable {
                            name: "ch".to_string(),
                            llvm_name: "%ch_ptr".to_string(),
                            type_hint: "i8* (omni_chan)".to_string(),
                        },
                        SsaVariable {
                            name: "data_sample".to_string(),
                            llvm_name: "%sample_val".to_string(),
                            type_hint: "i32".to_string(),
                        },
                    ],
                    struct_definitions: vec![],
                    raw_intent: "ch <- data_sample + 100".to_string(),
                    idiom_hint: Some("GoChannelSend".to_string()),
                    anchor_id: Some(102),
                    physical_source: Some("ch <- data_sample + 100".to_string()),
                    concurrency_mode: Some("M:N Fiber".to_string()),
                    yield_expectation: Some("i1".to_string()),
                    action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                },
                harness_preamble: r#"  %ch_ptr = alloca i8*
  %init_c = call i8* @__omni_chan_make(i64 8, i64 8)
  store i8* %init_c, i8** %ch_ptr
  %sample_val = alloca i32
  store i32 50, i32* %sample_val
"#
                .to_string(),
                completion: r#"  ; >>> [Synthesized: ch <- data_sample + 100]
  %t_20 = load i8*, i8** %ch_ptr
  %t_21 = load i32, i32* %sample_val
  %t_22 = add i32 %t_21, 100
  %t_23 = sext i32 %t_22 to i64
  %t_24 = call i1 @__omni_chan_send_int(i8* %t_20, i64 %t_23)
"#
                .to_string(),
            },
            // 3. Fast Bitwise Population Count Kernel
            ScenarioTemplate {
                domain: "bitwise_kernel",
                envelope: SsaContextEnvelope {
                    target_abi: "x86_64-pc-linux-gnu".to_string(),
                    enclosing_fn: "compute_hamming_weight".to_string(),
                    return_type: "i32".to_string(),
                    next_ssa_idx: 30,
                    active_symbols: vec![SsaVariable {
                        name: "mask".to_string(),
                        llvm_name: "%mask_val".to_string(),
                        type_hint: "i32".to_string(),
                    }],
                    struct_definitions: vec![],
                    raw_intent: "popcount_step = mask & (mask - 1)".to_string(),
                    idiom_hint: Some("BitwiseKernel".to_string()),
                    anchor_id: Some(103),
                    physical_source: Some("popcount_step = mask & (mask - 1)".to_string()),
                    concurrency_mode: None,
                    yield_expectation: Some("i32".to_string()),
                    action_directive: Some("SYNTHESIZE_ANONYMOUS_BLOCK".to_string()),
                },
                harness_preamble: r#"  %mask_val = alloca i32
  store i32 255, i32* %mask_val
"#
                .to_string(),
                completion: r#"  ; >>> [Synthesized: mask & (mask - 1)]
  %t_30 = load i32, i32* %mask_val
  %t_31 = sub i32 %t_30, 1
  %t_32 = and i32 %t_30, %t_31
  store i32 %t_32, i32* %mask_val
"#
                .to_string(),
            },
            // 4. Zero-GC Arena Buffer Allocation
            ScenarioTemplate {
                domain: "memory_arena",
                envelope: SsaContextEnvelope {
                    target_abi: "x86_64-pc-linux-gnu".to_string(),
                    enclosing_fn: "process_packet".to_string(),
                    return_type: "i32".to_string(),
                    next_ssa_idx: 40,
                    active_symbols: vec![SsaVariable {
                        name: "arena".to_string(),
                        llvm_name: "%arena_ptr".to_string(),
                        type_hint: "i8* (omni_arena)".to_string(),
                    }],
                    struct_definitions: vec![],
                    raw_intent: "packet_buf = arena.alloc(1024, align=16)".to_string(),
                    idiom_hint: Some("ZigArenaAlloc".to_string()),
                    anchor_id: Some(104),
                    physical_source: Some("packet_buf = arena.alloc(1024, align=16)".to_string()),
                    concurrency_mode: None,
                    yield_expectation: Some("i8*".to_string()),
                    action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                },
                harness_preamble: r#"  %arena_ptr = alloca i8*
  %init_a = call i8* @__omni_arena_create(i64 65536)
  store i8* %init_a, i8** %arena_ptr
"#
                .to_string(),
                completion: r#"  ; >>> [Synthesized: arena.alloc(1024, align=16)]
  %t_40 = load i8*, i8** %arena_ptr
  %t_41 = call i8* @__omni_arena_alloc(i8* %t_40, i64 1024, i64 16)
"#
                .to_string(),
            },
            // 5. Hash Map Lookup & Conditional Default
            ScenarioTemplate {
                domain: "map_lookup",
                envelope: SsaContextEnvelope {
                    target_abi: "x86_64-pc-linux-gnu".to_string(),
                    enclosing_fn: "query_metric".to_string(),
                    return_type: "i32".to_string(),
                    next_ssa_idx: 50,
                    active_symbols: vec![
                        SsaVariable {
                            name: "metrics".to_string(),
                            llvm_name: "%map_ptr".to_string(),
                            type_hint: "i8* (omni_map)".to_string(),
                        },
                        SsaVariable {
                            name: "metric_id".to_string(),
                            llvm_name: "%id_val".to_string(),
                            type_hint: "i64".to_string(),
                        },
                    ],
                    struct_definitions: vec![],
                    raw_intent: "val = metrics.get(metric_id, default=0)".to_string(),
                    idiom_hint: Some("MapLookup".to_string()),
                    anchor_id: Some(105),
                    physical_source: Some("val = metrics.get(metric_id, default=0)".to_string()),
                    concurrency_mode: None,
                    yield_expectation: Some("i64".to_string()),
                    action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                },
                harness_preamble: r#"  %map_ptr = alloca i8*
  %init_m = call i8* @__omni_map_create(i64 16)
  store i8* %init_m, i8** %map_ptr
  %id_val = alloca i64
  store i64 42, i64* %id_val
"#
                .to_string(),
                completion: r#"  ; >>> [Synthesized: metrics.get(metric_id, default=0)]
  %t_50 = load i8*, i8** %map_ptr
  %t_51 = load i64, i64* %id_val
  %t_52 = call i64 @__omni_map_get_int(i8* %t_50, i64 %t_51)
"#
                .to_string(),
            },
        ]
    }
}

struct ScenarioTemplate {
    pub domain: &'static str,
    pub envelope: SsaContextEnvelope,
    pub harness_preamble: String,
    pub completion: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_dataset_generation_and_verification() {
        let generator = SyntheticDatasetGenerator::new().expect("Verifier init failed");
        let records = generator.generate_verified_dataset(10);

        assert_eq!(records.len(), 10);
        for r in &records {
            assert!(r.verified);
            assert!(r.prompt.contains("<omni_ir_task>"));
            assert!(r.completion.contains("; >>> [Synthesized:"));
        }

        let out_path = std::env::temp_dir().join(format!("test_synthetic_dataset_{}.jsonl", std::process::id()));
        let written = SyntheticDatasetGenerator::write_jsonl(&records, &out_path)
            .expect("Failed to write jsonl");
        assert_eq!(written, 10);
        assert!(out_path.exists());
        let _ = std::fs::remove_file(&out_path);
    }
}
