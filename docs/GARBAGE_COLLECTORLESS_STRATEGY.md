# OmniScript: Garbage Collector-Less Memory Architecture

## 1. The Premise: Zero Tracing GC

Traditional dynamic and managed languages (Java, Python, Go, JavaScript) rely on runtime Garbage Collectors (tracing mark-and-sweep, generational GC, or tri-color concurrent GC) because their source syntax does not require explicit lifetime tracking. 

However, runtime GCs impose steep taxes:
- Non-deterministic latency spikes / stop-the-world pauses
- Significant RAM overhead (typically 2x–3x actual data volume to maintain GC throughput)
- Bulky runtime binaries (bundling the collector, thread schedulers, and GC barriers)

**OmniScript compiles code written in these languages into native machine code with zero runtime GC.** It achieves this by combining four compile-time strategies:

```
                           Target Object Allocation
                                      |
                                      v
                      [Pass 1: Static Escape Analysis]
                    Does the object escape the function?
                               /             \
                           [No]              [Yes]
                            |                  |
                            v                  v
                 [Scalar Replacement]    [Pass 2: Arena Boundary Analysis]
                 Object promoted to CPU   Does it escape outside request/task?
                 registers or stack.                  /             \
                 NO HEAP. ZERO OVERHEAD.          [No]              [Yes]
                                                   |                  |
                                                   v                  v
                                          [Scope / Task Arena]    [Deterministic ARC]
                                          Allocated in Arena.     Compiler inserts
                                          Freed in O(1) bulk      retain/release.
                                          reset upon exit.        Cycles converted
                                                                  to weak refs by AI.
```

---

## 2. The Four Pillars of GC Elimination

### 2.1 Pillar 1: Escape Analysis & Scalar Replacement of Aggregates (SROA)
- Over 70% of allocations in typical Python/JS/Java code are short-lived temporaries: dictionary lookups, tuple unpacking, string formatting fragments, or intermediate DTOs.
- In OmniScript, the compiler performs static data-flow escape analysis. If an object is proven not to escape its declaring scope, **the heap allocation is erased**.
- The fields of the object are decomposed into scalar stack variables or loaded directly into CPU registers (`rax`, `rbx`, `xmm0`). When the function returns, memory reclamation is instantaneous via the hardware stack pointer.

### 2.2 Pillar 2: Hierarchical Scope Arenas (Zig / Game-Engine Model)
For objects that outlive a single function call but belong to an operational lifecycle (e.g. an HTTP request, a batch database query, a JSON deserialization tree):
- The object is allocated out of a **Task Arena** (a contiguous chunk of virtual memory).
- No individual `free()` or garbage collection occurs during execution.
- When the top-level task or intent block completes, the entire arena is reclaimed in **$O(1)$ time** by resetting the bump pointer (`arena.offset = 0`).

### 2.3 Pillar 3: Deterministic ARC with Cycle Neutralization
For long-lived objects crossing thread channels or dynamic shared queues:
- The compiler inserts deterministic `retain` and `release` instructions at explicit AST control points.
- **Cycle Neutralization**: Naive reference counting fails on cyclic references (`a.b = b; b.a = a`). The Pass 2 Lifetime Agent inspects object relationship graphs:
  - Back-references are automatically demoted to non-owning `weak` pointers.
  - Strongly coupled cyclic clusters are isolated into a dedicated Arena, preventing memory leaks without a tracing GC.

### 2.4 Pillar 4: Perceus In-Place Mutation (Lean 4 / Koka Technique)
In dynamic languages, functional array/string operations normally force new allocations:
```python
items = [x * 2 for x in items]
```
Under OmniScript, if the reference count of `items` is statically or dynamically 1 (uniquely owned), the compiler modifies the memory buffer **in-place** with zero reallocation.
