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
    pub ecosystem_context: Option<String>,
    pub import_chain: Vec<String>,
}

impl SsaContextEnvelope {
    /// Formats the context into compact XML micro-tags (<300 tokens) for sub-2B models
    pub fn to_prompt_micro_tags(&self) -> String {
        self.to_prompt_with_feedback(None)
    }

    /// Formats the context with optional feedback from previous Clang compiler diagnostics
    pub fn to_prompt_with_feedback(&self, feedback: Option<&str>) -> String {
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
        if let Some(ref eco) = self.ecosystem_context {
            out.push_str(&format!("  <ecosystem_context>{}</ecosystem_context>\n", eco));
        }
        if !self.import_chain.is_empty() {
            out.push_str("  <import_chain>\n");
            for imp in &self.import_chain {
                out.push_str(&format!("    <import>{}</import>\n", imp));
            }
            out.push_str("  </import_chain>\n");
        }
        out.push_str("  <platform_abi>\n");
        out.push_str("    Full LLVM IR instruction set is active. You may emit standard 'declare' prototypes for external platform C ABI calls if not present in runtime declarations.\n");
        out.push_str("  </platform_abi>\n");
        out.push_str(&format!("  <intent>{}</intent>\n", self.raw_intent.trim()));
        out.push_str(&format!("  <next_reg>%t_{}</next_reg>\n", self.next_ssa_idx));

        if let Some(diag) = feedback {
            out.push_str("  <compiler_diagnostic>\n");
            for l in diag.lines() {
                out.push_str(&format!("    {}\n", l.trim()));
            }
            out.push_str("  </compiler_diagnostic>\n");
        }

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

        // 4. Standard sleep/delay invocation (POSIX fallback)
        if lower.contains("sleep") {
            let num = envelope.raw_intent
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<i32>()
                .unwrap_or(1);
            return Ok(format!("  call i32 @sleep(i32 {})\n", num));
        }

        // Default generic no-op / return
        Ok(format!(
            "  ; [Synthesized generic intent block: {}]\n",
            envelope.raw_intent.trim()
        ))
    }
}

/// Strips markdown fences, natural language chat, and trims lines to pure LLVM IR
pub fn sanitize_llvm_ir(raw: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;

    let valid_prefixes = [
        "%", ";", "store ", "load ", "call ", "alloca ", "br ", "ret ", "icmp ",
        "add ", "sub ", "mul ", "udiv ", "sdiv ", "urem ", "srem ",
        "and ", "or ", "xor ", "shl ", "lshr ", "ashr ",
        "getelementptr ", "sext ", "trunc ", "zext ", "bitcast ", "select ", "phi ",
        "declare ",
    ];

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }

        // Strip non-ascii trailing artifacts or stop residue
        let ascii_line: String = trimmed.chars().take_while(|c| c.is_ascii()).collect();
        let trimmed_ascii = ascii_line.trim();

        // Skip chat / prompt echoes
        if trimmed_ascii.starts_with("<omni_ir_task")
            || trimmed_ascii.starts_with("</omni_ir_task>")
            || trimmed_ascii.starts_with("<|")
            || trimmed_ascii.starts_with("Here is")
            || trimmed_ascii.starts_with("Sure")
            || trimmed_ascii.starts_with("Note:")
        {
            continue;
        }

        if valid_prefixes.iter().any(|prefix| trimmed_ascii.starts_with(prefix)) || (in_fence && !trimmed_ascii.is_empty()) {
            out.push_str("  ");
            out.push_str(trimmed_ascii);
            out.push('\n');
        }
    }

    if out.trim().is_empty() {
        raw.trim().to_string()
    } else {
        out
    }
}

/// Separates top-level `declare ...` statements from inner basic block instructions,
/// deduplicating against internal runtime declarations.
pub fn extract_declarations_and_body(raw_ir: &str) -> (Vec<String>, String) {
    let mut declarations = Vec::new();
    let mut body_lines = Vec::new();
    let runtime_decls = omni_runtime::runtime_llvm_declarations();

    for line in raw_ir.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("declare ") {
            // Deduplicate against internal omni_runtime declarations
            let fn_name = if let Some(open) = trimmed.find('@') {
                if let Some(paren) = trimmed[open..].find('(') {
                    &trimmed[open..open + paren]
                } else {
                    ""
                }
            } else {
                ""
            };

            if !fn_name.is_empty() && runtime_decls.contains(fn_name) {
                // Already provided by omni_runtime, skip
                continue;
            }

            if !declarations.iter().any(|d: &String| d.trim() == trimmed) {
                declarations.push(trimmed.to_string());
            }
        } else {
            body_lines.push(line);
        }
    }

    (declarations, body_lines.join("\n"))
}

/// Live HTTP inference engine connecting directly to a local Ollama server
pub struct OllamaInferenceEngine {
    pub endpoint: String,
    pub model_name: String,
    pub system_prompt: String,
}

impl Default for OllamaInferenceEngine {
    fn default() -> Self {
        Self::new("omni-coder")
    }
}

impl OllamaInferenceEngine {
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/chat".to_string(),
            model_name: model_name.into(),
            system_prompt: "You are OmniScript AI IR Lowering Engine. You synthesize strictly valid LLVM IR basic block instructions for polyglot intent. Output raw LLVM IR instructions only.".to_string(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

impl InferenceEngine for OllamaInferenceEngine {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError> {
        let prompt = envelope.to_prompt_micro_tags();
        let payload = serde_json::json!({
            "model": self.model_name,
            "messages": [
                {
                    "role": "system",
                    "content": self.system_prompt
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false,
            "options": {
                "temperature": temp.max(0.05),
                "num_predict": 256,
                "stop": ["</omni_ir_task>", "```", "<|im_end|>", "<|endoftext|>", "<|im_start|>"]
            }
        });

        let payload_str = serde_json::to_string(&payload)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

        let mut child = std::process::Command::new("curl")
            .args(["-s", "-X", "POST", &self.endpoint, "-H", "Content-Type: application/json", "-d", "@-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| EngineError::InferenceFailed(format!("Failed to execute curl: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = std::io::Write::write_all(&mut stdin, payload_str.as_bytes());
        }

        let output = child
            .wait_with_output()
            .map_err(|e| EngineError::InferenceFailed(format!("Failed reading curl response: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(EngineError::InferenceFailed(format!("Ollama HTTP request failed: {}", err)));
        }

        let resp_json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| EngineError::InferenceFailed(format!("Invalid Ollama JSON response: {}", e)))?;

        let raw_res = resp_json
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .or_else(|| resp_json.get("response").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();

        if raw_res.trim().is_empty() {
            return Err(EngineError::InferenceFailed("Ollama returned empty response".to_string()));
        }

        Ok(sanitize_llvm_ir(&raw_res))
    }
}

/// Self-Repairing synthesis engine verifying every LLVM IR block with Clang
pub struct VerifiedSynthesisEngine<'a> {
    pub backend: &'a dyn InferenceEngine,
    pub verifier: &'a omni_verify::LlvmVerifier,
    pub max_retries: usize,
}

impl<'a> VerifiedSynthesisEngine<'a> {
    pub fn new(backend: &'a dyn InferenceEngine, verifier: &'a omni_verify::LlvmVerifier) -> Self {
        Self {
            backend,
            verifier,
            max_retries: 3,
        }
    }

    /// Auto-generates dummy params and allocas for live registers to enable isolated Clang verification
    fn generate_harness_header(envelope: &SsaContextEnvelope) -> (String, String) {
        let mut params = Vec::new();
        let mut preamble = String::new();

        for sym in &envelope.active_symbols {
            if sym.llvm_name.starts_with('%') {
                let reg_name = &sym.llvm_name;
                if sym.llvm_name.contains("_ptr") || sym.llvm_name.starts_with("%col_ref_") {
                    preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                } else if sym.llvm_name.starts_with("%param_") {
                    if sym.type_hint.contains("i8*") {
                        preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                    } else if sym.type_hint.contains("i64") {
                        preamble.push_str(&format!("  {} = alloca i64\n", reg_name));
                    } else {
                        preamble.push_str(&format!("  {} = alloca i32\n", reg_name));
                    }
                } else if sym.llvm_name.contains("_val") || sym.llvm_name.starts_with("%var_") {
                    if sym.type_hint.contains("i64") {
                        preamble.push_str(&format!("  {} = alloca i64\n", reg_name));
                    } else {
                        preamble.push_str(&format!("  {} = alloca i32\n", reg_name));
                    }
                } else {
                    let ty = if sym.type_hint.contains("i8*") {
                        "i8*"
                    } else if sym.type_hint == "i64" {
                        "i64"
                    } else if sym.type_hint == "i1" {
                        "i1"
                    } else {
                        "i32"
                    };
                    params.push(format!("{} {}", ty, reg_name));
                }
            }
        }

        let header = format!("define i32 @harness({}) {{\nentry:\n", params.join(", "));
        (header, preamble)
    }
}

impl<'a> InferenceEngine for VerifiedSynthesisEngine<'a> {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError> {
        let mut last_error = String::new();
        let (header, preamble) = Self::generate_harness_header(envelope);

        for attempt in 0..self.max_retries {
            let feedback = if attempt > 0 { Some(last_error.as_str()) } else { None };

            // Re-package envelope with updated prompt feedback
            let mut trial_env = envelope.clone();
            if let Some(err_msg) = feedback {
                trial_env.raw_intent = format!("{}\n[Compiler Diagnostic Feedback: {}]", envelope.raw_intent, err_msg);
            }

            let raw_ir = self.backend.lower_intent(&trial_env, temp)?;
            let sanitized = sanitize_llvm_ir(&raw_ir);
            let (dyn_decls, body) = extract_declarations_and_body(&sanitized);

            // Construct verification harness module with dynamic declares at module scope!
            let mut harness = String::new();
            harness.push_str("target triple = \"x86_64-pc-linux-gnu\"\n\n");
            harness.push_str(omni_runtime::runtime_llvm_declarations());
            harness.push_str("\n");
            for decl in &dyn_decls {
                harness.push_str(decl);
                harness.push('\n');
            }
            harness.push_str("\n");
            harness.push_str(&header);
            harness.push_str(&preamble);
            harness.push_str(&body);
            harness.push_str("\n  ret i32 0\n}\n");

            // Verify with Clang
            match self.verifier.verify_ir(&harness) {
                Ok(_) => {
                    return Ok(sanitized);
                }
                Err(err) => {
                    last_error = err.to_string();
                }
            }
        }

        Err(EngineError::InferenceFailed(format!(
            "Failed Clang self-repair loop after {} attempts. Last error:\n{}",
            self.max_retries, last_error
        )))
    }
}

/// Fallback engine that tries the primary engine first, then gracefully degrades to offline mock engine
pub struct FallbackEngine<P: InferenceEngine, F: InferenceEngine> {
    pub primary: P,
    pub fallback: F,
}

impl<P: InferenceEngine, F: InferenceEngine> FallbackEngine<P, F> {
    pub fn new(primary: P, fallback: F) -> Self {
        Self { primary, fallback }
    }
}

impl<P: InferenceEngine, F: InferenceEngine> InferenceEngine for FallbackEngine<P, F> {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError> {
        match self.primary.lower_intent(envelope, temp) {
            Ok(res) => Ok(res),
            Err(err) => {
                tracing::warn!("Primary inference backend failed ({}); falling back to offline engine", err);
                self.fallback.lower_intent(envelope, temp)
            }
        }
    }
}

/// Standby wrapper for local GGUF models (Meta LLM-Compiler / Qwen2.5-Coder)
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
        Ok(format!(
            "  ; [Standby GGUF synthesis for intent: {}]\n",
            envelope.raw_intent.trim()
        ))
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    #[test]
    fn test_sanitize_llvm_ir() {
        let chatty = r#"
Sure! Here is the LLVM IR for the requested intent:
```llvm
  %t_10 = load i8*, i8** %items_ptr
  call void @__omni_slice_append(i8* %t_10, i64 42)
```
Let me know if you need any adjustments!
"#;
        let cleaned = sanitize_llvm_ir(chatty);
        assert!(cleaned.contains("%t_10 = load i8*, i8** %items_ptr"));
        assert!(cleaned.contains("call void @__omni_slice_append"));
        assert!(!cleaned.contains("Sure!"));
        assert!(!cleaned.contains("Let me know"));
        assert!(!cleaned.contains("```"));
    }

    #[test]
    fn test_verified_synthesis_engine_with_clang() {
        let verifier = omni_verify::LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        let mock = MockSynthesisEngine::new();
        let engine = VerifiedSynthesisEngine::new(&mock, &verifier);

        let env = SsaContextEnvelope {
            target_abi: "x86_64-pc-linux-gnu".to_string(),
            enclosing_fn: "test_fn".to_string(),
            return_type: "i32".to_string(),
            next_ssa_idx: 10,
            active_symbols: vec![
                SsaVariable {
                    name: "data".to_string(),
                    llvm_name: "%data".to_string(),
                    type_hint: "i8*".to_string(),
                },
                SsaVariable {
                    name: "len".to_string(),
                    llvm_name: "%len".to_string(),
                    type_hint: "i64".to_string(),
                },
                SsaVariable {
                    name: "temp_buf".to_string(),
                    llvm_name: "%temp_buf".to_string(),
                    type_hint: "i8*".to_string(),
                },
            ],
            struct_definitions: vec![],
            raw_intent: "hash data".to_string(),
            idiom_hint: None,
            anchor_id: Some(1),
            physical_source: Some("hash data".to_string()),
            concurrency_mode: None,
            yield_expectation: Some("void".to_string()),
            action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
            ecosystem_context: None,
            import_chain: vec![],
        };

        let result = engine.lower_intent(&env, 0.0).expect("Verified synthesis failed");
        assert!(result.contains("call void @fast_hash_block"));
    }

    #[test]
    fn test_live_ollama_verified_synthesis() {
        if std::net::TcpStream::connect("127.0.0.1:11434").is_err() {
            eprintln!("Skipping live Ollama test: server not reachable on 11434");
            return;
        }

        let verifier = omni_verify::LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        let ollama = OllamaInferenceEngine::new("omni-coder");
        let engine = VerifiedSynthesisEngine::new(&ollama, &verifier);

        let env = SsaContextEnvelope {
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
            ecosystem_context: None,
            import_chain: vec![],
        };

        let result = engine.lower_intent(&env, 0.05);
        match result {
            Ok(ir) => {
                println!("Live Synthesized and Clang-Verified IR:\n{}", ir);
                assert!(ir.contains("%items_ptr") || ir.contains("__omni_slice"));
            }
            Err(e) => {
                panic!("Live synthesis failed: {:?}", e);
            }
        }
    }
}
