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
        Self { cache_root }
    }

    pub fn with_cache_root(cache_root: PathBuf) -> Self {
        Self { cache_root }
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

    /// Shim C library (amalgamation / headers)
    pub fn shim_c(&self, name: &str, version: Option<&str>) -> Result<PathBuf, PreambleError> {
        let pkg_dir = self.packages_dir(&PackageEcosystem::C);
        let _ = fs::create_dir_all(&pkg_dir);

        let ver = version.unwrap_or("latest");
        let target_dir = pkg_dir.join(format!("{}-{}", name, ver));
        if target_dir.is_dir() {
            return Ok(target_dir);
        }

        // Check if library already exists in system headers (Unix/macOS and Windows paths)
        let candidate_headers = [
            PathBuf::from(format!("/usr/include/{}.h", name)),
            PathBuf::from(format!("/usr/local/include/{}.h", name)),
            PathBuf::from(format!("/opt/homebrew/include/{}.h", name)),
        ];
        for sys_header in candidate_headers {
            if sys_header.is_file() {
                let _ = fs::create_dir_all(target_dir.join("include"));
                let _ = fs::copy(&sys_header, target_dir.join("include").join(format!("{}.h", name)));
                return Ok(target_dir);
            }
        }

        // Built-in recipes for standard C packages (SQLite, zlib)
        if name == "sqlite3" || name == "sqlite" {
            let _ = fs::create_dir_all(&target_dir);
            // Download official sqlite amalgamation
            let url = "https://www.sqlite.org/2024/sqlite-amalgamation-3450100.zip";
            let zip_file = self.tmp_dir().join("sqlite.zip");
            if self.download_file(url, &zip_file).is_ok() {
                let extract_res = Self::extract_zip(&zip_file, &target_dir, true);
                let _ = fs::remove_file(&zip_file);
                if extract_res.is_ok() {
                    return Ok(target_dir);
                }
            }
        }

        Err(PreambleError::PathNotFound(format!(
            "C package '{}' not found in system headers or automated source recipes",
            name
        )))
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

    /// Download a file to disk via curl
    fn download_file(&self, url: &str, dest: &Path) -> Result<(), PreambleError> {
        let status = Command::new("curl")
            .args([
                "-s",
                "-L",
                "--connect-timeout",
                "5",
                "--max-time",
                "60",
                "-o",
                dest.to_str().unwrap(),
                url,
            ])
            .status();

        match status {
            Ok(s) if s.success() && dest.is_file() => Ok(()),
            _ => Err(PreambleError::PathNotFound(format!(
                "Failed to download archive from '{}'",
                url
            ))),
        }
    }

    /// Fetch URL response as string via curl
    fn fetch_url_string(&self, url: &str) -> Result<String, PreambleError> {
        let output = Command::new("curl")
            .args(["-s", "-L", "--connect-timeout", "5", "--max-time", "15", url])
            .output();

        match output {
            Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
            _ => Err(PreambleError::PathNotFound(format!(
                "Failed to query URL '{}'",
                url
            ))),
        }
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
}
