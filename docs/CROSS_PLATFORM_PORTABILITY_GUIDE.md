# OmniScript Cross-Platform Portability & Systems Guide

## 1. Vision & Architecture

OmniScript (`omnic`) is engineered as a zero-GC, self-contained polyglot systems compiler that operates uniformly across **Linux**, **macOS** (Apple Silicon `arm64` and Intel `x86_64`), and **Windows** (`x86_64` and `aarch64`).

To achieve cross-platform execution without imposing runtime virtual machines, foreign compiler dependencies (`rustc`, `javac`, `python`), or external shell utilities (`tar`, `unzip`, `curl`), OmniScript enforces four foundational portability guarantees:
1. **Dynamic Host Target Triple Detection**: Emits exact architecture/OS LLVM data layouts and target triples dynamically at compile time.
2. **In-Process Pure-Rust Archive & Network Engine**: Pulls dependencies over TLS via pure-Rust HTTP (`ureq`) and extracts `.crate`, `.whl`, `.tgz`, `.jar`, and `.nupkg` via pure-Rust in-memory decompressors (`flate2`, `tar`, `zip`).
3. **Multi-Architecture Native Assembly & Fiber Runtimes**: Cooperative M:N fibers with AAPCS64 (ARM64) and System V (x86_64) register preservation and 16-byte stack alignment.
4. **OS Abstraction Boundaries**: Dual-path networking and virtual memory (POSIX `mmap`/`mprotect` vs Windows `VirtualAlloc`/`VirtualProtect` and Winsock2).

---

## 2. Host Target Triple & LLVM Target Resolution

Target triples are dynamically detected at runtime via `omni_runtime::host_target_triple()` without hardcoded platform assumptions:

```rust
pub fn host_target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "arm64-apple-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-apple-darwin" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "x86_64-pc-windows-msvc" }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    { "aarch64-pc-windows-msvc" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-unknown-linux-gnu" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-pc-linux-gnu" }
}
```

This triple is injected into:
- LLVM IR module headers (`target triple = "..."`).
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

### 4.2 Guard Page Memory Management
Fiber stacks use hardware guard pages to detect stack overflow:
- **POSIX (Linux / macOS)**: `mmap(MAP_ANON | MAP_PRIVATE)` with `mprotect(PAGE_NONE)` and `munmap()`.
- **Windows**: `VirtualAlloc(MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)` with `VirtualProtect(PAGE_NOACCESS)` on the base page and `VirtualFree(MEM_RELEASE)`.

---

## 5. OS Abstraction Layer (`omni_value.c`)

### 5.1 Networking & Socket Syscalls
Low-level socket boundaries dynamically adapt between Berkeley Sockets and Winsock2:
```c
#ifdef _WIN32
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #pragma comment(lib, "ws2_32.lib")
#else
  #include <sys/socket.h>
  #include <netinet/in.h>
  #include <unistd.h>
#endif
```
- **Windows Socket Initialization**: `__omni_os_socket_listen` verifies socket subsystem initialization via a thread-safe `WSAStartup` check.
- **Teardown**: Sockets close via `closesocket()` on Windows and `close()` on POSIX.
- **Sleep Callbacks**: `__omni_sleep_ms` lowers to `Sleep((DWORD)ms)` on Windows and `usleep(ms * 1000)` on POSIX.

---

## 6. Toolchain Discovery & Verification

[`crates/omni_verify/src/lib.rs`](../crates/omni_verify/src/lib.rs) locates LLVM/Clang across all major installation paths:

1. **Linux**: Standard distribution paths (`/usr/bin/clang`, `/usr/local/bin/clang`).
2. **macOS**:
   - Homebrew LLVM: `/opt/homebrew/opt/llvm/bin/clang`, `/usr/local/opt/llvm/bin/clang`
   - Homebrew Bin: `/opt/homebrew/bin/clang`, `/usr/local/bin/clang`
   - Xcode Command Line Tools: `/usr/bin/clang`
3. **Windows**:
   - LLVM Windows installer: `C:\Program Files\LLVM\bin\clang.exe`
   - System `%PATH%`: `clang.exe`

### Shared Library & Binary Extensions
- **Shared Libraries**: `.so` (Linux), `.dylib` (macOS), `.dll` and `.lib` (Windows).
- **Executable Output**: Windows binaries automatically receive the `.exe` extension to comply with the Windows subsystem and `CreateProcess` loader expectations.
- **Null Device Redirection**: Verification output suppression uses `NUL` on Windows and `/dev/null` on POSIX systems.
