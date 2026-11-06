use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub mod dataset_generator;
pub use dataset_generator::{DatasetRecord, SyntheticDatasetGenerator};

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Model file not found at: {0}")]
    ModelNotFound(PathBuf),

    #[error("Model inference failed: {0}")]
    InferenceFailed(String),

    #[error("Synthesis timeout after {0} attempts")]
    Timeout(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SsaVariable {
    pub name: String,
    pub llvm_name: String,
    pub type_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SsaContextEnvelope {
    pub target_abi: String,
    pub enclosing_fn: String,
    pub return_type: String,
    pub next_ssa_idx: usize,
    pub active_symbols: Vec<SsaVariable>,
    pub struct_definitions: Vec<String>,
    pub raw_intent: String,
    pub idiom_hint: Option<String>,
    pub anchor_id: Option<usize>,
    pub physical_source: Option<String>,
    pub concurrency_mode: Option<String>,
    pub yield_expectation: Option<String>,
    pub action_directive: Option<String>,
}

impl SsaContextEnvelope {
    /// Formats the context into compact XML micro-tags (<300 tokens) for sub-2B models
    pub fn to_prompt_micro_tags(&self) -> String {
        let mut out = String::new();
        out.push_str("<omni_ir_task>\n");
        if let Some(anchor) = self.anchor_id {
            out.push_str(&format!("  <interpretive_anchor id=\"{}\"/>\n", anchor));
        }
        if let Some(ref ad) = self.action_directive {
            out.push_str(&format!("  <action_directive>{}</action_directive>\n", ad));
        }
        if let Some(ref ps) = self.physical_source {
            out.push_str("  <physical_source>\n");
            out.push_str(&format!("    {}\n", ps.trim()));
            out.push_str("  </physical_source>\n");
        }
        out.push_str(&format!("  <target_abi>{}</target_abi>\n", self.target_abi));
        out.push_str(&format!(
            "  <function name=\"{}\" ret=\"{}\"/>\n",
            self.enclosing_fn, self.return_type
        ));
        if let Some(ref cm) = self.concurrency_mode {
            out.push_str(&format!("  <concurrency_mode>{}</concurrency_mode>\n", cm));
        }
        if let Some(ref ye) = self.yield_expectation {
            out.push_str(&format!("  <yield_expectation>{}</yield_expectation>\n", ye));
        }
        out.push_str("  <live_registers>\n");
        for var in &self.active_symbols {
            out.push_str(&format!(
                "    <reg var=\"{}\" llvm=\"{}\" type=\"{}\"/>\n",
                var.name, var.llvm_name, var.type_hint
            ));
        }
        out.push_str("  </live_registers>\n");
        if !self.struct_definitions.is_empty() {
            out.push_str("  <structs>\n");
            for s in &self.struct_definitions {
                out.push_str(&format!("    <type>{}</type>\n", s));
            }
            out.push_str("  </structs>\n");
        }
        if let Some(ref hint) = self.idiom_hint {
            out.push_str(&format!("  <idiom_hint>{}</idiom_hint>\n", hint));
        }
        out.push_str(&format!("  <intent>{}</intent>\n", self.raw_intent.trim()));
        out.push_str(&format!("  <next_reg>%t_{}</next_reg>\n", self.next_ssa_idx));
        out.push_str("</omni_ir_task>\n");
        out
    }
}

pub trait InferenceEngine: Send + Sync {
    /// Lowers an abstract intent block + contextual environment into LLVM IR instructions
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError>;
}

/// Offline, deterministic rule/template synthesis engine.
/// Allows complete testing, validation, and native binary compilation without downloading GGUF weights.
pub struct MockSynthesisEngine;

impl Default for MockSynthesisEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSynthesisEngine {
    pub fn new() -> Self {
        Self
    }
}

impl InferenceEngine for MockSynthesisEngine {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        _temp: f32,
    ) -> Result<String, EngineError> {
        let lower = envelope.raw_intent.to_lowercase();

        // 1. Crypto / Hash block
        if lower.contains("hash") || lower.contains("crypto") {
            return Ok(r#"
  ; [Synthesized from Intent: hash data]
  call void @fast_hash_block(i8* %data, i64 %len, i8* %temp_buf)
"#
            .to_string());
        }

        // 2. Filter / Threshold scan
        if lower.contains("filter") || lower.contains("threshold") {
            let next_reg = format!("%t_{}", envelope.next_ssa_idx);
            return Ok(format!(
                r#"
  ; [Synthesized from Intent: filter elements]
  {} = icmp ugt i32 100, 50
"#,
                next_reg
            ));
        }

        // 3. Go Concurrency / Spawn block
        if lower.starts_with("go ") {
            let call_expr = envelope.raw_intent.trim_start_matches("go ").trim();
            let fn_name = call_expr.split('(').next().unwrap_or("").trim();
            if !fn_name.is_empty() {
                return Ok(format!(
                    r#"
  ; [Synthesized Fiber Spawn via exact static trampoline]
  call void @__omni_go_spawn(void (i8*)* @__omni_trampoline_{}, i8* null)
"#,
                    fn_name
                ));
            }
        }

        // Default generic no-op / return
        Ok(format!(
            "  ; [Synthesized generic intent block: {}]\n",
            envelope.raw_intent.trim()
        ))
    }
}

/// Standby wrapper for local GGUF models (Meta LLM-Compiler / Qwen2.5-Coder)
/// Will be engaged once weights are provided/downloaded.
pub struct LlamaCppEngine {
    pub model_path: PathBuf,
}

impl LlamaCppEngine {
    pub fn new(model_path: PathBuf) -> Result<Self, EngineError> {
        if !model_path.exists() {
            return Err(EngineError::ModelNotFound(model_path));
        }
        Ok(Self { model_path })
    }
}

impl InferenceEngine for LlamaCppEngine {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        _temp: f32,
    ) -> Result<String, EngineError> {
        let _prompt = envelope.to_prompt_micro_tags();
        // Standby scaffolding for local llama.cpp GGUF context evaluation
        Ok(format!(
            "  ; [Standby GGUF synthesis for intent: {}]\n",
            envelope.raw_intent.trim()
        ))
    }
}
