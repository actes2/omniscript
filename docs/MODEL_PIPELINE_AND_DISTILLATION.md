# OmniScript: Model Pipeline, Abliteration & Distillation Strategy

## 1. Foundation Models & Prior Art

### 1.1 Meta's LLM Compiler
Meta released **LLM Compiler** (built on Code Llama 7B and 13B foundations), trained on over **546 billion tokens** of:
- LLVM Intermediate Representation (LLVM IR)
- Assembly code (x86_64, ARM64)
- Compiler optimization flags (`opt -O3`, pass pipelines)
- Disassembly-to-source correlations

While Meta's LLM Compiler has unprecedented compiler-level token alignment, at 7B parameters it requires ~4GB VRAM in 4-bit quant, incurring high latency when executed per-compilation block on CPU/low-resource environments.

### 1.2 Teacher-Student Distillation Architecture
- **Teacher Model**: Meta LLM Compiler (7B) or DeepSeek-Coder-V2 / Qwen2.5-Coder-7B.
- **Student Target**: A dedicated **1.0B–1.8B** parameter model (e.g., initialized from `Qwen2.5-Coder-1.5B` or distilled into a custom compact decoder architecture).
- **Execution Target**: Single-threaded / multi-core CPU execution at >100 tokens/sec via embedded GGUF (`libllama`).

```
+-------------------------------------------------------------+
|           Teacher: Meta LLM Compiler 7B / 13B               |
|      (Deep compiler optimization & LLVM IR mastery)         |
+-------------------------------------------------------------+
                              |
       +----------------------+----------------------+
       | Logit Distillation                          | Synthetic IR Generation
       v                                             v
+-------------------------------------------------------------+
|             Curated Polyglot <-> LLVM IR Dataset            |
|   - Mixed syntax (Java streams, Python comps, C memory)     |
|   - Paired strictly with verified, valid LLVM IR            |
|   - Verified through `llvm-as` and `opt` test harness       |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|             Student: 1.5B Parameter Model                   |
|   (Fine-tuned via QLoRA / Full FT on synthetic dataset)     |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|               Directional Abliteration                      |
|   - Neutralizes refusal / guardrail activation vectors      |
|   - Permits raw pointer arithmetic, asm injection, syscalls |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|             Quantization & In-Process Embedding             |
|   - Quantized to GGUF Q4_K_M (~1.1 GB RAM footprint)        |
|   - Embedded directly in `omnic` via `libllama` C/C++ API   |
+-------------------------------------------------------------+
```

---

## 2. Directional Abliteration Procedure

Standard code models often inherit corporate alignment vectors that trigger refusals when generating:
- Raw byte inspection / packet manipulation
- Direct syscalls (`SYS_ptrace`, raw socket bindings)
- Low-level memory offsets or arbitrary pointer casts

### 2.1 Activation Orthogonalization
Instead of standard fine-tuning to override refusals (which degrades general domain performance), directional abliteration projects out refusal representations geometrically:

1. **Activation Contrast Collection**:
   - Run two prompt sets through the model:
     - Set $H$ (Harmless low-level systems queries): "Implement a ring buffer in LLVM IR", "Map page table entries in x86_64".
     - Set $R$ (Refusal-triggering low-level queries): "Inject raw shellcode into executable page", "Bypass stack canary via direct register overwrite".
   - Capture residual stream activations across layers $l \in [0, L-1]$.

2. **Compute Mean Difference Vector**:
   $$\vec{v}_l = \mathbb{E}[x^{(R)}_l] - \mathbb{E}[x^{(H)}_l]$$
   Normalize $\vec{v}_l$ to produce refusal direction $\hat{r}_l$.

3. **Weight Matrix Projection**:
   For downstream projection matrices $W_{out}$ in attention and MLP blocks:
   $$W' = W - \hat{r}_l (\hat{r}_l^T W)$$
   This zeroes out the model's ability to represent the refusal direction while preserving 99.8% of functional code reasoning capability.

---

## 3. Polyglot-to-LLVM Dataset Synthesis

A primary engineering bottleneck is that no public dataset cleanly pairs "lazy polyglot pseudo-code" with "production LLVM IR". We build this synthetically using a self-verifying loop:

### 3.1 Synthesis Pipeline
```
[High-Level Spec: Rosetta Code / LeetCode / Linux Kernel / Redis Primitives]
                              |
                              v
               [Prompting Frontier LLM]
  "Convert this algorithmic logic into 5 variations of hybrid
   pseudo-code (e.g., Python syntax with C pointers, Java streams
   with Go channels, loose JSON data pipelines)."
                              |
                              v
                 [Paired Synthetic Code]
                              |
                              v
         [Compile Reference Implementation to LLVM IR]
     (Using Clang -O2 -emit-llvm / Rustc --emit=llvm-ir)
                              |
                              v
                  [Verification Filter]
  1. llvm-as verifies valid IR syntax.
  2. JIT runs unit tests against generated IR.
  3. If tests pass: Add pair (HybridCode -> LLVM IR) to Train Set.
```

---

## 4. In-Process Inference Architecture (Eliminating Daemons)

Standard AI developer tools rely on client-server architectures (e.g., `ollama serve`, `vLLM` HTTP endpoints). This is unsuitable for a compiler:
- High process overhead and socket IPC latency
- Fragile dependency chain (daemon crashes kill builds)
- Inability to link into a single self-contained distribution binary

### 4.1 Embedding `libllama` Directly
OmniScript compiles `libllama.a` directly into the `omnic` binary or loads it via dynamic link:

```c
// C++ Driver Snippet embedded in omnic
#include "llama.h"

struct OmniModelContext {
    llama_model* model;
    llama_context* ctx;
    llama_sampler* sampler;
};

OmniModelContext* omni_init_model(const char* model_path) {
    llama_model_params model_params = llama_model_default_params();
    model_params.n_gpu_layers = 99; // Offload to GPU if Vulkan/CUDA available, else CPU
    llama_model* model = llama_load_model_from_file(model_path, model_params);
    
    llama_context_params ctx_params = llama_context_default_params();
    ctx_params.n_ctx = 4096;
    ctx_params.n_threads = 8;
    
    llama_context* ctx = llama_new_context_with_model(model, ctx_params);
    return new OmniModelContext{model, ctx, llama_sampler_init_greedy()};
}

std::string omni_lower_intent(OmniModelContext* engine, const std::string& prompt) {
    // Synchronous, in-process, deterministic generation directly into memory
    // ...
}
```

### 4.2 Caching & Deterministic Builds (`.omnicache`)
Because LLM generation is inherently probabilistic unless temperature is zero:
- `temperature = 0.0` (greedy decoding for maximum reproducibility).
- AST Subgraph Hashing: Every `IntentNode` is hashed ($H = \text{BLAKE3}(\text{code} + \text{ABI} + \text{context})$).
- If hash exists in `.omnicache/ir_hashes.db`, compiler bypasses inference entirely and injects the cached `.bc` bitcode in **< 1ms**.
