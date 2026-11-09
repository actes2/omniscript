# OmniScript Master Roadmap: Universal Polyglot Package Shimmer & Pure-Rust 1-to-1 Parity

## 1. System Vision & Directives
OmniScript (`omnic`) operates as a **zero-GC universal polyglot compiler**. It does not outsource compilation to external compilers (`rustc`, `javac`, `python`, `node`, `go`). Instead, it achieves full execution parity by natively comprehending polyglot ASTs, instruction structures, and type layouts, lowering them directly to LLVM IR basic blocks backed by its native zero-GC runtime (`omni_runtime`).

This roadmap formalizes two foundational frontiers:
1. **The Universal Polyglot Package Shimmer**: A unified dependency manifest (`omni-requirements.txt`) and in-process network/archive extraction engine supporting Python (PyPI), Rust (crates.io), Node (npm), C (tarball/source/system shims), and Direct Git Repositories (`[git]'<language>' <url>`) with Go-mod style cache pathing in `~/.omnicache/`.
2. **Pure-Rust 1-to-1 Parity**: Native instruction comprehension for Rust structs, inherent `impl` blocks, constructor literals, tagged enums (`type { i32, [N x i8] }`), pattern matching (`switch i32 %tag`), fat-pointer slices (`{ i8*, i64 }`), and devirtualized static calls without `rustc`.

---

## 2. Universal Dependency Manifest Specification (`omni-requirements.txt`)

### Unified Syntax Schema
```text
# Standard Registry Packages: '<ecosystem>' <package_name> [op] <version>
'python' fastapi == 0.115.0
'python' requests >= 2.31.0
'rust'   rand = "0.8.5"
'rust'   strsim = "0.11.1"
'rust'   crc32fast = "1.5.4"
'node'   express == 4.19.2
'node'   lodash >= 4.17.21
'c'      sqlite3 == 3.45.1
'c'      zlib == 1.3.1
'java'   org.apache.commons:commons-lang3 == 3.14.0
'java'   org.json:json == 20240303
'csharp' Newtonsoft.Json == 13.0.3
'csharp' Microsoft.Extensions.Logging == 8.0.0

# Direct Git Importers: [git]'<language>' <url>[#<tag_or_branch>]
[git]'go'     github.com/gin-gonic/gin#v1.9.1
[git]'rust'   https://github.com/dtolnay/anyhow.git#1.0.86
[git]'c'      https://github.com/madler/zlib.git#v1.3.1
[git]'python' https://github.com/psf/requests.git#v2.31.0
```

### Hermetic Cache Hierarchy (`~/.omnicache/`)
```
~/.omnicache/
├── packages/
│   ├── python/
│   │   └── fastapi-0.115.0/
│   ├── rust/
│   │   └── rand-0.8.5/
│   │       ├── Cargo.toml
│   │       └── src/lib.rs
│   ├── node/
│   │   └── express-4.19.2/
│   │       ├── package.json
│   │       └── index.js
│   ├── c/
│   │   └── sqlite3-3.45.1/
│   │       ├── sqlite3.h
│   │       └── sqlite3.c
│   ├── java/
│   │   └── commons-lang3-3.14.0/
│   │       └── org/apache/commons/lang3/*.class
│   └── csharp/
│       └── Newtonsoft.Json-13.0.3/
│           └── lib/net8.0/Newtonsoft.Json.dll
└── git/
    ├── go/
    │   └── github.com/gin-gonic/gin@v1.9.1/  <-- Go-mod style hierarchy
    ├── rust/
    │   └── github.com/dtolnay/anyhow@1.0.86/
    └── c/
        └── github.com/madler/zlib@v1.3.1/
```

---

## 3. The Architectural Roadmap & Status

### Phase 1: Universal Package Shimmer & Git Module Importer [COMPLETED]
- **1-A: Manifest Parser (`omni_preamble::shimmer::manifest`)**: [DONE]
  - Ingest `omni-requirements.txt` and inline `.omni` header annotations (`#deps: 'rust' rand="0.8"`).
  - Unify version constraints (`==`, `>=`, `<=`, `=`).
- **1-B: Multi-Registry In-Process Shimmers**: [DONE]
  - **Rust (crates.io)**: Sparse index API (`https://index.crates.io/`) + tarball extraction (`.crate` gzip -> pure Rust source via pure-Rust `flate2` + `tar::Archive`).
  - **Python (PyPI)**: JSON API (`https://pypi.org/pypi/<pkg>/json`) + wheel unzip (`.whl` -> pure Python modules & `.so` dynsym via pure-Rust `zip::ZipArchive`).
  - **Node (npm)**: Registry tarball (`https://registry.npmjs.org/<pkg>/-/<pkg>-<ver>.tgz` -> un-tar to `package/` via pure-Rust `flate2` + `tar::Archive`).
  - **C (Source/Amalgamation & System)**: Downloads raw release tarballs/amalgamations or discovers host headers/shared libraries via `pkg-config` fallback.
  - **Java / Maven / Gradle**: Maven Central repository (`https://repo1.maven.org/maven2/<group_path>/<artifact>/<version>/<artifact>-<version>.jar`) unpacked directly into class files -> lowered via `JvmBytecodeDecoder` directly to LLVM IR.
  - **C# / NuGet**: NuGet flat container (`https://api.nuget.org/v3-flatcontainer/<package>/<version>/<package>.<version>.nupkg`) unpacked (ZIP) -> extracted into `lib/` assemblies.
- **1-C: Direct Git Importer (`[git]'<lang>' <url>`)**: [DONE]
  - In-process Git checkout / clone to `~/.omnicache/git/<lang>/<domain>/<owner>/<repo>@<ref>/`.
  - Automatically structures the checkout based on language idioms (e.g. Go module tree paths for `go.mod`, `src/lib.rs` for Rust, `include/` and `*.c` for C).
- **1-D: CLI Integration**: [DONE]
  - Subcommand `omnic install [manifest_path]` (defaults to `omni-requirements.txt`).
  - Automatic on-demand resolution during `omnic build` and `omnic run` when unresolved imports occur.

---

### Phase 2: Rust Structs, Inherent `impl`, & Direct Monomorphization [COMPLETED]
- **2-A: Grammar & AST (`omni_parser`)**: [DONE]
  - Parse `struct Name { field: type, ... }` (public and private fields).
  - Parse `impl Name { pub fn method(&self, ...) -> Type { ... } }`.
  - Parse struct initialization literals: `let p = Point { x: 10, y: 20 }`.
- **2-B: Monomorphized Packed LLVM Structs (`omni_codegen`)**: [DONE]
  - Map struct definitions to packed LLVM types: `%struct.Point = type { i64, i64 }`.
  - Compute field offset indices in `ScopeTree::class_fields`.
  - Lower field reads and writes (`p.x`, `self.y = val`) via LLVM `getelementptr inbounds`.
- **2-C: Devirtualized Method Calls**: [DONE]
  - Inherent methods emitted as static C-ABI functions: `@__omni_fn_Point_norm_squared(i8* %self, ...)`.
  - Instance method calls `p.norm_squared()` lowered directly to static devirtualized calls without dynamic lookup or virtual tables.

---

### Phase 3: Rust Tagged Enums, Discriminants, & Pattern Matching [COMPLETED]
- **3-A: Enum Grammar & AST**: [DONE]
  - Parse `enum Name { Variant1(Type1), Variant2(Type2), UnitVariant }`.
  - Parse `match expr { Pattern::Variant(v) => stmt, _ => default }`.
- **3-B: LLVM Tagged Union Layout**: [DONE]
  - All enums lower to `%enum.Name = type { i32, [MaxPayloadSize x i8] }`.
  - Tag discriminant 0, 1, 2... represents each variant.
- **3-C: Pattern Match Jump Tables**: [DONE]
  - Extract discriminant: `%tag = load i32, i32* %discriminant_ptr`.
  - Lower to LLVM `switch i32 %tag` jump tables.
  - Bind and bitcast payload registers inside the matched arm basic blocks.

---

### Phase 4: Rust Slices (`&str`, `&[T]`), Fat Pointers & Trait Monomorphization [IN PROGRESS]
- **4-A: Fat-Pointer Slice Representation**: [DONE]
  - Memory layout: `%struct.Slice = type { i8* ptr, i64 len }`.
  - Support string slicing (`&s[0..5]`) and byte buffer views.
- **4-B: Trait Monomorphization & Static Dispatch**:
  - Parse `impl Trait for Struct { ... }`.
  - Static resolution: when `fn compute<T: Trait>(item: T)` is called with `Point`, lower directly to monomorphized `@__omni_fn_compute_Point`.
- **4-C: Dynamic Trait Objects (`&dyn Trait`)**:
  - Modeled as a 16-byte fat pointer: `{ i8* data_ptr, i8* vtable_ptr }`.
  - Emits global immutable vtables containing function pointers for dynamic invocation.

---

### Phase 5: Verification Matrix & Cross-Platform Portability [COMPLETED]
- **5-A: Polyglot Shimmer Matrix**: [DONE]
  - Automated tests downloading and caching Python, Rust, Node, C, and Git packages without host tools.
  - Multi-language integration matrix passes cleanly across Linux, macOS, and Windows.
- **5-B: Cross-Platform Systems Harness**: [DONE]
  - Dynamic host target triple resolution (`x86_64-pc-linux-gnu`, `arm64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`).
  - Hand-rolled AAPCS64 (ARM64) and System V (x86_64) assembly fiber context-switch routines.
  - POSIX `mmap`/`mprotect` and Windows `VirtualAlloc`/`VirtualProtect` + Winsock2.
- **5-C: Rust Algorithmic Parity Test Suite**: [DONE]
  - Verified math, hash, and struct/enum benchmarks compiling to native machine code with zero-GC and sub-millisecond execution.
