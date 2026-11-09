use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Error, Debug, Clone)]
pub enum VerifyError {
    #[error("Clang compiler tool not found on system")]
    ClangNotFound,

    #[error("LLVM IR syntax error at {file}:{line}:{column}: {message}\nDiagnostics:\n{raw_stderr}")]
    SyntaxError {
        file: String,
        line: usize,
        column: usize,
        message: String,
        raw_stderr: String,
    },

    #[error("Linking error during binary generation: {0}")]
    LinkError(String),

    #[error("IO error: {0}")]
    Io(String),
}

pub struct LlvmVerifier {
    clang_path: PathBuf,
    diag_re: Regex,
}

impl Default for LlvmVerifier {
    fn default() -> Self {
        Self::new().expect("Failed to locate clang toolchain")
    }
}

impl LlvmVerifier {
    pub fn new() -> Result<Self, VerifyError> {
        let candidates = [
            // Linux candidates
            PathBuf::from("/usr/lib/llvm20/bin/clang"),
            PathBuf::from("/usr/lib/llvm21/bin/clang"),
            PathBuf::from("/usr/lib/llvm19/bin/clang"),
            PathBuf::from("/usr/bin/clang"),
            // macOS Homebrew & Xcode candidates
            PathBuf::from("/opt/homebrew/opt/llvm/bin/clang"),
            PathBuf::from("/opt/homebrew/bin/clang"),
            PathBuf::from("/usr/local/opt/llvm/bin/clang"),
            // Windows LLVM installation candidates
            PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\LLVM\bin\clang.exe"),
            PathBuf::from("clang"),
            PathBuf::from("clang.exe"),
        ];

        let mut found_path = None;
        for c in &candidates {
            if c.exists() {
                found_path = Some(c.clone());
                break;
            } else if c.to_str() == Some("clang") || c.to_str() == Some("clang.exe") {
                if let Ok(output) = Command::new(c).arg("--version").output() {
                    if output.status.success() {
                        found_path = Some(c.clone());
                        break;
                    }
                }
            }
        }

        let clang_path = found_path.ok_or(VerifyError::ClangNotFound)?;
        let diag_re = Regex::new(r"^(.*?):(\d+):(\d+):\s+(?:error|fatal error):\s+(.*)$").unwrap();

        Ok(Self {
            clang_path,
            diag_re,
        })
    }

    /// Verify validity of LLVM IR bitcode without generating executable
    pub fn verify_ir(&self, ir: &str) -> Result<(), VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_ll = temp_dir.join(format!("omni_verify_{}_{}.ll", std::process::id(), unique_id));

        let mut f = fs::File::create(&temp_ll).map_err(|e| VerifyError::Io(e.to_string()))?;
        f.write_all(ir.as_bytes())
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let dev_null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let output = Command::new(&self.clang_path)
            .args(["-x", "ir", "-c", "-o", dev_null])
            .arg(&temp_ll)
            .output()
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let _ = fs::remove_file(&temp_ll);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(self.parse_diagnostic(&stderr));
        }

        Ok(())
    }

    /// Compile LLVM IR text into a standalone native ELF binary
    pub fn compile_ir_to_binary(
        &self,
        ir: &str,
        out_path: &Path,
        opt_level: u8,
        extra_c_files: &[PathBuf],
    ) -> Result<(), VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_ll = temp_dir.join(format!("omni_build_{}_{}.ll", std::process::id(), unique_id));

        let mut f = fs::File::create(&temp_ll).map_err(|e| VerifyError::Io(e.to_string()))?;
        f.write_all(ir.as_bytes())
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let mut cmd = Command::new(&self.clang_path);
        cmd.arg("-x").arg("ir");
        cmd.arg(format!("-O{}", opt_level));
        cmd.arg(&temp_ll);

        // Include any foreign C sources or shared objects (.so, .a, .dylib, .dll, .lib)
        for extra in extra_c_files {
            let ext = extra.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "so" || ext == "a" || ext == "dylib" || ext == "lib" || ext == "dll" {
                cmd.arg("-x").arg("none").arg(extra);
            } else {
                cmd.arg("-x").arg("c").arg(extra);
            }
        }

        cmd.arg("-o").arg(out_path);

        let output = cmd.output().map_err(|e| VerifyError::Io(e.to_string()))?;
        let _ = fs::remove_file(&temp_ll);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(self.parse_diagnostic(&stderr));
        }

        Ok(())
    }

    /// Directly execute LLVM IR bitcode in-memory / via ephemeral execution path, streaming stdio
    pub fn run_jit(
        &self,
        ir: &str,
        opt_level: u8,
        extra_c_files: &[PathBuf],
        args: &[String],
    ) -> Result<std::process::ExitStatus, VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_bin = if cfg!(windows) {
            temp_dir.join(format!("omni_jit_{}_{}.exe", std::process::id(), unique_id))
        } else {
            temp_dir.join(format!("omni_jit_{}_{}", std::process::id(), unique_id))
        };

        self.compile_ir_to_binary(ir, &temp_bin, opt_level, extra_c_files)?;

        let mut child = Command::new(&temp_bin)
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let status = child.wait().map_err(|e| VerifyError::Io(e.to_string()))?;
        let _ = fs::remove_file(&temp_bin);

        Ok(status)
    }

    /// Directly execute LLVM IR and capture stdout/stderr strings
    pub fn run_jit_captured(
        &self,
        ir: &str,
        opt_level: u8,
        extra_c_files: &[PathBuf],
        args: &[String],
    ) -> Result<(std::process::ExitStatus, String, String), VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_bin = if cfg!(windows) {
            temp_dir.join(format!("omni_jit_cap_{}_{}.exe", std::process::id(), unique_id))
        } else {
            temp_dir.join(format!("omni_jit_cap_{}_{}", std::process::id(), unique_id))
        };

        self.compile_ir_to_binary(ir, &temp_bin, opt_level, extra_c_files)?;

        let output = Command::new(&temp_bin)
            .args(args)
            .output()
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let _ = fs::remove_file(&temp_bin);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((output.status, stdout, stderr))
    }

    fn parse_diagnostic(&self, stderr: &str) -> VerifyError {
        for line in stderr.lines() {
            if let Some(caps) = self.diag_re.captures(line) {
                let file = caps[1].to_string();
                let line_num = caps[2].parse().unwrap_or(1);
                let col_num = caps[3].parse().unwrap_or(1);
                let msg = caps[4].to_string();
                return VerifyError::SyntaxError {
                    file,
                    line: line_num,
                    column: col_num,
                    message: msg,
                    raw_stderr: stderr.to_string(),
                };
            }
        }

        VerifyError::LinkError(stderr.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_valid_llvm_ir() {
        let verifier = LlvmVerifier::new().unwrap();
        let valid_ir = r#"
declare i32 @printf(i8*, ...)
define i32 @test_fn() {
  ret i32 42
}
"#;
        assert!(verifier.verify_ir(valid_ir).is_ok());
    }

    #[test]
    fn test_diagnose_invalid_llvm_ir() {
        let verifier = LlvmVerifier::new().unwrap();
        let bad_ir = r#"
define i32 @bad_fn() {
  %1 = add i32 1, %unknown_symbol
  ret i32 %1
}
"#;
        let res = verifier.verify_ir(bad_ir);
        assert!(res.is_err());
        if let Err(VerifyError::SyntaxError { line, message, .. }) = res {
            assert_eq!(line, 3);
            assert!(message.contains("use of undefined value"));
        } else {
            panic!("Expected SyntaxError diagnostic");
        }
    }

    #[test]
    fn test_run_jit_execution() {
        let verifier = LlvmVerifier::new().unwrap();
        let ir = r#"
declare i32 @printf(i8*, ...)
@.msg = private unnamed_addr constant [15 x i8] c"JIT Works: %d\0A\00", align 1

define i32 @main() {
  %ptr = getelementptr inbounds [15 x i8], [15 x i8]* @.msg, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %ptr, i32 42)
  ret i32 0
}
"#;
        let (status, stdout, _stderr) = verifier
            .run_jit_captured(ir, 0, &[], &[])
            .expect("JIT execution failed");
        assert!(status.success());
        assert_eq!(stdout.trim(), "JIT Works: 42");
    }
}
