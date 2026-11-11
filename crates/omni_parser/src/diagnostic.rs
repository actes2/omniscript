use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
    Help,
}

impl DiagnosticSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Help => "help",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSpan {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub length: usize,
    pub source_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmniDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub span: Option<DiagnosticSpan>,
    pub hint: Option<String>,
}

impl OmniDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            message: message.into(),
            span: None,
            hint: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code: code.into(),
            message: message.into(),
            span: None,
            hint: None,
        }
    }

    pub fn with_span(mut self, span: DiagnosticSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let sev_str = self.severity.as_str();
        out.push_str(&format!("{}[{}]: {}\n", sev_str, self.code, self.message));

        if let Some(ref span) = self.span {
            let line_num_str = span.line.to_string();
            let pad = " ".repeat(line_num_str.len());

            out.push_str(&format!("  --> {}:{}:{}\n", span.file, span.line, span.col));
            out.push_str(&format!("   {} |\n", pad));
            out.push_str(&format!("{} | {}\n", line_num_str, span.source_line));

            let col_pad = " ".repeat(span.col.saturating_sub(1));
            let caret_len = if span.length > 0 { span.length } else { 1 };
            let carets = "^".repeat(caret_len);
            out.push_str(&format!("   {} | {}{}\n", pad, col_pad, carets));
        }

        if let Some(ref hint) = self.hint {
            out.push_str(&format!("  = hint: {}\n", hint));
        }

        out
    }
}

impl fmt::Display for OmniDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}
