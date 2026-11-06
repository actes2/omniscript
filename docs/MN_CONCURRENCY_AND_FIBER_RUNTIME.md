# OmniScript: M:N Goroutine & Fiber Concurrency Runtime

## 1. Executive Summary

OmniScript adopts the Go concurrency model—lightweight cooperative user-space green threads (**Goroutines/Fibers**) running on an **M:N work-stealing scheduler**—while eliminating Go's largest systems drawback: **the global tracing garbage collector**.

By pairing each Goroutine with an isolated **Fiber-Local Task Arena** and enforcing **channel ownership transfer**, OmniScript supports millions of concurrent tasks with zero stop-the-world GC pauses, predictable microsecond latency, and native cross-platform compilation across Linux, Windows, and macOS (Darwin) on both x86_64 and AArch64 (ARM64).

---

## 2. The GMP Scheduler Architecture

The OmniScript runtime implements the classic GMP scheduler topology:

```
┌─────────────────────────────────────────────────────────────┐
│                     OmniScript Runtime                      │
│                                                             │
│   [Core 0 (M0)] <─── [Context P0] <─── [Local Run Queue]    │
│   [Core 1 (M1)] <─── [Context P1] <─── [Local Run Queue]    │
│          ▲                                     ▲            │
│          │         Work-Stealing Mechanism     │            │
│          └─────────────────────────────────────┘            │
│                                                             │
│   [io_uring / kqueue / IOCP Netpoller] ◄──► [Parked Fibers] │
└─────────────────────────────────────────────────────────────┘
```

1. **`G` (Goroutine / Fiber)**:
   - Initial lightweight stack (default 4KB to 8KB).
   - Dedicated local **Task Arena** handle (`G->arena`).
   - Current register context pointer (`rsp` or `sp`).
   - Status: `FIBER_RUNNABLE`, `FIBER_RUNNING`, `FIBER_WAITING`, `FIBER_DEAD`.
2. **`M` (Machine / OS Worker Thread)**:
   - A fixed pool of native OS threads strictly bound to available hardware CPU cores (`sysconf(_SC_NPROCESSORS_ONLN)`).
   - Executes the user-space fiber event loop; never blocks on channel I/O or network calls.
3. **`P` (Processor / Logical Execution Context)**:
   - Holds a lock-free local run queue (256-element circular ring buffer).
   - When a local queue empties, `P` initiates a **work-stealing pass**, stealing half the runnable fibers from another `P`'s queue.

---

## 3. Eliminating the Garbage Collector in Concurrent Go Code

In standard Go, goroutines share an arbitrary global heap. The runtime must continuously track pointers via a tri-color concurrent tracing collector and compile-time write barriers, consuming 20–30% CPU overhead and ballooning memory usage.

OmniScript eliminates tracing GC in Goroutines through two core rules:

```
Goroutine G1                                          Goroutine G2
┌───────────────────────────┐                         ┌───────────────────────────┐
│ Stack: 4KB Fiber Frame    │                         │ Stack: 4KB Fiber Frame    │
│ Memory: G1 Task Arena     │                         │ Memory: G2 Task Arena     │
│ (All local buffers/DTOs)  │                         │ (All local buffers/DTOs)  │
└─────────────┬─────────────┘                         └─────────────▲─────────────┘
              │                                                     │
              │         [OmniChannel Ring Buffer]                   │
              └────────► [Data Ownership Transfer] ─────────────────┘
                         Zero Shared Mutable Heap
                         Zero Tracing GC Required
```

### Rule 1: Fiber-Local Task Arenas (`G->arena`)
- Any allocation originating inside a goroutine (`malloc`, dynamic strings, JSON trees, slice unpacking) is allocated out of the goroutine's own private bump arena.
- When the goroutine returns or completes its entry function, **the entire arena is reclaimed in $O(1)$** by resetting the bump pointer (`arena.offset = 0`) or bulk-freeing the arena chunks.
- No per-object deallocation; no heap fragmentation; zero GC scanning.

### Rule 2: Channel Ownership Hand-off ("Share memory by communicating")
- Goroutines communicate strictly via thread-safe MPMC channels (`omni_chan`).
- **Value Types** (`i32`, `f64`, small structs): Copied directly into the channel's cacheline-aligned ring buffer.
- **Reference / Buffer Types**:
  - The sender relinquishes ownership (`UnifiedMemOp::Transfer`).
  - The receiving goroutine assumes ownership into its local task context.
  - Multi-consumer broadcast channels wrap buffers in atomic reference counters (`retain`/`release`).
- Because there is **no shared mutable heap graph across goroutines**, there are no cyclic references and no need for a global tracing collector.

---

## 4. Cross-Platform Context Switching (`swap_g_context`)

A fiber context switch requires zero syscalls and takes ~5–10 nanoseconds. It only saves callee-saved registers, swaps the stack pointer, and returns onto the target fiber.

### 4.1 x86_64 System V ABI (Linux & macOS / Darwin)

Callee-saved registers: `rbx`, `rsp`, `rbp`, `r12`, `r13`, `r14`, `r15`.

```nasm
; void swap_g_context(void** from_rsp, void* to_rsp);
global swap_g_context
swap_g_context:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov [rdi], rsp      ; Save current fiber RSP
    mov rsp, rsi        ; Load next fiber RSP

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret                 ; Jump into next fiber RIP
```

### 4.2 x86_64 Microsoft ABI (Windows)

Windows x64 requires saving non-volatile registers `rdi`, `rsi`, and `xmm6`–`xmm15`, plus updating the Thread Environment Block (TEB) stack bounds so Windows Structured Exception Handling (SEH) does not trigger stack corruption exceptions:

```nasm
; void swap_g_context_win64(void** from_rsp, void* to_rsp, void* stack_limit, void* stack_base);
global swap_g_context_win64
swap_g_context_win64:
    push rbp
    push rbx
    push rdi
    push rsi
    push r12
    push r13
    push r14
    push r15
    sub  rsp, 160       ; Space for 10 non-volatile XMM registers (XMM6-XMM15)
    movdqu [rsp + 0],   xmm6
    movdqu [rsp + 16],  xmm7
    movdqu [rsp + 32],  xmm8
    movdqu [rsp + 48],  xmm9
    movdqu [rsp + 64],  xmm10
    movdqu [rsp + 80],  xmm11
    movdqu [rsp + 96],  xmm12
    movdqu [rsp + 112], xmm13
    movdqu [rsp + 128], xmm14
    movdqu [rsp + 144], xmm15

    ; Swap TEB stack limits (GS:[0x08] = StackBase, GS:[0x10] = StackLimit)
    mov rax, gs:[0x30]  ; TEB linear pointer
    mov [rax + 0x08], r9
    mov [rax + 0x10], r8

    mov [rcx], rsp      ; Save current fiber RSP
    mov rsp, rdx        ; Load next fiber RSP

    movdqu xmm15, [rsp + 144]
    movdqu xmm14, [rsp + 128]
    movdqu xmm13, [rsp + 112]
    movdqu xmm12, [rsp + 96]
    movdqu xmm11, [rsp + 80]
    movdqu xmm10, [rsp + 64]
    movdqu xmm9,  [rsp + 48]
    movdqu xmm8,  [rsp + 32]
    movdqu xmm7,  [rsp + 16]
    movdqu xmm6,  [rsp + 0]
    add  rsp, 160

    pop r15
    pop r14
    pop r13
    pop r12
    pop rsi
    pop rdi
    pop rbx
    pop rbp
    ret
```

### 4.3 AArch64 / ARM64 (Linux, Apple Silicon / Darwin, Windows on ARM)

Conforms to AAPCS64. Callee-saved registers: `x19`–`x28`, `x29` (FP), `x30` (LR), `d8`–`d15`.
Enforces Apple Silicon Darwin strict 16-byte stack alignment:

```armasm
// void swap_g_context_arm64(void** from_sp, void* to_sp);
.globl swap_g_context_arm64
swap_g_context_arm64:
    stp  x19, x20, [sp, #-16]!
    stp  x21, x22, [sp, #-16]!
    stp  x23, x24, [sp, #-16]!
    stp  x25, x26, [sp, #-16]!
    stp  x27, x28, [sp, #-16]!
    stp  x29, x30, [sp, #-16]!
    stp  d8,  d9,  [sp, #-16]!
    stp  d10, d11, [sp, #-16]!
    stp  d12, d13, [sp, #-16]!
    stp  d14, d15, [sp, #-16]!

    mov  x2, sp
    str  x2, [x0]        // Save current SP
    mov  sp, x1          // Load target SP

    ldp  d14, d15, [sp], #16
    ldp  d12, d13, [sp], #16
    ldp  d10, d11, [sp], #16
    ldp  d8,  d9,  [sp], #16
    ldp  x29, x30, [sp], #16
    ldp  x27, x28, [sp], #16
    ldp  x25, x26, [sp], #16
    ldp  x23, x24, [sp], #16
    ldp  x21, x22, [sp], #16
    ldp  x19, x20, [sp], #16
    ret  x30
```

---

## 5. Non-Blocking Netpoller Integration

When a fiber attempts I/O (e.g. reading from a network socket or file):
1. The socket is placed in non-blocking mode (`O_NONBLOCK`).
2. If `read()` returns `EAGAIN` or `EWOULDBLOCK`:
   - The fiber status is set to `FIBER_WAITING`.
   - The file descriptor is registered with the OS event loop:
     - **Linux**: `io_uring` (fallback `epoll`).
     - **macOS / Darwin**: `kqueue`.
     - **Windows**: `IOCP` (I/O Completion Ports).
   - The OS thread $M$ calls `swap_g_context` to switch immediately to the next runnable fiber in $P$'s run queue.
3. When the kernel event fires, the background netpoller marks the fiber `FIBER_RUNNABLE` and pushes it back onto the nearest $P$ run queue.
