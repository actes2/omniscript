#include "omni_fiber.h"
#include <stdlib.h>
#include <string.h>

static OmniFiber* g_current_fiber = NULL;
static OmniFiber g_main_fiber = {0};
static OmniFiber* g_dead_fibers = NULL;
static int g_initialized = 0;

#if defined(__x86_64__) && !defined(_WIN32)
__attribute__((naked)) static void swap_g_context(void** from_rsp, void* to_rsp) {
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
#else
// Fallback stub for architectures without naked asm inline
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

    if (stack_size < 16384) {
        stack_size = 16384; // 16 KB default
    }

    OmniFiber* fiber = (OmniFiber*)malloc(sizeof(OmniFiber));
    if (!fiber) return NULL;

    fiber->stack_base = malloc(stack_size);
    if (!fiber->stack_base) {
        free(fiber);
        return NULL;
    }
    fiber->stack_size = stack_size;
    fiber->arena = __omni_arena_create(32 * 1024); // 32 KB Task Arena per fiber
    fiber->status = OMNI_FIBER_RUNNABLE;
    fiber->entry = entry;
    fiber->arg = arg;
    fiber->next = NULL;

    // 16-byte aligned stack top for System V x86_64 ABI
    uintptr_t stack_top = (uintptr_t)fiber->stack_base + stack_size;
    stack_top &= ~((uintptr_t)15);

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
        if (fiber->stack_base) {
            free(fiber->stack_base);
            fiber->stack_base = NULL;
        }
        free(fiber);
    }
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
