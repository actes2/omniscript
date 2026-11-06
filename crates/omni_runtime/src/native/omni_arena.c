#include "omni_arena.h"
#include <stdlib.h>
#include <string.h>

OmniArena* __omni_arena_create(size_t capacity) {
    if (capacity == 0) {
        capacity = 1024 * 64; // 64 KB default
    }
    OmniArena* arena = (OmniArena*)malloc(sizeof(OmniArena));
    if (!arena) return NULL;

    arena->buffer = (uint8_t*)malloc(capacity);
    if (!arena->buffer) {
        free(arena);
        return NULL;
    }
    arena->capacity = capacity;
    arena->offset = 0;
    return arena;
}

void* __omni_arena_alloc(OmniArena* arena, size_t size, size_t align) {
    if (!arena || size == 0) return NULL;

    if (align == 0) align = sizeof(void*);

    // Align offset upwards
    size_t current_addr = (size_t)(arena->buffer + arena->offset);
    size_t aligned_addr = (current_addr + (align - 1)) & ~(align - 1);
    size_t new_offset = (aligned_addr - (size_t)arena->buffer) + size;

    if (new_offset > arena->capacity) {
        return NULL; // Out of memory in fixed arena
    }

    arena->offset = new_offset;
    return (void*)aligned_addr;
}

void __omni_arena_reset(OmniArena* arena) {
    if (arena) {
        arena->offset = 0; // O(1) bulk free!
    }
}

void __omni_arena_destroy(OmniArena* arena) {
    if (arena) {
        if (arena->buffer) {
            free(arena->buffer);
        }
        free(arena);
    }
}
