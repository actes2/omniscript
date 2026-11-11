#include "omni_fiber.h"
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#include <windows.h>
#define OMNI_HAS_WIN32_VM 1
#elif defined(__unix__) || defined(__APPLE__) || defined(__linux__)
#include <sys/mman.h>
#include <unistd.h>
#include <pthread.h>
#define OMNI_HAS_MMAP 1
#endif

static OmniFiber* g_current_fiber = NULL;
static OmniFiber g_main_fiber = {0};
static OmniFiber* g_dead_fibers = NULL;
static int g_initialized = 0;

#if defined(__x86_64__) && !defined(_WIN32)
__attribute__((naked)) static void swap_g_context(void** from_rsp __attribute__((unused)), void* to_rsp __attribute__((unused))) {
    __asm__ volatile (
        "pushq %rbp\n\t"
        "pushq %rbx\n\t"
        "pushq %r12\n\t"
        "pushq %r13\n\t"
        "pushq %r14\n\t"
        "pushq %r15\n\t"
        "movq %rsp, (%rdi)\n\t"
        "movq %rsi, %rsp\n\t"
        "popq %r15\n\t"
        "popq %r14\n\t"
        "popq %r13\n\t"
        "popq %r12\n\t"
        "popq %rbx\n\t"
        "popq %rbp\n\t"
        "retq\n\t"
    );
}
#elif defined(__aarch64__)
__attribute__((naked)) static void swap_g_context(void** from_rsp __attribute__((unused)), void* to_rsp __attribute__((unused))) {
    __asm__ volatile (
        "stp x19, x20, [sp, #-16]!\n\t"
        "stp x21, x22, [sp, #-16]!\n\t"
        "stp x23, x24, [sp, #-16]!\n\t"
        "stp x25, x26, [sp, #-16]!\n\t"
        "stp x27, x28, [sp, #-16]!\n\t"
        "stp x29, x30, [sp, #-16]!\n\t"
        "mov x2, sp\n\t"
        "str x2, [x0]\n\t"
        "mov sp, x1\n\t"
        "ldp x29, x30, [sp], #16\n\t"
        "ldp x27, x28, [sp], #16\n\t"
        "ldp x25, x26, [sp], #16\n\t"
        "ldp x23, x24, [sp], #16\n\t"
        "ldp x21, x22, [sp], #16\n\t"
        "ldp x19, x20, [sp], #16\n\t"
        "ret\n\t"
    );
}
#else
// Fallback stub for environments without direct naked asm inline
static void swap_g_context(void** from_rsp, void* to_rsp) {
    (void)from_rsp;
    (void)to_rsp;
}
#endif

static void cleanup_dead_fibers(void) {
    while (g_dead_fibers) {
        OmniFiber* next = g_dead_fibers->next;
        __omni_fiber_destroy(g_dead_fibers);
        g_dead_fibers = next;
    }
}

void __omni_fiber_init(void) {
    if (!g_initialized) {
        g_main_fiber.rsp = NULL;
        g_main_fiber.status = OMNI_FIBER_RUNNING;
        g_main_fiber.arena = NULL;
        g_current_fiber = &g_main_fiber;
        g_initialized = 1;
    }
}

int __omni_fiber_is_in_fiber(void) {
    __omni_fiber_init();
    return (g_current_fiber != NULL && g_current_fiber != &g_main_fiber);
}

static void fiber_entry_stub(void) {
    OmniFiber* f = g_current_fiber;
    if (f && f->entry) {
        f->entry(f->arg);
    }
    if (f) {
        f->status = OMNI_FIBER_DEAD;
        // Reclaim fiber Task Arena in O(1)
        if (f->arena) {
            __omni_arena_destroy(f->arena);
            f->arena = NULL;
        }
        // Enqueue on dead-fiber list for reclamation on main stack
        f->next = g_dead_fibers;
        g_dead_fibers = f;
    }

    // Switch back to main fiber
    OmniFiber* target = &g_main_fiber;
    g_current_fiber = target;
    swap_g_context(&f->rsp, target->rsp);
}

OmniFiber* __omni_fiber_create(void (*entry)(void*), void* arg, size_t stack_size) {
    __omni_fiber_init();

    size_t page_size = 4096;
#if defined(OMNI_HAS_MMAP)
    long sc_page = sysconf(_SC_PAGESIZE);
    if (sc_page > 0) page_size = (size_t)sc_page;
#endif

    // Minimum 64 KB for foreign FFI and nested calls
    if (stack_size < 65536) {
        stack_size = 65536;
    }
    // Align stack_size to page boundary
    stack_size = (stack_size + page_size - 1) & ~(page_size - 1);
    size_t total_mmap_size = stack_size + page_size; // +1 guard page at the bottom

    OmniFiber* fiber = (OmniFiber*)malloc(sizeof(OmniFiber));
    if (!fiber) return NULL;

#if defined(OMNI_HAS_WIN32_VM)
    void* vm_base = VirtualAlloc(NULL, total_mmap_size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!vm_base) {
        free(fiber);
        return NULL;
    }
    DWORD old_protect;
    VirtualProtect(vm_base, page_size, PAGE_NOACCESS, &old_protect);

    fiber->stack_base = (char*)vm_base + page_size;
    fiber->stack_size = stack_size;
    fiber->total_mmap_size = total_mmap_size;
    fiber->guard_page_size = page_size;
#elif defined(OMNI_HAS_MMAP)
    void* mmap_base = mmap(NULL, total_mmap_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mmap_base == MAP_FAILED) {
        free(fiber);
        return NULL;
    }
    // Bottom page is PROT_NONE hardware guard page
    mprotect(mmap_base, page_size, PROT_NONE);

    fiber->stack_base = (char*)mmap_base + page_size;
    fiber->stack_size = stack_size;
    fiber->total_mmap_size = total_mmap_size;
    fiber->guard_page_size = page_size;
#else
    fiber->stack_base = malloc(stack_size);
    if (!fiber->stack_base) {
        free(fiber);
        return NULL;
    }
    fiber->stack_size = stack_size;
    fiber->total_mmap_size = 0;
    fiber->guard_page_size = 0;
#endif

    fiber->arena = __omni_arena_create(32 * 1024); // 32 KB Task Arena per fiber
    fiber->status = OMNI_FIBER_RUNNABLE;
    fiber->entry = entry;
    fiber->arg = arg;
    fiber->next = NULL;

    // 16-byte aligned stack top for AAPCS64 / System V x86_64 ABI
    uintptr_t stack_top = (uintptr_t)fiber->stack_base + stack_size;
    stack_top &= ~((uintptr_t)15);

#if defined(__x86_64__)
    // To satisfy System V AMD64 ABI:
    // At function entry (immediately following a `call` instruction which pushes 8 bytes),
    // (%rsp + 8) % 16 == 0.
    // When swap_g_context pops 6 registers (48 bytes) and executes `retq` (8 bytes),
    // %rsp must equal (stack_top - 8).
    // Therefore, push an 8-byte dummy alignment word before pushing fiber_entry_stub:
    uint64_t* sp = (uint64_t*)stack_top;
    *(--sp) = 0;                          // 8-byte dummy alignment padding
    *(--sp) = (uint64_t)fiber_entry_stub; // return address for retq
    *(--sp) = 0;                          // r15
    *(--sp) = 0;                          // r14
    *(--sp) = 0;                          // r13
    *(--sp) = 0;                          // r12
    *(--sp) = 0;                          // rbx
    *(--sp) = 0;                          // rbp
    fiber->rsp = (void*)sp;
#elif defined(__aarch64__)
    // AAPCS64: 12 callee-saved registers (x19-x30) = 96 bytes (aligned to 16 bytes).
    // x30 is the Link Register (return address for ret instruction).
    uint64_t* sp = (uint64_t*)stack_top;
    *(--sp) = (uint64_t)fiber_entry_stub; // x30 (lr)
    *(--sp) = 0;                          // x29 (fp)
    *(--sp) = 0;                          // x28
    *(--sp) = 0;                          // x27
    *(--sp) = 0;                          // x26
    *(--sp) = 0;                          // x25
    *(--sp) = 0;                          // x24
    *(--sp) = 0;                          // x23
    *(--sp) = 0;                          // x22
    *(--sp) = 0;                          // x21
    *(--sp) = 0;                          // x20
    *(--sp) = 0;                          // x19
    fiber->rsp = (void*)sp;
#else
    fiber->rsp = (void*)stack_top;
#endif
    return fiber;
}

void __omni_fiber_yield(void) {
    __omni_fiber_init();
    if (g_current_fiber && g_current_fiber != &g_main_fiber) {
        OmniFiber* from = g_current_fiber;
        OmniFiber* to = &g_main_fiber;
        g_current_fiber = to;
        swap_g_context(&from->rsp, to->rsp);
        cleanup_dead_fibers();
    }
}

void __omni_fiber_destroy(OmniFiber* fiber) {
    if (fiber) {
        if (fiber->arena) {
            __omni_arena_destroy(fiber->arena);
            fiber->arena = NULL;
        }
#if defined(OMNI_HAS_WIN32_VM)
        if (fiber->total_mmap_size > 0 && fiber->stack_base) {
            void* vm_base = (char*)fiber->stack_base - fiber->guard_page_size;
            VirtualFree(vm_base, 0, MEM_RELEASE);
            fiber->stack_base = NULL;
        }
#elif defined(OMNI_HAS_MMAP)
        if (fiber->total_mmap_size > 0 && fiber->stack_base) {
            void* mmap_base = (char*)fiber->stack_base - fiber->guard_page_size;
            munmap(mmap_base, fiber->total_mmap_size);
            fiber->stack_base = NULL;
        }
#else
        if (fiber->stack_base) {
            free(fiber->stack_base);
            fiber->stack_base = NULL;
        }
#endif
        free(fiber);
    }
}

void* __omni_fiber_run_on_main_stack(void* (*fn)(void*), void* arg) {
    // If already running on OS thread / main stack, execute directly
    if (!__omni_fiber_is_in_fiber()) {
        return fn(arg);
    }
    // Otherwise execute the foreign call (future: can pivot rsp to host thread stack)
    return fn(arg);
}

void __omni_go_spawn(void (*entry)(void*), void* arg) {
    __omni_fiber_init();
    OmniFiber* fiber = __omni_fiber_create(entry, arg, 65536);
    if (!fiber) return;

    // Switch to new fiber immediately (cooperative run)
    OmniFiber* from = g_current_fiber;
    g_current_fiber = fiber;
    swap_g_context(&from->rsp, fiber->rsp);

    // Upon return to main stack, reclaim any dead fiber stacks
    cleanup_dead_fibers();
}

#if !defined(_WIN32)
typedef struct OmniThreadCtx {
    void (*entry)(void*);
    void* arg;
} OmniThreadCtx;

static pthread_t g_spawned_threads[64];
static size_t g_spawned_thread_count = 0;

static void* omni_pthread_runner(void* raw_ctx) {
    OmniThreadCtx* ctx = (OmniThreadCtx*)raw_ctx;
    void (*entry)(void*) = ctx->entry;
    void* arg = ctx->arg;
    free(ctx);
    if (entry) {
        entry(arg);
    }
    return NULL;
}

void __omni_thread_spawn(void (*entry)(void*), void* arg) {
    OmniThreadCtx* ctx = (OmniThreadCtx*)malloc(sizeof(OmniThreadCtx));
    if (!ctx) return;
    ctx->entry = entry;
    ctx->arg = arg;
    pthread_t tid;
    if (pthread_create(&tid, NULL, omni_pthread_runner, ctx) == 0) {
        if (g_spawned_thread_count < 64) {
            g_spawned_threads[g_spawned_thread_count++] = tid;
        }
    } else {
        free(ctx);
    }
}

void __omni_thread_join_all(void) {
    for (size_t i = 0; i < g_spawned_thread_count; ++i) {
        pthread_join(g_spawned_threads[i], NULL);
    }
    g_spawned_thread_count = 0;
}
#else
void __omni_thread_spawn(void (*entry)(void*), void* arg) {
    __omni_go_spawn(entry, arg);
}
void __omni_thread_join_all(void) {}
#endif
