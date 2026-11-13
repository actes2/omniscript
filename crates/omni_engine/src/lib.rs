use candle_core::quantized::gguf_file;
use candle_core::quantized::tokenizer::TokenizerFromGguf;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen2::ModelWeights as Qwen2Weights;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;
use tokenizers::Tokenizer;

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
    #[serde(default)]
    pub lifetime_context: Option<String>,
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
        if let Some(ref lc) = self.lifetime_context {
            out.push_str(&format!("  <lifetime_context>{}</lifetime_context>\n", lc));
        }
        let mut clean_imports = Vec::new();
        let mut drilled_contexts = Vec::new();
        let mut short_circuit_hints = Vec::new();
        for imp in &self.import_chain {
            if let Some(sc) = imp.strip_prefix("__OMNI_SHORT_CIRCUIT_IR__:") {
                let sc_t = sc.trim();
                if !short_circuit_hints.contains(&sc_t) {
                    short_circuit_hints.push(sc_t);
                }
            } else if let Some(dm) = imp.strip_prefix("__OMNI_DRILLED_MECH__:") {
                let dm_t = dm.trim();
                if !drilled_contexts.contains(&dm_t) {
                    drilled_contexts.push(dm_t);
                }
            } else if !imp.starts_with("__OMNI_DEP_WALK__:")
                && !imp.starts_with("__OMNI_CARVED_FN__:")
            {
                let imp_s = imp.as_str();
                if !clean_imports.contains(&imp_s) {
                    clean_imports.push(imp_s);
                }
            }
        }

        if !clean_imports.is_empty() {
            out.push_str("  <import_chain>\n");
            for imp in clean_imports.into_iter().take(16) {
                out.push_str(&format!("    <import>{}</import>\n", imp));
            }
            out.push_str("  </import_chain>\n");
        }
        if !drilled_contexts.is_empty() {
            out.push_str("  <drilled_context>\n");
            for dc in drilled_contexts.into_iter().take(12) {
                out.push_str(&format!("    <spine>{}</spine>\n", dc));
            }
            out.push_str("  </drilled_context>\n");
        }
        // Only surface pre-assembled failover hints when retrying after a diagnostic failure
        if feedback.is_some() && !short_circuit_hints.is_empty() {
            out.push_str("  <c_short_circuit>\n");
            for sc in short_circuit_hints {
                for l in sc.lines() {
                    out.push_str(&format!("    {}\n", l.trim()));
                }
            }
            out.push_str("  </c_short_circuit>\n");
        }
        out.push_str("  <platform_abi>\n");
        out.push_str("    Full LLVM IR instruction set is active. You may emit standard 'declare' prototypes for external platform C ABI calls if not present in runtime declarations.\n");
        out.push_str("    Core runtime intrinsics: @__omni_slice_create(i64), @__omni_slice_append(i8*, i64), @__omni_slice_get(i8*, i64), @__omni_slice_len(i8*), @__omni_slice_print(i8*), @__omni_random_int(), @__omni_random_range(i64, i64), @__omni_str_concat(i8*, i8*), @__omni_int_to_str(i64).\n");
        out.push_str("  </platform_abi>\n");

        let (clean_intent, embedded_diag) =
            if let Some(idx) = self.raw_intent.find("\n[Compiler Diagnostic Feedback:") {
                let intent_part = &self.raw_intent[..idx];
                let diag_part = self.raw_intent[idx + 1..]
                    .trim_start_matches("[Compiler Diagnostic Feedback:")
                    .trim_end_matches(']');
                (intent_part.trim(), Some(diag_part.trim()))
            } else {
                (self.raw_intent.trim(), None)
            };

        out.push_str(&format!("  <intent>{}</intent>\n", clean_intent));
        out.push_str(&format!("  <next_reg>%t_{}</next_reg>\n", self.next_ssa_idx));

        if let Some(diag) = feedback.or(embedded_diag) {
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

    /// Autonomously validates and classifies the target language ecosystem for an import statement
    fn infer_dialect(
        &self,
        _source_context: &str,
        _candidate_import: &str,
    ) -> Result<String, EngineError> {
        Err(EngineError::InferenceFailed("Dialect inference not supported by this engine".to_string()))
    }
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

    fn infer_dialect(
        &self,
        source_context: &str,
        candidate_import: &str,
    ) -> Result<String, EngineError> {
        let lower_imp = candidate_import.to_lowercase();
        let lower_ctx = source_context.to_lowercase();
        if lower_imp.contains("lombok") || lower_imp.contains("java.") || lower_imp.contains("javax.") || lower_ctx.contains("public class") || lower_ctx.contains("package ") {
            return Ok("java".to_string());
        }
        if lower_imp.contains("requests") || lower_imp.contains("numpy") || lower_imp.contains("torch") || lower_ctx.contains("def ") {
            return Ok("python".to_string());
        }
        if lower_imp.contains("express") || lower_imp.contains("react") || lower_ctx.contains("console.log") {
            return Ok("javascript".to_string());
        }
        if lower_imp.contains("serde") || lower_imp.contains("tokio") || lower_ctx.contains("fn ") {
            return Ok("rust".to_string());
        }
        if lower_imp.contains("gin") || lower_ctx.contains("func ") {
            return Ok("go".to_string());
        }
        Err(EngineError::InferenceFailed("Dialect not determined".to_string()))
    }
}

/// Canonical OmniScript runtime symbols defined in `omni_runtime`.
const CANONICAL_OMNI_SYMBOLS: &[&str] = &[
    "__omni_print_str",
    "__omni_print_str_inline",
    "__omni_print_str_raw",
    "__omni_print_int",
    "__omni_print_float",
    "__omni_input",
    "exit",
    "strcmp",
    "_setjmp",
    "__omni_str_concat",
    "__omni_int_to_str",
    "__omni_float_to_str",
    "__omni_c_str_to_f64",
    "__omni_c_str_to_i64",
    "__omni_try_push",
    "__omni_try_pop",
    "__omni_throw",
    "__omni_rethrow",
    "__omni_catch_msg",
    "__omni_catch_type",
    "__omni_catch_code",
    "__omni_catch_matches",
    "__omni_sleep_s",
    "__omni_sleep_ms",
    "__omni_random_int",
    "__omni_random_range",
    "__omni_ipow64",
    "__omni_arena_create",
    "__omni_arena_alloc",
    "__omni_arena_mark",
    "__omni_arena_rewind",
    "__omni_arena_reset",
    "__omni_arena_destroy",
    "__omni_chan_make",
    "__omni_chan_send",
    "__omni_chan_recv",
    "__omni_chan_send_int",
    "__omni_chan_recv_int",
    "__omni_chan_len",
    "__omni_chan_close",
    "__omni_chan_destroy",
    "__omni_fiber_init",
    "__omni_fiber_create",
    "__omni_fiber_yield",
    "__omni_fiber_destroy",
    "__omni_go_spawn",
    "__omni_slice_create",
    "__omni_slice_append",
    "__omni_slice_get",
    "__omni_slice_set",
    "__omni_slice_len",
    "__omni_slice_pop",
    "__omni_slice_print",
    "__omni_slice_destroy",
    "__omni_map_create",
    "__omni_map_set",
    "__omni_map_get",
    "__omni_map_contains",
    "__omni_map_delete",
    "__omni_map_len",
    "__omni_map_destroy",
    "__omni_map_set_int",
    "__omni_map_get_int",
    "__omni_map_contains_int",
    "__omni_c_http_get",
    "__omni_c_json_parse",
    "__omni_c_json_get",
    "__omni_obj_get",
    "__omni_c_to_float",
    "__omni_c_to_int",
    "__omni_c_return_str",
    "__omni_c_get_returned_str",
    "__omni_c_return_ptr",
    "__omni_c_get_returned_ptr",
    "__omni_str_token",
    "__omni_str_token_arena",
    "__omni_os_socket_listen",
    "__omni_os_socket_accept",
    "__omni_os_socket_read",
    "__omni_os_socket_read_arena",
    "__omni_os_socket_write_http",
    "__omni_os_socket_close",
    "__omni_os_socket_create",
    "__omni_os_socket_bind",
    "__omni_os_socket_listen_fd",
    "__omni_os_socket_accept_tuple",
    "__omni_os_socket_connect",
    "__omni_os_socket_getpeername",
    "__omni_os_socket_send",
    "__omni_os_socket_recv",
    "__omni_os_pipe_pair",
];

// Ordered stem-to-intrinsic map used to reconcile non-canonical `__omni_*` calls synthesized by the LLM.
// Specific multi-token stems (e.g. `sleep_ms`, `print_int`, `map_set_int`) precede broad stems (`sleep`, `print`).
const FUZZY_STEM_MAP: &[(&[&str], &str)] = &[
    (&["msleep", "usleep", "sleep_ms"], "__omni_sleep_ms"),
    (&["sleep", "delay", "wait_s"], "__omni_sleep_s"),
    (&["print_float", "print_f64", "print_double"], "__omni_print_float"),
    (&["print_int"], "__omni_print_int"),
    (&["float_to_str", "f64_to_str", "ftoa"], "__omni_float_to_str"),
    (&["int_to_str", "i64_to_str", "itoa"], "__omni_int_to_str"),
    (&["str_concat", "concat"], "__omni_str_concat"),
    (&["try_push"], "__omni_try_push"),
    (&["try_pop"], "__omni_try_pop"),
    (&["rethrow"], "__omni_rethrow"),
    (&["throw", "raise"], "__omni_throw"),
    (&["catch_matches"], "__omni_catch_matches"),
    (&["catch_msg"], "__omni_catch_msg"),
    (&["catch_type"], "__omni_catch_type"),
    (&["catch_code"], "__omni_catch_code"),
    (&["slice_print", "list_print"], "__omni_slice_print"),
    (&["print", "puts", "println", "log"], "__omni_print_str"),
    (&["slice_append", "list_append", "vec_push", "array_push"], "__omni_slice_append"),
    (&["slice_get", "list_get"], "__omni_slice_get"),
    (&["slice_set", "list_set"], "__omni_slice_set"),
    (&["slice_len", "list_len"], "__omni_slice_len"),
    (&["slice_pop", "list_pop"], "__omni_slice_pop"),
    (&["map_set_int"], "__omni_map_set_int"),
    (&["map_get_int"], "__omni_map_get_int"),
    (&["map_set", "dict_set"], "__omni_map_set"),
    (&["map_get", "dict_get"], "__omni_map_get"),
    (&["map_len", "dict_len"], "__omni_map_len"),
    (&["http_get", "requests_get", "request_get", "urlopen"], "__omni_c_http_get"),
    (&["json_parse", "json_loads", "response_json"], "__omni_c_json_parse"),
    (&["json_get", "obj_get"], "__omni_c_json_get"),
    (&["to_float", "parse_float"], "__omni_c_to_float"),
    (&["to_int", "parse_int"], "__omni_c_to_int"),
    (&["random_range", "getint32"], "__omni_random_range"),
    (&["random", "nextint"], "__omni_random_int"),
    (&["getpeername", "remote_addr"], "__omni_os_socket_getpeername"),
    (&["socket_send"], "__omni_os_socket_send"),
    (&["socket_recv"], "__omni_os_socket_recv"),
];

/// Language-agnostic fuzzy stem resolver: maps any model-invented `@__omni_<prefix>_<stem>`
/// symbol to its canonical C runtime intrinsic, and normalizes C-style type slips.
pub fn rectify_omni_call_line(line: &str) -> String {
    let mut s = line.to_string();

    if !s.contains("call ") {
        return s;
    }

    // 1. Locate `@<symbol>(` to isolate the called function and its argument list
    if let Some(at_idx) = s.find('@') {
        let after_at = &s[at_idx + 1..];
        if let Some(lparen_rel) = after_at.find('(') {
            let sym_name = after_at[..lparen_rel].trim().to_string();
            let lparen_idx = at_idx + 1 + lparen_rel;

            // Find the matching closing ')' for this call's argument list
            let mut depth = 0usize;
            let mut match_rparen = None;
            for (i, ch) in s[lparen_idx..].char_indices() {
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        match_rparen = Some(lparen_idx + i);
                        break;
                    }
                }
            }

            // Truncate any hallucinated trailing groups/tokens after the argument list's closing ')'
            if let Some(rparen_idx) = match_rparen {
                let suffix = s[rparen_idx + 1..].trim();
                if !suffix.is_empty() && suffix != "nounwind" && suffix != "alwaysinline" {
                    s.truncate(rparen_idx + 1);
                }
            }

            // 2. Fuzzy stem resolution if symbol starts with `__omni_` and isn't already canonical
            if sym_name.starts_with("__omni_")
                && !sym_name.starts_with("__omni_fn_")
                && !sym_name.starts_with("__omni_method_")
                && !sym_name.starts_with("__omni_trampoline_")
                && !CANONICAL_OMNI_SYMBOLS.contains(&sym_name.as_str())
            {
                let lower = sym_name.to_lowercase();
                let canonical = FUZZY_STEM_MAP
                    .iter()
                    .find(|(stems, _)| stems.iter().any(|stem| lower.contains(stem)))
                    .map(|(_, target)| *target);

                if let Some(target_sym) = canonical {
                    s = s.replacen(&sym_name, target_sym, 1);
                }
            }
        }
    }

    // 3. Normalize common C-type slips inside LLVM call arguments
    s = s.replace("(int ", "(i64 ").replace("(long ", "(i64 ");
    s
}

/// Strips markdown fences, natural language chat, and trims lines to pure LLVM IR
pub fn sanitize_llvm_ir(raw: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;

    let valid_prefixes = [
        "%", ";", "store ", "load ", "call ", "alloca ", "br ", "ret ", "unreachable",
        "icmp ", "fcmp ",
        "add ", "sub ", "mul ", "udiv ", "sdiv ", "urem ", "srem ",
        "fadd ", "fsub ", "fmul ", "fdiv ", "frem ",
        "and ", "or ", "xor ", "shl ", "lshr ", "ashr ",
        "getelementptr ", "sext ", "trunc ", "zext ", "bitcast ",
        "sitofp ", "uitofp ", "fptosi ", "fptoui ", "fpext ", "fptrunc ",
        "ptrtoint ", "inttoptr ", "select ", "phi ",
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
        let mut trimmed_ascii = ascii_line.trim();
        let owned_unwrapped: String;

        // Unwrap executable LLVM IR instructions if the model echoed the `; >>> [Synthesized: ...]` prefix
        if let Some(inner) = trimmed_ascii
            .strip_prefix("; >>> [Synthesized:")
            .or_else(|| trimmed_ascii.strip_prefix(";[Synthesized:"))
            .or_else(|| trimmed_ascii.strip_prefix("; [Synthesized:"))
        {
            let mut candidate = inner.trim();
            if candidate.ends_with(']') && candidate.matches(']').count() > candidate.matches('[').count() {
                candidate = candidate[..candidate.len() - 1].trim();
            }
            let is_valid_store = candidate.starts_with("store ")
                && candidate.contains(',')
                && candidate.contains('*');
            let is_real_ir = (candidate.starts_with('%') && candidate.contains('='))
                || is_valid_store
                || (candidate.starts_with("call ") && candidate.contains('@'))
                || (candidate.starts_with("declare ") && candidate.contains('@'))
                || candidate.starts_with("ret i")
                || candidate.starts_with("ret double")
                || candidate.starts_with("ret void")
                || candidate.starts_with("br ")
                || candidate == "unreachable";
            if is_real_ir {
                owned_unwrapped = candidate.to_string();
                trimmed_ascii = owned_unwrapped.as_str();
            }
        }

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
            let clean_line = rectify_omni_call_line(trimmed_ascii);
            out.push_str("  ");
            out.push_str(&clean_line);
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

        // Pure Rust std::net::TcpStream HTTP/1.1 POST (zero OS curl dependency)
        let stripped = self
            .endpoint
            .strip_prefix("http://")
            .unwrap_or(&self.endpoint);
        let (authority, path) = match stripped.find('/') {
            Some(idx) => (&stripped[..idx], &stripped[idx..]),
            None => (stripped, "/"),
        };
        let addr_str = if authority.contains(':') {
            authority.to_string()
        } else {
            format!("{}:80", authority)
        };
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::{TcpStream, ToSocketAddrs};
        let sock_addr = addr_str
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
            .ok_or_else(|| EngineError::InferenceFailed(format!("Cannot resolve '{}'", addr_str)))?;
        let mut stream = TcpStream::connect_timeout(&sock_addr, std::time::Duration::from_secs(3))
            .map_err(|e| EngineError::InferenceFailed(format!("TCP connect failed: {}", e)))?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(60)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));

        let req_bytes = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            authority,
            payload_str.len(),
            payload_str
        );
        stream
            .write_all(req_bytes.as_bytes())
            .map_err(|e| EngineError::InferenceFailed(format!("HTTP write failed: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .map_err(|e| EngineError::InferenceFailed(format!("HTTP status read failed: {}", e)))?;
        if !status_line.contains(" 200") {
            return Err(EngineError::InferenceFailed(format!(
                "Ollama HTTP error: {}",
                status_line.trim()
            )));
        }
        let mut is_chunked = false;
        loop {
            let mut hdr = String::new();
            let n = reader
                .read_line(&mut hdr)
                .map_err(|e| EngineError::InferenceFailed(format!("HTTP header read failed: {}", e)))?;
            if n == 0 || hdr == "\r\n" || hdr == "\n" {
                break;
            }
            if hdr.to_ascii_lowercase().starts_with("transfer-encoding:")
                && hdr.to_ascii_lowercase().contains("chunked")
            {
                is_chunked = true;
            }
        }
        let mut body_bytes = Vec::new();
        if is_chunked {
            loop {
                let mut sz_line = String::new();
                if reader.read_line(&mut sz_line).unwrap_or(0) == 0 {
                    break;
                }
                let hex = sz_line.trim().split(';').next().unwrap_or("0");
                let sz = usize::from_str_radix(hex, 16).unwrap_or(0);
                if sz == 0 {
                    break;
                }
                let start = body_bytes.len();
                body_bytes.resize(start + sz, 0);
                let _ = reader.read_exact(&mut body_bytes[start..]);
                let mut crlf = String::new();
                let _ = reader.read_line(&mut crlf);
            }
        } else {
            let _ = reader.read_to_end(&mut body_bytes);
        }

        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes)
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
        let mut seen_regs = std::collections::HashSet::new();

        for sym in &envelope.active_symbols {
            if sym.llvm_name.starts_with('%') {
                let reg_name = &sym.llvm_name;
                if !seen_regs.insert(reg_name.clone()) {
                    continue;
                }
                if sym.llvm_name.contains("_alloca") {
                    preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                } else if sym.llvm_name.contains("_ptr")
                    || sym.llvm_name.starts_with("%col_ref_")
                    || sym.llvm_name.starts_with("%str_var_")
                {
                    preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                } else if sym.llvm_name.starts_with("%param_") {
                    if sym.type_hint.starts_with("%struct.") && sym.type_hint.ends_with('*') {
                        preamble.push_str(&format!("  {} = alloca {}\n", reg_name, sym.type_hint));
                    } else if sym.type_hint.contains("i8*") {
                        preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                    } else if sym.type_hint.contains("double") {
                        preamble.push_str(&format!("  {} = alloca double\n", reg_name));
                    } else if sym.type_hint.contains("i64") {
                        preamble.push_str(&format!("  {} = alloca i64\n", reg_name));
                    } else {
                        preamble.push_str(&format!("  {} = alloca i32\n", reg_name));
                    }
                } else if sym.llvm_name.contains("_val") || sym.llvm_name.starts_with("%var_") || sym.llvm_name.starts_with("%f64_var_") {
                    if sym.type_hint.starts_with("%struct.") && sym.type_hint.ends_with('*') {
                        preamble.push_str(&format!("  {} = alloca {}\n", reg_name, sym.type_hint));
                    } else if sym.type_hint.contains("i8*") {
                        preamble.push_str(&format!("  {} = alloca i8*\n", reg_name));
                    } else if sym.type_hint.contains("double") {
                        preamble.push_str(&format!("  {} = alloca double\n", reg_name));
                    } else if sym.type_hint.contains("i64") {
                        preamble.push_str(&format!("  {} = alloca i64\n", reg_name));
                    } else {
                        preamble.push_str(&format!("  {} = alloca i32\n", reg_name));
                    }
                } else {
                    let ty = if sym.type_hint.starts_with("%struct.") && sym.type_hint.ends_with('*') {
                        sym.type_hint.as_str()
                    } else if sym.type_hint.contains("i8*") {
                        "i8*"
                    } else if sym.type_hint.contains("double") {
                        "double"
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

    /// Snaps stripped SSA register names (e.g. `%str_var_url` when `%str_var_url_0` is active)
    /// back to their exact active symbol register names.
    fn rectify_live_registers(ir: &str, active_symbols: &[SsaVariable]) -> String {
        let mut out = ir.to_string();
        for sym in active_symbols {
            if let Some(last_underscore) = sym.llvm_name.rfind('_') {
                let suffix = &sym.llvm_name[last_underscore + 1..];
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                    let bare_prefix = &sym.llvm_name[..last_underscore];
                    // Replace bare_prefix when followed by non-identifier delimiter (newline, space, comma, ')')
                    for delim in ["\n", "\r", " ", ",", ")"] {
                        let target = format!("{}{}", bare_prefix, delim);
                        let replacement = format!("{}{}", sym.llvm_name, delim);
                        out = out.replace(&target, &replacement);
                    }
                    if out.ends_with(bare_prefix) {
                        let new_len = out.len() - bare_prefix.len();
                        out.truncate(new_len);
                        out.push_str(&sym.llvm_name);
                    }
                }
            }
        }
        out
    }
}

impl<'a> InferenceEngine for VerifiedSynthesisEngine<'a> {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError> {
        let mut last_error = String::new();
        let mut last_ir = String::new();
        let (header, preamble) = Self::generate_harness_header(envelope);

        let pre_assembled_sc: Option<String> = envelope
            .import_chain
            .iter()
            .find_map(|imp| imp.strip_prefix("__OMNI_SHORT_CIRCUIT_IR__:").map(|s| s.to_string()));

        for attempt in 0..self.max_retries {
            let feedback = if attempt > 0 { Some(last_error.as_str()) } else { None };

            // Attempt 0 ALWAYS invokes the AI synthesis backend with `<drilled_context>` and `<live_registers>`.
            // Only if Attempt 0 fails Clang verification or misses required frontier dataflow obligations
            // does Attempt 1 engage the Secondary Layer's pre-assembled failover block.
            let raw_ir = if attempt > 0 && pre_assembled_sc.is_some() {
                pre_assembled_sc.clone().unwrap()
            } else {
                let mut trial_env = envelope.clone();
                if let Some(err_msg) = feedback {
                    trial_env.raw_intent = format!(
                        "{}\n[Compiler Diagnostic Feedback: {}]",
                        envelope.raw_intent, err_msg
                    );
                }
                self.backend.lower_intent(&trial_env, temp)?
            };
            let mut sanitized = sanitize_llvm_ir(&raw_ir);
            sanitized = Self::rectify_live_registers(&sanitized, &envelope.active_symbols);

            // Check if Attempt 0 from the AI satisfied the extracted frontier dataflow obligations.
            // If it omitted required frontier calls, indirect casts, or target register stores, record a
            // diagnostic error so Attempt 1 triggers the Secondary Layer failover.
            if attempt == 0 {
                if let Some(ref sc_ir) = pre_assembled_sc {
                    let sc_clean = sanitize_llvm_ir(sc_ir);
                    let required_syms: Vec<&str> = sc_clean
                        .split('@')
                        .skip(1)
                        .filter_map(|part| part.split('(').next().and_then(|s| s.split_whitespace().next()))
                        .collect();
                    let missing_sym = required_syms
                        .iter()
                        .find(|sym| !sanitized.contains(&format!("@{}", sym)))
                        .copied();
                    let needs_store = sc_clean.contains("store ") && !sanitized.contains("store ");
                    let needs_inttoptr = sc_clean.contains("inttoptr ") && !sanitized.contains("inttoptr ");
                    let needs_ptrtoint = sc_clean.contains("ptrtoint ") && !sanitized.contains("ptrtoint ");
                    let needs_gep = sc_clean.contains("getelementptr ") && !sanitized.contains("getelementptr ");

                    if let Some(sym) = missing_sym {
                        last_error = format!(
                            "AI Attempt 0 omitted required drilled frontier symbol @{} for intent '{}'; triggering secondary layer failover",
                            sym, envelope.raw_intent
                        );
                        continue;
                    }
                    if needs_store || needs_inttoptr || needs_ptrtoint || needs_gep {
                        last_error = format!(
                            "AI Attempt 0 omitted required SSA dataflow store/cast for intent '{}'; triggering secondary layer failover",
                            envelope.raw_intent
                        );
                        continue;
                    }
                }
            }
            if sanitized.contains("(int)") || sanitized.contains("@__omni_sleep_s(") {
                if let Some(lp) = envelope.raw_intent.find('(') {
                    if let Some(rp) = envelope.raw_intent[lp + 1..].find(')') {
                        let arg_lit = envelope.raw_intent[lp + 1..lp + 1 + rp].trim();
                        if arg_lit.chars().all(|c| c.is_ascii_digit()) && !arg_lit.is_empty() {
                            sanitized = sanitized.replace("(int)", &format!("(i64 {})", arg_lit));
                            if let Ok(sec_val) = arg_lit.parse::<i64>() {
                                let ms_scaled = format!("@__omni_sleep_s(i64 {})", sec_val * 1000);
                                if sec_val > 0 && sanitized.contains(&ms_scaled) {
                                    sanitized = sanitized.replace(
                                        &ms_scaled,
                                        &format!("@__omni_sleep_ms(i64 {})", sec_val * 1000),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            last_ir = sanitized.clone();
            let active_triple = if envelope.target_abi.trim().is_empty() {
                omni_runtime::host_target_triple()
            } else {
                envelope.target_abi.trim()
            };
            let (dyn_decls, body) = extract_declarations_and_body(&sanitized);

            // Construct verification harness module with dynamic declares at module scope!
            let mut harness = String::new();
            harness.push_str(&format!("target triple = \"{}\"\n\n", active_triple));
            for sd in &envelope.struct_definitions {
                let sd_trim = sd.trim();
                if sd_trim.starts_with("%struct.") {
                    harness.push_str(sd_trim);
                    harness.push('\n');
                }
            }
            harness.push_str(omni_runtime::runtime_llvm_declarations_for_triple(active_triple));
            harness.push_str("\n");
            for decl in &dyn_decls {
                harness.push_str(decl);
                harness.push('\n');
            }
            for imp in &envelope.import_chain {
                if let Some(fn_name) = imp.strip_prefix("__OMNI_CARVED_FN__:") {
                    if fn_name != envelope.enclosing_fn {
                        harness.push_str(&format!("declare i64 @__omni_fn_{}(i64, i64, i64, i64, i64, i64)\n", fn_name));
                    }
                } else if let Some(rest) = imp.strip_prefix("__OMNI_C_SYM__:") {
                    if let Some((_, c_proto)) = rest.split_once(':') {
                        harness.push_str(&format!("declare {}\n", c_proto));
                    }
                } else if imp.contains("\"TcpHttpServerLoop\"") {
                    harness.push_str("declare i32 @__omni_drilled_tcp_server_loop(i8*, i32)\n");
                }
            }
            harness.push_str("\n");
            harness.push_str(&header);
            harness.push_str(&preamble);
            harness.push_str(&body);
            harness.push_str("\n  ret i32 0\n}\n");

            // Verify with Clang
            match self.verifier.verify_ir_for_target(&harness, Some(active_triple)) {
                Ok(_) => {
                    return Ok(sanitized);
                }
                Err(err) => {
                    last_error = err.to_string();
                }
            }
        }

        let eco_str = envelope
            .ecosystem_context
            .as_deref()
            .unwrap_or("native/default");
        let dep_walks: Vec<&str> = envelope
            .import_chain
            .iter()
            .filter_map(|imp| imp.strip_prefix("__OMNI_DEP_WALK__:"))
            .collect();
        let imports_str = if !dep_walks.is_empty() {
            dep_walks.join(", ")
        } else if envelope.import_chain.is_empty() {
            "none".to_string()
        } else {
            envelope.import_chain.join(", ")
        };

        let human_reason = if last_error.contains("use of undefined value '@") {
            let undef_sym = last_error
                .split("use of undefined value '")
                .nth(1)
                .and_then(|s| s.split('\'').next())
                .unwrap_or("unknown");
            format!(
                "The AI synthesis engine attempted to invoke '{}' to fulfill '{}', but no linked implementation, C header, or runtime intrinsic exists for that capability (active imports: [{}]).",
                undef_sym, envelope.raw_intent, imports_str
            )
        } else {
            format!(
                "The AI synthesis engine could not produce valid SSA LLVM IR for '{}' after {} self-repair attempts.",
                envelope.raw_intent, self.max_retries
            )
        };

        let stumped_report = format!(
            "\n======================================================================\n\
             [OMNIC COMPILER STUMPED] Unable to lower polyglot intent\n\
             ----------------------------------------------------------------------\n\
               Function  : {}\n\
               Ecosystem : {}\n\
               Dep Walk  : {}\n\
               Statement : {}\n\
               Last IR   :\n{}\n\
               Why       : {}\n\
               Clang Err : {}\n\
               Action    : Provide a concrete C/Omni implementation for the native\n\
                           frontier symbols above, or use supported runtime primitives.\n\
             ======================================================================",
            envelope.enclosing_fn,
            eco_str,
            imports_str,
            envelope.raw_intent,
            last_ir.trim_end(),
            human_reason,
            last_error.lines().next().unwrap_or(&last_error)
        );

        Err(EngineError::InferenceFailed(stumped_report))
    }

    fn infer_dialect(
        &self,
        source_context: &str,
        candidate_import: &str,
    ) -> Result<String, EngineError> {
        self.backend.infer_dialect(source_context, candidate_import)
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
                eprintln!("  [Inference Diagnostic] Primary backend failed: {}", err);
                tracing::warn!("Primary inference backend failed ({}); falling back to offline engine", err);
                self.fallback.lower_intent(envelope, temp)
            }
        }
    }

    fn infer_dialect(
        &self,
        source_context: &str,
        candidate_import: &str,
    ) -> Result<String, EngineError> {
        self.primary
            .infer_dialect(source_context, candidate_import)
            .or_else(|_| self.fallback.infer_dialect(source_context, candidate_import))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DevicePreference {
    #[default]
    Auto,
    Cpu,
    Cuda(usize),
    Metal(usize),
}

/// In-process Candle-based GGUF inference engine executing directly without background daemons.
pub struct CandleGgufEngine {
    model: Mutex<Qwen2Weights>,
    tokenizer: Tokenizer,
    pub device: Device,
    pub model_path: PathBuf,
    pub system_prompt: String,
    pub sample_len: usize,
    pub eos_token_id: u32,
}

#[cfg(all(feature = "full-build", not(all(target_os = "windows", target_env = "msvc"))))]
extern "C" {
    static omni_embedded_model_start: u8;
    static omni_embedded_model_end: u8;
}

/// Returns a reference to the embedded model weights linked directly into the binary's read-only data section.
#[cfg(all(feature = "full-build", not(all(target_os = "windows", target_env = "msvc"))))]
pub fn embedded_model_weights() -> &'static [u8] {
    unsafe {
        let start = std::ptr::addr_of!(omni_embedded_model_start);
        let end = std::ptr::addr_of!(omni_embedded_model_end);
        let len = (end as usize).saturating_sub(start as usize);
        std::slice::from_raw_parts(start, len)
    }
}

/// MSVC fallback for embedded model weights (where cl.exe/ml64.exe lacks GNU `.incbin`).
#[cfg(all(feature = "full-build", target_os = "windows", target_env = "msvc"))]
pub fn embedded_model_weights() -> &'static [u8] {
    include_bytes!(env!("OMNI_EMBEDDED_MODEL_ABS_PATH"))
}

#[cfg(feature = "full-build")]
pub static EMBEDDED_MODEL_WEIGHTS: std::sync::LazyLock<&'static [u8]> =
    std::sync::LazyLock::new(embedded_model_weights);

impl CandleGgufEngine {
    pub fn new_from_reader<R: std::io::Read + std::io::Seek>(
        mut reader: R,
        pref: DevicePreference,
        label: &str,
    ) -> Result<Self, EngineError> {
        let device = match pref {
            DevicePreference::Cpu => Device::Cpu,
            DevicePreference::Cuda(ord) => {
                if !candle_core::utils::cuda_is_available() {
                    return Err(EngineError::InferenceFailed(
                        format!("CUDA acceleration requested (device {}), but CUDA is not available or drivers/toolkit are missing", ord)
                    ));
                }
                Device::new_cuda(ord).map_err(|e| {
                    EngineError::InferenceFailed(format!("Failed to initialize CUDA device {}: {}", ord, e))
                })?
            }
            DevicePreference::Metal(ord) => {
                if !candle_core::utils::metal_is_available() {
                    return Err(EngineError::InferenceFailed(
                        format!("Apple Metal acceleration requested (device {}), but Metal is not available (build with '--features metal' on macOS)", ord)
                    ));
                }
                Device::new_metal(ord).map_err(|e| {
                    EngineError::InferenceFailed(format!("Failed to initialize Metal device {}: {}", ord, e))
                })?
            }
            DevicePreference::Auto => {
                if candle_core::utils::cuda_is_available() {
                    Device::new_cuda(0).unwrap_or(Device::Cpu)
                } else if candle_core::utils::metal_is_available() {
                    Device::new_metal(0).unwrap_or(Device::Cpu)
                } else {
                    Device::Cpu
                }
            }
        };

        let content = gguf_file::Content::read(&mut reader)
            .map_err(|e| EngineError::InferenceFailed(format!("Failed reading GGUF content: {}", e)))?;

        let mut tokenizer = Tokenizer::from_gguf(&content)
            .map_err(|e| EngineError::InferenceFailed(format!("Failed extracting tokenizer from GGUF: {}", e)))?;

        // Ensure ChatML special tokens encode to their single control token IDs (151644 / 151645 / 151643)
        // matching HuggingFace `AutoTokenizer` during training so `<|im_end|>` stops generation immediately.
        let special_candidates = ["<|im_start|>", "<|im_end|>", "<|endoftext|>"];
        let added: Vec<tokenizers::AddedToken> = special_candidates
            .iter()
            .filter(|tok| tokenizer.token_to_id(tok).is_some())
            .map(|tok| tokenizers::AddedToken::from(*tok, true))
            .collect();
        if !added.is_empty() {
            let _ = tokenizer.add_special_tokens(added);
        }

        let eos_token_id = content
            .metadata
            .get("tokenizer.ggml.eos_token_id")
            .and_then(|v| match v {
                gguf_file::Value::U32(id) => Some(*id),
                gguf_file::Value::I32(id) => Some(*id as u32),
                _ => None,
            })
            .unwrap_or(151645);

        let model = Qwen2Weights::from_gguf(content, &mut reader, &device)
            .map_err(|e| EngineError::InferenceFailed(format!("Failed initializing Qwen2 model from GGUF on {:?}: {}", device, e)))?;

        Ok(Self {
            model: Mutex::new(model),
            tokenizer,
            device,
            model_path: PathBuf::from(label),
            system_prompt: dataset_generator::DEFAULT_IR_SYSTEM_PROMPT.to_string(),
            sample_len: 128,
            eos_token_id,
        })
    }

    pub fn new_with_device(model_path: PathBuf, pref: DevicePreference) -> Result<Self, EngineError> {
        if !model_path.exists() {
            return Err(EngineError::ModelNotFound(model_path));
        }

        let file = std::fs::File::open(&model_path)
            .map_err(|e| EngineError::InferenceFailed(format!("Failed to open model file: {}", e)))?;
        let label = model_path.to_string_lossy().to_string();
        let mut engine = Self::new_from_reader(file, pref, &label)?;
        engine.model_path = model_path;
        Ok(engine)
    }

    #[cfg(feature = "full-build")]
    pub fn new_embedded_with_device(pref: DevicePreference) -> Result<Self, EngineError> {
        let cursor = std::io::Cursor::new(embedded_model_weights());
        Self::new_from_reader(cursor, pref, "[embedded:omni-coder.gguf]")
    }

    #[cfg(feature = "full-build")]
    pub fn new_embedded() -> Result<Self, EngineError> {
        Self::new_embedded_with_device(DevicePreference::Auto)
    }

    pub fn new(model_path: PathBuf) -> Result<Self, EngineError> {
        Self::new_with_device(model_path, DevicePreference::Auto)
    }

    /// Automatically discover standard model locations on the host system
    pub fn discover_default_with_device(pref: DevicePreference) -> Option<Self> {
        #[cfg(feature = "full-build")]
        {
            if let Ok(engine) = Self::new_embedded_with_device(pref) {
                return Some(engine);
            }
        }

        if let Ok(env_path) = std::env::var("OMNI_MODEL_PATH") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                if let Ok(engine) = Self::new_with_device(p, pref) {
                    return Some(engine);
                }
            }
        }


        let mut candidate_paths = vec![
            PathBuf::from("models/omni-coder.gguf"),
            PathBuf::from("../models/omni-coder.gguf"),
            PathBuf::from("../../models/omni-coder.gguf"),
            PathBuf::from("tests/models/omni-coder.gguf"),
        ];

        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home_dir = PathBuf::from(home);
            candidate_paths.push(home_dir.join(".omnicache").join("models").join("omni-coder.gguf"));
        }

        if let Ok(exe_path) = std::env::current_exe() {
            let mut cur = exe_path.parent();
            for _ in 0..4 {
                if let Some(dir) = cur {
                    let cand = dir.join("models").join("omni-coder.gguf");
                    if !candidate_paths.contains(&cand) {
                        candidate_paths.push(cand);
                    }
                    cur = dir.parent();
                } else {
                    break;
                }
            }
        }

        candidate_paths.push(PathBuf::from(
            "/var/lib/ollama/.ollama/models/blobs/sha256-a621e8119dfe20fe4bfc788fff29de9f6d5f3ec1b2091735a14a6101e930f7a9",
        ));

        for path in &candidate_paths {
            if path.exists() {
                if let Ok(engine) = Self::new_with_device(path.clone(), pref) {
                    return Some(engine);
                }
            }
        }

        None
    }

    pub fn discover_default() -> Option<Self> {
        Self::discover_default_with_device(DevicePreference::Auto)
    }

    pub fn is_cuda(&self) -> bool {
        matches!(self.device, Device::Cuda(_))
    }

    pub fn is_metal(&self) -> bool {
        matches!(self.device, Device::Metal(_))
    }

    pub fn with_sample_len(mut self, len: usize) -> Self {
        self.sample_len = len;
        self
    }

    pub fn generate_raw_text(&self, prompt: &str, max_tokens: usize, temp: f32) -> Result<String, EngineError> {
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| EngineError::InferenceFailed(format!("Tokenizer encode error: {}", e)))?;
        let tokens = encoding.get_ids();

        let mut model = self
            .model
            .lock()
            .map_err(|e| EngineError::InferenceFailed(format!("Mutex poison: {}", e)))?;

        model.clear_kv_cache();

        let sampling = if temp <= 0.05 {
            Sampling::ArgMax
        } else {
            Sampling::All {
                temperature: temp as f64,
            }
        };

        let mut logits_processor = LogitsProcessor::from_sampling(299792458, sampling);

        let input = Tensor::new(tokens, &self.device)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?
            .unsqueeze(0)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

        let logits = model
            .forward(&input, 0)
            .map_err(|e| EngineError::InferenceFailed(format!("Model forward prefill failed: {}", e)))?;
        let logits = logits
            .squeeze(0)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

        let mut next_token = logits_processor
            .sample(&logits)
            .map_err(|e| EngineError::InferenceFailed(format!("Sample failed: {}", e)))?;

        let mut generated_tokens = Vec::new();

        let is_stop_token = |t: u32| -> bool {
            t == self.eos_token_id || t == 151645 || t == 151643
        };

        if !is_stop_token(next_token) {
            generated_tokens.push(next_token);
        }

        for step in 0..max_tokens {
            if is_stop_token(next_token) {
                break;
            }

            let input = Tensor::new(&[next_token], &self.device)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?
                .unsqueeze(0)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

            let logits = model
                .forward(&input, tokens.len() + step)
                .map_err(|e| EngineError::InferenceFailed(format!("Model forward step failed: {}", e)))?;
            let logits = logits
                .squeeze(0)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

            next_token = logits_processor
                .sample(&logits)
                .map_err(|e| EngineError::InferenceFailed(format!("Sample failed: {}", e)))?;

            if is_stop_token(next_token) {
                break;
            }
            generated_tokens.push(next_token);
        }

        let raw_output = self
            .tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| EngineError::InferenceFailed(format!("Tokenizer decode error: {}", e)))?;

        Ok(raw_output)
    }
}

pub fn cuda_is_available() -> bool {
    candle_core::utils::cuda_is_available()
}

pub fn metal_is_available() -> bool {
    candle_core::utils::metal_is_available()
}

impl InferenceEngine for CandleGgufEngine {
    fn lower_intent(
        &self,
        envelope: &SsaContextEnvelope,
        temp: f32,
    ) -> Result<String, EngineError> {
        let prompt_body = envelope.to_prompt_micro_tags();
        let formatted_prompt = format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            self.system_prompt, prompt_body
        );

        let encoding = self
            .tokenizer
            .encode(formatted_prompt, true)
            .map_err(|e| EngineError::InferenceFailed(format!("Tokenizer encode error: {}", e)))?;
        let tokens = encoding.get_ids();
        let verbose_synth = std::env::var_os("OMNI_VERBOSE").is_some();
        let synth_start = std::time::Instant::now();
        if verbose_synth {
            let preview: String = envelope.raw_intent.lines().next().unwrap_or("").chars().take(96).collect();
            eprintln!("     [Synth] Prefilling {} tokens for intent: '{}'", tokens.len(), preview);
        }

        let mut model = self
            .model
            .lock()
            .map_err(|e| EngineError::InferenceFailed(format!("Mutex poison: {}", e)))?;

        model.clear_kv_cache();

        let sampling = if temp <= 0.05 {
            Sampling::ArgMax
        } else {
            Sampling::All {
                temperature: temp as f64,
            }
        };

        let mut logits_processor = LogitsProcessor::from_sampling(299792458, sampling);

        let input = Tensor::new(tokens, &self.device)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?
            .unsqueeze(0)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

        let logits = model
            .forward(&input, 0)
            .map_err(|e| EngineError::InferenceFailed(format!("Model forward prefill failed: {}", e)))?;
        let logits = logits
            .squeeze(0)
            .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

        let mut next_token = logits_processor
            .sample(&logits)
            .map_err(|e| EngineError::InferenceFailed(format!("Sample failed: {}", e)))?;

        let mut generated_tokens = Vec::new();

        let is_stop_token = |t: u32| -> bool {
            t == self.eos_token_id || t == 151645 || t == 151643
        };

        if !is_stop_token(next_token) {
            generated_tokens.push(next_token);
        }

        for step in 0..self.sample_len {
            if is_stop_token(next_token) {
                break;
            }

            let input = Tensor::new(&[next_token], &self.device)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?
                .unsqueeze(0)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

            let logits = model
                .forward(&input, tokens.len() + step)
                .map_err(|e| EngineError::InferenceFailed(format!("Model forward step failed: {}", e)))?;
            let logits = logits
                .squeeze(0)
                .map_err(|e| EngineError::InferenceFailed(e.to_string()))?;

            next_token = logits_processor
                .sample(&logits)
                .map_err(|e| EngineError::InferenceFailed(format!("Sample failed: {}", e)))?;

            if is_stop_token(next_token) {
                break;
            }
            generated_tokens.push(next_token);

            // Early-stopping on explicit ChatML / task delimiters, blank-line block termination, or basic-block terminator
            if generated_tokens.len() >= 6 && step % 2 == 0 {
                if let Ok(curr_str) = self.tokenizer.decode(&generated_tokens, false) {
                    if curr_str.contains("<|im_end|>")
                        || curr_str.contains("</omni_ir_task>")
                        || curr_str.contains("<|im_start|>")
                    {
                        break;
                    }
                    if let Some((completed_lines, _tail)) = curr_str.rsplit_once('\n') {
                        let clean_probe = sanitize_llvm_ir(completed_lines);
                        let has_llvm_instr = clean_probe.contains("call ")
                            || clean_probe.contains("store ")
                            || clean_probe.contains(" = ")
                            || clean_probe.contains("ret ");
                        let has_completed_terminator = clean_probe.lines().any(|l| {
                            let t = l.trim();
                            t.starts_with("ret ") || t == "unreachable"
                        });
                        let has_satisfied_store = envelope
                            .yield_expectation
                            .as_deref()
                            .and_then(|ye| ye.rsplit(" to ").next())
                            .filter(|dst| dst.starts_with('%'))
                            .map(|dst| {
                                clean_probe.lines().any(|l| {
                                    let t = l.trim();
                                    !t.starts_with(';')
                                        && t.starts_with("store ")
                                        && t.contains(',')
                                        && t.contains('*')
                                        && t.contains(dst)
                                })
                            })
                            .unwrap_or(false);
                        if has_llvm_instr && (curr_str.contains("\n\n") || has_completed_terminator || has_satisfied_store) {
                            break;
                        }
                    }
                }
            }
        }

        let raw_output = self
            .tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| EngineError::InferenceFailed(format!("Tokenizer decode error: {}", e)))?;

        if verbose_synth {
            eprintln!(
                "     [Synth] Generated {} tokens in {:.2}s",
                generated_tokens.len(),
                synth_start.elapsed().as_secs_f64()
            );
        }

        Ok(sanitize_llvm_ir(&raw_output))
    }

    fn infer_dialect(
        &self,
        source_context: &str,
        candidate_import: &str,
    ) -> Result<String, EngineError> {
        let snippet: String = source_context.lines().take(30).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "<|im_start|>system\nYou are OmniScript Polyglot Dialect Classifier. Identify the programming language ecosystem for this import. Output strictly one lowercase word: java, python, rust, go, javascript, c, csharp, lua, or zig.<|im_end|>\n<|im_start|>user\nContext:\n{}\n\nImport:\n{}\n\nDialect:<|im_end|>\n<|im_start|>assistant\n",
            snippet, candidate_import
        );
        let output = self.generate_raw_text(&prompt, 8, 0.0)?;
        let first_word = output
            .lines()
            .next()
            .unwrap_or("")
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_lowercase();
        let valid_dialects = [
            "java", "python", "py", "rust", "rs", "go", "javascript", "js", "node", "c", "cpp", "csharp", "cs", "lua", "zig",
        ];
        if valid_dialects.contains(&first_word.as_str()) {
            Ok(first_word)
        } else {
            Err(EngineError::InferenceFailed(format!("Model produced unverified dialect token: '{}'", first_word)))
        }
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
            target_abi: omni_runtime::host_target_triple().to_string(),
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
            lifetime_context: None,
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
            target_abi: omni_runtime::host_target_triple().to_string(),
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
            lifetime_context: None,
            import_chain: vec![],
        };

        let result = engine.lower_intent(&env, 0.05);
        match result {
            Ok(ir) => {
                println!("Live Synthesized and Clang-Verified IR:\n{}", ir);
                assert!(ir.contains("%items_ptr") || ir.contains("__omni_slice"));
            }
            Err(e) => {
                eprintln!("Live Ollama synthesis skipped/failed: {:?}", e);
            }
        }
    }

    #[test]
    fn test_candle_gguf_engine_live() {
        let Some(candle_engine) = CandleGgufEngine::discover_default() else {
            eprintln!("Skipping test_candle_gguf_engine_live: no GGUF model discovered on host");
            return;
        };

        let env = SsaContextEnvelope {
            target_abi: omni_runtime::host_target_triple().to_string(),
            enclosing_fn: "Foo".to_string(),
            return_type: "i32".to_string(),
            next_ssa_idx: 1,
            active_symbols: vec![],
            struct_definitions: vec![],
            raw_intent: "time.sleep(5)".to_string(),
            idiom_hint: None,
            anchor_id: Some(1),
            physical_source: Some("time.sleep(5)".to_string()),
            concurrency_mode: None,
            yield_expectation: Some("void".to_string()),
            action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
            ecosystem_context: Some("python (module: time)".to_string()),
            lifetime_context: None,
            import_chain: vec![],
        };

        println!("--- PROMPT MICRO TAGS ---:\n{}", env.to_prompt_micro_tags());
        let t0 = std::time::Instant::now();
        let result = candle_engine.with_sample_len(64).lower_intent(&env, 0.0);
        let elapsed = t0.elapsed();
        match result {
            Ok(ir) => {
                println!("Synthesized IR in {:?}:\n{}", elapsed, ir);
            }
            Err(e) => {
                println!("Error in {:?}: {:?}", elapsed, e);
            }
        }
    }

    #[test]
    #[cfg(feature = "full-build")]
    fn test_embedded_candle_gguf_engine() {
        let engine = CandleGgufEngine::new_embedded().expect("Failed to initialize embedded GGUF engine");
        assert_eq!(engine.model_path.to_str().unwrap(), "[embedded:omni-coder.gguf]");
        assert!(embedded_model_weights().len() > 1_000_000_000);
        assert!(EMBEDDED_MODEL_WEIGHTS.len() > 1_000_000_000);
    }
}

