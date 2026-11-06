#ifndef OMNI_ARENA_H
#define OMNI_ARENA_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint8_t* buffer;
    size_t capacity;
    size_t offset;
} OmniArena;

OmniArena* __omni_arena_create(size_t capacity);
void* __omni_arena_alloc(OmniArena* arena, size_t size, size_t align);
size_t __omni_arena_mark(OmniArena* arena);
void __omni_arena_rewind(OmniArena* arena, size_t mark);
void __omni_arena_reset(OmniArena* arena);
void __omni_arena_destroy(OmniArena* arena);

#endif // OMNI_ARENA_H
