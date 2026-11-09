# OmniScript: Universal Dependency & Polyglot Import System

## 1. Vision: Zero-Impedance Foreign Ingestion

Traditional compilers force languages into isolated silos:
- In Go, you are bound by package directory structures, GOPATH/modules, and exported uppercase naming rules (`Parser` vs `parser`).
- In C/C++, you must wrangle header guards, `extern "C"`, CMake targets, and link flags.
- In Node.js/Python, you wrestle with CommonJS vs ESM and CPython ABI extensions.

**OmniScript dissolves language boundaries at the import level.** You can import any source file (`.js`, `.c`, `.go`, `.py`, `.rs`) or entire directory directly into your namespace, access its methods and objects natively, and compile the entire polyglot dependency tree down to a single unified LLVM bitcode module.

---

## 2. Universal Import Syntax

OmniScript defines a uniform, quote-delimited import schema:

```python
# 1. Direct file import with namespace aliasing
import "foo.js" as Foo
import "./query_runner.c" as qr
import "telemetry/agent.go" as Agent

# 2. Directory-wide wildcard import
import "drivers/sensors" as Sensors

# 3. Targeted symbol hunting from directory or package
from "stuff" import Parser
from "engine/crypto" import SHA256Signer, KeyPair
from "./network" import fetch_async as fetch

# 4. First-class ecosystem & native package manager drilling
import 'python' flask
import 'python' requests
import 'javascript' risk_engine
import 'rust' crypto
import 'go' math
import 'c' unistd
```

---

## 2.1 Multi-Ecosystem Package Manager Driller & Shimmer

OmniScript autonomously discovers, installs, and traverses native package managers within the project root and host platform without requiring wrappers:

- **Universal Package Shimmer (`omnic install`)**: Parses `omni-requirements.txt` (or inline package annotations) and pulls dependencies directly from official registries:
  - **Python (PyPI)**: Fetches JSON metadata and downloads wheels (`.whl`), unzipping them via pure-Rust `zip::ZipArchive` directly into `~/.omnicache/packages/python/<pkg>-<ver>/`.
  - **Rust (crates.io)**: Queries the crates.io sparse index, downloads `.crate` tarballs, and extracts pure Rust source via `flate2` + `tar::Archive` directly into `~/.omnicache/packages/rust/<pkg>-<ver>/`.
  - **Node.js (npm)**: Fetches package metadata from `registry.npmjs.org`, downloads `.tgz`, and extracts via `flate2` + `tar::Archive` directly into `~/.omnicache/packages/node/<pkg>-<ver>/`.
  - **C / C++**: Downloads raw source archives/amalgamations or links directly to system shared libraries (`.so`, `.dylib`, `.dll`) discovered via `pkg-config`.
  - **Java (Maven Central)**: Downloads `.jar` archives and extracts class files, lowered directly to LLVM IR via the built-in `JvmBytecodeDecoder`.
  - **C# (NuGet)**: Downloads `.nupkg` ZIPs from NuGet flat containers and extracts `.dll` assemblies.
  - **Direct Git Repositories (`[git]'<lang>' <url>`)**: Clones or checks out git repositories directly into Go-mod style hierarchical cache paths under `~/.omnicache/git/<lang>/`.
- **Local Ecosystem Discovery**:
  - **Python**: Searches `./venv`, virtualenvs, conda environments, host `site-packages`, and dynamic `importlib.util.find_spec` probes down to `_socket`/`posix` C-frontiers.
  - **JavaScript / TypeScript**: Traverses workspace `.js`/`.ts`/`.mjs`/`.cjs`, `./node_modules`, `NODE_PATH`, Homebrew `/opt/homebrew/lib/node_modules`, and Windows `%APPDATA%\npm\node_modules`.
  - **Rust / Cargo**: Scans `src/`, local vendored crates (`./vendor/<crate>/src/lib.rs`), and global Cargo registries (`~/.cargo/registry/src`).
  - **Go**: Resolves `go.mod`, local packages, vendored libraries (`./vendor/`), and `$GOPATH/pkg/mod`.
  - **C / C++**: Discovers project headers (`include/`), vcpkg (`./vcpkg_installed`), Conan, Homebrew `/opt/homebrew/include`, and `/usr/include`.
  - **Java / JVM**: Scans Maven/Gradle source roots (`src/main/java`) and `~/.m2/repository`.
  - **C# / .NET**: Scans `.csproj` structures and global NuGet caches (`~/.nuget/packages`, `%LOCALAPPDATA%\NuGet\v3-cache`).
  - **Lua**: Resolves workspace `.lua`, `<mod>/init.lua`, and Luarocks (`~/.luarocks/share/lua/5.1`).

---

## 2.2 Isolated Polyglot Environments (`oenv`)

OmniScript supports isolated local project environments (`oenv`) akin to polyglot virtual environments, encapsulating all external dependencies (packages, extracted bytecode, and git clones) into a hermetic workspace directory:

```bash
# 1. Initialize local oenv and install dependencies
omnic install --oenv oenv                      # Targets ./oenv from omni-requirements.txt
omnic install --oenv oenv 'java' lombok         # Installs single package into ./oenv
omnic install --oenv .oenv                     # Supports hidden environments

# 2. Build or run with zero-sourcing auto-discovery
omnic run app.omni                             # Automatically discovers ./oenv
omnic build app.omni -o app_bin                # Statically links symbols from ./oenv
```

### Zero-Sourcing Architecture
Unlike language-specific virtual environments (`venv`, `nvm`) that require sourcing shell scripts (`source venv/bin/activate`), `omnic` autonomously resolves environments via `DependencyDriller::discover_oenv`:
1. **Explicit CLI Flag**: `--oenv <dir>` takes top precedence.
2. **Environment Variable**: `OMNI_ENV` or `OENV`.
3. **Workspace Auto-Scan**: Direct presence of `base_dir/oenv` or `base_dir/.oenv`.
4. **Current Working Directory**: `./oenv` or `./.oenv`.
5. **Parent Directory Traversal**: Traverses ancestor directories looking for `oenv/` or `omni-env.json`.

### Tier 0 Resolution Priority (`OR` Fall-through Semantics)
When an `oenv` is active:
- **Priority Tier 0**: `<oenv>/packages/<eco>/` and `<oenv>/git/<eco>/` are searched **first** for all symbol resolution and bytecode decompilation.
- **Fall-through (`OR`)**: If a module or symbol is absent from `oenv`, resolution transparently falls through to workspace-local files, host SDKs/virtualenvs, and global `~/.omnicache`.

### Automatic Package Resolution (`--auto-resolve-dependencies`)
When running or building with `--auto-resolve-dependencies`, missing third-party packages are automatically downloaded and unpacked directly into `<oenv>/packages/<eco>/` instead of mutating the host's global cache.

---

## 2.3 Autonomous Dialect Resolution & Bare Imports

OmniScript supports both explicit ecosystem declarations and bare polyglot imports:

```python
# Style 1: Explicit Dialect Annotation
import 'java' lombok.Data;
import 'python' requests;

# Style 2: Bare Polyglot Import (Auto-Detected)
import lombok.Data;
import requests;
```

When bare imports are encountered, the compiler executes a 3-tier dialect resolution pipeline:
1. **Tier 1: Local AI Semantic Hypothesis**: The in-process 1.5B Candle GGUF engine (`CandleGgufEngine::infer_dialect`) evaluates the source context and candidate import to predict the target language ecosystem token.
2. **Tier 2: Static Parser Fallback**: If the local AI is uninitialized or undecided, `omni_preamble::static_infer_dialect` performs deterministic AST cue pattern matching (e.g., `@Data`, `public static void main`, `System.out.println` -> `java`; `def`, `self.`, `__init__` -> `python`).
3. **Tier 3: Actionable Error Diagnostics**: If the dependency is missing from the active `oenv` or `~/.omnicache`, the compiler emits an actionable error listing the top 3 candidate packages and exact `omnic install` command:
   ```text
   [ERROR] Missing dependency 'lombok' for ecosystem 'java' in oenv (oenv).
   To install this dependency manually, run:
     omnic install --oenv oenv 'java' org.projectlombok:lombok

   Top 3 candidate packages for java:
     1. org.projectlombok:lombok (version: 1.18.36)
     2. org.projectlombok:lombok-mapstruct-binding (version: 0.2.0)
     3. io.freefair.gradle:lombok-plugin (version: 8.4)

   Alternatively, re-run with '--auto-resolve-dependencies' to fetch and shim automatically.
   ```

---

## 3. Two-Tier Symbol Resolution Engine

Import resolution operates in two complementary stages:
1. **Tier 1: Deterministic Symbol Carving (Preamble Assembly)**
   - High-speed heuristic pass using regex, ripgrep algorithms, and lightweight Tree-sitter parsers.
   - Extracts symbol tables, class names, function signatures, and exported tokens in **< 10ms**.
2. **Tier 2: AI Compiler Semantic Extraction**
   - Engaged when foreign code uses unconventional constructs (e.g., Go internal packages, C macros, dynamic JS prototypes, complex nested modules).
   - Generates exact LLVM ABI contracts and FFI wrappers automatically.

```
                      from "stuff" import Parser
                                   |
                                   v
             +-------------------------------------------+
             |         Directory Existence Check         |
             |       Does directory "stuff" exist?       |
             +---------------------+---------------------+
                                   |
                        +----------+----------+
                        |                     |
                      [No]                  [Yes]
                        |                     |
                        v                     v
              [COMPILER ERROR]     +---------------------+
           Directory not found     |  Preamble Assembly: |
                                   |   Symbol Carving    |
                                   +----------+----------+
                                              |
               +------------------------------+------------------------------+
               |                                                             |
         [File Match]                                                 [Symbol Search]
   Does `stuff/parser.*` exist?                                 Search contents of `stuff/**/*`
   (e.g., parser.go, parser.c, parser.py)                       (Regex / Tree-sitter query)
               |                                                             |
               +------------------------------+------------------------------+
                                              |
                                     +--------v--------+
                                     | Candidate Match |
                                     +--------+--------+
                                              |
                             +----------------+----------------+
                             |                                 |
                      [Single Match]                  [Ambiguous / Hidden]
                             |                                 |
                             v                                 v
                 [Bind Symbol Directly]            [AI Compiler Ingestion]
                 Extract signature & bind          - Deconstruct AST / Macro
                 to local scope                    - Synthesize LLVM ABI contract
                                                   - Verify with llvm-as
```

---

## 4. Symbol Carving Pipeline (The Preamble Pass)

During compiler initialization, before full AST construction or LLVM lowering, the **Preamble Assembler** runs across all declared imports:

### 4.1 Carving Rules & Heuristics
1. **Exact File Precedence**:
   If importing `from "stuff" import Parser`:
   - Check `stuff/parser.omni`, `stuff/parser.py`, `stuff/parser.go`, `stuff/parser.c`, `stuff/parser.js`, `stuff/parser.rs`.
   - If found, carve all public/top-level declarations from that file.

2. **Regex & Tree-Sitter Symbol Sweeper**:
   If no exact file match, run parallel regex/AST queries across all files in the target directory:
   - **Go**: `func (\(.*\)\s+)?Parser\(`, `type Parser struct`, `type Parser interface`
   - **Python**: `class Parser\b`, `def Parser\b`
   - **C / C++**: `typedef struct .* Parser;`, `class Parser\b`, `struct Parser\b`, `void\* Parser\(`
   - **JS / TS**: `class Parser`, `function Parser`, `export const Parser`, `module.exports = { Parser`

3. **Go Module "Un-caging"**:
   In standard Go, uncapitalized symbols (`func parse()`) cannot be exported outside their package. OmniScript's carver strips package visibility gates:
   - Any function or struct in a `.go` file can be ingested directly regardless of case.
   - Go runtime runtime dependencies (goroutine runtime, GC) are either stubbed or lowered into lightweight native pthreads/LLVM coroutines.

---

## 5. Polyglot Lowering Strategies by File Extension

| Source Type | Resolution Strategy | Generated LLVM Artifact |
| :--- | :--- | :--- |
| **`.c` / `.h`** | Clang AST parsing (`libclang`) or Clang `-emit-llvm` | Native LLVM IR bitcode linked directly |
| **`.go`** | Symbol carver extracts function body; lowered via Go-to-LLVM or synthesized via model | Un-caged Go structs & functions lowered to C ABI |
| **`.js` / `.ts`**| Strips dynamic prototype fluff; monomorphizes functions into typed LLVM functions | Compiled native functions (zero V8/Node.js overhead) |
| **`.py`** | Ingests classes, functions, and decorators; infers types via AST data-flow | Native LLVM structs & function symbols |
| **Directory** | Recursively indexes all supported files into an aggregate virtual namespace | Composite LLVM module containing all extracted symbols |

---

## 6. Real-World Polyglot Ingestion Example

### The Project Structure
```
my_service/
├── main.omni
├── native/
│   └── crypto_hasher.c
└── helpers/
    ├── token_bucket.go
    └── json_utils.js
```

### `native/crypto_hasher.c`
```c
#include <stdint.h>

void fast_hash_block(const uint8_t* data, uint64_t len, uint8_t* out_digest) {
    // low-level C implementation
    for (uint64_t i = 0; i < len; i++) {
        out_digest[i % 32] ^= data[i];
    }
}
```

### `helpers/token_bucket.go`
```go
package helpers

type RateLimiter struct {
    tokens    int64
    maxTokens int64
}

// Notice lowercase: unexported in standard Go!
func (r *RateLimiter) allow() bool {
    if r.tokens > 0 {
        r.tokens--
        return true
    }
    return false
}
```

### `main.omni` (OmniScript Source)
```python
# Direct polyglot imports:
import "./native/crypto_hasher.c" as Crypto
from "helpers" import RateLimiter, fast_json_parse

@dataclass
class GatewayService:
    limiter: RateLimiter
    hash_buf: *mut u8

    def __init__(self, max_req_per_sec: i64):
        # Using Go struct constructor directly
        self.limiter = RateLimiter(tokens=max_req_per_sec, maxTokens=max_req_per_sec)
        self.hash_buf = alloc(32)

    def handle_request(self, payload: *const u8, length: usize):
        # 1. Call unexported Go method directly
        if not self.limiter.allow():
            return {"status": 429, "error": "Too Many Requests"}

        # 2. Call C function without header files or extern "C"
        Crypto.fast_hash_block(payload, length, self.hash_buf)

        # 3. Call JS utility function compiled to native code
        parsed_doc = fast_json_parse(payload)
        
        return {"status": 200, "digest": self.hash_buf}
```

---

## 7. Compiler Diagnostics & Failure Handling

When an import fails, the compiler emits structured, actionable diagnostics:

### 1. Directory Not Found
```
[ERROR] omnic: Import Resolution Failure
  --> main.omni:2:1
   |
 2 | from "invalid_path" import Worker
   | ^^^^^^^^^^^^^^^^^^^ Directory 'invalid_path' does not exist in workspace or module search paths.
```

### 2. Ambiguous Symbol Match
If `from "helpers" import Parser` matches both `helpers/xml/parser.c` and `helpers/json/parser.py`:
```
[ERROR] omnic: Ambiguous Symbol 'Parser' in directory 'helpers/'
  Matches discovered:
    1) helpers/xml/parser.c:14      (struct Parser)
    2) helpers/json/parser.py:8     (class Parser)
  Fix: Disambiguate import path:
    from "helpers/xml" import Parser
    from "helpers/json" import Parser
```

### 3. Symbol Not Found in Directory
```
[ERROR] omnic: Symbol 'NonExistent' not found
  --> main.omni:3:1
   |
 3 | from "helpers" import NonExistent
   |                       ^^^^^^^^^^^
   Searched 12 files in 'helpers/':
   Found symbols: [RateLimiter, TokenBucket, fast_json_parse, format_log]
   Did you mean: 'TokenBucket'?
```
