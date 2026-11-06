#include "omni_fiber.h"
#include <stdlib.h>
#include <string.h>

static OmniFiber* g_current_fiber = NULL;
static OmniFiber g_main_fiber = {0};
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

void __omni_fiber_init(void) {
    if (!g_initialized) {
        g_main_fiber.rsp = NULL;
        g_main_fiber.status = OMNI_FIBER_RUNNING;
        g_main_fiber.arena = NULL;
        g_current_fiber = &g_main_fiber;
        g_initialized = 1;
    }
}

static void fiber_entry_stub(void) {
    OmniFiber* f = g_current_fiber;
    if (f && f->entry) {
        f->entry(f->arg);
    }
    if (f) {
        f->status = OMNI_FIBER_DEAD;
        // Reclaim fiber Task Arena in O(1)!
        if (f->arena) {
            __omni_arena_destroy(f->arena);
            f->arena = NULL;
        }
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

    // 16-byte aligned stack top
    uintptr_t stack_top = (uintptr_t)fiber->stack_base + stack_size;
    stack_top &= ~((uintptr_t)15);

    // Push initial fiber frame for swap_g_context:
    // [fiber_entry_stub address], [r15=0], [r14=0], [r13=0], [r12=0], [rbx=0], [rbp=0]
    uint64_t* sp = (uint64_t*)stack_top;
    *(--sp) = (uint64_t)fiber_entry_stub;
    *(--sp) = 0; // r15
    *(--sp) = 0; // r14
    *(--sp) = 0; // r13
    *(--sp) = 0; // r12
    *(--sp) = 0; // rbx
    *(--sp) = 0; // rbp

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
}
