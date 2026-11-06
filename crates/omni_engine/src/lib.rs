use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Model file not found at: {0}")]
    ModelNotFound(PathBuf),

    #[error("Model inference failed: {0}")]
    InferenceFailed(String),

    #[error("Synthesis timeout after {0} attempts")]
    Timeout(usize),
}

pub trait InferenceEngine: Send + Sync {
    /// Lowers an abstract intent block + contextual environment into LLVM IR instructions
    fn lower_intent(
        &self,
        context: &str,
        intent_instruction: &str,
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
        _context: &str,
        intent_instruction: &str,
        _temp: f32,
    ) -> Result<String, EngineError> {
        let lower = intent_instruction.to_lowercase();

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
            return Ok(r#"
  ; [Synthesized from Intent: filter elements]
  %cond = icmp ugt i32 100, 50
"#
            .to_string());
        }

        // Default generic no-op / return
        Ok(format!(
            "  ; [Synthesized generic intent block: {}]\n",
            intent_instruction.trim()
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
        _context: &str,
        intent_instruction: &str,
        _temp: f32,
    ) -> Result<String, EngineError> {
        // Scaffolding for in-process libllama generation
        Ok(format!(
            "  ; [LLaMA generated intent block for: {}]\n",
            intent_instruction
        ))
    }
}
