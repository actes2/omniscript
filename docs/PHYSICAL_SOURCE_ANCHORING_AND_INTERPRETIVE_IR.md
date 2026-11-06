# OmniScript: Physical Source Anchoring & Interpretive IR

## 1. The Core Dilemma

In traditional compilers, parsing is purely reductive:
- Source code is lexed, mapped to concrete syntax trees, and flattened into an Intermediate Representation (IR).
- Comments, variable naming subtleties, multi-language idioms, and subjective developer intent are destroyed.

When integrating a lightweight local LLM (~1.5B parameters) to lower non-standard polyglot syntax:
1. **Giving the LLM only IR**: The model loses semantic grounding. Abstract register graphs (`%t_14 = add i32 %t_1, %t_2`) conceal the high-level intent (e.g. Go channel pipeline, Python list comprehension, Ruby filter).
2. **Giving the LLM only raw Source Code**: The model invents arbitrary function signatures, miscalculates LLVM IR types, collides register indices, and produces unverifiable IR that fails `clang` compilation.

OmniScript resolves this via **Bi-directional Interpretive Anchoring**: anchoring verbatim physical source code directly into deterministic LLVM SSA landing pads.

---

## 2. The Bi-Directional Interpretive Anchor

When `omni_parser` encounters non-standard polyglot syntax, ambiguous operators (`:=`, `=>`), or subjective intent blocks, it does not force them into an artificial AST node or discard them. It stamps an **Interpretive Anchor**:

```rust
pub enum Statement {
    // ...
    /// Source-anchored subjective or polyglot statement
    InterpretiveAnchor {
        anchor_id: usize,
        file: String,
        line_start: usize,
        line_end: usize,
        physical_source: String, // VERBATIM original character string
        idiom: IdiomFamily,
    },
}
```

### The In-IR Landing Pad

In the generated LLVM IR module, the deterministic compiler emits an exact SSA landing pad:

```llvm
; >>> INTERPRETIVE ANCHOR #42 [file: telemetry.omni, line: 24]
; Physical Source: "go func() { ch <- [x * 2 for x in raw_stream if x > 0] }()"
; Inbound Contract: %ch = i8* (omni_chan), %raw_stream = %struct.Stream*
; Outbound Target: %t_19 (void / status)
; <<< [AI INFERENCE HOOK: ANCHOR_42]
```

---

## 3. The 3 Static Passes Establishing the Grounding Contract

Before dispatching an anchor to the inference engine, the compiler runs three deterministic static analysis passes to establish an unbreakable boundary contract:

```
                  Physical Source Code Snippet
                              │
               ┌──────────────▼──────────────┐
               │ Pass 1: Symbol Free-Variable │
               │ Intersection Analysis       │
               └──────────────┬──────────────┘
                              │
               ┌──────────────▼──────────────┐
               │ Pass 2: Syntactic Yield /   │
               │ Role Expectation Analysis   │
               └──────────────┬──────────────┘
                              │
               ┌──────────────▼──────────────┐
               │ Pass 3: Memory & Concurrency│
               │ Boundary Enforcement        │
               └──────────────┬──────────────┘
                              │
               ┌──────────────▼──────────────┐
               │ Emitted SsaContextEnvelope  │
               └─────────────────────────────┘
```

### Pass 1: Free-Variable Symbol Intersection
- The parser extracts all candidate identifier tokens from the physical source string.
- It intersects them with the compiler's stacked `ScopeTree`.
- Identified symbols become **Inbound Register Bindings**:
  - `ch` $\implies$ `%ch` (`i8*`, `omni_chan`)
  - `raw_stream` $\implies$ `%raw_stream` (`%struct.Stream*`)
- Any identifier *not* in `ScopeTree` is tagged as a block-local variable that the synthesized code must declare locally.

### Pass 2: Syntactic Yield & Role Analysis
The compiler inspects where the anchor sits in the AST:
- **Assignment Role**: `val = <anchor>` $\implies$ Contract demands a result register `%t_res` matching `val`'s type hint.
- **Conditional Role**: `if <anchor>:` $\implies$ Contract demands an `i1` boolean predicate register (`%cond`).
- **Concurrent Statement Role**: `go <anchor>` $\implies$ Contract expects a `void` return within a detached fiber trampoline.

### Pass 3: Memory & Concurrency Boundary Enforcement
- **Scalars** (`i32`, `f64`, pointers): Copied by value into the fiber task payload.
- **Channels** (`omni_chan`): Shared safely; MPMC ring buffer handles synchronization.
- **Buffers / Structs**: Scoped to the fiber's local Task Arena or marked for ownership transfer (`UnifiedMemOp::Transfer`). Never naively borrowed across concurrent boundaries.

---

## 4. The Unified Prompt / XML Envelope Schema

The inference engine receives a dense XML micro-tagged envelope (<250 tokens) containing both physical ground truth and strict LLVM boundaries:

```xml
<interpretive_task id="42" file="telemetry.omni" line="24">
  <!-- 1. The Human Subjective Intent (Verbatim Source) -->
  <physical_source>
    go func() { 
        ch <- [x * 2 for x in raw_stream if x > 0] 
    }()
  </physical_source>

  <!-- 2. The Strict LLVM Grounding Contract -->
  <ir_grounding>
    <enclosing_fn>@__omni_fn_worker</enclosing_fn>
    <target_abi>x86_64-pc-linux-gnu</target_abi>
    <concurrency_mode>fiber_m_to_n</concurrency_mode>
    <live_registers>
      <reg name="ch" llvm="%ch" type="i8* (omni_chan)"/>
      <reg name="raw_stream" llvm="%raw_stream" type="%struct.Stream*"/>
    </live_registers>
    <yield_reg next_ssa="%t_19" type="void"/>
  </ir_grounding>
</interpretive_task>
```

---

## 5. Model Execution & Bounded Self-Repair

When the embedded 1.5B model generates LLVM IR instructions for an anchor:
1. **Instruction Insertion**: The generated basic blocks are spliced directly into the landing pad between `>>> INTERPRETIVE ANCHOR` and `<<<`.
2. **Deterministic Verification**: `omni_verify` invokes system `clang -c -x ir -o /dev/null`.
3. **Bounded 1-Shot Self-Repair**: If `clang` emits a syntax or type diagnostic:
   - Diagnostic stderr is parsed by `omni_verify::extract_diagnostics`.
   - A single repair prompt is issued with exact error line and column offsets.
   - If repair succeeds, bitcode is committed to `.omnicache`. If it fails, compile-time error highlights the exact physical source line.
