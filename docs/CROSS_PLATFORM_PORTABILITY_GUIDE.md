# OmniScript Cross-Platform Portability & Systems Guide

## 1. Vision & Architecture

OmniScript (`omnic`) is engineered as a zero-GC, self-contained polyglot systems compiler that operates uniformly across **Linux**, **macOS** (Apple Silicon `arm64` and Intel `x86_64`), and **Windows** (`x86_64` and `aarch64`).

To achieve cross-platform execution without imposing runtime virtual machines, foreign compiler dependencies (`rustc`, `javac`, `python`), or external shell utilities (`tar`, `unzip`, `curl`), OmniScript enforces four foundational portability guarantees:
1. **Dynamic Host Target Triple Detection**: Emits exact architecture/OS LLVM data layouts and target triples dynamically at compile time.
2. **In-Process Pure-Rust Network & Archive Engine**: Pulls dependencies over HTTP/1.1 + TLS 1.2/1.3 using an in-process Rust socket handshake (`std::net::TcpStream` + `rustls` + `webpki-roots` in [`crates/omni_preamble/src/http.rs`](../crates/omni_preamble/src/http.rs)—zero OS `curl` subprocesses) and extracts `.crate`, `.whl`, `.tgz`, `.jar`, `.nupkg`, `.pkg.tar.zst`, `.deb`, `.rpm`, and `.apk` via pure-Rust in-memory decompressors (`flate2`, `ruzstd`, `lzma_rs`, `tar`, `zip`, SVR4 `070701` CPIO, Unix `ar`).
3. **Multi-Architecture Native Assembly & Fiber Runtimes**: Cooperative M:N fibers with AAPCS64 (ARM64) and System V (x86_64) register preservation and 16-byte stack alignment.
4. **OS Abstraction Boundaries**: Dual-path networking and virtual memory (POSIX `mmap`/`mprotect` vs Windows `VirtualAlloc`/`VirtualProtect` and Winsock2).

---

## 2. Host Target Triple & LLVM Target Resolution

Target triples are dynamically detected at compile/run time via `omni_runtime::host_target_triple()` without hardcoded platform assumptions, including explicit macOS deployment targets (required for Mach-O `_Thread_local` TLV descriptors and `LC_BUILD_VERSION`) and Windows MSVC vs MinGW (`gnu`) ABI distinction:

```rust
pub fn host_target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "arm64-apple-macosx11.0.0";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-macosx10.15.0";
    #[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "gnu"))]
    return "x86_64-pc-windows-gnu";
    #[cfg(all(target_os = "windows", target_arch = "x86_64", not(target_env = "gnu")))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64", target_env = "gnu"))]
    return "aarch64-pc-windows-gnu";
    #[cfg(all(target_os = "windows", target_arch = "aarch64", not(target_env = "gnu")))]
    return "aarch64-pc-windows-msvc";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-pc-linux-gnu";
}
```

This triple is injected into:
- LLVM IR module headers (`target triple = "..."`).
- Exception unwinding ABI selection (`declare i32 @_setjmp(i8*, i8*) returns_twice` with `null` frame pointer on Windows x64/ARM64 vs `declare i32 @_setjmp(i8*) returns_twice` on POSIX/macOS, preventing `STATUS_BAD_STACK` during `longjmp`).
- Bitcode cache key generation in `omni_cli::main` (BLAKE3 hash incorporating the host triple to prevent cross-architecture cache poisoning).
- Clang JIT execution and AOT linker invocations.

---

## 3. Pure-Rust In-Process Archive Extraction

OmniScript eliminates external shell dependencies (`tar`, `unzip`, `expand-archive`) by handling all archive decompression directly in the compiler process memory:

| Ecosystem | Archive Format | Rust Extraction Engine | Handled Extensions |
|---|---|---|---|
| **Rust (crates.io)** | Gzipped Tarball (`.crate`) | `flate2::read::GzDecoder` + `tar::Archive` | `.crate`, `.tar.gz`, `.tgz` |
| **Python (PyPI)** | Deflate ZIP (`.whl`) | `zip::ZipArchive` with `deflate` | `.whl`, `.zip` |
| **Node.js (npm)** | Gzipped Tarball (`.tgz`) | `flate2::read::GzDecoder` + `tar::Archive` (strips `package/` prefix) | `.tgz` |
| **Java (Maven)** | Deflate ZIP (`.jar`) | `zip::ZipArchive` | `.jar` |
| **C# (NuGet)** | Deflate ZIP (`.nupkg`) | `zip::ZipArchive` | `.nupkg` |
| **C (`oenv` Arch / MSYS2)** | Zstandard / XZ Tarball (`.pkg.tar.zst`, `.db`) | `ruzstd::decoding::StreamingDecoder` / `lzma_rs` + `tar::Archive` | `.pkg.tar.zst`, `.pkg.tar.xz`, `.db` |
| **C (`oenv` Debian / Ubuntu)** | Unix `ar` (`!<arch>\n`) + `control.tar.*` / `data.tar.*` | Built-in `ar` parser + `Depends:` resolver + `ruzstd` / `lzma_rs` / `flate2` | `.deb` |
| **C (`oenv` Yum / DNF & Zypper)** | RPM v3/v4 Lead/Header + SVR4 `070701` CPIO | Built-in `.rpm` header & `070701` CPIO parser + `ruzstd` / `lzma_rs` / `flate2` | `.rpm`, `repomd.xml`, `primary.xml.*` |
| **C (`oenv` Alpine `apk`)** | Multi-member concatenated Gzip Tarball (`.apk`) | Per-stream `flate2::bufread::GzDecoder` + `tar::Archive` | `.apk`, `APKINDEX.tar.gz` |
| **C (`oenv` macOS Homebrew)** | OCI Gzipped Tarball Bottle (`.tar.gz`) | Anonymous GHCR Bearer token + `flate2::read::GzDecoder` | `.tar.gz` |

### Cache Root Resolution Across OSs
Directory paths for `.omnicache` are resolved via:
```rust
let home = std::env::var_os("HOME")
    .or_else(|| std::env::var_os("USERPROFILE"))
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."));
let cache_dir = home.join(".omnicache");
```

---

## 4. Multi-Architecture Native Assembly & Fiber Runtimes

### 4.1 Cooperative Context Switching
OmniScript includes dedicated hand-rolled naked assembly context switches in [`crates/omni_runtime/src/native/omni_fiber.c`](../crates/omni_runtime/src/native/omni_fiber.c):

#### x86_64 (System V AMD64 ABI)
Preserves callee-saved registers (`%rbx`, `%rbp`, `%r12`–`%r15`):
- Upon context creation, an 8-byte dummy word is pushed prior to the entry address so that `(%rsp + 8) % 16 == 0` upon function entry, satisfying System V SSE alignment requirements and preventing hardware `#GP` faults.

#### ARM64 / AArch64 (AAPCS64 - macOS Apple Silicon & Linux ARM64)
Preserves callee-saved registers (`x19`–`x30`):
```c
__asm__(
    ".global __omni_fiber_swap_context\n"
    "__omni_fiber_swap_context:\n"
    "  stp x19, x20, [sp, #-96]!\n"
    "  stp x21, x22, [sp, #16]\n"
    "  stp x23, x24, [sp, #32]\n"
    "  stp x25, x26, [sp, #48]\n"
    "  stp x27, x28, [sp, #64]\n"
    "  stp x29, x30, [sp, #80]\n"
    "  mov x2, sp\n"
    "  str x2, [x0]\n"
    "  ldr x2, [x1]\n"
    "  mov sp, x2\n"
    "  ldp x29, x30, [sp, #80]\n"
    "  ldp x27, x28, [sp, #64]\n"
    "  ldp x25, x26, [sp, #48]\n"
    "  ldp x23, x24, [sp, #32]\n"
    "  ldp x21, x22, [sp, #16]\n"
    "  ldp x19, x20, [sp], #96\n"
    "  ret\n"
);
```

### 4.2 Cooperative Fibers, Synchronization & Guard Pages
- **POSIX (Linux / macOS)**:
  - Stacks allocated via `mmap(MAP_ANON | MAP_PRIVATE)` (`MAP_ANONYMOUS` normalized to `MAP_ANON` on Darwin) with `mprotect(PROT_NONE)` hardware guard pages and `munmap()`.
  - Thread-local scheduler state (`_Thread_local`) and `pthread_mutex_t` / `pthread_cond_t` MPMC channel primitives in `omni_chan.c`.
- **Windows (`x86_64` & `aarch64` MSVC/MinGW)**:
  - Uses native Win32 Fibers (`ConvertThreadToFiberEx`, `CreateFiberEx` with `FIBER_FLAG_FLOAT_SWITCH`, `SwitchToFiber`, `DeleteFiber`), preserving the Windows Thread Environment Block (`TEB`), SEH unwind chain, stack limits, and XMM6–XMM15 non-volatile vector registers automatically.
  - Zero-dependency Win32 `SRWLOCK` and `CONDITION_VARIABLE` (`AcquireSRWLockExclusive`, `SleepConditionVariableSRW`) in `omni_chan.c` and `CreateThread` / `WaitForSingleObject` in `omni_fiber.c`—eliminating any external `winpthreads` requirement on MSVC.
  - Static archive resolution handles both `omni_runtime_native.lib` (MSVC `lib.exe`) and `libomni_runtime_native.a` (Unix/macOS/MinGW `ar`).

---

## 5. OS Abstraction Layer (`omni_value.c`) & GPU Acceleration (`omni_engine`)

### 5.1 Networking, Pipes & LLP64 Safety
Low-level socket and OS boundaries dynamically adapt between Berkeley Sockets and Winsock2:
```c
#ifdef _WIN32
  #ifndef WIN32_LEAN_AND_MEAN
  #define WIN32_LEAN_AND_MEAN
  #endif
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #include <windows.h>
  #include <io.h>
  #include <fcntl.h>
#else
  #include <sys/socket.h>
  #include <netinet/in.h>
  #include <unistd.h>
#endif
```
- **Header & Linker Discipline**: `<winsock2.h>` always precedes `<windows.h>` with `WIN32_LEAN_AND_MEAN`, and `omni_verify` / `omni_runtime` automatically pass `-lws2_32 -ladvapi32 -luserenv` on Windows.
- **LLP64 64-bit Cleanliness**: All `int64_t` formatting across `omni_value.c`, `omni_slice.c`, and `omni_map.c` uses `%lld` / `PRId64` (never `%ld`, which truncates to 32 bits under Windows LLP64).
- **OS Pipes & Sleep**: `__omni_os_pipe_pair` uses `_pipe(fds, 4096, _O_BINARY)` on Windows and `pipe(fds)` on POSIX; `_MSC_VER` builds also expose `sleep`/`usleep` compatibility shims backed by Win32 `Sleep()`.

### 5.2 Hardware Acceleration & Standalone Embedding (`omni_engine`)
- **Linux / Windows NVIDIA GPU**: `cargo build --release --features cuda` (`--device cuda` or `--device auto`).
- **macOS Apple Silicon GPU**: `cargo build --release --features metal,accelerate` (`--device metal` or `--device auto`).
- **Standalone `--features full-build`**: Uses zero-overhead `.incbin` section embedding (`__TEXT,__const` on macOS, `.rodata` on Linux, `.rdata` on MinGW) and automatically switches to compile-time `include_bytes!` on Windows MSVC (`cl.exe`).

---

## 6. Toolchain & Package Discovery (`omni_verify` & `omni_preamble`)

[`crates/omni_verify/src/lib.rs`](../crates/omni_verify/src/lib.rs) and [`crates/omni_preamble/src/lib.rs`](../crates/omni_preamble/src/lib.rs) locate LLVM/Clang, SDK sysroots, headers, and native libraries across all three platforms:

1. **Override Environment Variable**: `OMNI_CLANG_PATH` or `CLANG_PATH` takes highest priority on all platforms.
2. **Linux**: `/usr/lib/llvm{21,20,19,18}/bin/clang`, `/usr/bin/clang`, `pkg-config` (`-I`/`-L`/`-l`), `/usr/include`, `/usr/lib*`.
3. **macOS**:
   - Homebrew & MacPorts LLVM: `/opt/homebrew/opt/llvm/bin/clang`, `/usr/local/opt/llvm/bin/clang`, `/opt/local/bin/clang`
   - Xcode Command Line Tools: `/Library/Developer/CommandLineTools/usr/bin/clang`, `/usr/bin/clang`
   - Automatic macOS SDK Sysroot (`-isysroot`): Resolves `$SDKROOT`, `xcrun --show-sdk-path`, or `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` so standalone Homebrew LLVM links `-lSystem` out of the box.
4. **Windows**:
   - Official LLVM, MSYS2 & Visual Studio Clang: `C:\Program Files\LLVM\bin\clang.exe`, `C:\msys64\{ucrt64,clang64,mingw64}\bin\clang.exe`, Visual Studio 2022 `VC\Tools\Llvm\{x64,ARM64}\bin\clang.exe`, and `%PATH%` (`clang.exe`).
   - Native C Header/Library Discovery: Scans `$VCPKG_ROOT/installed/{x64-windows,arm64-windows,...}`, MSYS2 `include`/`lib`, and MSVC `INCLUDE`/`LIB` environment paths for `.h`/`.hpp` and `.lib`/`.dll.a`/`.dll`.
   - Diagnostic Parsing: Non-greedy drive-letter-safe regex (`C:\...\file.ll:line:col: error: ...`) preserves exact Clang self-repair feedback on Windows paths.
   - Executable Output: `omnic build` normalizes default `a.out` to `a.exe` (and appends `.exe` when extensionless) on Windows.

---

## 7. Universal Rootless C `oenv` Package Importer & `--platform` Flag

Rather than shelling out to privileged host package managers (`sudo pacman -S`, `sudo apt install`, `brew`, or `choco`), `omnic` acts directly as a **cross-platform userland C package manager**, parameterized globally by `--platform <spec>` (`default = "auto"`).

### 7.1 Supported `--platform` Presets & Upstream Providers

| `--platform` Preset(s) | Provider (`CPackageProvider`) | LLVM Target Triple | Upstream Index & Archive Format |
|---|---|---|---|
| `auto` (host default) | Auto-detected (`/etc/os-release`, host OS/arch) | Host triple | Matches host OS/distro & CPU architecture |
| `arch-x86_64`, `arch-aarch64` | `ArchPacman` | `{x86_64,aarch64}-unknown-linux-gnu` | Arch JSON API (`provides`) / ALARM `.db` + `.pkg.tar.zst` |
| `debian-x86_64`, `debian-aarch64` (`ubuntu-*`) | `DebianApt` | `{x86_64,aarch64}-unknown-linux-gnu` | Launchpad API + `Packages.gz` (`Provides:`/`Source:`) + `.deb` |
| `yum-x86_64`, `yum-aarch64` (`dnf-*`, `rhel-*`, `fedora-*`, `amzn-*`) | `RpmYum` | `{x86_64,aarch64}-unknown-linux-gnu` | EL9 `repomd.xml` + `primary.xml.*` (`pkgconfig(...)`) + `.rpm` CPIO |
| `suse-x86_64`, `suse-aarch64` (`opensuse-*`, `zypper-*`) | `RpmZypper` | `{x86_64,aarch64}-unknown-linux-gnu` | openSUSE Tumbleweed `repomd.xml` + `primary.xml.zst` + `.rpm` CPIO |
| `alpine-x86_64`, `alpine-aarch64` (`apk-*`, `musl-*`) | `AlpineApk` | `{x86_64,aarch64}-unknown-linux-musl` | Alpine `APKINDEX.tar.gz` (`pc:...`) + `-dev`/`-libs`/`-static` `.apk` |
| `macos-arm64`, `macos-x86_64` | `MacosHomebrew` | `arm64-apple-macosx11.0.0` / `x86_64-apple-macosx10.15.0` | `formulae.brew.sh` JSON + `Aliases/` + `ghcr.io` OCI bottle `.tar.gz` |
| `windows-x86_64`, `windows-aarch64` (`msys2-*`) | `WindowsMsys2` | `{x86_64,aarch64}-pc-windows-msvc` (or `-w64-windows-gnu`) | MSYS2 `ucrt64.db` / `clangarm64.db` + `.pkg.tar.zst` |

### 7.2 Canonical `oenv` C Layout & Static-First Internalization

Unpacked C packages are normalized into `<oenv>/packages/c/<platform.slug>/<pkg>-<ver>/`:
- `include/**`: Headers (automatically flattening Debian Multiarch `include/x86_64-linux-gnu/` subdirectories).
- `lib/*`: Static archives (`lib*.a`, `*.lib`), import libraries (`lib*.dll.a`), and shared libraries (`lib*.so*`, `lib*.dylib`) with reconstructed SONAME links.
- `bin/*.dll`: Windows runtime DLLs (automatically staged next to `.exe` outputs or injected into `PATH` during `omnic run`).
- `omni-c-pkg.json`: Provenance metadata.

When linking `import 'c' <pkg>`, `DependencyDriller` selects **static archives (`lib<pkg>.a` / `<pkg>.lib`) first** so self-contained C libraries (e.g. `lz4`, `zlib`) are internalized directly into the output native binary with zero external `.so`/`.dll` runtime dependency. If static linking fails due to unbundled transitive static symbols, `LlvmVerifier` automatically falls back to the sibling `.so`/`.dylib`/`.dll.a` and embeds `-Wl,-rpath,<oenv_lib_dir>`.

```bash
# Install C package for the current host into ./oenv (zero sudo)
omnic install --oenv oenv 'c' lz4

# Stage C packages for any target OS/architecture from any host
omnic install --oenv oenv --platform debian-x86_64  'c' lz4
omnic install --oenv oenv --platform macos-arm64    'c' lz4
omnic install --oenv oenv --platform windows-x86_64 'c' lz4

# Build or run parameterized by --platform (or auto-fetch on the fly)
omnic build --oenv oenv --platform debian-x86_64 app.omni -o app
omnic run --oenv oenv --auto-resolve-dependencies app.omni
```
