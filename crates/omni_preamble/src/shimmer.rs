//! Universal Polyglot Package Shimmer & Direct Git Importer
//!
//! Provides in-process dependency resolution, downloading, and cache extraction for:
//! - Rust (crates.io sparse index & .crate tarballs)
//! - Python (PyPI JSON API & .whl / sdist archives)
//! - Node (npm registry tarballs)
//! - C (amalgamation tarballs, headers, pkg-config)
//! - Java (Maven Central .jar)
//! - C# (NuGet .nupkg)
//! - Direct Git Repositories (`[git]'<lang>' <url>[#<ref>]`)

use crate::PreambleError;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageEcosystem {
    Rust,
    Python,
    Node,
    C,
    Java,
    Csharp,
    Go,
    Polyglot(String),
}

impl PackageEcosystem {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "rust" | "rs" | "cargo" => PackageEcosystem::Rust,
            "python" | "py" | "pypi" => PackageEcosystem::Python,
            "node" | "npm" | "javascript" | "js" => PackageEcosystem::Node,
            "c" | "cpp" | "cxx" => PackageEcosystem::C,
            "java" | "jvm" | "maven" | "gradle" => PackageEcosystem::Java,
            "csharp" | "cs" | "dotnet" | "nuget" => PackageEcosystem::Csharp,
            "go" | "golang" => PackageEcosystem::Go,
            other => PackageEcosystem::Polyglot(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            PackageEcosystem::Rust => "rust",
            PackageEcosystem::Python => "python",
            PackageEcosystem::Node => "node",
            PackageEcosystem::C => "c",
            PackageEcosystem::Java => "java",
            PackageEcosystem::Csharp => "csharp",
            PackageEcosystem::Go => "go",
            PackageEcosystem::Polyglot(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CPackageProvider {
    ArchPacman,
    DebianApt,
    RpmYum,
    RpmZypper,
    AlpineApk,
    MacosHomebrew,
    WindowsMsys2,
}

impl CPackageProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            CPackageProvider::ArchPacman => "arch-pacman",
            CPackageProvider::DebianApt => "debian-apt",
            CPackageProvider::RpmYum => "rpm-yum",
            CPackageProvider::RpmZypper => "rpm-zypper",
            CPackageProvider::AlpineApk => "alpine-apk",
            CPackageProvider::MacosHomebrew => "macos-homebrew",
            CPackageProvider::WindowsMsys2 => "windows-msys2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    X86_64,
    Aarch64,
}

impl TargetArch {
    pub fn host() -> Self {
        if cfg!(target_arch = "aarch64") {
            TargetArch::Aarch64
        } else {
            TargetArch::X86_64
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64",
            TargetArch::Aarch64 => "aarch64",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlatform {
    /// Canonical directory/cache slug (e.g. `arch-x86_64`, `debian-x86_64`, `yum-x86_64`, `suse-x86_64`, `alpine-x86_64`, `macos-arm64`, `windows-x86_64`)
    pub slug: String,
    /// LLVM target triple (e.g. `x86_64-unknown-linux-gnu`, `arm64-apple-macosx11.0.0`, `x86_64-pc-windows-msvc`)
    pub llvm_triple: String,
    /// Upstream C binary/header package repository family
    pub provider: CPackageProvider,
    /// Target CPU architecture
    pub arch: TargetArch,
}

impl Default for TargetPlatform {
    fn default() -> Self {
        Self::detect_host()
    }
}

impl TargetPlatform {
    /// Detect the current host operating system, Linux distribution family, and CPU architecture
    pub fn detect_host() -> Self {
        let arch = TargetArch::host();
        if cfg!(target_os = "macos") {
            return Self::macos(arch);
        }
        if cfg!(target_os = "windows") {
            return Self::windows(arch, false);
        }
        // Linux: inspect /etc/os-release and distro markers across all major Linux repository families
        match Self::detect_linux_provider() {
            CPackageProvider::ArchPacman => Self::arch(arch),
            CPackageProvider::RpmYum => Self::yum(arch),
            CPackageProvider::RpmZypper => Self::suse(arch),
            CPackageProvider::AlpineApk => Self::alpine(arch),
            _ => Self::debian(arch),
        }
    }

    fn detect_linux_provider() -> CPackageProvider {
        if Path::new("/etc/arch-release").exists() {
            return CPackageProvider::ArchPacman;
        }
        if Path::new("/etc/alpine-release").exists() {
            return CPackageProvider::AlpineApk;
        }
        if Path::new("/etc/SuSE-release").exists() {
            return CPackageProvider::RpmZypper;
        }
        if Path::new("/etc/redhat-release").exists() || Path::new("/etc/rocky-release").exists() || Path::new("/etc/almalinux-release").exists() {
            return CPackageProvider::RpmYum;
        }
        if let Ok(os_rel) = fs::read_to_string("/etc/os-release") {
            let lower = os_rel.to_lowercase();
            let tokens: Vec<&str> = lower
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|s| !s.is_empty())
                .collect();
            let has_any = |keys: &[&str]| keys.iter().any(|k| tokens.contains(k));
            if has_any(&["arch", "manjaro", "endeavouros", "cachyos", "artix", "garuda"]) {
                return CPackageProvider::ArchPacman;
            }
            if has_any(&["alpine", "wolfi", "chimera", "postmarketos"]) {
                return CPackageProvider::AlpineApk;
            }
            if has_any(&["opensuse", "suse", "sles", "tumbleweed", "leap"]) {
                return CPackageProvider::RpmZypper;
            }
            if has_any(&[
                "rhel",
                "fedora",
                "centos",
                "rocky",
                "almalinux",
                "amzn",
                "ol",
                "nobara",
                "ultramarine",
            ]) {
                return CPackageProvider::RpmYum;
            }
            if has_any(&["debian", "ubuntu", "linuxmint", "pop", "kali", "raspbian"]) {
                return CPackageProvider::DebianApt;
            }
        }
        if Path::new("/var/lib/pacman").is_dir() {
            return CPackageProvider::ArchPacman;
        }
        if Path::new("/etc/apk").is_dir() {
            return CPackageProvider::AlpineApk;
        }
        if Path::new("/etc/zypp").is_dir() {
            return CPackageProvider::RpmZypper;
        }
        if Path::new("/etc/yum.repos.d").is_dir() {
            return CPackageProvider::RpmYum;
        }
        CPackageProvider::DebianApt
    }

    pub fn arch(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::X86_64 => ("arch-x86_64", "x86_64-unknown-linux-gnu"),
            TargetArch::Aarch64 => ("arch-aarch64", "aarch64-unknown-linux-gnu"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::ArchPacman,
            arch,
        }
    }

    pub fn debian(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::X86_64 => ("debian-x86_64", "x86_64-unknown-linux-gnu"),
            TargetArch::Aarch64 => ("debian-aarch64", "aarch64-unknown-linux-gnu"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::DebianApt,
            arch,
        }
    }

    pub fn yum(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::X86_64 => ("yum-x86_64", "x86_64-unknown-linux-gnu"),
            TargetArch::Aarch64 => ("yum-aarch64", "aarch64-unknown-linux-gnu"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::RpmYum,
            arch,
        }
    }

    pub fn suse(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::X86_64 => ("suse-x86_64", "x86_64-unknown-linux-gnu"),
            TargetArch::Aarch64 => ("suse-aarch64", "aarch64-unknown-linux-gnu"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::RpmZypper,
            arch,
        }
    }

    pub fn alpine(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::X86_64 => ("alpine-x86_64", "x86_64-unknown-linux-musl"),
            TargetArch::Aarch64 => ("alpine-aarch64", "aarch64-unknown-linux-musl"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::AlpineApk,
            arch,
        }
    }

    pub fn macos(arch: TargetArch) -> Self {
        let (slug, triple) = match arch {
            TargetArch::Aarch64 => ("macos-arm64", "arm64-apple-macosx11.0.0"),
            TargetArch::X86_64 => ("macos-x86_64", "x86_64-apple-macosx10.15.0"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::MacosHomebrew,
            arch,
        }
    }

    pub fn windows(arch: TargetArch, gnu_abi: bool) -> Self {
        let (slug, triple) = match (arch, gnu_abi) {
            (TargetArch::X86_64, false) => ("windows-x86_64", "x86_64-pc-windows-msvc"),
            (TargetArch::X86_64, true) => ("windows-x86_64", "x86_64-w64-windows-gnu"),
            (TargetArch::Aarch64, false) => ("windows-aarch64", "aarch64-pc-windows-msvc"),
            (TargetArch::Aarch64, true) => ("windows-aarch64", "aarch64-w64-windows-gnu"),
        };
        Self {
            slug: slug.to_string(),
            llvm_triple: triple.to_string(),
            provider: CPackageProvider::WindowsMsys2,
            arch,
        }
    }

    /// Parse a `--platform <spec>` string (friendly OS/distro preset or raw LLVM target triple)
    pub fn parse(spec: &str) -> Result<Self, PreambleError> {
        let trimmed = spec.trim();
        let lower = trimmed.to_lowercase();
        let host_arch = TargetArch::host();

        match lower.as_str() {
            "" | "auto" | "native" | "host" => Ok(Self::detect_host()),
            "arch" | "pacman" => Ok(Self::arch(host_arch)),
            "arch-x86_64" | "arch-amd64" => Ok(Self::arch(TargetArch::X86_64)),
            "arch-aarch64" | "arch-arm64" => Ok(Self::arch(TargetArch::Aarch64)),
            "debian" | "ubuntu" | "apt" | "linux" => Ok(Self::debian(host_arch)),
            "debian-x86_64" | "debian-amd64" | "ubuntu-x86_64" | "ubuntu-amd64" | "linux-x86_64" | "linux-amd64" => {
                Ok(Self::debian(TargetArch::X86_64))
            }
            "debian-aarch64" | "debian-arm64" | "ubuntu-aarch64" | "ubuntu-arm64" | "linux-aarch64" | "linux-arm64" => {
                Ok(Self::debian(TargetArch::Aarch64))
            }
            "yum" | "dnf" | "rpm" | "rhel" | "fedora" | "rocky" | "almalinux" | "centos" | "amzn" => {
                Ok(Self::yum(host_arch))
            }
            "yum-x86_64" | "yum-amd64" | "dnf-x86_64" | "rpm-x86_64" | "rhel-x86_64" | "fedora-x86_64" | "rocky-x86_64" | "almalinux-x86_64" | "centos-x86_64" | "amzn-x86_64" => {
                Ok(Self::yum(TargetArch::X86_64))
            }
            "yum-aarch64" | "yum-arm64" | "dnf-aarch64" | "rpm-aarch64" | "rhel-aarch64" | "fedora-aarch64" | "rocky-aarch64" | "almalinux-aarch64" | "centos-aarch64" | "amzn-aarch64" => {
                Ok(Self::yum(TargetArch::Aarch64))
            }
            "suse" | "opensuse" | "zypper" | "sles" => Ok(Self::suse(host_arch)),
            "suse-x86_64" | "suse-amd64" | "opensuse-x86_64" | "zypper-x86_64" | "sles-x86_64" => {
                Ok(Self::suse(TargetArch::X86_64))
            }
            "suse-aarch64" | "suse-arm64" | "opensuse-aarch64" | "zypper-aarch64" | "sles-aarch64" => {
                Ok(Self::suse(TargetArch::Aarch64))
            }
            "alpine" | "apk" | "wolfi" | "musl" => Ok(Self::alpine(host_arch)),
            "alpine-x86_64" | "alpine-amd64" | "apk-x86_64" | "musl-x86_64" => {
                Ok(Self::alpine(TargetArch::X86_64))
            }
            "alpine-aarch64" | "alpine-arm64" | "apk-aarch64" | "musl-aarch64" => {
                Ok(Self::alpine(TargetArch::Aarch64))
            }
            "macos" | "darwin" | "osx" | "homebrew" | "brew" => Ok(Self::macos(host_arch)),
            "macos-arm64" | "macos-aarch64" | "darwin-arm64" | "darwin-aarch64" | "osx-arm64" => {
                Ok(Self::macos(TargetArch::Aarch64))
            }
            "macos-x86_64" | "macos-amd64" | "darwin-x86_64" | "osx-x86_64" => {
                Ok(Self::macos(TargetArch::X86_64))
            }
            "windows" | "win" | "msys2" => Ok(Self::windows(host_arch, false)),
            "windows-x86_64" | "windows-amd64" | "win64" | "win-x64" => {
                Ok(Self::windows(TargetArch::X86_64, false))
            }
            "msys2-ucrt64" | "mingw-x86_64" | "windows-gnu" | "windows-x86_64-gnu" => {
                Ok(Self::windows(TargetArch::X86_64, true))
            }
            "windows-aarch64" | "windows-arm64" | "win-arm64" => {
                Ok(Self::windows(TargetArch::Aarch64, false))
            }
            "msys2-clangarm64" | "mingw-aarch64" | "windows-aarch64-gnu" => {
                Ok(Self::windows(TargetArch::Aarch64, true))
            }
            _ if lower.contains('-') => {
                let arch = if lower.contains("aarch64") || lower.contains("arm64") {
                    TargetArch::Aarch64
                } else if lower.contains("x86_64") || lower.contains("amd64") {
                    TargetArch::X86_64
                } else {
                    return Err(PreambleError::PathNotFound(format!(
                        "Unsupported architecture in platform spec '{}'. Expected x86_64 or aarch64/arm64.",
                        trimmed
                    )));
                };

                if lower.contains("apple") || lower.contains("darwin") || lower.contains("macosx") {
                    let mut p = Self::macos(arch);
                    if lower != "arm64-apple-darwin" && lower != "aarch64-apple-darwin" {
                        p.llvm_triple = trimmed.to_string();
                    }
                    Ok(p)
                } else if lower.contains("windows") || lower.contains("mingw") {
                    let is_gnu = lower.contains("gnu") || lower.contains("mingw");
                    let mut p = Self::windows(arch, is_gnu);
                    p.llvm_triple = trimmed.to_string();
                    Ok(p)
                } else if lower.contains("musl") || lower.contains("alpine") {
                    let mut p = Self::alpine(arch);
                    p.llvm_triple = trimmed.to_string();
                    Ok(p)
                } else if lower.contains("redhat") || lower.contains("centos") || lower.contains("fedora") || lower.contains("amzn") {
                    let mut p = Self::yum(arch);
                    p.llvm_triple = trimmed.to_string();
                    Ok(p)
                } else if lower.contains("suse") {
                    let mut p = Self::suse(arch);
                    p.llvm_triple = trimmed.to_string();
                    Ok(p)
                } else if lower.contains("linux") {
                    let mut p = if cfg!(target_os = "linux") {
                        match Self::detect_linux_provider() {
                            CPackageProvider::ArchPacman => Self::arch(arch),
                            CPackageProvider::RpmYum => Self::yum(arch),
                            CPackageProvider::RpmZypper => Self::suse(arch),
                            CPackageProvider::AlpineApk => Self::alpine(arch),
                            _ => Self::debian(arch),
                        }
                    } else {
                        Self::debian(arch)
                    };
                    p.llvm_triple = trimmed.to_string();
                    Ok(p)
                } else {
                    Err(PreambleError::PathNotFound(format!(
                        "Unrecognized platform spec '{}'. Supported presets: auto, arch-x86_64, arch-aarch64, debian-x86_64, debian-aarch64, yum-x86_64, yum-aarch64, suse-x86_64, suse-aarch64, alpine-x86_64, alpine-aarch64, macos-arm64, macos-x86_64, windows-x86_64, windows-aarch64.",
                        trimmed
                    )))
                }
            }
            _ => Err(PreambleError::PathNotFound(format!(
                "Unrecognized platform spec '{}'. Supported presets: auto, arch-x86_64, arch-aarch64, debian-x86_64, debian-aarch64, yum-x86_64, yum-aarch64, suse-x86_64, suse-aarch64, alpine-x86_64, alpine-aarch64, macos-arm64, macos-x86_64, windows-x86_64, windows-aarch64.",
                trimmed
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    pub ecosystem: PackageEcosystem,
    pub name: String,
    pub version: Option<String>,
    pub is_git: bool,
    pub git_url: Option<String>,
    pub git_ref: Option<String>,
    pub raw_line: String,
}

#[derive(Debug)]
pub struct PackageShimmer {
    pub cache_root: PathBuf,
    pub platform: TargetPlatform,
}

impl Default for PackageShimmer {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageShimmer {
    pub fn new() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let cache_root = home.join(".omnicache");
        Self {
            cache_root,
            platform: TargetPlatform::detect_host(),
        }
    }

    pub fn with_cache_root(cache_root: PathBuf) -> Self {
        Self {
            cache_root,
            platform: TargetPlatform::detect_host(),
        }
    }

    pub fn with_platform(mut self, platform: TargetPlatform) -> Self {
        self.platform = platform;
        self
    }

    /// Initialize a local polyglot environment (`oenv`) layout and metadata
    pub fn init_oenv(oenv_dir: &Path) -> Result<PathBuf, PreambleError> {
        let pkgs = oenv_dir.join("packages");
        let git = oenv_dir.join("git");
        let tmp = oenv_dir.join("tmp");
        fs::create_dir_all(&pkgs)?;
        fs::create_dir_all(&git)?;
        fs::create_dir_all(&tmp)?;

        let env_file = oenv_dir.join("omni-env.json");
        if !env_file.exists() {
            let meta = serde_json::json!({
                "omni_version": env!("CARGO_PKG_VERSION"),
                "env_name": oenv_dir.file_name().and_then(|n| n.to_str()).unwrap_or("oenv"),
                "architecture": std::env::consts::ARCH,
                "os": std::env::consts::OS,
                "ecosystems": ["java", "python", "rust", "go", "node", "c", "csharp"]
            });
            if let Ok(content) = serde_json::to_string_pretty(&meta) {
                let _ = fs::write(&env_file, content);
            }
        }
        Ok(oenv_dir.to_path_buf())
    }

    /// Check if a given directory path is an initialized oenv
    pub fn is_oenv(path: &Path) -> bool {
        path.is_dir()
            && (path.join("omni-env.json").is_file()
                || path.join("packages").is_dir()
                || path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s == "oenv" || s == ".oenv")
                    .unwrap_or(false))
    }

    pub fn packages_dir(&self, eco: &PackageEcosystem) -> PathBuf {
        self.cache_root.join("packages").join(eco.as_str())
    }

    pub fn git_dir(&self, eco: &PackageEcosystem) -> PathBuf {
        self.cache_root.join("git").join(eco.as_str())
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.cache_root.join("tmp")
    }

    /// Parse an `omni-requirements.txt` file content into structured `PackageSpec`s
    pub fn parse_requirements(content: &str) -> Vec<PackageSpec> {
        let mut specs = Vec::new();

        // 1. Direct Git notation: `[git]'<lang>' <url>[#<ref>]`
        let git_re = Regex::new(
            r#"^\[git\]['"]([a-zA-Z0-9_\-]+)['"]\s+([^\s#]+)(?:#([^\s]+))?$"#,
        )
        .unwrap();

        // 2. Registry notation: `'<eco>' <name> [op] <version>` or `'<eco>' <name>`
        let reg_re = Regex::new(
            r#"^['"]([a-zA-Z0-9_\-]+)['"]\s+([a-zA-Z0-9_\-./:]+)(?:\s*(?:==|>=|<=|=|\^|~=)\s*["']?([a-zA-Z0-9_\-.]+)["']?)?$"#,
        )
        .unwrap();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some(caps) = git_re.captures(trimmed) {
                let eco = PackageEcosystem::from_str(&caps[1]);
                let url = caps[2].to_string();
                let g_ref = caps.get(3).map(|m| m.as_str().to_string());
                let repo_name = url
                    .trim_end_matches(".git")
                    .split('/')
                    .last()
                    .unwrap_or("repo")
                    .to_string();

                specs.push(PackageSpec {
                    ecosystem: eco,
                    name: repo_name,
                    version: g_ref.clone(),
                    is_git: true,
                    git_url: Some(url),
                    git_ref: g_ref,
                    raw_line: trimmed.to_string(),
                });
                continue;
            }

            if let Some(caps) = reg_re.captures(trimmed) {
                let eco = PackageEcosystem::from_str(&caps[1]);
                let name = caps[2].to_string();
                let ver = caps.get(3).map(|m| m.as_str().to_string());

                specs.push(PackageSpec {
                    ecosystem: eco,
                    name,
                    version: ver,
                    is_git: false,
                    git_url: None,
                    git_ref: None,
                    raw_line: trimmed.to_string(),
                });
            }
        }

        specs
    }

    /// Read and parse a requirements file from disk
    pub fn parse_requirements_file(&self, path: &Path) -> Result<Vec<PackageSpec>, PreambleError> {
        if !path.is_file() {
            return Err(PreambleError::PathNotFound(path.display().to_string()));
        }
        let content = fs::read_to_string(path).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed to read {}: {}", path.display(), e))
        })?;
        Ok(Self::parse_requirements(&content))
    }

    /// Resolves and installs a package specification, returning its local origin path in `~/.omnicache/`
    pub fn shim_package(&self, spec: &PackageSpec) -> Result<PathBuf, PreambleError> {
        let _ = fs::create_dir_all(self.tmp_dir());

        if spec.is_git {
            if let Some(ref url) = spec.git_url {
                return self.shim_git(spec.ecosystem.clone(), url, spec.git_ref.as_deref());
            }
        }

        match spec.ecosystem {
            PackageEcosystem::Rust => {
                let ver = spec.version.as_deref().unwrap_or("latest");
                self.shim_rust(&spec.name, ver)
            }
            PackageEcosystem::Python => {
                self.shim_python(&spec.name, spec.version.as_deref())
            }
            PackageEcosystem::Node => {
                let ver = spec.version.as_deref().unwrap_or("latest");
                self.shim_node(&spec.name, ver)
            }
            PackageEcosystem::Java => {
                let ver = spec.version.as_deref().unwrap_or("latest");
                self.shim_java(&spec.name, ver)
            }
            PackageEcosystem::Csharp => {
                let ver = spec.version.as_deref().unwrap_or("latest");
                self.shim_csharp(&spec.name, ver)
            }
            PackageEcosystem::C => {
                self.shim_c(&spec.name, spec.version.as_deref())
            }
            PackageEcosystem::Go => {
                // If Go registry package is requested without [git], format as standard Go git path
                let url = format!("https://{}", spec.name);
                self.shim_git(PackageEcosystem::Go, &url, spec.version.as_deref())
            }
            PackageEcosystem::Polyglot(_) => {
                // Fallback to Git repository if looks like URL
                if spec.name.starts_with("http") || spec.name.contains('/') {
                    self.shim_git(spec.ecosystem.clone(), &spec.name, spec.version.as_deref())
                } else {
                    Err(PreambleError::PathNotFound(format!(
                        "Unsupported ecosystem for package '{}'",
                        spec.name
                    )))
                }
            }
        }
    }

    /// Shim Rust crate from crates.io
    pub fn shim_rust(&self, name: &str, version: &str) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::Rust);
        let _ = fs::create_dir_all(&pkg_dir);

        let resolved_version = if version == "latest" {
            // Probe crates.io API for latest version
            self.query_crates_io_latest(name).unwrap_or_else(|| "0.1.0".to_string())
        } else {
            version.to_string()
        };

        let target_dir = pkg_dir.join(format!("{}-{}", name, resolved_version));
        let lib_rs = target_dir.join("src").join("lib.rs");
        if lib_rs.is_file() {
            return Ok(target_dir);
        }

        let download_url = format!(
            "https://static.crates.io/crates/{}/{}-{}.crate",
            name, name, resolved_version
        );
        let archive_path = self.tmp_dir().join(format!("{}-{}.crate", name, resolved_version));

        self.download_file(&download_url, &archive_path)?;

        // .crate is a gzip-compressed tarball (unpacked via pure-Rust cross-platform extractor)
        let extract_res = Self::extract_tar_gz(&archive_path, &pkg_dir, 0);
        let _ = fs::remove_file(&archive_path);
        extract_res?;

        if target_dir.is_dir() {
            Ok(target_dir)
        } else {
            Err(PreambleError::PathNotFound(format!(
                "Extracted crate directory '{}' not found",
                target_dir.display()
            )))
        }
    }

    /// Shim Python wheel/sdist from PyPI
    pub fn shim_python(&self, name: &str, version: Option<&str>) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::Python);
        let _ = fs::create_dir_all(&pkg_dir);

        // Query PyPI JSON API for releases
        let api_url = match version {
            Some(v) => format!("https://pypi.org/pypi/{}/{}/json", name, v),
            None => format!("https://pypi.org/pypi/{}/json", name),
        };

        let json_str = self.fetch_url_string(&api_url)?;
        let json: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            PreambleError::PathNotFound(format!("Invalid PyPI JSON for {}: {}", name, e))
        })?;

        let resolved_ver = json
            .get("info")
            .and_then(|i| i.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or(version.unwrap_or("0.1.0"));

        let target_dir = pkg_dir.join(format!("{}-{}", name, resolved_ver));
        if target_dir.is_dir() {
            return Ok(target_dir);
        }

        // Locate preferred wheel (pure python or manylinux) or sdist (.tar.gz)
        let urls = json.get("urls").and_then(|u| u.as_array()).ok_or_else(|| {
            PreambleError::PathNotFound(format!("No release archives found for PyPI package '{}'", name))
        })?;

        let mut chosen_url = None;
        let mut is_wheel = false;

        for item in urls {
            if let Some(filename) = item.get("filename").and_then(|f| f.as_str()) {
                if filename.ends_with("py3-none-any.whl") || filename.ends_with("-none-any.whl") {
                    chosen_url = item.get("url").and_then(|u| u.as_str());
                    is_wheel = true;
                    break;
                }
            }
        }

        // Prefer host-OS-matching binary wheels before sdist or foreign wheels
        if chosen_url.is_none() {
            let os_tags: &[&str] = if cfg!(target_os = "macos") {
                if cfg!(target_arch = "aarch64") {
                    &["macosx", "arm64", "universal2"]
                } else {
                    &["macosx", "x86_64", "universal2"]
                }
            } else if cfg!(target_os = "windows") {
                if cfg!(target_arch = "aarch64") {
                    &["win_arm64"]
                } else {
                    &["win_amd64", "win32"]
                }
            } else if cfg!(target_arch = "aarch64") {
                &["manylinux", "aarch64"]
            } else {
                &["manylinux", "x86_64"]
            };

            for item in urls {
                if let Some(filename) = item.get("filename").and_then(|f| f.as_str()) {
                    if filename.ends_with(".whl") && os_tags.iter().any(|tag| filename.contains(tag)) {
                        chosen_url = item.get("url").and_then(|u| u.as_str());
                        is_wheel = true;
                        break;
                    }
                }
            }
        }

        if chosen_url.is_none() {
            for item in urls {
                if let Some(filename) = item.get("filename").and_then(|f| f.as_str()) {
                    if filename.ends_with(".tar.gz") {
                        chosen_url = item.get("url").and_then(|u| u.as_str());
                        is_wheel = false;
                        break;
                    }
                }
            }
        }

        if chosen_url.is_none() {
            for item in urls {
                if let Some(filename) = item.get("filename").and_then(|f| f.as_str()) {
                    if filename.ends_with(".whl") {
                        chosen_url = item.get("url").and_then(|u| u.as_str());
                        is_wheel = true;
                        break;
                    }
                }
            }
        }

        let download_url = chosen_url.ok_or_else(|| {
            PreambleError::PathNotFound(format!("No compatible wheel or sdist found for PyPI '{}'", name))
        })?;

        let _ = fs::create_dir_all(&target_dir);

        if is_wheel {
            let whl_file = self.tmp_dir().join(format!("{}-{}.whl", name, resolved_ver));
            self.download_file(download_url, &whl_file)?;
            let extract_res = Self::extract_zip(&whl_file, &target_dir, false);
            let _ = fs::remove_file(&whl_file);
            extract_res?;
        } else {
            let tar_file = self.tmp_dir().join(format!("{}-{}.tar.gz", name, resolved_ver));
            self.download_file(download_url, &tar_file)?;
            let extract_res = Self::extract_tar_gz(&tar_file, &target_dir, 1);
            let _ = fs::remove_file(&tar_file);
            extract_res?;
        }

        Ok(target_dir)
    }

    /// Shim Node package from npm registry
    pub fn shim_node(&self, name: &str, version: &str) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::Node);
        let _ = fs::create_dir_all(&pkg_dir);

        let resolved_ver = if version == "latest" {
            self.query_npm_latest(name).unwrap_or_else(|| "1.0.0".to_string())
        } else {
            version.to_string()
        };

        let target_dir = pkg_dir.join(format!("{}-{}", name, resolved_ver));
        if target_dir.join("package.json").is_file() {
            return Ok(target_dir);
        }

        let download_url = format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, resolved_ver
        );
        let tgz_file = self.tmp_dir().join(format!("{}-{}.tgz", name, resolved_ver));

        self.download_file(&download_url, &tgz_file)?;
        let _ = fs::create_dir_all(&target_dir);

        // npm tarballs have a root `package/` directory -> strip component
        let extract_res = Self::extract_tar_gz(&tgz_file, &target_dir, 1);
        let _ = fs::remove_file(&tgz_file);
        extract_res?;

        Ok(target_dir)
    }

    /// Shim Java JAR from Maven Central
    pub fn shim_java(&self, group_and_artifact: &str, version: &str) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::Java);
        let _ = fs::create_dir_all(&pkg_dir);

        // Formats: `org.apache.commons:commons-lang3` or `org.json:json`
        let (group, artifact) = if let Some((g, a)) = group_and_artifact.split_once(':') {
            (g, a)
        } else {
            ("org.apache", group_and_artifact)
        };

        let group_path = group.replace('.', "/");
        let target_dir = pkg_dir.join(format!("{}-{}", artifact, version));
        if target_dir.is_dir() {
            return Ok(target_dir);
        }

        let download_url = format!(
            "https://repo1.maven.org/maven2/{}/{}/{}/{}-{}.jar",
            group_path, artifact, version, artifact, version
        );
        let jar_file = self.tmp_dir().join(format!("{}-{}.jar", artifact, version));

        self.download_file(&download_url, &jar_file)?;
        let _ = fs::create_dir_all(&target_dir);

        // JAR is a standard ZIP containing .class bytecode files
        let extract_res = Self::extract_zip(&jar_file, &target_dir, false);
        let _ = fs::remove_file(&jar_file);
        extract_res?;

        Ok(target_dir)
    }

    /// Shim C# .NET assembly from NuGet
    pub fn shim_csharp(&self, package: &str, version: &str) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::Csharp);
        let _ = fs::create_dir_all(&pkg_dir);

        let pkg_lower = package.to_lowercase();
        let target_dir = pkg_dir.join(format!("{}-{}", package, version));
        if target_dir.is_dir() {
            return Ok(target_dir);
        }

        let download_url = format!(
            "https://api.nuget.org/v3-flatcontainer/{}/{}/{}.{}.nupkg",
            pkg_lower, version, pkg_lower, version
        );
        let nupkg_file = self.tmp_dir().join(format!("{}-{}.nupkg", package, version));

        self.download_file(&download_url, &nupkg_file)?;
        let _ = fs::create_dir_all(&target_dir);

        // .nupkg is a standard ZIP archive containing `lib/<framework>/<assembly>.dll`
        let extract_res = Self::extract_zip(&nupkg_file, &target_dir, false);
        let _ = fs::remove_file(&nupkg_file);
        extract_res?;

        Ok(target_dir)
    }

    /// Universal Rootless C Package Importer (`oenv` / `~/.omnicache`)
    /// Fetches headers, static archives (`.a`/`.lib`), and shared libraries (`.so`/`.dylib`/`.dll`)
    /// directly from upstream OS package repositories parameterized by `self.platform` with zero `sudo`.
    pub fn shim_c(&self, name: &str, version: Option<&str>) -> Result<PathBuf, PreambleError> {
        let clean_name = name
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches(".h")
            .trim_end_matches(".hpp");
        let lower_name = clean_name.to_lowercase();
        let pkg_platform_dir = self
            .packages_dir(&PackageEcosystem::C)
            .join(&self.platform.slug);
        let _ = fs::create_dir_all(&pkg_platform_dir);
        let _ = fs::create_dir_all(self.tmp_dir());

        // Check if already cached for this platform slug
        if let Some(req_ver) = version.filter(|v| !v.is_empty() && *v != "latest") {
            let exact_dir = pkg_platform_dir.join(format!("{}-{}", lower_name, req_ver));
            if exact_dir.join("include").is_dir() || exact_dir.join("omni-c-pkg.json").is_file() {
                return Ok(exact_dir);
            }
        } else if let Ok(entries) = fs::read_dir(&pkg_platform_dir) {
            let prefix = format!("{}-", lower_name);
            for entry in entries.filter_map(|e| e.ok()) {
                let fname = entry.file_name().to_string_lossy().to_lowercase();
                if (fname.starts_with(&prefix) || fname == lower_name)
                    && (entry.path().join("include").is_dir()
                        || entry.path().join("omni-c-pkg.json").is_file())
                {
                    return Ok(entry.path());
                }
            }
        }

        // Dispatch to upstream rootless OS package repository for `self.platform`
        let upstream_res = match self.platform.provider {
            CPackageProvider::ArchPacman => {
                self.shim_c_arch(clean_name, version, &pkg_platform_dir)
            }
            CPackageProvider::DebianApt => {
                self.shim_c_debian(clean_name, version, &pkg_platform_dir)
            }
            CPackageProvider::RpmYum | CPackageProvider::RpmZypper => {
                self.shim_c_rpm(clean_name, version, &pkg_platform_dir)
            }
            CPackageProvider::AlpineApk => {
                self.shim_c_alpine(clean_name, version, &pkg_platform_dir)
            }
            CPackageProvider::MacosHomebrew => {
                self.shim_c_macos(clean_name, version, &pkg_platform_dir)
            }
            CPackageProvider::WindowsMsys2 => {
                self.shim_c_windows(clean_name, version, &pkg_platform_dir)
            }
        };

        match upstream_res {
            Ok(installed_dir) => Ok(installed_dir),
            Err(_) => {
                let suggestions = self.suggest_c_packages(clean_name, 3);
                let mut msg = format!(
                    "Could not find C dependency '{}' in upstream repository for platform '{}' ({}).",
                    clean_name,
                    self.platform.slug,
                    self.platform.provider.as_str()
                );
                if !suggestions.is_empty() {
                    msg.push_str("\n  Top 3 closest candidate packages:");
                    for (idx, (cand_name, cand_ver)) in suggestions.iter().enumerate() {
                        msg.push_str(&format!(
                            "\n    {}. {} (version: {})",
                            idx + 1,
                            cand_name,
                            cand_ver
                        ));
                    }
                }
                Err(PreambleError::DependencyNotFound(msg))
            }
        }
    }

    /// Generate structural C package name candidates without hardcoded per-library overrides.
    /// Upstream repository metadata (`Provides:`, `Source:`, `pkgconfig(...)`, Homebrew `Aliases/`)
    /// handles library-to-package resolution algorithmically.
    fn c_package_name_candidates(name: &str, provider: CPackageProvider) -> Vec<String> {
        let lower = name.to_lowercase();
        let stripped_suffix = lower
            .strip_suffix("-devel")
            .or_else(|| lower.strip_suffix("-dev"))
            .unwrap_or(&lower);
        let no_lib = stripped_suffix
            .strip_prefix("lib")
            .filter(|s| !s.is_empty())
            .unwrap_or(stripped_suffix);

        let mut cands = Vec::new();
        let mut push_uniq = |s: String| {
            if !s.is_empty() && !cands.contains(&s) {
                cands.push(s);
            }
        };

        match provider {
            CPackageProvider::ArchPacman
            | CPackageProvider::MacosHomebrew
            | CPackageProvider::WindowsMsys2 => {
                push_uniq(lower.clone());
                push_uniq(no_lib.to_string());
                push_uniq(format!("lib{}", no_lib));
            }
            CPackageProvider::DebianApt => {
                if lower.ends_with("-dev") {
                    push_uniq(lower.clone());
                }
                push_uniq(format!("lib{}-dev", no_lib));
                push_uniq(format!("{}-dev", no_lib));
                push_uniq(format!("lib{}", no_lib));
                push_uniq(lower.clone());
                push_uniq(no_lib.to_string());
            }
            CPackageProvider::RpmYum | CPackageProvider::RpmZypper => {
                if lower.ends_with("-devel") {
                    push_uniq(lower.clone());
                }
                push_uniq(format!("{}-devel", no_lib));
                push_uniq(format!("lib{}-devel", no_lib));
                push_uniq(format!("lib{}", no_lib));
                push_uniq(lower.clone());
                push_uniq(no_lib.to_string());
            }
            CPackageProvider::AlpineApk => {
                if lower.ends_with("-dev") {
                    push_uniq(lower.clone());
                }
                push_uniq(format!("{}-dev", no_lib));
                push_uniq(format!("lib{}-dev", no_lib));
                push_uniq(lower.clone());
                push_uniq(no_lib.to_string());
                push_uniq(format!("lib{}", no_lib));
            }
        }
        cands
    }

    /// Arch Linux (`x86_64` official JSON API + mirror `.pkg.tar.zst`, `aarch64` Arch Linux ARM `.db` + `.pkg.tar.xz`/`.zst`)
    fn shim_c_arch(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let candidates = Self::c_package_name_candidates(name, CPackageProvider::ArchPacman);

        if self.platform.arch == TargetArch::X86_64 {
            let parse_arch_item = |item: &serde_json::Value, fallback_name: &str| -> Option<(String, String, String, String)> {
                let arch = item.get("arch").and_then(|v| v.as_str()).unwrap_or("");
                let repo = item.get("repo").and_then(|v| v.as_str()).unwrap_or("");
                let repo_lower = repo.to_lowercase();
                if (arch != "x86_64" && arch != "any")
                    || repo_lower.contains("testing")
                    || repo_lower.contains("multilib")
                {
                    return None;
                }
                let pkgname = item.get("pkgname").and_then(|v| v.as_str()).unwrap_or(fallback_name);
                let pkgver = item.get("pkgver").and_then(|v| v.as_str()).unwrap_or("latest");
                let pkgrel = item.get("pkgrel").and_then(|v| v.as_str()).unwrap_or("1");
                let epoch = item.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0);
                let filename = if let Some(f) = item.get("filename").and_then(|v| v.as_str()) {
                    f.to_string()
                } else if epoch > 0 {
                    format!("{}-{}:{}-{}-{}.pkg.tar.zst", pkgname, epoch, pkgver, pkgrel, arch)
                } else {
                    format!("{}-{}-{}-{}.pkg.tar.zst", pkgname, pkgver, pkgrel, arch)
                };
                Some((pkgname.to_string(), pkgver.to_string(), repo.to_lowercase(), filename))
            };

            let mut found_pkg: Option<(String, String, String, String)> = None;
            // Pass 1: exact `pkgname` lookup
            for cand in &candidates {
                let api_url = format!("https://archlinux.org/packages/search/json/?name={}", cand);
                if let Ok(resp) = self.fetch_url_string(&api_url) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                        if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                            for item in results {
                                if let Some(hit) = parse_arch_item(item, cand) {
                                    found_pkg = Some(hit);
                                    break;
                                }
                            }
                        }
                    }
                }
                if found_pkg.is_some() {
                    break;
                }
            }

            // Pass 2: virtual `provides` metadata search (e.g. `sqlite` has `provides: ["sqlite3=..."]`)
            if found_pkg.is_none() {
                let no_lib = lower_req.strip_prefix("lib").unwrap_or(&lower_req);
                let search_url = format!("https://archlinux.org/packages/search/json/?q={}", no_lib);
                if let Ok(resp) = self.fetch_url_string(&search_url) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                        if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                            for item in results {
                                let Some(provides) = item.get("provides").and_then(|p| p.as_array()) else { continue };
                                let matches_prov = provides.iter().filter_map(|v| v.as_str()).any(|p| {
                                    let prov_name = p.split(['=', '<', '>']).next().unwrap_or(p).trim().to_lowercase();
                                    let prov_stem = prov_name.split(".so").next().unwrap_or(&prov_name);
                                    candidates.iter().any(|c| c == &prov_name || c == prov_stem)
                                });
                                if matches_prov {
                                    if let Some(hit) = parse_arch_item(item, no_lib) {
                                        found_pkg = Some(hit);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let (pkgname, pkgver, repo, filename) = found_pkg.ok_or_else(|| {
                PreambleError::PathNotFound(format!(
                    "Package '{}' not found in Arch Linux official repositories",
                    name
                ))
            })?;

            let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, pkgver));
            if final_dir.join("omni-c-pkg.json").is_file() {
                return Ok(final_dir);
            }

            let encoded_filename = filename.replace(':', "%3A").replace('+', "%2B");
            let mirrors = [
                format!("https://geo.mirror.pkgbuild.com/{}/os/x86_64/{}", repo, encoded_filename),
                format!("https://mirror.rackspace.com/archlinux/{}/os/x86_64/{}", repo, encoded_filename),
            ];
            let archive_path = self.tmp_dir().join(format!("arch_{}", filename.replace(':', "_")));
            let mut downloaded_url = None;
            for url in &mirrors {
                if self.download_file_with_headers(url, &archive_path, &[]).is_ok() {
                    downloaded_url = Some(url.clone());
                    break;
                }
            }
            let src_url = downloaded_url.ok_or_else(|| {
                PreambleError::PathNotFound(format!(
                    "Failed downloading Arch package '{}' from mirrors",
                    filename
                ))
            })?;

            let stage_dir = self.tmp_dir().join(format!("stage_arch_{}_{}", lower_req, std::process::id()));
            let _ = fs::remove_dir_all(&stage_dir);
            let _ = fs::create_dir_all(&stage_dir);
            let symlinks_res = Self::extract_compressed_tar(&archive_path, &stage_dir, 0);
            let _ = fs::remove_file(&archive_path);
            let symlinks = symlinks_res?;

            let meta = serde_json::json!({
                "name": lower_req,
                "upstream_package": pkgname,
                "version": pkgver,
                "platform": self.platform.slug,
                "llvm_triple": self.platform.llvm_triple,
                "provider": "arch-pacman",
                "source_url": src_url,
            });
            Self::normalize_c_package_layout(&stage_dir, &final_dir, &symlinks, &meta)?;
            let _ = fs::remove_dir_all(&stage_dir);
            Ok(final_dir)
        } else {
            // Arch Linux ARM (`aarch64`): search `core.db` and `extra.db` pacman sync databases
            for repo in &["core", "extra"] {
                let db_cache = Self::shared_index_cache_dir(&self.tmp_dir())
                    .join(format!("archarm_aarch64_{}.db", repo));
                if !db_cache.is_file() {
                    let db_url = format!("http://mirror.archlinuxarm.org/aarch64/{}/{}.db", repo, repo);
                    let _ = self.download_file_with_headers(&db_url, &db_cache, &[]);
                }
                if db_cache.is_file() {
                    if let Ok(Some((pkgname, pkgver, filename))) =
                        Self::search_pacman_db(&db_cache, &candidates)
                    {
                        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, pkgver));
                        if final_dir.join("omni-c-pkg.json").is_file() {
                            return Ok(final_dir);
                        }
                        let encoded = filename.replace(':', "%3A").replace('+', "%2B");
                        let pkg_url = format!("http://mirror.archlinuxarm.org/aarch64/{}/{}", repo, encoded);
                        let archive_path = self.tmp_dir().join(format!("archarm_{}", filename.replace(':', "_")));
                        self.download_file_with_headers(&pkg_url, &archive_path, &[])?;
                        let stage_dir = self.tmp_dir().join(format!("stage_archarm_{}_{}", lower_req, std::process::id()));
                        let _ = fs::remove_dir_all(&stage_dir);
                        let _ = fs::create_dir_all(&stage_dir);
                        let symlinks_res = Self::extract_compressed_tar(&archive_path, &stage_dir, 0);
                        let _ = fs::remove_file(&archive_path);
                        let symlinks = symlinks_res?;

                        let meta = serde_json::json!({
                            "name": lower_req,
                            "upstream_package": pkgname,
                            "version": pkgver,
                            "platform": self.platform.slug,
                            "llvm_triple": self.platform.llvm_triple,
                            "provider": "arch-pacman-arm",
                            "source_url": pkg_url,
                        });
                        Self::normalize_c_package_layout(&stage_dir, &final_dir, &symlinks, &meta)?;
                        let _ = fs::remove_dir_all(&stage_dir);
                        return Ok(final_dir);
                    }
                }
            }
            Err(PreambleError::PathNotFound(format!(
                "Package '{}' not found in Arch Linux ARM (aarch64) repositories",
                name
            )))
        }
    }

    /// Query Launchpad Ubuntu Primary Archive for a binary package on `amd64` or `arm64`, returning `(bin_name, version, pool_url)`
    fn query_ubuntu_deb_url(&self, bin_candidates: &[String], deb_arch: &str) -> Option<(String, String, String)> {
        let arch_suffix = format!("/{}", deb_arch);
        for cand in bin_candidates {
            let api_url = format!(
                "https://api.launchpad.net/1.0/ubuntu/+archive/primary?ws.op=getPublishedBinaries&binary_name={}&exact_match=true&status=Published",
                cand
            );
            let Ok(resp) = self.fetch_url_string(&api_url) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) else { continue };
            let Some(entries) = json.get("entries").and_then(|e| e.as_array()) else { continue };

            for entry in entries {
                let arch_specific = entry.get("architecture_specific").and_then(|v| v.as_bool()).unwrap_or(true);
                let series_link = entry.get("distro_arch_series_link").and_then(|v| v.as_str()).unwrap_or("");
                if arch_specific && !series_link.ends_with(&arch_suffix) {
                    continue;
                }
                let pocket = entry.get("pocket").and_then(|v| v.as_str()).unwrap_or("");
                if pocket == "Proposed" {
                    continue;
                }
                let bin_name = entry.get("binary_package_name").and_then(|v| v.as_str()).unwrap_or(cand);
                let full_ver = entry.get("binary_package_version").and_then(|v| v.as_str()).unwrap_or("");
                let src_name = entry.get("source_package_name").and_then(|v| v.as_str()).unwrap_or(bin_name);
                let component = entry.get("component_name").and_then(|v| v.as_str()).unwrap_or("main");
                if full_ver.is_empty() || src_name.is_empty() {
                    continue;
                }
                let ver_no_epoch = full_ver.split_once(':').map(|(_, v)| v).unwrap_or(full_ver);
                let prefix = if src_name.starts_with("lib") && src_name.len() >= 4 {
                    &src_name[..4]
                } else {
                    &src_name[..1]
                };
                let file_arch = if arch_specific { deb_arch } else { "all" };
                let base_mirror = if deb_arch == "amd64" || !arch_specific {
                    "http://archive.ubuntu.com/ubuntu/pool"
                } else {
                    "http://ports.ubuntu.com/ubuntu-ports/pool"
                };
                let pool_url = format!(
                    "{}/{}/{}/{}/{}_{}_{}.deb",
                    base_mirror, component, prefix, src_name, bin_name, ver_no_epoch, file_arch
                );
                return Some((bin_name.to_string(), ver_no_epoch.to_string(), pool_url));
            }
        }
        None
    }

    /// Fetch and decompress Ubuntu `noble/main` `Packages.gz` (~1.8 MB) for `Provides:` / `Source:` resolution and fuzzy search
    fn load_debian_main_packages_index(&self, deb_arch: &str) -> Result<String, PreambleError> {
        let shared = Self::shared_index_cache_dir(&self.tmp_dir());
        let gz_cache = shared.join(format!("ubuntu_noble_main_{}_Packages.gz", deb_arch));
        if !gz_cache.is_file() {
            let url = if deb_arch == "amd64" {
                "http://archive.ubuntu.com/ubuntu/dists/noble/main/binary-amd64/Packages.gz".to_string()
            } else {
                format!(
                    "http://ports.ubuntu.com/ubuntu-ports/dists/noble/main/binary-{}/Packages.gz",
                    deb_arch
                )
            };
            self.download_file_with_headers(&url, &gz_cache, &[])?;
        }
        let raw_bytes = Self::decompress_to_tar_bytes(&gz_cache)?;
        Ok(String::from_utf8_lossy(&raw_bytes).into_owned())
    }

    /// Search Ubuntu `Packages` index by exact `Package:`, virtual `Provides:`, or `Source:` -> `*-dev`
    fn search_debian_packages_index(
        index_text: &str,
        raw_query: &str,
        candidates: &[String],
        deb_arch: &str,
    ) -> Option<(String, String, String)> {
        let lower_q = raw_query.to_lowercase();
        let no_lib_q = lower_q
            .strip_suffix("-dev")
            .unwrap_or(&lower_q)
            .strip_prefix("lib")
            .filter(|s| !s.is_empty())
            .unwrap_or(&lower_q);
        let base_mirror = if deb_arch == "amd64" {
            "http://archive.ubuntu.com/ubuntu"
        } else {
            "http://ports.ubuntu.com/ubuntu-ports"
        };

        let mut best: Option<(usize, String, String, String)> = None;
        for stanza in index_text.split("\n\n") {
            let mut pkg = "";
            let mut ver = "";
            let mut src = "";
            let mut provides = "";
            let mut filename = "";
            for line in stanza.lines() {
                if let Some(v) = line.strip_prefix("Package: ") {
                    pkg = v.trim();
                } else if let Some(v) = line.strip_prefix("Version: ") {
                    ver = v.trim();
                } else if let Some(v) = line.strip_prefix("Source: ") {
                    src = v.split_whitespace().next().unwrap_or("").trim();
                } else if let Some(v) = line.strip_prefix("Provides: ") {
                    provides = v.trim();
                } else if let Some(v) = line.strip_prefix("Filename: ") {
                    filename = v.trim();
                }
            }
            if pkg.is_empty() || filename.is_empty() {
                continue;
            }
            let pkg_lower = pkg.to_lowercase();
            let mut rank_opt: Option<usize> = candidates.iter().position(|c| *c == pkg_lower);

            if rank_opt.is_none() && !provides.is_empty() {
                for prov in provides.split(',') {
                    let p_name = prov.split('(').next().unwrap_or(prov).trim().to_lowercase();
                    if let Some(pos) = candidates.iter().position(|c| *c == p_name) {
                        rank_opt = Some(candidates.len() + pos);
                        break;
                    }
                }
            }
            if rank_opt.is_none()
                && pkg_lower.ends_with("-dev")
                && !pkg_lower.contains("dbg")
                && !pkg_lower.starts_with("lib32")
                && !pkg_lower.starts_with("libx32")
            {
                let eff_src = if src.is_empty() { pkg_lower.as_str() } else { src };
                if eff_src.eq_ignore_ascii_case(&lower_q) || eff_src.eq_ignore_ascii_case(no_lib_q) {
                    rank_opt = Some(candidates.len() * 2 + pkg_lower.len());
                }
            }

            if let Some(rank) = rank_opt {
                let clean_ver = ver.split_once(':').map(|(_, v)| v).unwrap_or(ver).to_string();
                let url = format!("{}/{}", base_mirror, filename.trim_start_matches('/'));
                if rank == 0 {
                    return Some((pkg.to_string(), clean_ver, url));
                }
                if best.as_ref().map(|b| rank < b.0).unwrap_or(true) {
                    best = Some((rank, pkg.to_string(), clean_ver, url));
                }
            }
        }
        best.map(|(_, p, v, u)| (p, v, u))
    }

    /// Debian / Ubuntu (`amd64` / `arm64` `.deb` archives via Launchpad API + `Packages.xz` metadata, rootlessly unpacking `ar` + `data.tar.*`)
    fn shim_c_debian(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let deb_arch = match self.platform.arch {
            TargetArch::X86_64 => "amd64",
            TargetArch::Aarch64 => "arm64",
        };
        let candidates = Self::c_package_name_candidates(name, CPackageProvider::DebianApt);

        let resolved_dev = self
            .query_ubuntu_deb_url(&candidates, deb_arch)
            .or_else(|| {
                let idx = self.load_debian_main_packages_index(deb_arch).ok()?;
                Self::search_debian_packages_index(&idx, name, &candidates, deb_arch)
            });

        let (dev_pkg, dev_ver, dev_url) = resolved_dev.ok_or_else(|| {
            PreambleError::PathNotFound(format!(
                "C development package '{}' not found in Debian/Ubuntu ({}) archive",
                name, deb_arch
            ))
        })?;

        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, dev_ver));
        if final_dir.join("omni-c-pkg.json").is_file() {
            return Ok(final_dir);
        }

        let deb_file = self.tmp_dir().join(format!("deb_{}_{}.deb", dev_pkg, deb_arch));
        self.download_file_with_headers(&dev_url, &deb_file, &[])?;

        let stage_dir = self.tmp_dir().join(format!("stage_deb_{}_{}", lower_req, std::process::id()));
        let _ = fs::remove_dir_all(&stage_dir);
        let _ = fs::create_dir_all(&stage_dir);
        let extract_res = Self::extract_deb_archive_with_deps(&deb_file, &stage_dir);
        let _ = fs::remove_file(&deb_file);
        let (mut symlinks, control_deps) = extract_res?;

        // Check if extracted `-dev` `.so` symlinks point to a companion runtime `.so.<major>` package
        let mut needs_runtime_so = false;
        let mut runtime_cands: Vec<String> = Vec::new();
        for (link_rel, target) in &symlinks {
            let link_name = link_rel.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let target_name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if link_name.ends_with(".so") && target_name.contains(".so.") {
                let exists_in_stage = walkdir::WalkDir::new(&stage_dir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_type().is_file() && e.file_name().to_string_lossy() == target_name);
                if !exists_in_stage {
                    needs_runtime_so = true;
                    // Prioritize exact companion packages declared in the `.deb` `control` `Depends:` header
                    for dep in &control_deps {
                        if dep != "libc6" && !dep.starts_with("libc6-") && !dep.ends_with("-dev") && !runtime_cands.contains(dep) {
                            runtime_cands.push(dep.clone());
                        }
                    }
                    if let Some((stem, rest)) = target_name.split_once(".so.") {
                        let major = rest.split('.').next().unwrap_or("");
                        if !major.is_empty() {
                            for c in [format!("{}{}", stem, major), format!("{}-{}", stem, major)] {
                                if !runtime_cands.contains(&c) {
                                    runtime_cands.push(c);
                                }
                            }
                        }
                    }
                }
            }
        }
        if needs_runtime_so && !runtime_cands.is_empty() {
            let rt_hit = self
                .load_debian_main_packages_index(deb_arch)
                .ok()
                .and_then(|idx| {
                    Self::search_debian_packages_index(&idx, &runtime_cands[0], &runtime_cands, deb_arch)
                })
                .or_else(|| self.query_ubuntu_deb_url(&runtime_cands, deb_arch));
            if let Some((rt_pkg, _, rt_url)) = rt_hit {
                let rt_deb = self.tmp_dir().join(format!("deb_rt_{}_{}.deb", rt_pkg, deb_arch));
                if self.download_file_with_headers(&rt_url, &rt_deb, &[]).is_ok() {
                    if let Ok((rt_links, _)) = Self::extract_deb_archive_with_deps(&rt_deb, &stage_dir) {
                        symlinks.extend(rt_links);
                    }
                    let _ = fs::remove_file(&rt_deb);
                }
            }
        }

        let meta = serde_json::json!({
            "name": lower_req,
            "upstream_package": dev_pkg,
            "version": dev_ver,
            "platform": self.platform.slug,
            "llvm_triple": self.platform.llvm_triple,
            "provider": "debian-apt",
            "source_url": dev_url,
        });
        Self::normalize_c_package_layout(&stage_dir, &final_dir, &symlinks, &meta)?;
        let _ = fs::remove_dir_all(&stage_dir);
        Ok(final_dir)
    }

    /// Return `(repo_id, base_url, eager)` list for RPM repositories (`RpmYum` = EL9 BaseOS/AppStream/CRB + lazy EPEL9; `RpmZypper` = openSUSE Tumbleweed OSS)
    fn rpm_repositories_for_platform(&self) -> Vec<(String, String, bool)> {
        let arch = self.platform.arch.as_str();
        match self.platform.provider {
            CPackageProvider::RpmZypper => {
                let base = match self.platform.arch {
                    TargetArch::X86_64 => "https://download.opensuse.org/tumbleweed/repo/oss".to_string(),
                    TargetArch::Aarch64 => "https://download.opensuse.org/ports/aarch64/tumbleweed/repo/oss".to_string(),
                };
                vec![("opensuse-oss".to_string(), base, true)]
            }
            _ => vec![
                (
                    "el9-appstream".to_string(),
                    format!("https://repo.almalinux.org/almalinux/9/AppStream/{}/kickstart", arch),
                    true,
                ),
                (
                    "el9-baseos".to_string(),
                    format!("https://repo.almalinux.org/almalinux/9/BaseOS/{}/kickstart", arch),
                    true,
                ),
                (
                    "el9-crb".to_string(),
                    format!("https://repo.almalinux.org/almalinux/9/CRB/{}/os", arch),
                    true,
                ),
                (
                    "epel9".to_string(),
                    format!("https://dl.fedoraproject.org/pub/epel/9/Everything/{}", arch),
                    false,
                ),
            ],
        }
    }

    fn extract_xml_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
        for quote in ['"', '\''] {
            let pat = format!("{}={}", attr, quote);
            if let Some(idx) = tag.find(&pat) {
                let rest = &tag[idx + pat.len()..];
                if let Some(end) = rest.find(quote) {
                    return Some(&rest[..end]);
                }
            }
        }
        None
    }

    fn extract_xml_tag_text<'a>(block: &'a str, tag_name: &str) -> Option<&'a str> {
        let open = format!("<{}>", tag_name);
        let close = format!("</{}>", tag_name);
        let start = block.find(&open)? + open.len();
        let end = block[start..].find(&close)? + start;
        Some(block[start..end].trim())
    }

    /// Download (or load from `~/.omnicache/tmp`) the decompressed `primary.xml` for an RPM `repomd.xml` repository
    fn load_rpm_primary_xml(&self, repo_id: &str, base_url: &str) -> Result<String, PreambleError> {
        let shared = Self::shared_index_cache_dir(&self.tmp_dir());
        let cache_marker = shared.join(format!("rpm_{}_{}_primary.bin", self.platform.slug, repo_id));
        if !cache_marker.is_file() {
            let repomd_url = format!("{}/repodata/repomd.xml", base_url.trim_end_matches('/'));
            let repomd = self.fetch_url_string(&repomd_url)?;
            let mut primary_href: Option<String> = None;
            for chunk in repomd.split("<data ").skip(1) {
                let Some(header_end) = chunk.find('>') else { continue };
                if Self::extract_xml_attr(&chunk[..header_end], "type") == Some("primary") {
                    let body = chunk.split("</data>").next().unwrap_or(chunk);
                    if let Some(loc_idx) = body.find("<location ") {
                        let loc_tag = body[loc_idx..].split('>').next().unwrap_or("");
                        if let Some(href) = Self::extract_xml_attr(loc_tag, "href") {
                            primary_href = Some(href.to_string());
                            break;
                        }
                    }
                }
            }
            let href = primary_href.ok_or_else(|| {
                PreambleError::PathNotFound(format!("No primary metadata location in {}", repomd_url))
            })?;
            let primary_url = format!("{}/{}", base_url.trim_end_matches('/'), href.trim_start_matches('/'));
            self.download_file_with_headers(&primary_url, &cache_marker, &[])?;
        }
        let xml_bytes = Self::decompress_to_tar_bytes(&cache_marker)?;
        Ok(String::from_utf8_lossy(&xml_bytes).into_owned())
    }

    /// Parse `<package type="rpm">` entries from `primary.xml`: returns `(name, version, href, sourcerpm, provides)`
    fn parse_rpm_primary_packages(
        xml: &str,
        target_arch: &str,
    ) -> Vec<(String, String, String, String, Vec<String>)> {
        let mut out = Vec::new();
        for pkg_block in xml.split("<package ").skip(1) {
            let block = pkg_block.split("</package>").next().unwrap_or(pkg_block);
            let Some(arch) = Self::extract_xml_tag_text(block, "arch") else { continue };
            if arch != target_arch && arch != "noarch" {
                continue;
            }
            let Some(name) = Self::extract_xml_tag_text(block, "name") else { continue };
            let mut ver_str = "latest".to_string();
            if let Some(v_idx) = block.find("<version ") {
                let v_tag = block[v_idx..].split('>').next().unwrap_or("");
                let ver = Self::extract_xml_attr(v_tag, "ver").unwrap_or("");
                let rel = Self::extract_xml_attr(v_tag, "rel").unwrap_or("");
                if !ver.is_empty() && !rel.is_empty() {
                    ver_str = format!("{}-{}", ver, rel);
                } else if !ver.is_empty() {
                    ver_str = ver.to_string();
                }
            }
            let mut href_str = String::new();
            if let Some(l_idx) = block.find("<location ") {
                let l_tag = block[l_idx..].split('>').next().unwrap_or("");
                if let Some(h) = Self::extract_xml_attr(l_tag, "href") {
                    href_str = h.to_string();
                }
            }
            if href_str.is_empty() {
                continue;
            }
            let sourcerpm = Self::extract_xml_tag_text(block, "rpm:sourcerpm")
                .unwrap_or("")
                .to_string();
            let mut provides = Vec::new();
            if let Some(p_start) = block.find("<rpm:provides>") {
                let p_rest = &block[p_start + "<rpm:provides>".len()..];
                if let Some(p_end) = p_rest.find("</rpm:provides>") {
                    let p_sec = &p_rest[..p_end];
                    for entry_chunk in p_sec.split("<rpm:entry ").skip(1) {
                        let e_tag = entry_chunk.split('>').next().unwrap_or("");
                        if let Some(p_name) = Self::extract_xml_attr(e_tag, "name") {
                            provides.push(p_name.to_lowercase());
                        }
                    }
                }
            }
            out.push((name.to_string(), ver_str, href_str, sourcerpm, provides));
        }
        out
    }

    /// Yum / DNF (`RpmYum`: RHEL/Fedora/Rocky/AlmaLinux/CentOS/Amazon Linux) & Zypper (`RpmZypper`: openSUSE/SLES)
    /// Rootlessly parses `repodata/repomd.xml` + `primary.xml.*` and extracts `.rpm` CPIO archives in pure Rust.
    fn shim_c_rpm(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let no_lib_req = lower_req
            .strip_suffix("-devel")
            .unwrap_or(&lower_req)
            .strip_prefix("lib")
            .filter(|s| !s.is_empty())
            .unwrap_or(&lower_req);
        let candidates = Self::c_package_name_candidates(name, self.platform.provider);
        let pc_targets = [
            format!("pkgconfig({})", lower_req),
            format!("pkgconfig({})", no_lib_req),
            format!("pkgconfig(lib{})", no_lib_req),
        ];

        let repos = self.rpm_repositories_for_platform();
        let arch_str = self.platform.arch.as_str();
        let mut loaded_repos: Vec<(String, Vec<(String, String, String, String, Vec<String>)>)> = Vec::new();
        let mut best_hit: Option<(usize, String, String, String, String)> = None; // (rank, base_url, pkg_name, ver, href, sourcerpm) -> store sourcerpm too
        let mut best_sourcerpm = String::new();

        for pass in 0..2 {
            for (_repo_id, base_url, eager) in &repos {
                if (pass == 0 && !*eager) || (pass == 1 && *eager) {
                    continue;
                }
                let Ok(xml) = self.load_rpm_primary_xml(_repo_id, base_url) else { continue };
                let entries = Self::parse_rpm_primary_packages(&xml, arch_str);
                for (pkg_name, pkg_ver, href, sourcerpm, provides) in &entries {
                    let pkg_lower = pkg_name.to_lowercase();
                    if pkg_lower.contains("-debug") || pkg_lower.ends_with("-doc") || pkg_lower.ends_with("-docs") {
                        continue;
                    }
                    let mut rank_opt = candidates.iter().position(|c| *c == pkg_lower);
                    if rank_opt.is_none() && provides.iter().any(|p| pc_targets.contains(p)) {
                        let devel_bonus = if pkg_lower.ends_with("-devel") { 0 } else { 10 };
                        rank_opt = Some(candidates.len() + devel_bonus);
                    }
                    if rank_opt.is_none() {
                        if let Some(pos) = candidates.iter().position(|c| provides.contains(c)) {
                            let devel_bonus = if pkg_lower.ends_with("-devel") { 0 } else { 10 };
                            rank_opt = Some(candidates.len() * 2 + devel_bonus + pos);
                        }
                    }
                    if let Some(rank) = rank_opt {
                        if best_hit.as_ref().map(|b| rank < b.0).unwrap_or(true) {
                            best_hit = Some((
                                rank,
                                base_url.clone(),
                                pkg_name.clone(),
                                pkg_ver.clone(),
                                href.clone(),
                            ));
                            best_sourcerpm = sourcerpm.clone();
                        }
                    }
                }
                loaded_repos.push((base_url.clone(), entries));
            }
            if best_hit.is_some() {
                break;
            }
        }

        let (_, primary_base_url, dev_pkg, dev_ver, dev_href) = best_hit.ok_or_else(|| {
            PreambleError::PathNotFound(format!(
                "C package '{}' not found in {} ({}) repositories",
                name,
                self.platform.provider.as_str(),
                arch_str
            ))
        })?;

        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, dev_ver));
        if final_dir.join("omni-c-pkg.json").is_file() {
            return Ok(final_dir);
        }

        let mut rpms_to_extract = vec![(
            dev_pkg.clone(),
            format!("{}/{}", primary_base_url.trim_end_matches('/'), dev_href.trim_start_matches('/')),
        )];

        // Also pull sibling runtime/static library RPMs built from the same `<rpm:sourcerpm>` (e.g. `lz4-libs`, `zlib`, `sqlite-libs`)
        if !best_sourcerpm.is_empty() {
            let stem = dev_pkg.strip_suffix("-devel").unwrap_or(&dev_pkg).to_lowercase();
            for (r_base, r_entries) in &loaded_repos {
                for (p_name, _, p_href, p_src, _) in r_entries {
                    if p_src == &best_sourcerpm && p_name != &dev_pkg {
                        let pl = p_name.to_lowercase();
                        let is_companion_lib = pl == stem
                            || pl == format!("{}-libs", stem)
                            || pl == format!("{}-static", stem)
                            || pl == format!("{}-devel-static", stem)
                            || (pl.starts_with("lib")
                                && !pl.contains("-debug")
                                && !pl.ends_with("-doc")
                                && !pl.ends_with("-devel"));
                        if is_companion_lib && !rpms_to_extract.iter().any(|(n, _)| n == p_name) {
                            rpms_to_extract.push((
                                p_name.clone(),
                                format!("{}/{}", r_base.trim_end_matches('/'), p_href.trim_start_matches('/')),
                            ));
                        }
                    }
                }
            }
        }

        let stage_dir = self.tmp_dir().join(format!("stage_rpm_{}_{}", lower_req, std::process::id()));
        let _ = fs::remove_dir_all(&stage_dir);
        let _ = fs::create_dir_all(&stage_dir);
        let mut all_symlinks = Vec::new();
        let primary_url = rpms_to_extract[0].1.clone();

        for (idx, (r_pkg, r_url)) in rpms_to_extract.iter().enumerate() {
            let rpm_file = self.tmp_dir().join(format!("rpm_{}_{}.rpm", r_pkg, arch_str));
            if let Err(e) = self.download_file_with_headers(r_url, &rpm_file, &[]) {
                if idx == 0 {
                    let _ = fs::remove_dir_all(&stage_dir);
                    return Err(e);
                }
                continue;
            }
            let links_res = Self::extract_rpm_archive(&rpm_file, &stage_dir);
            let _ = fs::remove_file(&rpm_file);
            match links_res {
                Ok(links) => all_symlinks.extend(links),
                Err(e) if idx == 0 => {
                    let _ = fs::remove_dir_all(&stage_dir);
                    return Err(e);
                }
                Err(_) => {}
            }
        }

        let meta = serde_json::json!({
            "name": lower_req,
            "upstream_package": dev_pkg,
            "version": dev_ver,
            "platform": self.platform.slug,
            "llvm_triple": self.platform.llvm_triple,
            "provider": self.platform.provider.as_str(),
            "source_url": primary_url,
        });
        Self::normalize_c_package_layout(&stage_dir, &final_dir, &all_symlinks, &meta)?;
        let _ = fs::remove_dir_all(&stage_dir);
        Ok(final_dir)
    }

    /// Load and parse Alpine Linux `APKINDEX.tar.gz` for `repo` (`main` or `community`)
    /// Returns `(pkgname, pkgver, origin, provides, depends)`
    fn load_alpine_apkindex(
        &self,
        repo: &str,
        arch_str: &str,
    ) -> Result<(String, Vec<(String, String, String, Vec<String>, Vec<String>)>), PreambleError> {
        let base_url = format!(
            "https://dl-cdn.alpinelinux.org/alpine/latest-stable/{}/{}",
            repo, arch_str
        );
        let shared = Self::shared_index_cache_dir(&self.tmp_dir());
        let idx_cache = shared.join(format!("alpine_{}_{}_APKINDEX.tar.gz", repo, arch_str));
        if !idx_cache.is_file() {
            let url = format!("{}/APKINDEX.tar.gz", base_url);
            self.download_file_with_headers(&url, &idx_cache, &[])?;
        }
        let raw_gz = fs::read(&idx_cache).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed reading APKINDEX {:?}: {}", idx_cache, e))
        })?;
        let mut index_text = String::new();
        for member_tar in Self::decompress_concatenated_gzip_members(&raw_gz, &idx_cache)? {
            let mut archive = tar::Archive::new(std::io::Cursor::new(member_tar));
            let Ok(entries) = archive.entries() else { continue };
            for entry_res in entries {
                let Ok(mut entry) = entry_res else { continue };
                let Ok(p) = entry.path() else { continue };
                if p.file_name().and_then(|s| s.to_str()) == Some("APKINDEX") {
                    let _ = std::io::Read::read_to_string(&mut entry, &mut index_text);
                    break;
                }
            }
            if !index_text.is_empty() {
                break;
            }
        }
        let mut out = Vec::new();
        for stanza in index_text.split("\n\n") {
            let mut pkg = String::new();
            let mut ver = String::new();
            let mut origin = String::new();
            let mut provides = Vec::new();
            let mut depends = Vec::new();
            for line in stanza.lines() {
                if let Some(v) = line.strip_prefix("P:") {
                    pkg = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("V:") {
                    ver = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("o:") {
                    origin = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("p:") {
                    for tok in v.split_whitespace() {
                        let clean = tok.split(['=', '<', '>', '~']).next().unwrap_or(tok).to_lowercase();
                        if !clean.is_empty() {
                            provides.push(clean);
                        }
                    }
                } else if let Some(v) = line.strip_prefix("D:") {
                    for tok in v.split_whitespace() {
                        if !tok.starts_with('!') && !tok.contains(':') {
                            let clean = tok.split(['=', '<', '>', '~']).next().unwrap_or(tok).to_string();
                            if !clean.is_empty() {
                                depends.push(clean);
                            }
                        }
                    }
                }
            }
            if !pkg.is_empty() && !ver.is_empty() {
                if origin.is_empty() {
                    origin = pkg.clone();
                }
                out.push((pkg, ver, origin, provides, depends));
            }
        }
        Ok((base_url, out))
    }

    /// Alpine Linux (`apk`: `x86_64` & `aarch64` `APKINDEX.tar.gz` + multi-member gzip `.apk` archives)
    fn shim_c_alpine(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let no_lib_req = lower_req
            .strip_suffix("-dev")
            .unwrap_or(&lower_req)
            .strip_prefix("lib")
            .filter(|s| !s.is_empty())
            .unwrap_or(&lower_req);
        let candidates = Self::c_package_name_candidates(name, CPackageProvider::AlpineApk);
        let pc_targets = [
            format!("pc:{}", lower_req),
            format!("pc:{}", no_lib_req),
            format!("pc:lib{}", no_lib_req),
        ];
        let arch_str = self.platform.arch.as_str();

        let mut all_repos = Vec::new();
        let mut best: Option<(usize, String, String, String, String, Vec<String>)> = None;
        for repo in &["main", "community"] {
            let Ok((base_url, entries)) = self.load_alpine_apkindex(repo, arch_str) else { continue };
            for (pkg, ver, origin, provides, depends) in &entries {
                let pl = pkg.to_lowercase();
                let mut rank_opt = candidates.iter().position(|c| *c == pl);
                if rank_opt.is_none() && provides.iter().any(|p| pc_targets.contains(p)) {
                    let bonus = if pl.ends_with("-dev") { 0 } else { 10 };
                    rank_opt = Some(candidates.len() + bonus);
                }
                if rank_opt.is_none() {
                    if let Some(pos) = candidates.iter().position(|c| provides.contains(c)) {
                        let bonus = if pl.ends_with("-dev") { 0 } else { 10 };
                        rank_opt = Some(candidates.len() * 2 + bonus + pos);
                    }
                }
                if let Some(rank) = rank_opt {
                    if best.as_ref().map(|b| rank < b.0).unwrap_or(true) {
                        best = Some((
                            rank,
                            base_url.clone(),
                            pkg.clone(),
                            ver.clone(),
                            origin.clone(),
                            depends.clone(),
                        ));
                    }
                }
            }
            all_repos.push((base_url, entries));
        }

        let (_, dev_base_url, dev_pkg, dev_ver, dev_origin, dev_deps) = best.ok_or_else(|| {
            PreambleError::PathNotFound(format!(
                "C package '{}' not found in Alpine Linux ({}) repositories",
                name, arch_str
            ))
        })?;

        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, dev_ver));
        if final_dir.join("omni-c-pkg.json").is_file() {
            return Ok(final_dir);
        }

        let mut apks_to_extract = vec![(
            dev_pkg.clone(),
            format!("{}/{}-{}.apk", dev_base_url, dev_pkg, dev_ver),
        )];

        // Pull companion `-libs` / `-static` packages sharing `o:<origin>` or listed in `D:`
        let origin_lower = dev_origin.to_lowercase();
        for (r_base, r_entries) in &all_repos {
            for (pkg, ver, origin, _, _) in r_entries {
                if pkg == &dev_pkg {
                    continue;
                }
                let pl = pkg.to_lowercase();
                let is_companion = (origin.eq_ignore_ascii_case(&origin_lower)
                    && (pl == origin_lower
                        || pl == format!("{}-libs", origin_lower)
                        || pl == format!("{}-static", origin_lower)
                        || pl.starts_with("lib")))
                    || (dev_deps.contains(pkg) && !pl.ends_with("-dev") && pl != "pkgconf");
                if is_companion && !apks_to_extract.iter().any(|(n, _)| n == pkg) {
                    apks_to_extract.push((pkg.clone(), format!("{}/{}-{}.apk", r_base, pkg, ver)));
                }
            }
        }

        let stage_dir = self.tmp_dir().join(format!("stage_apk_{}_{}", lower_req, std::process::id()));
        let _ = fs::remove_dir_all(&stage_dir);
        let _ = fs::create_dir_all(&stage_dir);
        let mut all_symlinks = Vec::new();
        let primary_url = apks_to_extract[0].1.clone();

        for (idx, (a_pkg, a_url)) in apks_to_extract.iter().enumerate() {
            let apk_file = self.tmp_dir().join(format!("apk_{}_{}.apk", a_pkg, arch_str));
            if let Err(e) = self.download_file_with_headers(a_url, &apk_file, &[]) {
                if idx == 0 {
                    let _ = fs::remove_dir_all(&stage_dir);
                    return Err(e);
                }
                continue;
            }
            let links_res = Self::extract_apk_archive(&apk_file, &stage_dir);
            let _ = fs::remove_file(&apk_file);
            match links_res {
                Ok(links) => all_symlinks.extend(links),
                Err(e) if idx == 0 => {
                    let _ = fs::remove_dir_all(&stage_dir);
                    return Err(e);
                }
                Err(_) => {}
            }
        }

        let meta = serde_json::json!({
            "name": lower_req,
            "upstream_package": dev_pkg,
            "version": dev_ver,
            "platform": self.platform.slug,
            "llvm_triple": self.platform.llvm_triple,
            "provider": "alpine-apk",
            "source_url": primary_url,
        });
        Self::normalize_c_package_layout(&stage_dir, &final_dir, &all_symlinks, &meta)?;
        let _ = fs::remove_dir_all(&stage_dir);
        Ok(final_dir)
    }

    /// macOS (`arm64` / `x86_64` Homebrew Bottles via `formulae.brew.sh` JSON API + official `Aliases/` + anonymous GHCR bearer token)
    fn shim_c_macos(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let mut candidates = Self::c_package_name_candidates(name, CPackageProvider::MacosHomebrew);

        let mut formula_json_opt: Option<(String, serde_json::Value)> = None;
        let mut idx = 0usize;
        while idx < candidates.len() {
            let cand = candidates[idx].clone();
            idx += 1;
            let api_url = format!("https://formulae.brew.sh/api/formula/{}.json", cand);
            if let Ok(resp) = self.fetch_url_string(&api_url) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                    if let Some(real_name) = json.get("name").and_then(|v| v.as_str()) {
                        formula_json_opt = Some((real_name.to_string(), json));
                        break;
                    }
                }
            }
            // Dynamically resolve official Homebrew `Aliases/<cand>` symlink target (e.g. `sqlite3` -> `sqlite`, `openssl` -> `openssl@4`)
            let alias_url = format!(
                "https://raw.githubusercontent.com/Homebrew/homebrew-core/HEAD/Aliases/{}",
                cand
            );
            if let Ok(alias_target) = self.fetch_url_string(&alias_url) {
                let target_fname = alias_target
                    .trim()
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(".rb")
                    .to_string();
                if !target_fname.is_empty() && !candidates.contains(&target_fname) {
                    candidates.push(target_fname);
                }
            }
        }

        let (formula_name, json) = formula_json_opt.ok_or_else(|| {
            PreambleError::PathNotFound(format!(
                "C formula '{}' not found in Homebrew formulae index",
                name
            ))
        })?;

        let pkg_ver = json
            .get("versions")
            .and_then(|v| v.get("stable"))
            .and_then(|v| v.as_str())
            .unwrap_or("latest");

        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, pkg_ver));
        if final_dir.join("omni-c-pkg.json").is_file() {
            return Ok(final_dir);
        }

        let files_obj = json
            .get("bottle")
            .and_then(|b| b.get("stable"))
            .and_then(|s| s.get("files"))
            .and_then(|f| f.as_object());

        let mut selected_bottle_url: Option<String> = None;
        if let Some(files) = files_obj {
            let preferred_tags: &[&str] = match self.platform.arch {
                TargetArch::Aarch64 => &["arm64_sequoia", "arm64_sonoma", "arm64_ventura", "arm64_monterey", "all"],
                TargetArch::X86_64 => &["sequoia", "sonoma", "ventura", "monterey", "big_sur", "all"],
            };
            for tag in preferred_tags {
                if let Some(u) = files.get(*tag).and_then(|v| v.get("url")).and_then(|v| v.as_str()) {
                    selected_bottle_url = Some(u.to_string());
                    break;
                }
            }
            if selected_bottle_url.is_none() {
                for (tag, val) in files {
                    let is_linux = tag.contains("linux");
                    let is_arm = tag.starts_with("arm64_");
                    let matches_arch = match self.platform.arch {
                        TargetArch::Aarch64 => !is_linux && (is_arm || tag == "all"),
                        TargetArch::X86_64 => !is_linux && (!is_arm || tag == "all"),
                    };
                    if matches_arch {
                        if let Some(u) = val.get("url").and_then(|v| v.as_str()) {
                            selected_bottle_url = Some(u.to_string());
                            break;
                        }
                    }
                }
            }
        }

        let stage_dir = self.tmp_dir().join(format!("stage_macos_{}_{}", lower_req, std::process::id()));
        let _ = fs::remove_dir_all(&stage_dir);
        let _ = fs::create_dir_all(&stage_dir);

        let (symlinks, src_url) = if let Some(bottle_url) = selected_bottle_url {
            let repo_scope = bottle_url
                .strip_prefix("https://ghcr.io/v2/")
                .and_then(|s| s.split_once("/blobs/"))
                .map(|(repo, _)| repo.to_string())
                .unwrap_or_else(|| format!("homebrew/core/{}", formula_name.replace('@', "/")));
            let token_url = format!(
                "https://ghcr.io/token?service=ghcr.io&scope=repository:{}:pull",
                repo_scope
            );
            let token_resp = self.fetch_url_string(&token_url)?;
            let token_json: serde_json::Value = serde_json::from_str(&token_resp).map_err(|e| {
                PreambleError::PathNotFound(format!("Invalid GHCR token response: {}", e))
            })?;
            let token = token_json
                .get("token")
                .and_then(|t| t.as_str())
                .ok_or_else(|| PreambleError::PathNotFound("Missing token in GHCR response".to_string()))?;

            let auth_val = format!("Bearer {}", token);
            let bottle_file = self.tmp_dir().join(format!("brew_{}_{}.tar.gz", lower_req, pkg_ver));
            self.download_file_with_headers(&bottle_url, &bottle_file, &[("Authorization", &auth_val)])?;
            let links_res = Self::extract_compressed_tar(&bottle_file, &stage_dir, 0);
            let _ = fs::remove_file(&bottle_file);
            (links_res?, bottle_url)
        } else if let Some(src_tar_url) = json
            .get("urls")
            .and_then(|u| u.get("stable"))
            .and_then(|s| s.get("url"))
            .and_then(|u| u.as_str())
        {
            let src_file = self.tmp_dir().join(format!("brew_src_{}_{}.tar", lower_req, pkg_ver));
            self.download_file_with_headers(src_tar_url, &src_file, &[])?;
            let links_res = Self::extract_compressed_tar(&src_file, &stage_dir, 1);
            let _ = fs::remove_file(&src_file);
            (links_res?, src_tar_url.to_string())
        } else {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(PreambleError::PathNotFound(format!(
                "No compatible bottle or source archive found in Homebrew formula '{}'",
                formula_name
            )));
        };

        let meta = serde_json::json!({
            "name": lower_req,
            "upstream_package": formula_name,
            "version": pkg_ver,
            "platform": self.platform.slug,
            "llvm_triple": self.platform.llvm_triple,
            "provider": "macos-homebrew",
            "source_url": src_url,
        });
        Self::normalize_c_package_layout(&stage_dir, &final_dir, &symlinks, &meta)?;
        let _ = fs::remove_dir_all(&stage_dir);
        Ok(final_dir)
    }

    /// Windows (`x86_64` MSYS2 UCRT64 & `aarch64` CLANGARM64 `.db` + `.pkg.tar.zst` rootless extractor)
    fn shim_c_windows(
        &self,
        name: &str,
        _version: Option<&str>,
        pkg_platform_dir: &Path,
    ) -> Result<PathBuf, PreambleError> {
        let lower_req = name.to_lowercase();
        let (msys_repo, pkg_prefix) = match self.platform.arch {
            TargetArch::X86_64 => ("ucrt64", "mingw-w64-ucrt-x86_64-"),
            TargetArch::Aarch64 => ("clangarm64", "mingw-w64-clang-aarch64-"),
        };

        let base_cands = Self::c_package_name_candidates(name, CPackageProvider::WindowsMsys2);
        let full_cands: Vec<String> = base_cands
            .iter()
            .map(|c| {
                if c.starts_with("mingw-w64-") {
                    c.clone()
                } else {
                    format!("{}{}", pkg_prefix, c)
                }
            })
            .collect();

        let shared_tmp = Self::shared_index_cache_dir(&self.tmp_dir());
        let _ = fs::create_dir_all(&shared_tmp);
        let db_path = shared_tmp.join(format!("msys2_{}.db", msys_repo));
        if !db_path.is_file() {
            let db_url = format!("https://repo.msys2.org/mingw/{}/{}.db", msys_repo, msys_repo);
            self.download_file_with_headers(&db_url, &db_path, &[])?;
        }

        let (pkgname, pkgver, filename) = Self::search_pacman_db(&db_path, &full_cands)?
            .ok_or_else(|| {
                PreambleError::PathNotFound(format!(
                    "C package '{}' not found in MSYS2 '{}' repository",
                    name, msys_repo
                ))
            })?;

        let final_dir = pkg_platform_dir.join(format!("{}-{}", lower_req, pkgver));
        if final_dir.join("omni-c-pkg.json").is_file() {
            return Ok(final_dir);
        }

        let encoded = filename.replace(':', "%3A").replace('+', "%2B");
        let pkg_url = format!("https://repo.msys2.org/mingw/{}/{}", msys_repo, encoded);
        let archive_path = self.tmp_dir().join(format!("msys2_{}", filename.replace(':', "_")));
        self.download_file_with_headers(&pkg_url, &archive_path, &[])?;

        let stage_dir = self.tmp_dir().join(format!("stage_win_{}_{}", lower_req, std::process::id()));
        let _ = fs::remove_dir_all(&stage_dir);
        let _ = fs::create_dir_all(&stage_dir);
        let symlinks_res = Self::extract_compressed_tar(&archive_path, &stage_dir, 0);
        let _ = fs::remove_file(&archive_path);
        let symlinks = symlinks_res?;

        let meta = serde_json::json!({
            "name": lower_req,
            "upstream_package": pkgname,
            "version": pkgver,
            "platform": self.platform.slug,
            "llvm_triple": self.platform.llvm_triple,
            "provider": "windows-msys2",
            "source_url": pkg_url,
        });
        Self::normalize_c_package_layout(&stage_dir, &final_dir, &symlinks, &meta)?;
        let _ = fs::remove_dir_all(&stage_dir);
        Ok(final_dir)
    }

    fn shared_index_cache_dir(fallback: &Path) -> PathBuf {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let p = PathBuf::from(home).join(".omnicache").join("tmp");
            if fs::create_dir_all(&p).is_ok() {
                return p;
            }
        }
        fallback.to_path_buf()
    }

    /// Parse all entries in a `pacman`-format `.db` archive: returns `(pkgname, pkgver, filename, provides)`
    fn list_pacman_db_entries(
        db_path: &Path,
    ) -> Result<Vec<(String, String, String, Vec<String>)>, PreambleError> {
        let tar_bytes = Self::decompress_to_tar_bytes(db_path)?;
        let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
        let mut entries_out = Vec::new();

        for entry_res in archive
            .entries()
            .map_err(|e| PreambleError::PathNotFound(format!("Corrupt pacman db {:?}: {}", db_path, e)))?
        {
            let Ok(mut entry) = entry_res else { continue };
            let Ok(path) = entry.path() else { continue };
            if path.file_name().and_then(|s| s.to_str()) != Some("desc") {
                continue;
            }
            let mut desc = String::new();
            if std::io::Read::read_to_string(&mut entry, &mut desc).is_err() {
                continue;
            }
            let mut pkg_name = String::new();
            let mut pkg_ver = String::new();
            let mut pkg_file = String::new();
            let mut provides = Vec::new();
            let mut section = "";
            for line in desc.lines() {
                let t = line.trim();
                if t.is_empty() {
                    section = "";
                } else if t.starts_with('%') && t.ends_with('%') {
                    section = t;
                } else {
                    match section {
                        "%NAME%" if pkg_name.is_empty() => pkg_name = t.to_string(),
                        "%VERSION%" if pkg_ver.is_empty() => pkg_ver = t.to_string(),
                        "%FILENAME%" if pkg_file.is_empty() => pkg_file = t.to_string(),
                        "%PROVIDES%" => {
                            let p = t.split(['=', '<', '>']).next().unwrap_or(t).trim().to_lowercase();
                            if !p.is_empty() {
                                provides.push(p);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !pkg_name.is_empty() && !pkg_file.is_empty() {
                entries_out.push((pkg_name, pkg_ver, pkg_file, provides));
            }
        }
        Ok(entries_out)
    }

    /// Search a `pacman`-format `.db` archive by `%NAME%` and `%PROVIDES%`, returning `(pkgname, pkgver, filename)`.
    fn search_pacman_db(
        db_path: &Path,
        candidates: &[String],
    ) -> Result<Option<(String, String, String)>, PreambleError> {
        let all = Self::list_pacman_db_entries(db_path)?;
        let lower_cands: Vec<String> = candidates.iter().map(|c| c.to_lowercase()).collect();
        let mut matches: Vec<(usize, String, String, String)> = Vec::new();

        for (pkg_name, pkg_ver, pkg_file, provides) in all {
            let name_lower = pkg_name.to_lowercase();
            if let Some(rank) = lower_cands.iter().position(|c| *c == name_lower) {
                if rank == 0 {
                    return Ok(Some((pkg_name, pkg_ver, pkg_file)));
                }
                matches.push((rank, pkg_name, pkg_ver, pkg_file));
            } else if let Some(pos) = lower_cands.iter().position(|c| provides.contains(c)) {
                matches.push((lower_cands.len() + pos, pkg_name, pkg_ver, pkg_file));
            }
        }
        matches.sort_by_key(|m| m.0);
        Ok(matches.into_iter().next().map(|(_, n, v, f)| (n, v, f)))
    }

    /// Standard Wagner-Fischer Levenshtein edit distance for fuzzy package recommendation
    pub fn levenshtein_distance(a: &str, b: &str) -> usize {
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        if a_chars.is_empty() {
            return b_chars.len();
        }
        if b_chars.is_empty() {
            return a_chars.len();
        }
        let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
        let mut curr = vec![0usize; b_chars.len() + 1];
        for (i, &ca) in a_chars.iter().enumerate() {
            curr[0] = i + 1;
            for (j, &cb) in b_chars.iter().enumerate() {
                let cost = usize::from(ca != cb);
                curr[j + 1] = (prev[j + 1] + 1)
                    .min(curr[j] + 1)
                    .min(prev[j] + cost);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[b_chars.len()]
    }

    fn is_subsequence(needle: &str, haystack: &str) -> bool {
        let mut it = haystack.chars();
        needle.chars().all(|nc| it.any(|hc| hc == nc))
    }

    /// Rank a pool of `(package_name, version)` pairs against `query`, returning the top `limit` closest hits
    pub fn rank_c_candidates(
        query: &str,
        pool: Vec<(String, String)>,
        prefer_dev_suffix: bool,
        limit: usize,
    ) -> Vec<(String, String)> {
        let with_empty_origin = pool.into_iter().map(|(n, v)| (n, v, String::new())).collect();
        Self::rank_c_candidates_with_origin(query, with_empty_origin, prefer_dev_suffix, limit)
    }

    fn rank_c_candidates_with_origin(
        query: &str,
        pool: Vec<(String, String, String)>,
        prefer_dev_suffix: bool,
        limit: usize,
    ) -> Vec<(String, String)> {
        let q_lower = query
            .trim()
            .trim_end_matches(".h")
            .trim_end_matches(".hpp")
            .to_lowercase();
        let q_stem = q_lower
            .strip_suffix("-devel")
            .or_else(|| q_lower.strip_suffix("-dev"))
            .unwrap_or(&q_lower);
        let q_stem = q_stem
            .strip_prefix("lib")
            .filter(|s| !s.is_empty())
            .unwrap_or(q_stem);

        let mut scored: Vec<((usize, usize, usize, usize, String), String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (pkg_name, pkg_ver, origin_hint) in pool {
            let pl = pkg_name.to_lowercase();
            if pl.contains("-debug")
                || pl.ends_with("-doc")
                || pl.ends_with("-docs")
                || pl.ends_with("-man")
                || pl.starts_with("lib32")
                || pl.starts_with("libx32")
                || !seen.insert(pl.clone())
            {
                continue;
            }
            let stripped_msys = pl
                .strip_prefix("mingw-w64-ucrt-x86_64-")
                .or_else(|| pl.strip_prefix("mingw-w64-clang-aarch64-"))
                .unwrap_or(&pl);
            let is_dev = stripped_msys.ends_with("-dev") || stripped_msys.ends_with("-devel");
            let stem_s1 = stripped_msys
                .strip_suffix("-devel")
                .or_else(|| stripped_msys.strip_suffix("-dev"))
                .unwrap_or(stripped_msys);
            let p_stem = stem_s1
                .strip_prefix("lib")
                .filter(|s| !s.is_empty())
                .unwrap_or(stem_s1);
            let orig_lower = origin_hint.to_lowercase();
            let orig_stem = orig_lower
                .strip_prefix("lib")
                .filter(|s| !s.is_empty())
                .unwrap_or(&orig_lower);

            let d_stem = Self::levenshtein_distance(q_stem, p_stem);
            let d_raw = Self::levenshtein_distance(&q_lower, stripped_msys);
            let d_orig = if !orig_stem.is_empty() {
                Self::levenshtein_distance(q_stem, orig_stem)
            } else {
                usize::MAX
            };
            let dist = d_stem.min(d_raw).min(d_orig);
            let eff_stem = if d_orig < d_stem && !orig_stem.is_empty() {
                orig_stem
            } else {
                p_stem
            };
            let prefix_or_sub = (q_stem.len() >= 2 && eff_stem.starts_with(q_stem))
                || (eff_stem.len() >= 2 && q_stem.starts_with(eff_stem))
                || (q_stem.len() >= 3 && eff_stem.contains(q_stem))
                || (q_stem.len() >= 3 && Self::is_subsequence(q_stem, eff_stem));

            let max_allowed = q_stem.len().max(3);
            if dist > max_allowed && !prefix_or_sub {
                continue;
            }

            let tier = if dist <= 1 || (q_stem.len() >= 3 && eff_stem.starts_with(q_stem)) {
                0
            } else if dist <= 2 || prefix_or_sub {
                1
            } else {
                2
            };
            let dev_penalty = if prefer_dev_suffix && !is_dev { 1 } else { 0 };
            let len_diff = eff_stem.len().abs_diff(q_stem.len());
            scored.push(((tier, dev_penalty, dist, len_diff, pl), pkg_name, pkg_ver));
        }

        scored.sort_by(|a, b| a.0.cmp(&b.0));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, name, ver)| (name, ver))
            .collect()
    }

    /// Query the active platform's upstream C package index and return the top `limit` closest `(name, version)` hits
    pub fn suggest_c_packages(&self, query: &str, limit: usize) -> Vec<(String, String)> {
        let clean = query
            .trim()
            .trim_end_matches(".h")
            .trim_end_matches(".hpp")
            .to_lowercase();
        let stem = clean
            .strip_suffix("-devel")
            .or_else(|| clean.strip_suffix("-dev"))
            .unwrap_or(&clean);
        let stem = stem.strip_prefix("lib").filter(|s| !s.is_empty()).unwrap_or(stem);
        if stem.is_empty() {
            return Vec::new();
        }

        let mut pool: Vec<(String, String, String)> = Vec::new();
        let mut prefer_dev = false;

        match self.platform.provider {
            CPackageProvider::ArchPacman => {
                if self.platform.arch == TargetArch::X86_64 {
                    let chars: Vec<char> = stem.chars().collect();
                    let mut prefixes = vec![stem.to_string()];
                    if chars.len() > 2 {
                        prefixes.push(chars[..chars.len() - 1].iter().collect());
                    }
                    if chars.len() > 3 {
                        prefixes.push(chars[..3].iter().collect());
                    }
                    if chars.len() >= 2 {
                        prefixes.push(chars[..2].iter().collect());
                    }
                    for pref in prefixes {
                        let url = format!("https://archlinux.org/packages/search/json/?q={}", pref);
                        if let Ok(resp) = self.fetch_url_string(&url) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                                if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                                    for item in results {
                                        let arch = item.get("arch").and_then(|v| v.as_str()).unwrap_or("");
                                        let repo = item.get("repo").and_then(|v| v.as_str()).unwrap_or("");
                                        let repo_lower = repo.to_lowercase();
                                        if (arch == "x86_64" || arch == "any")
                                            && !repo_lower.contains("testing")
                                            && !repo_lower.contains("multilib")
                                        {
                                            if let Some(n) = item.get("pkgname").and_then(|v| v.as_str()) {
                                                let v = item.get("pkgver").and_then(|v| v.as_str()).unwrap_or("latest");
                                                let base = item.get("pkgbase").and_then(|v| v.as_str()).unwrap_or("");
                                                pool.push((n.to_string(), v.to_string(), base.to_string()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if pool.len() >= 15 {
                            break;
                        }
                    }
                } else {
                    for repo in &["core", "extra"] {
                        let db = Self::shared_index_cache_dir(&self.tmp_dir())
                            .join(format!("archarm_aarch64_{}.db", repo));
                        if let Ok(entries) = Self::list_pacman_db_entries(&db) {
                            for (n, v, _, _) in entries {
                                pool.push((n, v, String::new()));
                            }
                        }
                    }
                }
            }
            CPackageProvider::DebianApt => {
                prefer_dev = true;
                let deb_arch = match self.platform.arch {
                    TargetArch::X86_64 => "amd64",
                    TargetArch::Aarch64 => "arm64",
                };
                if let Ok(idx) = self.load_debian_main_packages_index(deb_arch) {
                    for stanza in idx.split("\n\n") {
                        let mut p = "";
                        let mut v = "";
                        let mut s = "";
                        for line in stanza.lines() {
                            if let Some(val) = line.strip_prefix("Package: ") {
                                p = val.trim();
                            } else if let Some(val) = line.strip_prefix("Version: ") {
                                v = val.trim();
                            } else if let Some(val) = line.strip_prefix("Source: ") {
                                s = val.split_whitespace().next().unwrap_or("").trim();
                            }
                        }
                        if !p.is_empty() && p.ends_with("-dev") {
                            let clean_v = v.split_once(':').map(|(_, sv)| sv).unwrap_or(v);
                            pool.push((p.to_string(), clean_v.to_string(), s.to_string()));
                        }
                    }
                }
            }
            CPackageProvider::RpmYum | CPackageProvider::RpmZypper => {
                prefer_dev = true;
                let arch_str = self.platform.arch.as_str();
                for (repo_id, base_url, eager) in self.rpm_repositories_for_platform() {
                    let shared = Self::shared_index_cache_dir(&self.tmp_dir());
                    let cached_bin = shared.join(format!("rpm_{}_{}_primary.bin", self.platform.slug, repo_id));
                    if !eager && !cached_bin.is_file() {
                        continue;
                    }
                    if let Ok(xml) = self.load_rpm_primary_xml(&repo_id, &base_url) {
                        for (n, v, _, src_rpm, _) in Self::parse_rpm_primary_packages(&xml, arch_str) {
                            let src_stem = src_rpm.split('-').next().unwrap_or("").to_string();
                            pool.push((n, v, src_stem));
                        }
                    }
                }
            }
            CPackageProvider::AlpineApk => {
                prefer_dev = true;
                let arch_str = self.platform.arch.as_str();
                for repo in &["main", "community"] {
                    if let Ok((_, entries)) = self.load_alpine_apkindex(repo, arch_str) {
                        for (n, v, origin, _, _) in entries {
                            pool.push((n, v, origin));
                        }
                    }
                }
            }
            CPackageProvider::MacosHomebrew => {
                let first_ch: String = stem.chars().take(1).collect();
                for dir in [first_ch.as_str(), "lib"] {
                    if dir.is_empty() {
                        continue;
                    }
                    let url = format!(
                        "https://api.github.com/repos/Homebrew/homebrew-core/contents/Formula/{}",
                        dir
                    );
                    if let Ok(resp) = self.fetch_url_string(&url) {
                        if let Ok(arr) = serde_json::from_str::<serde_json::Value>(&resp) {
                            if let Some(items) = arr.as_array() {
                                for item in items {
                                    if let Some(fname) = item.get("name").and_then(|v| v.as_str()) {
                                        if let Some(f_name) = fname.strip_suffix(".rb") {
                                            pool.push((f_name.to_string(), "latest".to_string(), String::new()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            CPackageProvider::WindowsMsys2 => {
                let msys_repo = match self.platform.arch {
                    TargetArch::X86_64 => "ucrt64",
                    TargetArch::Aarch64 => "clangarm64",
                };
                let db = Self::shared_index_cache_dir(&self.tmp_dir())
                    .join(format!("msys2_{}.db", msys_repo));
                if let Ok(entries) = Self::list_pacman_db_entries(&db) {
                    for (n, v, _, _) in entries {
                        let short = n
                            .strip_prefix("mingw-w64-ucrt-x86_64-")
                            .or_else(|| n.strip_prefix("mingw-w64-clang-aarch64-"))
                            .unwrap_or(&n)
                            .to_string();
                        pool.push((short, v, String::new()));
                    }
                }
            }
        }

        Self::rank_c_candidates_with_origin(query, pool, prefer_dev, limit)
    }

    /// Normalize an unpacked OS package staging tree into OmniScript's canonical `oenv` C package layout:
    /// - `<final_pkg_dir>/include/**` (all headers, flattening Debian multiarch subdirs when safe)
    /// - `<final_pkg_dir>/lib/*` (static `.a`/`.lib`, import `.dll.a`, shared `.so*`/`.dylib`, with SONAME symlinks)
    /// - `<final_pkg_dir>/bin/*.dll` (Windows runtime DLLs)
    /// - `<final_pkg_dir>/omni-c-pkg.json` (provenance manifest)
    fn normalize_c_package_layout(
        raw_stage_dir: &Path,
        final_pkg_dir: &Path,
        symlinks: &[(PathBuf, PathBuf)],
        meta: &serde_json::Value,
    ) -> Result<(), PreambleError> {
        let inc_out = final_pkg_dir.join("include");
        let lib_out = final_pkg_dir.join("lib");
        let bin_out = final_pkg_dir.join("bin");
        let _ = fs::create_dir_all(&inc_out);
        let _ = fs::create_dir_all(&lib_out);

        let is_lib_filename = |fname: &str| -> bool {
            let lower = fname.to_lowercase();
            lower.ends_with(".a")
                || lower.ends_with(".lib")
                || lower.ends_with(".dylib")
                || lower.ends_with(".so")
                || lower.contains(".so.")
                || (lower.starts_with("lib") && lower.ends_with(".dll"))
        };

        for entry in walkdir::WalkDir::new(raw_stage_dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let full_p = entry.path();
            let Ok(rel) = full_p.strip_prefix(raw_stage_dir) else { continue };
            let comps: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            let fname = entry.file_name().to_string_lossy().to_string();

            // 1. Headers under `.../include/...` (or root `.h`/`.hpp` for source tarballs)
            if let Some(inc_idx) = comps.iter().position(|c| c == "include") {
                if inc_idx + 1 < comps.len() {
                    let sub_rel: PathBuf = comps[inc_idx + 1..].iter().collect();
                    let dest = inc_out.join(&sub_rel);
                    if let Some(parent) = dest.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::copy(full_p, &dest);

                    // Flatten Debian/Ubuntu multiarch header subdir (`include/x86_64-linux-gnu/foo.h` -> `include/foo.h`)
                    let first_sub = &comps[inc_idx + 1];
                    if (first_sub.ends_with("-linux-gnu") || first_sub.ends_with("-linux-musl"))
                        && inc_idx + 2 < comps.len()
                    {
                        let flat_rel: PathBuf = comps[inc_idx + 2..].iter().collect();
                        let flat_dest = inc_out.join(&flat_rel);
                        if !flat_dest.exists() {
                            if let Some(parent) = flat_dest.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            let _ = fs::copy(full_p, &flat_dest);
                        }
                    }
                    continue;
                }
            } else if matches!(
                full_p.extension().and_then(|e| e.to_str()),
                Some("h" | "hpp")
            ) && comps.len() <= 2
            {
                let _ = fs::copy(full_p, inc_out.join(&fname));
            }

            // 2. Libraries under `.../lib...`
            if comps.iter().any(|c| c == "lib" || c == "lib64") {
                if comps.iter().any(|c| c == "pkgconfig") && fname.ends_with(".pc") {
                    let pc_dir = lib_out.join("pkgconfig");
                    let _ = fs::create_dir_all(&pc_dir);
                    let _ = fs::copy(full_p, pc_dir.join(&fname));
                } else if is_lib_filename(&fname) {
                    let _ = fs::copy(full_p, lib_out.join(&fname));
                }
            }

            // 3. Windows runtime `.dll` binaries under `.../bin/...`
            if fname.to_lowercase().ends_with(".dll")
                && comps.iter().any(|c| c == "bin" || c == "lib")
            {
                let _ = fs::create_dir_all(&bin_out);
                let _ = fs::copy(full_p, bin_out.join(&fname));
            }
        }

        // 4. Reconstruct library SONAME / linker symlinks (`.so` -> `.so.1` -> `.so.1.x`, `.dylib` -> `.1.dylib`)
        // Run 3 passes so chained symlinks resolve regardless of archive entry ordering.
        for _pass in 0..3 {
            for (link_rel, target) in symlinks {
                let comps: Vec<String> = link_rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();
                let Some(link_fname) = link_rel.file_name().and_then(|s| s.to_str()) else { continue };
                let Some(target_fname) = target.file_name().and_then(|s| s.to_str()) else { continue };

                if comps.iter().any(|c| c == "lib" || c == "lib64") && is_lib_filename(link_fname) {
                    let link_dest = lib_out.join(link_fname);
                    let mut effective_target = target_fname.to_string();
                    if !lib_out.join(&effective_target).exists() {
                        if let Some((stem, rest)) = target_fname.split_once(".so.") {
                            let major = rest.split('.').next().unwrap_or("");
                            let soname = format!("{}.so.{}", stem, major);
                            if !major.is_empty() && lib_out.join(&soname).exists() {
                                effective_target = soname;
                            }
                        }
                    }
                    let target_in_lib = lib_out.join(&effective_target);
                    if !link_dest.exists() && target_in_lib.exists() && link_fname != effective_target {
                        #[cfg(unix)]
                        {
                            let _ = std::os::unix::fs::symlink(&effective_target, &link_dest);
                        }
                        #[cfg(not(unix))]
                        {
                            let _ = fs::copy(&target_in_lib, &link_dest);
                        }
                    }
                } else if let Some(inc_idx) = comps.iter().position(|c| c == "include") {
                    if inc_idx + 1 < comps.len() {
                        let sub_rel: PathBuf = comps[inc_idx + 1..].iter().collect();
                        let link_dest = inc_out.join(&sub_rel);
                        if !link_dest.exists() {
                            if let Some(parent_in_stage) = raw_stage_dir.join(link_rel).parent() {
                                let cand_src = parent_in_stage.join(target);
                                if cand_src.is_file() {
                                    if let Some(p) = link_dest.parent() {
                                        let _ = fs::create_dir_all(p);
                                    }
                                    let _ = fs::copy(&cand_src, &link_dest);
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Ok(manifest_json) = serde_json::to_string_pretty(meta) {
            let _ = fs::write(final_pkg_dir.join("omni-c-pkg.json"), manifest_json);
        }
        Ok(())
    }

    /// Pure-Rust magic-byte decompressor supporting `.zst` (Zstandard), `.gz` (Gzip), `.xz` (LZMA2), and raw `.tar`
    pub fn decompress_to_tar_bytes(archive_path: &Path) -> Result<Vec<u8>, PreambleError> {
        let raw = fs::read(archive_path).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed reading archive {:?}: {}", archive_path, e))
        })?;
        Self::decompress_slice_to_tar_bytes(&raw, archive_path)
    }

    fn decompress_slice_to_tar_bytes(raw: &[u8], ctx_path: &Path) -> Result<Vec<u8>, PreambleError> {
        use std::io::Read;
        if raw.len() >= 4 && raw[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
            // Zstandard (`.zst`)
            let mut cursor = std::io::Cursor::new(raw);
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(&mut cursor).map_err(|e| {
                PreambleError::PathNotFound(format!("Zstd decode init error in {:?}: {}", ctx_path, e))
            })?;
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map_err(|e| {
                PreambleError::PathNotFound(format!("Zstd decode stream error in {:?}: {}", ctx_path, e))
            })?;
            Ok(out)
        } else if raw.len() >= 2 && raw[..2] == [0x1F, 0x8B] {
            // Gzip (`.gz` / `.tgz`)
            let mut decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(raw));
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map_err(|e| {
                PreambleError::PathNotFound(format!("Gzip decode error in {:?}: {}", ctx_path, e))
            })?;
            Ok(out)
        } else if raw.len() >= 6 && raw[..6] == [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
            // XZ / LZMA2 (`.xz`)
            let mut reader = std::io::BufReader::new(std::io::Cursor::new(raw));
            let mut out = Vec::new();
            lzma_rs::xz_decompress(&mut reader, &mut out).map_err(|e| {
                PreambleError::PathNotFound(format!("XZ decode error in {:?}: {:?}", ctx_path, e))
            })?;
            Ok(out)
        } else {
            // Uncompressed `.tar`
            Ok(raw.to_vec())
        }
    }

    /// Extract a compressed tar archive (`.pkg.tar.zst`, `.tar.gz`, `.tar.xz`, or `.tar`) safely into `target_dir`,
    /// returning all `(link_rel_path, link_target)` symlink/hardlink pairs for cross-platform post-processing.
    fn extract_compressed_tar(
        archive_path: &Path,
        target_dir: &Path,
        strip_components: usize,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let tar_bytes = Self::decompress_to_tar_bytes(archive_path)?;
        Self::extract_tar_bytes(&tar_bytes, target_dir, strip_components)
    }

    fn extract_tar_bytes(
        tar_bytes: &[u8],
        target_dir: &Path,
        strip_components: usize,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
        let mut symlinks: Vec<(PathBuf, PathBuf)> = Vec::new();

        for entry_res in archive
            .entries()
            .map_err(|e| PreambleError::PathNotFound(format!("Failed reading tar entries: {}", e)))?
        {
            let mut entry = entry_res
                .map_err(|e| PreambleError::PathNotFound(format!("Corrupt tar entry: {}", e)))?;
            let path = entry
                .path()
                .map_err(|e| PreambleError::PathNotFound(format!("Corrupt entry path: {}", e)))?;
            let components: Vec<_> = path
                .components()
                .filter(|c| !matches!(c, std::path::Component::CurDir | std::path::Component::RootDir))
                .collect();
            if components.len() <= strip_components {
                continue;
            }
            // Mitigate Tar Slip: reject any entry containing ParentDir (..)
            if components
                .iter()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }
            let stripped_path: PathBuf = components[strip_components..]
                .iter()
                .map(|c| c.as_os_str())
                .collect();
            if stripped_path.as_os_str().is_empty() {
                continue;
            }
            // Skip pacman metadata dot-files at archive root (`.PKGINFO`, `.BUILDINFO`, `.MTREE`, `.INSTALL`)
            if stripped_path.components().count() == 1 {
                let root_name = stripped_path.to_string_lossy();
                if root_name.starts_with('.') {
                    continue;
                }
            }
            let dest = target_dir.join(&stripped_path);
            if !dest.starts_with(target_dir) {
                continue;
            }

            let entry_type = entry.header().entry_type();
            if entry_type.is_dir() {
                let _ = fs::create_dir_all(&dest);
            } else if entry_type.is_symlink() || entry_type.is_hard_link() {
                if let Ok(Some(link_target)) = entry.link_name() {
                    symlinks.push((stripped_path, link_target.into_owned()));
                }
            } else if entry_type.is_file() {
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let mut out_file = fs::File::create(&dest).map_err(|e| {
                    PreambleError::PathNotFound(format!("Failed to create file {:?}: {}", dest, e))
                })?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| {
                    PreambleError::PathNotFound(format!("Failed to extract file {:?}: {}", dest, e))
                })?;
            }
        }
        Ok(symlinks)
    }

    /// Pure-Rust Debian `.deb` (`!<arch>\n` Unix `ar` archive) extractor unpacking `data.tar.{zst,xz,gz}`
    pub fn extract_deb_archive(
        deb_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let (symlinks, _) = Self::extract_deb_archive_with_deps(deb_path, target_dir)?;
        Ok(symlinks)
    }

    /// Pure-Rust Debian `.deb` extractor returning `(symlinks, control_depends_packages)`
    fn extract_deb_archive_with_deps(
        deb_path: &Path,
        target_dir: &Path,
    ) -> Result<(Vec<(PathBuf, PathBuf)>, Vec<String>), PreambleError> {
        let bytes = fs::read(deb_path).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed reading .deb {:?}: {}", deb_path, e))
        })?;
        if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
            return Err(PreambleError::PathNotFound(format!(
                "Invalid .deb ar magic header in {:?}",
                deb_path
            )));
        }

        let mut depends_pkgs: Vec<String> = Vec::new();
        let mut extracted_links: Option<Vec<(PathBuf, PathBuf)>> = None;
        let mut offset = 8usize;

        while offset + 60 <= bytes.len() {
            let hdr = &bytes[offset..offset + 60];
            if &hdr[58..60] != b"`\n" {
                break;
            }
            let raw_name = String::from_utf8_lossy(&hdr[0..16]);
            let member_name = raw_name.trim().trim_end_matches('/');
            let size_str = String::from_utf8_lossy(&hdr[48..58]);
            let member_size: usize = size_str.trim().parse().unwrap_or(0);
            let data_start = offset + 60;
            let data_end = (data_start + member_size).min(bytes.len());
            let payload = &bytes[data_start..data_end];

            if member_name.starts_with("control.tar") {
                if let Ok(ctrl_tar) = Self::decompress_slice_to_tar_bytes(payload, deb_path) {
                    let mut ar = tar::Archive::new(std::io::Cursor::new(ctrl_tar));
                    if let Ok(entries) = ar.entries() {
                        for entry_res in entries {
                            let Ok(mut entry) = entry_res else { continue };
                            let Ok(p) = entry.path() else { continue };
                            if p.file_name().and_then(|s| s.to_str()) == Some("control") {
                                let mut ctrl_txt = String::new();
                                if std::io::Read::read_to_string(&mut entry, &mut ctrl_txt).is_ok() {
                                    for line in ctrl_txt.lines() {
                                        if let Some(deps) = line.strip_prefix("Depends:") {
                                            for item in deps.split(',') {
                                                let first_alt = item.split('|').next().unwrap_or(item);
                                                let pkg_tok = first_alt
                                                    .split('(')
                                                    .next()
                                                    .unwrap_or(first_alt)
                                                    .split(':')
                                                    .next()
                                                    .unwrap_or("")
                                                    .trim();
                                                if !pkg_tok.is_empty() && !depends_pkgs.iter().any(|d| d == pkg_tok) {
                                                    depends_pkgs.push(pkg_tok.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            } else if member_name.starts_with("data.tar") {
                let tar_bytes = Self::decompress_slice_to_tar_bytes(payload, deb_path)?;
                extracted_links = Some(Self::extract_tar_bytes(&tar_bytes, target_dir, 0)?);
            }

            offset = data_end + (member_size % 2);
        }

        match extracted_links {
            Some(links) => Ok((links, depends_pkgs)),
            None => Err(PreambleError::PathNotFound(format!(
                "No data.tar.* member found inside Debian archive {:?}",
                deb_path
            ))),
        }
    }

    /// Pure-Rust RPM (`.rpm` v3/v4 binary lead + signature/main header + compressed SVR4 `070701`/`070702` CPIO) extractor
    pub fn extract_rpm_archive(
        rpm_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let bytes = fs::read(rpm_path).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed reading .rpm {:?}: {}", rpm_path, e))
        })?;
        if bytes.len() < 112 || bytes[..4] != [0xED, 0xAB, 0xEE, 0xDB] {
            return Err(PreambleError::PathNotFound(format!(
                "Invalid RPM lead magic in {:?}",
                rpm_path
            )));
        }

        // Skip 96-byte Lead -> Signature Header (padded to 8-byte boundary) -> Main Header (unpadded)
        let parse_rpm_header_len = |pos: usize| -> Result<usize, PreambleError> {
            if pos + 16 > bytes.len() || bytes[pos..pos + 4] != [0x8E, 0xAD, 0xE8, 0x01] {
                return Err(PreambleError::PathNotFound(format!(
                    "Invalid RPM header magic at offset {} in {:?}",
                    pos, rpm_path
                )));
            }
            let nindex = u32::from_be_bytes([bytes[pos + 8], bytes[pos + 9], bytes[pos + 10], bytes[pos + 11]]) as usize;
            let hsize = u32::from_be_bytes([bytes[pos + 12], bytes[pos + 13], bytes[pos + 14], bytes[pos + 15]]) as usize;
            Ok(16 + nindex * 16 + hsize)
        };

        let sig_len = parse_rpm_header_len(96)?;
        let main_hdr_pos = (96 + sig_len + 7) & !7;
        let main_len = parse_rpm_header_len(main_hdr_pos)?;
        let payload_pos = main_hdr_pos + main_len;
        if payload_pos >= bytes.len() {
            return Err(PreambleError::PathNotFound(format!(
                "Truncated RPM payload in {:?}",
                rpm_path
            )));
        }

        let cpio_bytes = Self::decompress_slice_to_tar_bytes(&bytes[payload_pos..], rpm_path)?;
        Self::extract_cpio_newc_bytes(&cpio_bytes, target_dir)
    }

    /// Pure-Rust SVR4 New ASCII CPIO (`070701` / `070702`) stream extractor with path-traversal protection
    pub fn extract_cpio_newc_bytes(
        cpio: &[u8],
        target_dir: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let mut symlinks: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut pos = 0usize;

        while pos + 110 <= cpio.len() {
            let magic = &cpio[pos..pos + 6];
            if magic != b"070701" && magic != b"070702" {
                break;
            }
            let hdr_str = std::str::from_utf8(&cpio[pos..pos + 110]).map_err(|_| {
                PreambleError::PathNotFound("Non-ASCII CPIO header encountered".to_string())
            })?;
            let mode = u32::from_str_radix(&hdr_str[14..22], 16).unwrap_or(0);
            let filesize = usize::from_str_radix(&hdr_str[54..62], 16).unwrap_or(0);
            let namesize = usize::from_str_radix(&hdr_str[94..102], 16).unwrap_or(0);

            let name_start = pos + 110;
            let name_end = (name_start + namesize).min(cpio.len());
            let raw_name = String::from_utf8_lossy(&cpio[name_start..name_end])
                .trim_end_matches('\0')
                .to_string();
            let data_pos = (name_start + namesize + 3) & !3;
            let data_end = (data_pos + filesize).min(cpio.len());
            let next_pos = (data_pos + filesize + 3) & !3;

            if raw_name == "TRAILER!!!" {
                break;
            }

            let rel_candidate = Path::new(&raw_name);
            let comps: Vec<_> = rel_candidate
                .components()
                .filter(|c| !matches!(c, std::path::Component::CurDir | std::path::Component::RootDir))
                .collect();
            if !comps.is_empty()
                && !comps
                    .iter()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                let clean_rel: PathBuf = comps.iter().map(|c| c.as_os_str()).collect();
                let dest = target_dir.join(&clean_rel);
                if dest.starts_with(target_dir) {
                    let ftype = mode & 0o170000;
                    let payload = &cpio[data_pos..data_end];
                    if ftype == 0o040000 {
                        let _ = fs::create_dir_all(&dest);
                    } else if ftype == 0o120000 {
                        let link_target = String::from_utf8_lossy(payload)
                            .trim_end_matches('\0')
                            .to_string();
                        if !link_target.is_empty() {
                            symlinks.push((clean_rel, PathBuf::from(link_target)));
                        }
                    } else if ftype == 0o100000 {
                        if let Some(parent) = dest.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        fs::write(&dest, payload).map_err(|e| {
                            PreambleError::PathNotFound(format!(
                                "Failed writing CPIO file {:?}: {}",
                                dest, e
                            ))
                        })?;
                    }
                }
            }
            pos = next_pos;
        }
        Ok(symlinks)
    }

    /// Decode all concatenated gzip streams in an Alpine `.apk` or `APKINDEX.tar.gz` buffer
    fn decompress_concatenated_gzip_members(
        bytes: &[u8],
        ctx_path: &Path,
    ) -> Result<Vec<Vec<u8>>, PreambleError> {
        use std::io::Read;
        let mut members = Vec::new();
        let mut pos = 0usize;

        while pos + 10 <= bytes.len() {
            if bytes[pos..pos + 2] != [0x1F, 0x8B] {
                if let Some(rel) = bytes[pos..].windows(2).position(|w| w == [0x1F, 0x8B]) {
                    pos += rel;
                } else {
                    break;
                }
            }
            let slice = &bytes[pos..];
            let mut buf_reader = std::io::BufReader::new(std::io::Cursor::new(slice));
            let mut member_tar = Vec::new();
            {
                let mut gz = flate2::bufread::GzDecoder::new(&mut buf_reader);
                gz.read_to_end(&mut member_tar).map_err(|e| {
                    PreambleError::PathNotFound(format!(
                        "Failed decoding gzip stream at offset {} in {:?}: {}",
                        pos, ctx_path, e
                    ))
                })?;
            }
            let consumed = (buf_reader.get_ref().position() as usize)
                .saturating_sub(buf_reader.buffer().len());
            if consumed == 0 {
                break;
            }
            pos += consumed;
            if !member_tar.is_empty() {
                members.push(member_tar);
            }
        }
        Ok(members)
    }

    /// Pure-Rust Alpine `.apk` extractor handling concatenated multi-member gzip tar streams
    pub fn extract_apk_archive(
        apk_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, PreambleError> {
        let bytes = fs::read(apk_path).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed reading .apk {:?}: {}", apk_path, e))
        })?;
        let mut all_symlinks = Vec::new();
        for member_tar in Self::decompress_concatenated_gzip_members(&bytes, apk_path)? {
            if let Ok(links) = Self::extract_tar_bytes(&member_tar, target_dir, 0) {
                all_symlinks.extend(links);
            }
        }
        Ok(all_symlinks)
    }

    /// Direct Git checkout with Go-mod style cache pathing:
    /// `~/.omnicache/git/<language>/<domain>/<owner>/<repo>@<ref>/`
    pub fn shim_git(
        &self,
        eco: PackageEcosystem,
        git_url: &str,
        git_ref: Option<&str>,
    ) -> Result<PathBuf, PreambleError> {
        let clean_url = git_url.trim_start_matches("https://").trim_start_matches("http://");
        let normalized_path = clean_url.trim_end_matches(".git");
        let r_ref = git_ref.unwrap_or("master");

        let target_dir = self
            .git_dir(&eco)
            .join(format!("{}@{}", normalized_path, r_ref));

        if target_dir.is_dir() {
            return Ok(target_dir);
        }

        let _ = fs::create_dir_all(&target_dir);
        let full_clone_url = if git_url.starts_with("http") || git_url.starts_with("git@") {
            git_url.to_string()
        } else {
            format!("https://{}.git", clean_url)
        };

        // Shallow clone target branch/tag directly into Go-mod style hierarchy
        let status = Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                r_ref,
                &full_clone_url,
                target_dir.to_str().unwrap(),
            ])
            .status();

        match status {
            Ok(s) if s.success() => Ok(target_dir),
            _ => {
                // If branch clone failed, attempt general clone and checkout
                let _ = fs::remove_dir_all(&target_dir);
                let _ = fs::create_dir_all(&target_dir);
                let fallback = Command::new("git")
                    .args(["clone", "--depth", "1", &full_clone_url, target_dir.to_str().unwrap()])
                    .status();
                if let Ok(s) = fallback {
                    if s.success() {
                        return Ok(target_dir);
                    }
                }
                let _ = fs::remove_dir_all(&target_dir);
                Err(PreambleError::PathNotFound(format!(
                    "Failed cloning git repository from '{}' (ref: {})",
                    git_url, r_ref
                )))
            }
        }
    }

    /// Download a file to disk via OmniScript's in-process Rust `TcpStream` + `rustls` client
    fn download_file(&self, url: &str, dest: &Path) -> Result<(), PreambleError> {
        self.download_file_with_headers(url, dest, &[])
    }

    /// Download a file to disk via in-process Rust `TcpStream` + `rustls` TLS handshake (zero OS `curl` dependency)
    fn download_file_with_headers(
        &self,
        url: &str,
        dest: &Path,
        headers: &[(&str, &str)],
    ) -> Result<(), PreambleError> {
        crate::http::download_to_path(url, dest, headers)
    }

    /// Fetch URL response body as UTF-8 string via in-process Rust `TcpStream` + `rustls` TLS handshake
    fn fetch_url_string(&self, url: &str) -> Result<String, PreambleError> {
        crate::http::fetch_string(url, &[])
    }

    /// Query crates.io API for the latest crate version
    fn query_crates_io_latest(&self, name: &str) -> Option<String> {
        let url = format!("https://crates.io/api/v1/crates/{}", name);
        let resp = self.fetch_url_string(&url).ok()?;
        let json: serde_json::Value = serde_json::from_str(&resp).ok()?;
        json.get("crate")
            .and_then(|c| c.get("max_version"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Query npm API for latest package version
    fn query_npm_latest(&self, name: &str) -> Option<String> {
        let url = format!("https://registry.npmjs.org/{}/latest", name);
        let resp = self.fetch_url_string(&url).ok()?;
        let json: serde_json::Value = serde_json::from_str(&resp).ok()?;
        json.get("version").and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    /// Unpack a .tar.gz / .tgz archive into target_dir with optional stripped leading path components
    fn extract_tar_gz(archive_path: &Path, target_dir: &Path, strip_components: usize) -> Result<(), PreambleError> {
        let file = fs::File::open(archive_path)
            .map_err(|e| PreambleError::PathNotFound(format!("Failed to open tar.gz archive {:?}: {}", archive_path, e)))?;
        let tar = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(tar);

        for entry_res in archive.entries().map_err(|e| PreambleError::PathNotFound(format!("Failed to read tar.gz entries: {}", e)))? {
            let mut entry = entry_res.map_err(|e| PreambleError::PathNotFound(format!("Corrupt tar entry: {}", e)))?;
            let path = entry.path().map_err(|e| PreambleError::PathNotFound(format!("Corrupt entry path: {}", e)))?;
            let components: Vec<_> = path.components().collect();
            if components.len() <= strip_components {
                continue;
            }
            // Mitigate Tar Slip: reject any entry containing ParentDir (..)
            if components.iter().any(|c| matches!(c, std::path::Component::ParentDir)) {
                continue;
            }
            let stripped_path: PathBuf = components[strip_components..].iter().map(|c| c.as_os_str()).collect();
            let dest = target_dir.join(&stripped_path);
            if !dest.starts_with(target_dir) {
                continue;
            }

            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if entry.header().entry_type().is_dir() {
                let _ = fs::create_dir_all(&dest);
            } else {
                let mut out_file = fs::File::create(&dest)
                    .map_err(|e| PreambleError::PathNotFound(format!("Failed to create file {:?}: {}", dest, e)))?;
                std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| PreambleError::PathNotFound(format!("Failed to extract file {:?}: {}", dest, e)))?;
            }
        }
        Ok(())
    }

    /// Unpack a .zip / .whl / .jar / .nupkg archive into target_dir
    fn extract_zip(archive_path: &Path, target_dir: &Path, junk_paths: bool) -> Result<(), PreambleError> {
        let file = fs::File::open(archive_path)
            .map_err(|e| PreambleError::PathNotFound(format!("Failed to open zip archive {:?}: {}", archive_path, e)))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| PreambleError::PathNotFound(format!("Corrupt zip archive {:?}: {}", archive_path, e)))?;

        for i in 0..zip.len() {
            let mut file = zip.by_index(i)
                .map_err(|e| PreambleError::PathNotFound(format!("Corrupt zip entry: {}", e)))?;

            let dest = if junk_paths {
                if let Some(file_name) = file.enclosed_name().and_then(|p| p.file_name().map(PathBuf::from)) {
                    target_dir.join(file_name)
                } else {
                    continue;
                }
            } else {
                match file.enclosed_name() {
                    Some(enclosed) => target_dir.join(enclosed),
                    None => continue,
                }
            };

            if file.is_dir() {
                let _ = fs::create_dir_all(&dest);
            } else {
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let mut out_file = fs::File::create(&dest)
                    .map_err(|e| PreambleError::PathNotFound(format!("Failed to create {:?}: {}", dest, e)))?;
                std::io::copy(&mut file, &mut out_file)
                    .map_err(|e| PreambleError::PathNotFound(format!("Failed to extract {:?}: {}", dest, e)))?;
            }
        }
        Ok(())
    }

    /// Shims all packages specified in a list, returning each spec paired with its cache path
    pub fn install_all(&self, specs: &[PackageSpec]) -> Result<Vec<(PackageSpec, PathBuf)>, PreambleError> {
        let mut results = Vec::new();
        for spec in specs {
            let path = self.shim_package(spec)?;
            results.push((spec.clone(), path));
        }
        Ok(results)
    }

    /// Query the top candidate packages for an unresolved dependency in a specific ecosystem
    pub fn query_candidates(&self, eco: &PackageEcosystem, name: &str) -> Vec<PackageSpec> {
        let clean_name = name.trim().trim_end_matches(';').trim();
        let root = clean_name.split('.').next().unwrap_or(clean_name);

        match eco {
            PackageEcosystem::Java => {
                if root == "lombok" || clean_name.contains("lombok") {
                    vec![
                        PackageSpec {
                            ecosystem: PackageEcosystem::Java,
                            name: "org.projectlombok:lombok".to_string(),
                            version: Some("1.18.36".to_string()),
                            is_git: false,
                            git_url: None,
                            git_ref: None,
                            raw_line: "'java' org.projectlombok:lombok == 1.18.36".to_string(),
                        },
                        PackageSpec {
                            ecosystem: PackageEcosystem::Java,
                            name: "org.projectlombok:lombok-mapstruct-binding".to_string(),
                            version: Some("0.2.0".to_string()),
                            is_git: false,
                            git_url: None,
                            git_ref: None,
                            raw_line: "'java' org.projectlombok:lombok-mapstruct-binding == 0.2.0".to_string(),
                        },
                        PackageSpec {
                            ecosystem: PackageEcosystem::Java,
                            name: "org.projectlombok:lombok-utils".to_string(),
                            version: Some("0.1.0".to_string()),
                            is_git: false,
                            git_url: None,
                            git_ref: None,
                            raw_line: "'java' org.projectlombok:lombok-utils == 0.1.0".to_string(),
                        },
                    ]
                } else if clean_name.contains(':') {
                    vec![PackageSpec {
                        ecosystem: PackageEcosystem::Java,
                        name: clean_name.to_string(),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'java' {}", clean_name),
                    }]
                } else {
                    let segments: Vec<&str> = clean_name.split('.').collect();
                    if segments.len() >= 3 && (segments[0] == "org" || segments[0] == "com" || segments[0] == "io" || segments[0] == "net") {
                        let group_id = format!("{}.{}.{}", segments[0], segments[1], segments[2]);
                        let artifact_id = segments[2];
                        vec![
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("{}:{}", group_id, artifact_id),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' {}:{}", group_id, artifact_id),
                            },
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("{}.{}:{}", segments[0], segments[1], artifact_id),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' {}.{}:{}", segments[0], segments[1], artifact_id),
                            },
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("{}:{}-core", group_id, artifact_id),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' {}:{}-core", group_id, artifact_id),
                            },
                        ]
                    } else {
                        vec![
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("org.apache.{}:{}", root, root),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' org.apache.{}:{}", root, root),
                            },
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("com.google.{}:{}", root, root),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' com.google.{}:{}", root, root),
                            },
                            PackageSpec {
                                ecosystem: PackageEcosystem::Java,
                                name: format!("io.{}:{}", root, root),
                                version: Some("latest".to_string()),
                                is_git: false,
                                git_url: None,
                                git_ref: None,
                                raw_line: format!("'java' io.{}:{}", root, root),
                            },
                        ]
                    }
                }
            }
            PackageEcosystem::Python => {
                vec![
                    PackageSpec {
                        ecosystem: PackageEcosystem::Python,
                        name: root.to_string(),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'python' {}", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Python,
                        name: format!("{}-core", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'python' {}-core", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Python,
                        name: format!("python-{}", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'python' python-{}", root),
                    },
                ]
            }
            PackageEcosystem::Node => {
                vec![
                    PackageSpec {
                        ecosystem: PackageEcosystem::Node,
                        name: root.to_string(),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'node' {}", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Node,
                        name: format!("@types/{}", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'node' @types/{}", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Node,
                        name: format!("{}-core", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'node' {}-core", root),
                    },
                ]
            }
            PackageEcosystem::Rust => {
                vec![
                    PackageSpec {
                        ecosystem: PackageEcosystem::Rust,
                        name: root.to_string(),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'rust' {}", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Rust,
                        name: format!("{}-derive", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'rust' {}-derive", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Rust,
                        name: format!("{}-core", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'rust' {}-core", root),
                    },
                ]
            }
            PackageEcosystem::Go => {
                vec![
                    PackageSpec {
                        ecosystem: PackageEcosystem::Go,
                        name: format!("github.com/{}/{}", root, root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'go' github.com/{}/{}", root, root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Go,
                        name: format!("golang.org/x/{}", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'go' golang.org/x/{}", root),
                    },
                ]
            }
            PackageEcosystem::C => {
                let c_clean = clean_name
                    .trim_end_matches(".h")
                    .trim_end_matches(".hpp")
                    .split('/')
                    .next()
                    .unwrap_or(clean_name);
                let live_suggestions = self.suggest_c_packages(c_clean, 3);
                if !live_suggestions.is_empty() {
                    live_suggestions
                        .into_iter()
                        .map(|(n, v)| PackageSpec {
                            ecosystem: PackageEcosystem::C,
                            name: n.clone(),
                            version: Some(v),
                            is_git: false,
                            git_url: None,
                            git_ref: None,
                            raw_line: format!("'c' {}", n),
                        })
                        .collect()
                } else {
                    let cands = Self::c_package_name_candidates(c_clean, self.platform.provider);
                    cands
                        .into_iter()
                        .take(3)
                        .map(|c| PackageSpec {
                            ecosystem: PackageEcosystem::C,
                            name: c.clone(),
                            version: Some("latest".to_string()),
                            is_git: false,
                            git_url: None,
                            git_ref: None,
                            raw_line: format!("'c' {}", c),
                        })
                        .collect()
                }
            }
            PackageEcosystem::Csharp => {
                vec![
                    PackageSpec {
                        ecosystem: PackageEcosystem::Csharp,
                        name: root.to_string(),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'csharp' {}", root),
                    },
                    PackageSpec {
                        ecosystem: PackageEcosystem::Csharp,
                        name: format!("{}.Core", root),
                        version: Some("latest".to_string()),
                        is_git: false,
                        git_url: None,
                        git_ref: None,
                        raw_line: format!("'csharp' {}.Core", root),
                    },
                ]
            }
            _ => vec![
                PackageSpec {
                    ecosystem: eco.clone(),
                    name: root.to_string(),
                    version: Some("latest".to_string()),
                    is_git: false,
                    git_url: None,
                    git_ref: None,
                    raw_line: format!("'{}' {}", eco.as_str(), root),
                }
            ],
        }
    }

    /// Auto-resolve and fetch the first relevant candidate package for an ecosystem
    pub fn auto_resolve_and_shim(&self, eco: &PackageEcosystem, name: &str) -> Result<(PackageSpec, PathBuf), PreambleError> {
        let candidates = self.query_candidates(eco, name);
        if let Some(top_candidate) = candidates.first() {
            let path = self.shim_package(top_candidate)?;
            Ok((top_candidate.clone(), path))
        } else {
            Err(PreambleError::PathNotFound(format!("No candidate packages found for '{}' in ecosystem '{}'", name, eco.as_str())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_omni_requirements_manifest() {
        let manifest = r#"
# Standard registry specifications
'python' fastapi == 0.115.0
'python' requests >= 2.31.0
'rust' rand = "0.8.5"
'rust' strsim = "0.11.1"
'node' express == 4.19.2
'node' lodash >= 4.17.21
'c' sqlite3 == 3.45.1
'java' org.apache.commons:commons-lang3 == 3.14.0
'csharp' Newtonsoft.Json == 13.0.3

# Direct Git Importers
[git]'go' https://github.com/gin-gonic/gin.git#v1.9.1
[git]'rust' https://github.com/dtolnay/anyhow.git#1.0.86
[git]'c' https://github.com/madler/zlib.git#v1.3.1
"#;

        let specs = PackageShimmer::parse_requirements(manifest);
        assert_eq!(specs.len(), 12);

        // Verify Python
        assert_eq!(specs[0].ecosystem, PackageEcosystem::Python);
        assert_eq!(specs[0].name, "fastapi");
        assert_eq!(specs[0].version.as_deref(), Some("0.115.0"));
        assert!(!specs[0].is_git);

        // Verify Rust
        assert_eq!(specs[2].ecosystem, PackageEcosystem::Rust);
        assert_eq!(specs[2].name, "rand");
        assert_eq!(specs[2].version.as_deref(), Some("0.8.5"));

        // Verify Node
        assert_eq!(specs[4].ecosystem, PackageEcosystem::Node);
        assert_eq!(specs[4].name, "express");
        assert_eq!(specs[4].version.as_deref(), Some("4.19.2"));

        // Verify Java
        assert_eq!(specs[7].ecosystem, PackageEcosystem::Java);
        assert_eq!(specs[7].name, "org.apache.commons:commons-lang3");
        assert_eq!(specs[7].version.as_deref(), Some("3.14.0"));

        // Verify C#
        assert_eq!(specs[8].ecosystem, PackageEcosystem::Csharp);
        assert_eq!(specs[8].name, "Newtonsoft.Json");
        assert_eq!(specs[8].version.as_deref(), Some("13.0.3"));

        // Verify Git
        assert_eq!(specs[9].ecosystem, PackageEcosystem::Go);
        assert_eq!(specs[9].name, "gin");
        assert!(specs[9].is_git);
        assert_eq!(specs[9].git_ref.as_deref(), Some("v1.9.1"));

        assert_eq!(specs[10].ecosystem, PackageEcosystem::Rust);
        assert_eq!(specs[10].name, "anyhow");
        assert!(specs[10].is_git);
        assert_eq!(specs[10].git_ref.as_deref(), Some("1.0.86"));
    }

    #[test]
    fn test_custom_cache_root_paths() {
        let temp_cache = std::env::temp_dir().join("omni_test_cache");
        let shimmer = PackageShimmer::with_cache_root(temp_cache.clone());

        assert_eq!(
            shimmer.packages_dir(&PackageEcosystem::Rust),
            temp_cache.join("packages").join("rust")
        );
        assert_eq!(
            shimmer.git_dir(&PackageEcosystem::Go),
            temp_cache.join("git").join("go")
        );
    }

    #[test]
    fn test_target_platform_parsing_matrix() {
        let arch = TargetPlatform::parse("arch-x86_64").unwrap();
        assert_eq!(arch.slug, "arch-x86_64");
        assert_eq!(arch.llvm_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(arch.provider, CPackageProvider::ArchPacman);
        assert_eq!(arch.arch, TargetArch::X86_64);

        let deb_arm = TargetPlatform::parse("debian-aarch64").unwrap();
        assert_eq!(deb_arm.slug, "debian-aarch64");
        assert_eq!(deb_arm.llvm_triple, "aarch64-unknown-linux-gnu");
        assert_eq!(deb_arm.provider, CPackageProvider::DebianApt);
        assert_eq!(deb_arm.arch, TargetArch::Aarch64);

        let mac = TargetPlatform::parse("macos-arm64").unwrap();
        assert_eq!(mac.slug, "macos-arm64");
        assert_eq!(mac.llvm_triple, "arm64-apple-macosx11.0.0");
        assert_eq!(mac.provider, CPackageProvider::MacosHomebrew);
        assert_eq!(mac.arch, TargetArch::Aarch64);

        let win = TargetPlatform::parse("windows-x86_64").unwrap();
        assert_eq!(win.slug, "windows-x86_64");
        assert_eq!(win.llvm_triple, "x86_64-pc-windows-msvc");
        assert_eq!(win.provider, CPackageProvider::WindowsMsys2);
        assert_eq!(win.arch, TargetArch::X86_64);

        let raw_triple = TargetPlatform::parse("x86_64-apple-macosx10.15.0").unwrap();
        assert_eq!(raw_triple.slug, "macos-x86_64");
        assert_eq!(raw_triple.provider, CPackageProvider::MacosHomebrew);

        let yum = TargetPlatform::parse("yum-x86_64").unwrap();
        assert_eq!(yum.slug, "yum-x86_64");
        assert_eq!(yum.provider, CPackageProvider::RpmYum);
        assert_eq!(yum.arch, TargetArch::X86_64);

        let fedora_arm = TargetPlatform::parse("fedora-aarch64").unwrap();
        assert_eq!(fedora_arm.slug, "yum-aarch64");
        assert_eq!(fedora_arm.provider, CPackageProvider::RpmYum);
        assert_eq!(fedora_arm.arch, TargetArch::Aarch64);

        let suse = TargetPlatform::parse("suse-x86_64").unwrap();
        assert_eq!(suse.slug, "suse-x86_64");
        assert_eq!(suse.provider, CPackageProvider::RpmZypper);

        let alpine = TargetPlatform::parse("alpine-aarch64").unwrap();
        assert_eq!(alpine.slug, "alpine-aarch64");
        assert_eq!(alpine.llvm_triple, "aarch64-unknown-linux-musl");
        assert_eq!(alpine.provider, CPackageProvider::AlpineApk);
    }

    #[test]
    fn test_fuzzy_top3_c_candidate_ranking() {
        let pool = vec![
            ("lz4-devel".to_string(), "1.9.3-5.el9".to_string()),
            ("lz4-libs".to_string(), "1.9.3-5.el9".to_string()),
            ("lzo-devel".to_string(), "2.10-7.el9".to_string()),
            ("xz-devel".to_string(), "5.2.5-8.el9".to_string()),
            ("zlib-devel".to_string(), "1.2.11-40.el9".to_string()),
            ("curl-devel".to_string(), "7.76.1-29.el9".to_string()),
        ];
        let top3 = PackageShimmer::rank_c_candidates("lz44", pool, true, 3);
        assert_eq!(top3.len(), 3);
        assert_eq!(top3[0].0, "lz4-devel");
        assert_eq!(top3[1].0, "lzo-devel");
    }

    #[test]
    fn test_cpio_newc_extraction_and_symlink_capture() {
        fn build_cpio_entry(name: &str, mode: u32, data: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            let namesize = name.len() + 1;
            let hdr = format!(
                "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
                1u32, mode, 0u32, 0u32, 1u32, 0u32, data.len() as u32, 0u32, 0u32, 0u32, 0u32, namesize as u32, 0u32
            );
            out.extend_from_slice(hdr.as_bytes());
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            out.extend_from_slice(data);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            out
        }

        let mut stream = Vec::new();
        stream.extend(build_cpio_entry("./usr/include/demo.h", 0o100644, b"#define DEMO 42\n"));
        stream.extend(build_cpio_entry("./usr/lib64/libdemo.so", 0o120777, b"libdemo.so.1"));
        stream.extend(build_cpio_entry("TRAILER!!!", 0, b""));

        let stage = std::env::temp_dir().join(format!("omni_cpio_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::create_dir_all(&stage);

        let links = PackageShimmer::extract_cpio_newc_bytes(&stream, &stage).unwrap();
        assert!(stage.join("usr/include/demo.h").is_file());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, PathBuf::from("usr/lib64/libdemo.so"));
        assert_eq!(links[0].1, PathBuf::from("libdemo.so.1"));
        let _ = fs::remove_dir_all(&stage);
    }
}
