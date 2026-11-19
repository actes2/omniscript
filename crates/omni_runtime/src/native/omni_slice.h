#ifndef OMNI_SLICE_H
#define OMNI_SLICE_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#define OMNI_SLICE_MAGIC 0x534C49434F4D4EFFULL

typedef struct {
    uint64_t magic;
    int64_t* data;
    size_t len;
    size_t cap;
} OmniSlice;

static inline bool __omni_is_slice_ptr(const void* ptr) {
    const unsigned char* p = (const unsigned char*)ptr;
    if (!p || (uintptr_t)p < 4096) return false;
    if (p[0] != 0xFF || (((uintptr_t)p & 7u) != 0u)) return false;
    return ((const OmniSlice*)ptr)->magic == OMNI_SLICE_MAGIC;
}

OmniSlice* __omni_slice_create(size_t initial_cap);
void __omni_slice_append(OmniSlice* slice, int64_t val);
int64_t __omni_slice_get(OmniSlice* slice, int64_t index);
void __omni_slice_set(OmniSlice* slice, int64_t index, int64_t val);
size_t __omni_slice_len(OmniSlice* slice);
int64_t __omni_slice_pop(OmniSlice* slice);
OmniSlice* __omni_slice_subslice(OmniSlice* slice, int64_t start, int64_t end, int64_t step, int64_t has_start, int64_t has_end);
int64_t __omni_slice_contains(OmniSlice* slice, int64_t val, int64_t is_str);
void __omni_slice_print(OmniSlice* slice);
void __omni_slice_destroy(OmniSlice* slice);

#endif // OMNI_SLICE_H
