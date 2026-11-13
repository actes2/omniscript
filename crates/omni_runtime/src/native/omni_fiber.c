#if defined(_WIN32)
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0601
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#define OMNI_HAS_WIN32_FIBERS 1
#elif defined(__unix__) || defined(__APPLE__) || defined(__linux__)
#include <sys/mman.h>
#include <unistd.h>
#include <pthread.h>
#ifndef MAP_ANONYMOUS
#define MAP_ANONYMOUS MAP_ANON
#endif
#define OMNI_HAS_MMAP 1
#endif

#include "omni_fiber.h"
#include <stdlib.h>
#include <string.h>

#ifdef _MSC_VER
#define OMNI_FIBER_TLS __declspec(thread)
#else
#define OMNI_FIBER_TLS _Thread_local
#endif

static OMNI_FIBER_TLS OmniFiber* g_current_fiber = NULL;
static OMNI_FIBER_TLS OmniFiber g_main_fiber = {0};
static OMNI_FIBER_TLS OmniFiber* g_dead_fibers = NULL;
static OMNI_FIBER_TLS int g_initialized = 0;

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
#elif defined(__aarch64__) && !defined(_WIN32)
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
#elif !defined(_WIN32)
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
        memset(&g_main_fiber, 0, sizeof(g_main_fiber));
#if defined(OMNI_HAS_WIN32_FIBERS)
        g_main_fiber.win_fiber_handle = IsThreadAFiber()
            ? GetCurrentFiber()
            : ConvertThreadToFiberEx(NULL, 0);
#endif
        g_main_fiber.status = OMNI_FIBER_RUNNING;
        g_current_fiber = &g_main_fiber;
        g_initialized = 1;
    }
}

int __omni_fiber_is_in_fiber(void) {
    __omni_fiber_init();
    return (g_current_fiber != NULL && g_current_fiber != &g_main_fiber);
}

#if defined(OMNI_HAS_WIN32_FIBERS)
static VOID WINAPI win32_fiber_entry_stub(LPVOID param) {
    OmniFiber* f = (OmniFiber*)param;
    if (f && f->entry) {
        f->entry(f->arg);
    }
    if (f) {
        f->status = OMNI_FIBER_DEAD;
        if (f->arena) {
            __omni_arena_destroy(f->arena);
            f->arena = NULL;
        }
        f->next = g_dead_fibers;
        g_dead_fibers = f;
    }
    g_current_fiber = &g_main_fiber;
    SwitchToFiber(g_main_fiber.win_fiber_handle);
}
#else
static void fiber_entry_stub(void) {
    OmniFiber* f = g_current_fiber;
    if (f && f->entry) {
        f->entry(f->arg);
    }
    if (f) {
        f->status = OMNI_FIBER_DEAD;
        if (f->arena) {
            __omni_arena_destroy(f->arena);
            f->arena = NULL;
        }
        f->next = g_dead_fibers;
        g_dead_fibers = f;
    }

    OmniFiber* target = &g_main_fiber;
    g_current_fiber = target;
    swap_g_context(&f->rsp, target->rsp);
}
#endif

OmniFiber* __omni_fiber_create(void (*entry)(void*), void* arg, size_t stack_size) {
    __omni_fiber_init();

    if (stack_size < 65536) {
        stack_size = 65536;
    }

    OmniFiber* fiber = (OmniFiber*)calloc(1, sizeof(OmniFiber));
    if (!fiber) return NULL;

    fiber->arena = __omni_arena_create(32 * 1024);
    fiber->status = OMNI_FIBER_RUNNABLE;
    fiber->entry = entry;
    fiber->arg = arg;
    fiber->next = NULL;

#if defined(OMNI_HAS_WIN32_FIBERS)
    fiber->stack_size = stack_size;
    fiber->win_fiber_handle = CreateFiberEx(stack_size, stack_size, 0, win32_fiber_entry_stub, fiber);
    if (!fiber->win_fiber_handle) {
        if (fiber->arena) __omni_arena_destroy(fiber->arena);
        free(fiber);
        return NULL;
    }
#else
    size_t page_size = 4096;
#if defined(OMNI_HAS_MMAP)
    long sc_page = sysconf(_SC_PAGESIZE);
    if (sc_page > 0) page_size = (size_t)sc_page;
#endif
    stack_size = (stack_size + page_size - 1) & ~(page_size - 1);
    size_t total_mmap_size = stack_size + page_size;

#if defined(OMNI_HAS_MMAP)
    void* mmap_base = mmap(NULL, total_mmap_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mmap_base == MAP_FAILED) {
        if (fiber->arena) __omni_arena_destroy(fiber->arena);
        free(fiber);
        return NULL;
    }
    mprotect(mmap_base, page_size, PROT_NONE);

    fiber->stack_base = (char*)mmap_base + page_size;
    fiber->stack_size = stack_size;
    fiber->total_mmap_size = total_mmap_size;
    fiber->guard_page_size = page_size;
#else
    fiber->stack_base = malloc(stack_size);
    if (!fiber->stack_base) {
        if (fiber->arena) __omni_arena_destroy(fiber->arena);
        free(fiber);
        return NULL;
    }
    fiber->stack_size = stack_size;
    fiber->total_mmap_size = 0;
    fiber->guard_page_size = 0;
#endif

    uintptr_t stack_top = (uintptr_t)fiber->stack_base + stack_size;
    stack_top &= ~((uintptr_t)15);

#if defined(__x86_64__)
    uint64_t* sp = (uint64_t*)stack_top;
    *(--sp) = 0;                          // 16-byte alignment padding before call retaddr
    *(--sp) = (uint64_t)fiber_entry_stub; // return address for retq
    *(--sp) = 0;                          // r15
    *(--sp) = 0;                          // r14
    *(--sp) = 0;                          // r13
    *(--sp) = 0;                          // r12
    *(--sp) = 0;                          // rbx
    *(--sp) = 0;                          // rbp
    fiber->rsp = (void*)sp;
#elif defined(__aarch64__)
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
#endif
    return fiber;
}

void __omni_fiber_yield(void) {
    __omni_fiber_init();
    if (g_current_fiber && g_current_fiber != &g_main_fiber) {
        OmniFiber* from = g_current_fiber;
        OmniFiber* to = &g_main_fiber;
        g_current_fiber = to;
#if defined(OMNI_HAS_WIN32_FIBERS)
        (void)from;
        SwitchToFiber(to->win_fiber_handle);
#else
        swap_g_context(&from->rsp, to->rsp);
#endif
        cleanup_dead_fibers();
    }
}

void __omni_fiber_destroy(OmniFiber* fiber) {
    if (fiber) {
        if (fiber->arena) {
            __omni_arena_destroy(fiber->arena);
            fiber->arena = NULL;
        }
#if defined(OMNI_HAS_WIN32_FIBERS)
        if (fiber->win_fiber_handle) {
            DeleteFiber(fiber->win_fiber_handle);
            fiber->win_fiber_handle = NULL;
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
    if (!__omni_fiber_is_in_fiber()) {
        return fn(arg);
    }
    return fn(arg);
}

void __omni_go_spawn(void (*entry)(void*), void* arg) {
    __omni_fiber_init();
    OmniFiber* fiber = __omni_fiber_create(entry, arg, 65536);
    if (!fiber) return;

    OmniFiber* from = g_current_fiber;
    g_current_fiber = fiber;
#if defined(OMNI_HAS_WIN32_FIBERS)
    (void)from;
    SwitchToFiber(fiber->win_fiber_handle);
#else
    swap_g_context(&from->rsp, fiber->rsp);
#endif
    cleanup_dead_fibers();
}

typedef struct OmniThreadCtx {
    void (*entry)(void*);
    void* arg;
} OmniThreadCtx;

#if defined(_WIN32)
static SRWLOCK g_thread_lock = SRWLOCK_INIT;
static HANDLE g_spawned_threads[256];
static size_t g_spawned_thread_count = 0;

static DWORD WINAPI omni_win32_thread_runner(LPVOID raw_ctx) {
    OmniThreadCtx* ctx = (OmniThreadCtx*)raw_ctx;
    void (*entry)(void*) = ctx->entry;
    void* arg = ctx->arg;
    free(ctx);
    if (entry) {
        entry(arg);
    }
    return 0;
}

void __omni_thread_spawn(void (*entry)(void*), void* arg) {
    OmniThreadCtx* ctx = (OmniThreadCtx*)malloc(sizeof(OmniThreadCtx));
    if (!ctx) return;
    ctx->entry = entry;
    ctx->arg = arg;
    HANDLE th = CreateThread(NULL, 0, omni_win32_thread_runner, ctx, 0, NULL);
    if (th != NULL) {
        AcquireSRWLockExclusive(&g_thread_lock);
        if (g_spawned_thread_count < 256) {
            g_spawned_threads[g_spawned_thread_count++] = th;
            ReleaseSRWLockExclusive(&g_thread_lock);
        } else {
            ReleaseSRWLockExclusive(&g_thread_lock);
            CloseHandle(th);
        }
    } else {
        free(ctx);
    }
}

void __omni_thread_join_all(void) {
    AcquireSRWLockExclusive(&g_thread_lock);
    size_t count = g_spawned_thread_count;
    HANDLE local_handles[256];
    for (size_t i = 0; i < count; ++i) {
        local_handles[i] = g_spawned_threads[i];
    }
    g_spawned_thread_count = 0;
    ReleaseSRWLockExclusive(&g_thread_lock);

    for (size_t i = 0; i < count; ++i) {
        WaitForSingleObject(local_handles[i], INFINITE);
        CloseHandle(local_handles[i]);
    }
}
#else
static pthread_mutex_t g_thread_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_t g_spawned_threads[256];
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
        pthread_mutex_lock(&g_thread_lock);
        if (g_spawned_thread_count < 256) {
            g_spawned_threads[g_spawned_thread_count++] = tid;
            pthread_mutex_unlock(&g_thread_lock);
        } else {
            pthread_mutex_unlock(&g_thread_lock);
            pthread_detach(tid);
        }
    } else {
        free(ctx);
    }
}

void __omni_thread_join_all(void) {
    pthread_mutex_lock(&g_thread_lock);
    size_t count = g_spawned_thread_count;
    pthread_t local_tids[256];
    for (size_t i = 0; i < count; ++i) {
        local_tids[i] = g_spawned_threads[i];
    }
    g_spawned_thread_count = 0;
    pthread_mutex_unlock(&g_thread_lock);

    for (size_t i = 0; i < count; ++i) {
        pthread_join(local_tids[i], NULL);
    }
}
#endif
