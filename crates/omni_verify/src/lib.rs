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
    macos_sysroot: Option<PathBuf>,
    diag_re: Regex,
}

impl Default for LlvmVerifier {
    fn default() -> Self {
        Self::new().expect("Failed to locate clang toolchain")
    }
}

impl LlvmVerifier {
    fn detect_macos_sysroot() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            if let Some(sdk) = std::env::var_os("SDKROOT") {
                let p = PathBuf::from(sdk);
                if p.exists() {
                    return Some(p);
                }
            }
            if let Ok(out) = Command::new("xcrun").arg("--show-sdk-path").output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() {
                        let p = PathBuf::from(s);
                        if p.exists() {
                            return Some(p);
                        }
                    }
                }
            }
            let clt_sdk = PathBuf::from("/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk");
            if clt_sdk.exists() {
                return Some(clt_sdk);
            }
        }
        None
    }

    pub fn new() -> Result<Self, VerifyError> {
        let mut found_path = None;

        if let Some(custom) = std::env::var_os("OMNI_CLANG_PATH")
            .or_else(|| std::env::var_os("CLANG_PATH"))
        {
            let p = PathBuf::from(custom);
            if p.exists() {
                found_path = Some(p);
            }
        }

        let candidates = [
            // Linux candidates
            PathBuf::from("/usr/lib/llvm21/bin/clang"),
            PathBuf::from("/usr/lib/llvm20/bin/clang"),
            PathBuf::from("/usr/lib/llvm19/bin/clang"),
            PathBuf::from("/usr/lib/llvm18/bin/clang"),
            // macOS Homebrew, MacPorts & Xcode CLT candidates (prefer full LLVM before Apple shim if present)
            PathBuf::from("/opt/homebrew/opt/llvm/bin/clang"),
            PathBuf::from("/opt/homebrew/bin/clang"),
            PathBuf::from("/usr/local/opt/llvm/bin/clang"),
            PathBuf::from("/opt/local/bin/clang"),
            PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/clang"),
            PathBuf::from("/usr/bin/clang"),
            // Windows LLVM, MSYS2 & Visual Studio ClangCL/LLVM installation candidates
            PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\LLVM\bin\clang.exe"),
            PathBuf::from(r"C:\msys64\ucrt64\bin\clang.exe"),
            PathBuf::from(r"C:\msys64\clang64\bin\clang.exe"),
            PathBuf::from(r"C:\msys64\mingw64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\clang.exe"),
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\ARM64\bin\clang.exe"),
            PathBuf::from("clang"),
            PathBuf::from("clang.exe"),
        ];

        if found_path.is_none() {
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
        }

        let clang_path = found_path.ok_or(VerifyError::ClangNotFound)?;
        let macos_sysroot = Self::detect_macos_sysroot();
        let diag_re = Regex::new(r"^(.*?):(\d+):(\d+):\s+(?:error|fatal error):\s+(.*)$").unwrap();

        Ok(Self {
            clang_path,
            macos_sysroot,
            diag_re,
        })
    }

    /// Verify validity of LLVM IR bitcode without generating executable
    pub fn verify_ir(&self, ir: &str) -> Result<(), VerifyError> {
        self.verify_ir_for_target(ir, None)
    }

    /// Verify validity of LLVM IR bitcode for an explicit target triple (or host default when `None`).
    pub fn verify_ir_for_target(
        &self,
        ir: &str,
        target_triple: Option<&str>,
    ) -> Result<(), VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_ll = temp_dir.join(format!("omni_verify_{}_{}.ll", std::process::id(), unique_id));

        let mut f = fs::File::create(&temp_ll).map_err(|e| VerifyError::Io(e.to_string()))?;
        f.write_all(ir.as_bytes())
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let dev_null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut cmd = Command::new(&self.clang_path);
        if let Some(triple) = target_triple {
            if !triple.trim().is_empty() {
                cmd.arg(format!("--target={}", triple.trim()));
            }
        }
        cmd.arg("-Wno-override-module");
        let output = cmd
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

    fn is_non_system_lib_dir(dir: &Path) -> bool {
        let s = dir.to_string_lossy();
        !matches!(
            s.as_ref(),
            "/usr/lib"
                | "/usr/lib64"
                | "/lib"
                | "/lib64"
                | "/usr/lib/x86_64-linux-gnu"
                | "/usr/lib/aarch64-linux-gnu"
                | "/lib/x86_64-linux-gnu"
        )
    }

    fn find_sibling_shared_lib(static_lib: &Path) -> Option<PathBuf> {
        let fname = static_lib.file_name()?.to_str()?;
        if fname.contains("omni_runtime_native") {
            return None;
        }
        let parent = static_lib.parent()?;
        let stem = static_lib.file_stem()?.to_str()?;
        for cand_ext in &["so", "dylib", "dll.a", "dll"] {
            let cand = parent.join(format!("{}.{}", stem, cand_ext));
            if cand.is_file() {
                return Some(cand);
            }
        }
        // Also inspect versioned SONAME siblings like `liblz4.so.1`
        if let Ok(entries) = fs::read_dir(parent) {
            let so_prefix = format!("{}.so.", stem);
            for entry in entries.filter_map(|e| e.ok()) {
                let n = entry.file_name().to_string_lossy().to_string();
                if n.starts_with(&so_prefix) && entry.path().is_file() {
                    return Some(entry.path());
                }
            }
        }
        None
    }

    fn collect_runtime_search_dirs(extra_c_files: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut lib_dirs = Vec::new();
        let mut bin_dirs = Vec::new();
        for extra in extra_c_files {
            if let Some(parent) = extra.parent() {
                if Self::is_non_system_lib_dir(parent) && !lib_dirs.contains(&parent.to_path_buf()) {
                    lib_dirs.push(parent.to_path_buf());
                }
                if let Some(pkg_root) = parent.parent() {
                    let sibling_bin = pkg_root.join("bin");
                    if sibling_bin.is_dir() && !bin_dirs.contains(&sibling_bin) {
                        bin_dirs.push(sibling_bin);
                    }
                }
            }
        }
        (lib_dirs, bin_dirs)
    }

    fn invoke_clang_link(
        &self,
        temp_ll: &Path,
        out_path: &Path,
        opt_level: u8,
        link_inputs: &[PathBuf],
        target_triple: Option<&str>,
    ) -> Result<std::process::Output, VerifyError> {
        let mut cmd = Command::new(&self.clang_path);
        if let Some(triple) = target_triple {
            if !triple.trim().is_empty() {
                cmd.arg(format!("--target={}", triple.trim()));
            }
        }
        if let Some(ref sysroot) = self.macos_sysroot {
            if target_triple.map(|t| t.contains("apple") || t.contains("darwin") || t.contains("macos")).unwrap_or(true) {
                cmd.arg("-isysroot").arg(sysroot);
            }
        }
        cmd.arg("-Wno-override-module");
        cmd.arg("-x").arg("ir");
        cmd.arg(format!("-O{}", opt_level));
        cmd.arg(temp_ll);

        let effective_triple = target_triple.unwrap_or("");
        let targeting_windows = if effective_triple.is_empty() {
            cfg!(windows)
        } else {
            effective_triple.contains("windows")
        };
        let targeting_macos = if effective_triple.is_empty() {
            cfg!(target_os = "macos")
        } else {
            effective_triple.contains("apple") || effective_triple.contains("darwin") || effective_triple.contains("macos")
        };

        let mut rpath_dirs: Vec<PathBuf> = Vec::new();
        for extra in link_inputs {
            let fname = extra.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
            let ext = extra
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_binary_archive_or_so = ext == "so"
                || ext == "a"
                || ext == "dylib"
                || ext == "lib"
                || ext == "dll"
                || fname.contains(".so.");
            if is_binary_archive_or_so {
                cmd.arg("-x").arg("none").arg(extra);
                if !targeting_windows && (ext == "so" || ext == "dylib" || fname.contains(".so.")) {
                    if let Some(parent) = extra.parent() {
                        if Self::is_non_system_lib_dir(parent) && !rpath_dirs.contains(&parent.to_path_buf()) {
                            rpath_dirs.push(parent.to_path_buf());
                        }
                    }
                }
            } else {
                if targeting_windows {
                    cmd.arg("-D_CRT_SECURE_NO_WARNINGS");
                    cmd.arg("-DWIN32_LEAN_AND_MEAN");
                }
                cmd.arg("-x").arg("c").arg(extra);
            }
        }

        for rdir in &rpath_dirs {
            cmd.arg(format!("-Wl,-rpath,{}", rdir.display()));
        }

        cmd.arg("-o").arg(out_path);
        if targeting_macos {
            cmd.arg("-lm");
        } else if targeting_windows {
            cmd.arg("-lws2_32");
            cmd.arg("-ladvapi32");
            cmd.arg("-luserenv");
        } else {
            cmd.arg("-lm");
            cmd.arg("-lpthread");
        }

        cmd.output().map_err(|e| VerifyError::Io(e.to_string()))
    }

    /// Compile LLVM IR text into a standalone native binary (ELF / Mach-O / PE-COFF)
    pub fn compile_ir_to_binary(
        &self,
        ir: &str,
        out_path: &Path,
        opt_level: u8,
        extra_c_files: &[PathBuf],
    ) -> Result<(), VerifyError> {
        self.compile_ir_to_binary_for_target(ir, out_path, opt_level, extra_c_files, None)
    }

    /// Compile LLVM IR text into a standalone native binary parameterized by an optional target triple.
    pub fn compile_ir_to_binary_for_target(
        &self,
        ir: &str,
        out_path: &Path,
        opt_level: u8,
        extra_c_files: &[PathBuf],
        target_triple: Option<&str>,
    ) -> Result<(), VerifyError> {
        let temp_dir = std::env::temp_dir();
        let unique_id = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_ll = temp_dir.join(format!("omni_build_{}_{}.ll", std::process::id(), unique_id));

        let mut f = fs::File::create(&temp_ll).map_err(|e| VerifyError::Io(e.to_string()))?;
        f.write_all(ir.as_bytes())
            .map_err(|e| VerifyError::Io(e.to_string()))?;

        let mut output = self.invoke_clang_link(&temp_ll, out_path, opt_level, extra_c_files, target_triple)?;

        // If a static `.a` archive in `oenv` failed linking due to transitive static dependencies,
        // automatically retry with its sibling `.so`/`.dylib`/`.dll.a` + RPATH if present!
        if !output.status.success() {
            let mut swapped = false;
            let mut fallback_inputs = Vec::with_capacity(extra_c_files.len());
            for item in extra_c_files {
                let ext = item.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("a") && !item.to_string_lossy().ends_with(".dll.a") {
                    if let Some(shared_sib) = Self::find_sibling_shared_lib(item) {
                        fallback_inputs.push(shared_sib);
                        swapped = true;
                        continue;
                    }
                }
                fallback_inputs.push(item.clone());
            }
            if swapped {
                output = self.invoke_clang_link(&temp_ll, out_path, opt_level, &fallback_inputs, target_triple)?;
            }
        }

        let _ = fs::remove_file(&temp_ll);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(self.parse_diagnostic(&stderr));
        }

        // Stage any sibling `bin/*.dll` runtime dependencies from `oenv` alongside Windows `.exe` outputs
        let (_, bin_dirs) = Self::collect_runtime_search_dirs(extra_c_files);
        if !bin_dirs.is_empty() {
            if let Some(out_dir) = out_path.parent() {
                let out_dir = if out_dir.as_os_str().is_empty() { Path::new(".") } else { out_dir };
                let is_ephemeral_jit = out_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("omni_jit_"))
                    .unwrap_or(false);
                if !is_ephemeral_jit && out_dir.is_dir() {
                    for bdir in &bin_dirs {
                        if let Ok(entries) = fs::read_dir(bdir) {
                            for entry in entries.filter_map(|e| e.ok()) {
                                let p = entry.path();
                                if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                                    let dst = out_dir.join(entry.file_name());
                                    if !dst.exists() {
                                        let _ = fs::copy(&p, &dst);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn apply_runtime_search_env(cmd: &mut Command, extra_c_files: &[PathBuf]) {
        let (lib_dirs, bin_dirs) = Self::collect_runtime_search_dirs(extra_c_files);
        if !bin_dirs.is_empty() || !lib_dirs.is_empty() {
            if cfg!(windows) {
                let mut paths: Vec<PathBuf> = bin_dirs;
                paths.extend(lib_dirs);
                if let Some(existing) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&existing));
                }
                if let Ok(joined) = std::env::join_paths(paths) {
                    cmd.env("PATH", joined);
                }
            } else if !lib_dirs.is_empty() {
                let env_key = if cfg!(target_os = "macos") {
                    "DYLD_LIBRARY_PATH"
                } else {
                    "LD_LIBRARY_PATH"
                };
                let mut paths = lib_dirs;
                if let Some(existing) = std::env::var_os(env_key) {
                    paths.extend(std::env::split_paths(&existing));
                }
                if let Ok(joined) = std::env::join_paths(paths) {
                    cmd.env(env_key, joined);
                }
            }
        }
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

        let mut cmd = Command::new(&temp_bin);
        Self::apply_runtime_search_env(&mut cmd, extra_c_files);
        let mut child = cmd
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

        let mut cmd = Command::new(&temp_bin);
        Self::apply_runtime_search_env(&mut cmd, extra_c_files);
        let output = cmd
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

    #[test]
    fn test_parse_windows_drive_letter_diagnostics() {
        let verifier = LlvmVerifier::new().unwrap();
        let win_stderr = r"C:\Users\dev\AppData\Local\Temp\omni_verify_42_0.ll:17:9: error: expected instruction opcode";
        match verifier.parse_diagnostic(win_stderr) {
            VerifyError::SyntaxError {
                file,
                line,
                column,
                message,
                ..
            } => {
                assert_eq!(file, r"C:\Users\dev\AppData\Local\Temp\omni_verify_42_0.ll");
                assert_eq!(line, 17);
                assert_eq!(column, 9);
                assert_eq!(message, "expected instruction opcode");
            }
            other => panic!("Expected SyntaxError for Windows path, got {:?}", other),
        }
    }
}
