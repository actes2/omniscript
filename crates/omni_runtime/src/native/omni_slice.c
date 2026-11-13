#include "omni_slice.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

OmniSlice* __omni_slice_create(size_t initial_cap) {
    if (initial_cap == 0) {
        initial_cap = 4;
    }

    OmniSlice* slice = (OmniSlice*)malloc(sizeof(OmniSlice));
    if (!slice) return NULL;

    slice->data = (int64_t*)malloc(initial_cap * sizeof(int64_t));
    if (!slice->data) {
        free(slice);
        return NULL;
    }

    slice->len = 0;
    slice->cap = initial_cap;
    return slice;
}

void __omni_slice_append(OmniSlice* slice, int64_t val) {
    if (!slice) return;

    if (slice->len >= slice->cap) {
        size_t new_cap = slice->cap * 2;
        if (new_cap < 4) new_cap = 4;
        int64_t* new_data = (int64_t*)realloc(slice->data, new_cap * sizeof(int64_t));
        if (!new_data) return;
        slice->data = new_data;
        slice->cap = new_cap;
    }

    slice->data[slice->len++] = val;
}

int64_t __omni_slice_get(OmniSlice* slice, int64_t index) {
    if (!slice || (uintptr_t)slice < 4096) {
        return 0;
    }
    if (index < 0) {
        index += (int64_t)slice->len;
    }
    if (index < 0 || (size_t)index >= slice->len) {
        return 0;
    }
    return slice->data[index];
}

void __omni_slice_set(OmniSlice* slice, int64_t index, int64_t val) {
    if (!slice || (uintptr_t)slice < 4096) {
        return;
    }
    if (index < 0) {
        index += (int64_t)slice->len;
    }
    if (index < 0 || (size_t)index >= slice->len) {
        return;
    }
    slice->data[index] = val;
}

size_t __omni_slice_len(OmniSlice* slice) {
    if (!slice || (uintptr_t)slice < 4096) return 0;
    return slice->len;
}

int64_t __omni_slice_pop(OmniSlice* slice) {
    if (!slice || slice->len == 0) return 0;
    slice->len--;
    return slice->data[slice->len];
}

void __omni_slice_print(OmniSlice* slice) {
    if (!slice) {
        printf("[]\n");
        return;
    }
    printf("[");
    for (size_t i = 0; i < slice->len; i++) {
        if (i > 0) printf(", ");
        printf("%lld", (long long)slice->data[i]);
    }
    printf("]\n");
}

void __omni_slice_destroy(OmniSlice* slice) {
    if (!slice) return;
    if (slice->data) {
        free(slice->data);
        slice->data = NULL;
    }
    free(slice);
}
