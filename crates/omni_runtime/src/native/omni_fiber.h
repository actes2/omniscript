#ifndef OMNI_FIBER_H
#define OMNI_FIBER_H

#include "omni_arena.h"
#include <stddef.h>
#include <stdint.h>

#define OMNI_FIBER_RUNNABLE 0
#define OMNI_FIBER_RUNNING  1
#define OMNI_FIBER_WAITING  2
#define OMNI_FIBER_DEAD     3

typedef struct OmniFiber {
    void* rsp;                  // Current stack pointer (offset 0 for assembly)
    void* stack_base;           // Usable stack base (above guard page)
    size_t stack_size;          // Usable stack size
    size_t total_mmap_size;     // Total mapped size including guard page
    size_t guard_page_size;     // Guard page size
    void* win_fiber_handle;     // Native Win32 fiber handle when targeting _WIN32
    OmniArena* arena;           // Fiber-local Task Arena
    int status;
    void (*entry)(void*);
    void* arg;
    struct OmniFiber* next;
} OmniFiber;

void __omni_fiber_init(void);
int __omni_fiber_is_in_fiber(void);
OmniFiber* __omni_fiber_create(void (*entry)(void*), void* arg, size_t stack_size);
void __omni_fiber_yield(void);
void __omni_fiber_destroy(OmniFiber* fiber);
void __omni_go_spawn(void (*entry)(void*), void* arg);
void __omni_thread_spawn(void (*entry)(void*), void* arg);
void __omni_thread_join_all(void);
void* __omni_fiber_run_on_main_stack(void* (*fn)(void*), void* arg);

#endif // OMNI_FIBER_H
