# OmniScript: Autonomous Memory Management & Polyglot Runtime Framework

## 1. The Autonomous Borrow-Checker & Memory Marshalling

Memory management in OmniScript merges the performance and safety of Rust's borrow checker with the ergonomics of Python, achieved through a **hybrid deterministic-lexical normalizer + multi-pass AI lifetime enforcer**.

```
+-------------------------------------------------------------+
|                  Source Code with Allocations               |
|      (malloc, new, alloc(), dynamic lists, Go structs)      |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|              Lexical Memory Instruction Normalizer          |
|  - Ingests memory tropes across all languages               |
|  - Maps into Unified Memory Operations (UMO):               |
|    * @alloc(size, align)      * @free(ptr)                  |
|    * @borrow_shared(ptr)      * @borrow_mut(ptr)            |
|    * @transfer_ownership(ptr) * @escape_scope(ptr)          |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|                  Multi-Pass AI Compilation                  |
|                                                             |
|   +-----------------------------------------------------+   |
|   | Pass 1: Lowering Engine (Instruction Synthesis)     |   |
|   | Synthesizes logic into typed LLVM IR with memory ops|   |
|   +--------------------------+--------------------------+   |
|                              |                              |
|                              v                              |
|   +-----------------------------------------------------+   |
|   | Pass 2: Autonomous Lifetime & Borrow Checker Agent  |   |
|   | - Special prompt/instruction-set on same model base |   |
|   | - Traces object lifespans, aliasing, and escapes    |   |
|   | - Validates zero use-after-free, leaks, or race cond|   |
|   +--------------------------+--------------------------+   |
+------------------------------+------------------------------+
                               |
                   +-----------+-----------+
                   |                       |
               [Valid]                 [Invalid]
                   |                       |
                   v                       v
          [Opt Passes: opt -O2]    [Fatal Compiler Error]
          LLVM verifies bitcode    Diagnostics: Escaping pointer
                                   or invalid lifetime detected
```

---

## 2. The Lexical Memory Instruction Normalizer

To reduce inference overhead and eliminate non-deterministic misinterpretations of obvious memory operations, the front-end features a deterministic **Lexical Memory Normalizer**.

It unifies cross-language allocation paradigms into a concise **Unified Memory Operation (UMO)** dialect:

| Source Idiom | Language Origin | Normalized UMO Token | Inferred Lifetime Rule |
| :--- | :--- | :--- | :--- |
| `malloc(sz)` / `alloc(sz)` | C / Systems | `@alloc(heap, sz)` | Must have matching `@free` or `@transfer` |
| `new ClassName()` | Java / C++ / JS | `@alloc(obj, ClassName)` | Managed via scope RAII or reference pass |
| `defer f.Close()` / `free(p)` | Go / C | `@free(p)` | Pinned to scope exit landing pad |
| `drop(val)` / `move val` | Rust | `@transfer(val, target)` | Invalidates source symbol in scope |
| `return &local_var` | C / C++ (Bug) | `@escape_scope(ptr)` | **Trigger Fatal Compile-time Error** |
| `self.buf = alloc(4096)` | Pythonic Dunders | `@bind_lifetime(self, buf)`| Memory lifespan bound to `self` |

### Autonomous Lifetime Verification
If the secondary verification pass detects an escaping pointer:
```
[ERROR] omnic: Autonomous Lifetime Violation (Use-After-Scope)
  --> network/stream.omni:42:9
   |
42 |     let buffer = alloc(1024)
43 |     worker_pool.dispatch(&buffer)
44 |     free(buffer)
   |     ^^^^^^^^^^^^ Free occurred while 'worker_pool' retains a live borrow.
   |
   = Lifetime trace:
     - 'buffer' allocated at line 42 with local scope
     - Borrowed mutably by 'dispatch' at line 43
     - Deallocated prematurely at line 44 before worker thread completion
```

---

## 3. Polyglot Runtime Primitives (The LLVM Framework Shim)

Rather than forcing the model to re-invent complex concurrency and runtime primitives from raw LLVM instructions on every compilation, OmniScript bundles a minimal, ultra-performant **LLVM Framework Runtime**:

### 3.1 Pre-Fabricated LLVM Runtime Primitives
1. **Concurrency & Channels (Go Emulation)**:
   - `__omni_chan_make(cap, elem_size)`: Lock-free MPMC ring buffer.
   - `__omni_chan_send(chan, val)` / `__omni_chan_recv(chan)`: Fast non-blocking / blocking channel I/O.
   - `__omni_spawn(fn_ptr, args)`: Lightweight userspace green-thread / fiber scheduling over a native OS thread pool.
2. **Dynamic Structures (Python & JS Emulation)**:
   - `__omni_dict_get` / `__omni_dict_set`: High-speed Robin Hood hashing table compiled to C ABI.
   - `__omni_str_concat` / `__omni_str_slice`: Fast zero-copy string slices (`&str`).
3. **Outlier Inference**:
   - Any high-level construct not natively represented in the runtime shim is synthesized on-the-fly by the model.

---

## 4. Rust-Based Architecture (`omnic`)

The compiler is implemented 100% in **Rust** from Day 1 for maximum throughput, memory safety, and native integration with the LLVM toolchain.

### 4.1 Crate Architecture
```
omnic/
├── Cargo.toml
├── crates/
│   ├── omni_preamble/     # Fast ripgrep/regex & Tree-sitter symbol carver
│   ├── omni_parser/       # Resilient Pythonic AST parser (tree-sitter-omni)
│   ├── omni_mem/          # Lexical Memory Instruction Normalizer (UMO)
│   ├── omni_engine/       # Embedded llama.cpp bindings (libllama FFI)
│   ├── omni_codegen/      # inkwell / llvm-sys bindings & IR assembler
│   ├── omni_verify/       # llvm-as / opt validation & self-repair loop
│   └── omni_cli/          # Command-line driver (omnic & omni REPL)
```

### 4.2 Key Rust Dependencies
- **`inkwell` / `llvm-sys`**: Safe/raw bindings to LLVM 18/19 for IR manipulation and verification.
- **`tree-sitter`**: Multi-language parsing and symbol extraction across `.c`, `.go`, `.js`, `.py`, `.rs`.
- **`regex` & `ignore`**: Hyper-fast directory walking and symbol sweeping (ripgrep algorithms).
- **`llama-cpp-2`** or custom C++ FFI: In-process GGUF execution without external daemons.
- **`sha2` / `blake3`**: Cryptographic AST hashing for `.omnicache`.

---

## 5. Non-Determinism, Caching & CLI Controls

Since OmniScript relies on an LLM for intent synthesis and lifetime verification, the compiler embraces non-determinism with pragmatic systems tooling:

### 5.1 The `.omnicache/` Database
- **AST Subgraph Hash**: $H = \text{BLAKE3}(\text{AST\_Node} + \text{UMO\_Tokens} + \text{Target\_ABI})$.
- **Persistent Storage**: SQLite or RocksDB store inside `.omnicache/` storing verified LLVM bitcode (`.bc`).
- **Cache Hit**: If $H$ exists, the compiler reuses the verified bitcode immediately (**0 ms overhead**).
- **Cache Miss**: Invokes the local model, validates via `llvm-as`, passes the lifetime checker, and commits the newly verified bitcode to the cache.

### 5.2 CLI Invoker & Temperature Control
```bash
# Production deterministic build (fast, cache-reliant, greedy decoding)
omnic main.omni -o service_bin --opt-level=3 --temp=0.0

# Architectural / Complex lowering pass (creative intent resolution)
omnic main.omni -o service_bin --temp=0.4 --max-retries=5

# Force re-synthesis bypassing cache
omnic main.omni --no-cache --temp=0.2
```
