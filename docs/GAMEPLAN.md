# OmniScript: Architectural Specification & Implementation Gameplan

## 1. Vision & Core Philosophy

Syntax should be a transport layer for logic, not a friction barrier. Senior engineers possess deep architectural intuition—data flow, concurrency boundaries, memory access patterns, and anti-pattern recognition. However, traditional compilers penalize minor syntactic deviations, forcing engineers into rigid, language-specific boilerplate (e.g., enterprise class hierarchies, verbose interface implementations, manual memory bookkeeping).

**OmniScript** is an **omni-faceted, Pythonic-first language** that compiles directly to native machine code via LLVM IR:
- **Surface Syntax**: Ergonomic, indentation-aware or relaxed-brace Pythonic baseline. It adopts Python's extraordinarily flexible class model (`class`, `def`, `self`, decorators, dunder protocols, mixins, duck-typing) as the structural standard.
- **Omnivorous Ingestion**: The engineer can write in virtually any language idiom (Pythonic comprehensions, C raw pointers, JS arrow functions, Go channels, or pure pseudo-logic). As long as the causal dependencies and logical flow are coherent, it compiles.
- **Front-end**: A resilient, semi-strict AST parser that normalizes multi-paradigm expressions into a unified structural representation without throwing pedantic syntax errors.
- **Middle-end (Inference Lowering)**: An embedded, distilled local LLM (fine-tuned on polyglot logic -> LLVM IR) that infers missing types, devirtualizes duck-typed calls, maps high-level intent to native memory layouts, and emits strictly typed LLVM IR.
- **Verification Engine**: Deterministic compiler verification loop (`llvm-as`, `opt -O2`). Hallucinations or malformed IR are trapped in-memory, diagnosed via compiler stderr, and self-repaired via bounded feedback before binary emission.
- **Back-end**: Direct native code generation (`llc` + `lld` / system linker) or interactive in-memory execution via LLVM OrcJIT.

```
+-------------------------------------------------------------+
|                      OmniScript Source                      |
|  - Pythonic class/method structure (self, def, decorators)  |
|  - Universal polyglot imports (import "foo.js", from "pkg") |
|  - Omnivorous syntax: Python, C pointers, JS lambdas, etc.  |
|  - Fluid types / duck-typing / structural protocols         |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|          Preamble Assembler & Polyglot Symbol Carving       |
|  - Validates file/dir paths; errors on non-existent targets |
|  - Fast regex/AST sweeping hunts symbols across directories |
|  - "Un-cages" foreign language scopes (Go, C, JS, Rust)     |
|  - Binds foreign declarations into virtual module scopes    |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|               Semi-Strict Resilient AST Parser              |
|  - Normalizes indentation / braces into unified block tree   |
|  - Classifies nodes: Concrete (rigid) vs. Intent (inferred) |
|  - Resolves Pythonic class hierarchies & method scopes      |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|        Lexical Memory Normalizer (UMO) & Monomorphization   |
|  - Maps allocs/frees to Unified Memory Operations (UMO)     |
|  - Resolves duck-typing into concrete struct layouts        |
|  - Links Pre-Fabricated Runtime (Go channels, MPMC, dicts)  |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|          Embedded In-Process LLM Engine (libllama)          |
|  - Pass 1: Lowers intent blocks into typed LLVM IR          |
|  - Pass 2: Autonomous Lifetime & Borrow Checker Agent       |
|  - Zero daemon overhead (runs in-process inside omnic)       |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|               LLVM Verification & Repair Loop               |
|  - Validates bitcode with `llvm-as`                         |
|  - Runs `opt -O2` (inlining, SIMD vectorization, mem2reg)   |
|  - Rejection sampling on diagnostic failure (self-healing)  |
+-------------------------------------------------------------+
                              |
              +---------------+---------------+
              |                               |
              v                               v
    [AOT Compilation: omnic]       [Interactive REPL: omni]
    - Native ELF/Mach-O binary     - Direct OrcJIT execution
    - Zero runtime dependency      - Interactive ambiguity query
    - C-level execution speed      - Instant iteration loop
```

---

## 2. Object Model: Python Class Architecture as the Native Baseline

Python's object model is one of the most expressive and multi-faceted in computer science: it unifies data structures, behaviors, operator overloading, and metadata seamlessly. OmniScript adopts this mental model but compiles it down to zero-overhead native structures.

### 2.1 The Class Scaffold
Classes in OmniScript look and feel like modern Python, supporting:
1. **Dynamic / Duck-Typed Signatures**: Types are optional. The compiler infers optimal native layouts via flow analysis and LLVM synthesis.
2. **Dunder Protocols**: Built-in native mapping for standard dunder methods (`__init__`, `__enter__`/`__exit__` for RAII memory management, `__getitem__` for native indexing, `__str__`, `__call__`).
3. **Decorators & Mixins**: High-level behavioral annotations (e.g., `@dataclass`, `@inline`, `@simd`, `@kernel`) that guide compiler optimization passes.

```omniscript
@dataclass
class PacketProcessor:
    device_id: u32
    threshold: f32 = 80.0
    buffer: *mut u8 = null

    def __enter__(self):
        # Native RAII allocation without CPython overhead
        if not self.buffer:
            self.buffer = alloc(4096)
        return self

    def __exit__(self, exc_type, exc_val):
        if self.buffer:
            free(self.buffer)

    def process(self, raw_data, length):
        # Omni-faceted logic: mix of pythonic list processing and pointer arithmetic
        for i in range(length):
            byte = *(self.buffer + i)
            if byte > self.threshold:
                self.handle_alert(byte, i)

    def handle_alert(self, val, offset):
        print(f"[ALERT] Device {self.device_id} exceeded limit: {val} at offset {offset}")
```

### 2.2 Native Monomorphization & Devirtualization
In standard Python, every attribute lookup traverses dynamic dicts (`__dict__`). In OmniScript:
- The compiler maps fields defined in `__init__` or class attributes directly into a packed **LLVM Struct** (`%struct.PacketProcessor = type { i32, float, i8* }`).
- Methods are compiled to static functions taking `self` as the first argument (`%self*`), matching standard C ABI conventions.
- If a class is never dynamically subclassed at runtime, all method calls are **devirtualized** into direct near-calls or inlined completely.

---

## 3. The Omnivorous Parsing Strategy

The parser must never choke on syntactic flavor. It recognizes common language idioms and normalizes them into a unified semantic tree:

### 3.1 Accepted Surface Flavors
| Idiom Style | Source Example | Normalized Semantic Form |
| :--- | :--- | :--- |
| **Pythonic** | `evens = [x for x in data if x % 2 == 0]` | Vector filter & transform pipeline |
| **C / Low-level** | `*dest = *src++; if (!ptr) return -EINVAL;` | Direct memory store/load & ptr null check |
| **JS / Functional** | `items.map(x => x.token).filter(t => t != null)` | Iterator map-filter chain |
| **Go-style** | `chan <- event; select { case ... }` | Lock-free ring buffer / sync queue |
| **Declarative Intent**| `intent: parse body as json, pluck user.id, return` | Synthesized parser & extraction routine |

### 3.2 AST Node Architecture: Concrete vs. Intent
- **`ConcreteNode`**: Fully determined code (explicit control flow, arithmetic, direct pointers, typed functions). Emitted directly or with deterministic lowerings.
- **`IntentNode`**: Any polyglot idiom, duck-typed operation, or high-level pseudo-construct. Packaged with context (surrounding structs, variable types, expected return) and sent to the embedded model for LLVM IR synthesis.

---

## 4. The Lowering & Verification Pipeline

```
[OmniScript AST Unit]
          |
          v
[Context Serialization]
  - Target ABI: x86_64-linux-gnu
  - Scope: Class PacketProcessor (%struct.PacketProcessor*)
  - Method: def process(self, raw_data, length)
  - Intent: Synthesize vector scan over buffer with threshold comparison
          |
          v
[In-Process Model Inference (libllama)]
  - Emits candidate LLVM IR function body
          |
          v
[Verification via llvm-as]
     /         \
  [Pass]     [Fail]
    |           |
    |           v
    |     Extract compiler stderr diagnostic
    |     Feed diagnostic back to model for bounded 1-shot repair
    v
[LLVM Optimization Passes (opt -O2)]
  - mem2reg, sroa, loop-vectorize, dce
    v
[Link / Execution]
  - Ahead-of-Time: `llc` + `lld` -> Standalone ELF executable
  - REPL / JIT: LLVM OrcJIT in-memory execution
```

---

## 5. Model Strategy & Engine Delivery

### 5.1 Foundation & Model Sizing
- **Teacher Model**: Meta's `LLM-Compiler-7B` or `Qwen2.5-Coder-7B`.
- **Target Student Model**: `1.5B` parameter decoder (e.g. distilled onto a `Qwen2.5-Coder-1.5B` base).
- **Format**: GGUF `Q4_K_M` (~1.1 GB memory footprint).
- **Execution**: Direct C/C++ in-process execution via `libllama`. No external daemon (Ollama/Docker) required.

### 5.2 Directional Abliteration
- Security guardrails in general-purpose models often refuse to generate raw pointer arithmetic, arbitrary memory offsets, or raw socket/syscall code.
- Directional abliteration orthogonalizes weight matrices against refusal directions, guaranteeing unrestricted compilation of systems primitives while maintaining 100% of functional code reasoning.

### 5.3 Rust Architecture & Implementation Stack
The compiler driver is built entirely in **Rust** from Day 1 for maximum speed, concurrency, and memory safety:
- **`omni_preamble`**: Multi-threaded ripgrep/regex & Tree-sitter symbol carver and path validator.
- **`omni_parser`**: Resilient Pythonic AST parser (supporting polyglot expressions and loose syntax).
- **`omni_mem`**: Lexical Memory Instruction Normalizer (UMO) mapping all allocations to unified operations.
- **`omni_engine`**: Embedded `libllama` in-process inference engine with `--temp` control.
- **`omni_codegen`**: Pre-fabricated LLVM runtime framework (Go MPMC channels, fibers, dicts) + LLVM IR assembler (`inkwell`).
- **`omni_verify`**: Multi-pass autonomous borrow/lifetime checker and `llvm-as` / `opt` self-healing loop.
- **`omni_cli`**: CLI binary (`omnic`) managing `.omnicache` AST bitcode caching.

---

## 6. Implementation Roadmap

### Phase 0: Rust Foundations, Symbol Carver & Memory Normalizer (Weeks 1–2)
- [ ] Initialize Cargo workspace (`omni_preamble`, `omni_parser`, `omni_mem`, `omni_cli`).
- [ ] Implement Preamble Symbol Carver (directory verification, regex/tree-sitter symbol hunting, Go un-caging).
- [ ] Implement Lexical Memory Instruction Normalizer (UMO) for standard memory operations (`alloc`, `free`, `drop`, `new`).
- [ ] Build Pythonic AST parser with resilient polyglot expression ingestion.

### Phase 1: Embedded `libllama` & Multi-Pass Lifetime Verification (Weeks 3–5)
- [ ] Integrate in-process GGUF inference in Rust via `libllama` FFI with CLI `--temp` support.
- [ ] Implement Pass 1 (Lowering Engine: polyglot logic -> LLVM IR).
- [ ] Implement Pass 2 (Autonomous Lifetime & Borrow Checker Agent validating object lifetimes).
- [ ] Implement automated self-healing loop using `llvm-as` diagnostic feedback.

### Phase 2: Runtime Shim & Model Fine-Tuning (Weeks 6–9)
- [ ] Build pre-fabricated LLVM runtime shim (lock-free Go-style MPMC channels, lightweight coroutines, Robin Hood hash maps).
- [ ] Curate synthetic dataset: Polyglot/Pythonic snippets paired with validated LLVM IR.
- [ ] Apply directional abliteration to remove refusal vectors on systems primitives.
- [ ] Distill / fine-tune 1.5B student model on compilation and lifetime checking tasks.

### Phase 3: The `omnic` Compiler CLI & `.omnicache` (Weeks 10–12)
- [ ] Build end-to-end compiler driver: `.omni` source -> AST -> UMO -> IR -> `opt` -> `llc` -> native ELF binary.
- [ ] Implement `.omnicache` AST BLAKE3 hashing for sub-millisecond cached builds.

### Phase 4: Interactive REPL & JIT (Weeks 13+)
- [ ] Integrate LLVM OrcJIT for instant in-memory execution.
- [ ] Conversational disambiguation: compiler queries user when intent is mathematically ambiguous.

---

## 7. Documentation Suite
- [`GAMEPLAN.md`](file:///home/actes/projects/slop/omniscript/docs/GAMEPLAN.md) - Master architecture & phase roadmap.
- [`STOLEN_COMPILER_CONCEPTS.md`](file:///home/actes/projects/slop/omniscript/docs/STOLEN_COMPILER_CONCEPTS.md) - Best-in-class concepts borrowed from Go, Zig, Erlang, LuaJIT, and Rust.
- [`MEMORY_AND_RUNTIME_SYSTEM.md`](file:///home/actes/projects/slop/omniscript/docs/MEMORY_AND_RUNTIME_SYSTEM.md) - UMO normalizer, autonomous borrow checking & runtime shims.
- [`DEPENDENCY_AND_IMPORT_SYSTEM.md`](file:///home/actes/projects/slop/omniscript/docs/DEPENDENCY_AND_IMPORT_SYSTEM.md) - Polyglot imports, preamble symbol carving, and Go un-caging.
- [`SYNTAX_SPEC_AND_EXAMPLES.md`](file:///home/actes/projects/slop/omniscript/docs/SYNTAX_SPEC_AND_EXAMPLES.md) - Pythonic class baseline, omnivorous syntax, and EBNF grammar.
- [`MODEL_PIPELINE_AND_DISTILLATION.md`](file:///home/actes/projects/slop/omniscript/docs/MODEL_PIPELINE_AND_DISTILLATION.md) - Teacher-student distillation, directional abliteration, and embedded execution.
