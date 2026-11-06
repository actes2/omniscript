#ifndef OMNI_SLICE_H
#define OMNI_SLICE_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    int64_t* data;
    size_t len;
    size_t cap;
} OmniSlice;

OmniSlice* __omni_slice_create(size_t initial_cap);
void __omni_slice_append(OmniSlice* slice, int64_t val);
int64_t __omni_slice_get(OmniSlice* slice, size_t index);
void __omni_slice_set(OmniSlice* slice, size_t index, int64_t val);
size_t __omni_slice_len(OmniSlice* slice);
int64_t __omni_slice_pop(OmniSlice* slice);
void __omni_slice_destroy(OmniSlice* slice);

#endif // OMNI_SLICE_H
