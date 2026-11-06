# OmniScript: High-Leverage Compiler Concepts to "Steal"

To make OmniScript an elite, modern systems programming environment, we can intentionally borrow the most battle-tested runtime, architectural, and compiler breakthroughs from across computing history:

---

## 1. From Go: The M:N Work-Stealing Scheduler & Netpoller

### What We're Stealing:
- **The GMP Model**:
  - **`G` (Goroutine/Fiber)**: Ultra-lightweight user-space execution context (~2KB initial stack).
  - **`M` (Machine/OS Thread)**: Bound to hardware CPU cores.
  - **`P` (Processor/Logical Context)**: Manages local run queues.
- **Work-Stealing Deque**: If thread $M_1$ runs out of fibers to execute, it steals half the run-queue from thread $M_2$, ensuring 100% core utilization.
- **Kernel Netpoller (`io_uring` / `epoll`)**:
  - Whenever an OmniScript network socket or file descriptor blocks, the runtime automatically parks the fiber and registers the fd with `io_uring`.
  - The OS thread never sleeps; it immediately swaps to another runnable fiber. When the kernel completes the I/O, the fiber awakens.

```
+-------------------------------------------------------------+
|                     OmniScript Runtime                      |
|                                                             |
|   [Core 0 (M0)] <--- [Context P0] <--- [Local Fiber Deque]  |
|   [Core 1 (M1)] <--- [Context P1] <--- [Local Fiber Deque]  |
|          ^                                     ^            |
|          |         Work-Stealing Mechanism     |            |
|          +-------------------------------------+            |
|                                                             |
|   [io_uring Netpoller Ring] <--> [Parked Asynchronous Fibers]
+-------------------------------------------------------------+
```

---

## 2. From Zig: Explicit Allocator Injection & Comptime

### What We're Stealing:
- **No Hidden Allocations**: In traditional C++/Java, `new` or `malloc` is invisible and un-trackable. In OmniScript, functions that allocate memory can receive an implicit or explicit `Allocator` handle.
- **Scope-Based Arena Allocators**:
  - For web requests, batch processing, or parsers: allocate all objects out of a fast contiguous memory block (**Arena**).
  - When the request completes, **free the entire arena with a single pointer reset** (`offset = 0`), completely eliminating per-object `free()` overhead and memory fragmentation.
- **`comptime` Code Execution**:
  - Any expression or block marked `comptime` (or inferred as pure constant) is compiled and executed in-memory during compilation.
  - Generates lookup tables, pre-computes cryptographic constants, or evaluates regex engines directly into LLVM constants.

---

## 3. From Erlang / BEAM: Fault Isolation & Supervision Trees

### What We're Stealing:
- **"Let It Crash" Philosophy**: If a complex data transformation, network parser, or third-party imported C routine segfaults or panics, it must **never** take down the entire application daemon.
- **Fiber Isolation Boundaries**:
  - Critical worker fibers can be wrapped in an isolated boundary:
    ```python
    supervisor.spawn(worker_task, restart_policy="transient")
    ```
  - Under the hood, memory protection (or LLVM signal landing pads) traps hardware memory faults (`SIGSEGV`, `SIGBUS`, arithmetic overflow), cleans up the fiber's isolated Arena, and notifies the supervisor to restart the task cleanly.

---

## 4. From Java / HotSpot: Escape Analysis & Scalar Replacement

### What We're Stealing:
- **Scalar Replacement of Aggregates (SROA)**:
  - If the autonomous lifetime checker proves an object (e.g., `@dataclass PacketHeader` or a dynamic dict) never escapes the local function scope, **no heap allocation occurs**.
  - The compiler decomposes the class fields into individual CPU registers or stack slots.
  - You write clean, high-level object-oriented code with classes and methods, but the emitted LLVM machine instructions execute purely in x86_64/ARM64 registers (`rax`, `rbx`, `xmm0`).

---

## 5. From LuaJIT: NaN Boxing & Linear Tracing

### What We're Stealing:
- **NaN Boxing (Zero-Cost Polyglot Values)**:
  - When storing dynamic, loosely typed variables (from Python, JS, or dynamic user input), traditional engines allocate boxed objects on the heap.
  - LuaJIT encodes all types inside an unused 64-bit IEEE 754 floating-point "Quiet NaN" space:
    - `0x7FF8...` = Integers, Pointers, Booleans, Nil, or Function Pointers.
  - This allows OmniScript's dynamic types to occupy **exactly 8 bytes** on the stack with zero pointer dereferences or heap overhead.

---

## 6. From Rust: Zero-Cost Async State Machines & Type Monomorphization

### What We're Stealing:
- **No Heap for Futures/Async**:
  - JavaScript Promises and Go goroutines allocate memory frames on the heap.
  - Rust compiles `async fn` into an enum-based state machine that lives directly on the caller's stack.
  - OmniScript compiles async Pythonic methods into stack-allocated state machine structs whenever the future's lifetime is bounded by the caller.
- **Multiple Dispatch & Monomorphization (like Julia/Rust)**:
  - Writing generic code without type annotations (`def add(a, b): return a + b`) automatically generates specialized, SIMD-vectorized native functions for each concrete type combination invoked in the program.

---

## 7. Concept Synthesis Matrix

| Borrowed Concept | Originating Language | OmniScript Superpower |
| :--- | :--- | :--- |
| **GMP Work-Stealing Scheduler** | Go | Millions of concurrent tasks with minimal CPU thread context switching. |
| **`io_uring` Kernel Netpoller** | Go / Linux Kernel | Ultra-high throughput non-blocking network/disk I/O. |
| **Arena Memory Model** | Zig | Zero-overhead bulk deallocation for request-scoped workloads. |
| **Supervision / Fault Traps**| Erlang / BEAM | Segfaults in imported C code or foreign modules won't crash the host daemon. |
| **Scalar Replacement (SROA)**| HotSpot JVM / LLVM | Object-oriented classes compiled directly into CPU registers. |
| **NaN Boxing** | LuaJIT | Dynamic untyped values fit into single 64-bit registers without heap allocation. |
| **Async State Machines** | Rust | Zero-allocation async execution. |
