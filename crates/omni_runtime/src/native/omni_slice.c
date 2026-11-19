#include "omni_slice.h"
#include "omni_map.h"
#include "omni_chan.h"
#include "omni_value.h"
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

    slice->magic = OMNI_SLICE_MAGIC;
    slice->len = 0;
    slice->cap = initial_cap;
    return slice;
}

void __omni_slice_append(OmniSlice* slice, int64_t val) {
    if (!__omni_is_slice_ptr(slice)) return;

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
    if (!__omni_is_slice_ptr(slice)) {
        if (__omni_is_map_ptr(slice)) {
            return __omni_map_get_int((OmniMap*)slice, index);
        }
        if (__omni_is_chan_ptr(slice)) {
            return 0;
        }
        // Polymorphic string-as-array fallback when untyped `i8*` C string reaches `slice_get`
        return (int64_t)(uintptr_t)__omni_str_char_at((const char*)slice, index);
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
    if (!__omni_is_slice_ptr(slice)) {
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
    const unsigned char* p = (const unsigned char*)slice;
    if (!p || (uintptr_t)p < 4096) return 0;
    if (p[0] == 0) return 0;
    if (__omni_is_slice_ptr(slice)) {
        return slice->len;
    }
    if (__omni_is_map_ptr(slice)) {
        return __omni_map_len((OmniMap*)slice);
    }
    if (__omni_is_chan_ptr(slice)) {
        return __omni_chan_len((OmniChan*)slice);
    }
    // Polymorphic string-as-array fallback when untyped `i8*` C string reaches `len(x)`
    return strlen((const char*)slice);
}

int64_t __omni_slice_pop(OmniSlice* slice) {
    if (!__omni_is_slice_ptr(slice) || slice->len == 0) return 0;
    slice->len--;
    return slice->data[slice->len];
}

OmniSlice* __omni_slice_subslice(
    OmniSlice* slice,
    int64_t start,
    int64_t end,
    int64_t step,
    int64_t has_start,
    int64_t has_end
) {
    if (step == 0) step = 1;
    if (!__omni_is_slice_ptr(slice)) {
        OmniSlice* empty = __omni_slice_create(4);
        __omni_c_return_ptr(empty);
        return empty;
    }
    int64_t n = (int64_t)slice->len;
    int64_t s, e;
    if (step > 0) {
        s = has_start ? start : 0;
        e = has_end ? end : n;
        if (s < 0) s += n;
        if (s < 0) s = 0;
        if (s > n) s = n;
        if (e < 0) e += n;
        if (e < 0) e = 0;
        if (e > n) e = n;
        size_t cap = (e > s) ? (size_t)((e - s + step - 1) / step) : 4;
        OmniSlice* out = __omni_slice_create(cap < 4 ? 4 : cap);
        for (int64_t i = s; i < e; i += step) {
            __omni_slice_append(out, slice->data[i]);
        }
        __omni_c_return_ptr(out);
        return out;
    } else {
        s = has_start ? start : (n - 1);
        e = has_end ? end : -1;
        if (has_start) {
            if (s < 0) s += n;
            if (s < -1) s = -1;
            if (s >= n) s = n - 1;
        }
        if (has_end) {
            if (e < 0) e += n;
            if (e < -1) e = -1;
            if (e >= n) e = n - 1;
        }
        OmniSlice* out = __omni_slice_create(n > 4 ? (size_t)n : 4);
        for (int64_t i = s; i > e; i += step) {
            if (i >= 0 && i < n) {
                __omni_slice_append(out, slice->data[i]);
            }
        }
        __omni_c_return_ptr(out);
        return out;
    }
}

int64_t __omni_slice_contains(OmniSlice* slice, int64_t val, int64_t is_str) {
    if (!slice || (uintptr_t)slice < 4096) return 0;
    if (!__omni_is_slice_ptr(slice)) {
        if (is_str && val != 0 && (uintptr_t)val >= 4096) {
            return __omni_str_contains((const char*)slice, (const char*)(uintptr_t)val);
        }
        return 0;
    }
    for (size_t i = 0; i < slice->len; i++) {
        if (is_str) {
            const char* elem_s = (const char*)(uintptr_t)slice->data[i];
            const char* target_s = (const char*)(uintptr_t)val;
            if (elem_s == target_s) return 1;
            if (elem_s && (uintptr_t)elem_s >= 4096 && target_s && (uintptr_t)target_s >= 4096) {
                if (strcmp(elem_s, target_s) == 0) return 1;
            }
        } else if (slice->data[i] == val) {
            return 1;
        }
    }
    return 0;
}

void __omni_slice_print(OmniSlice* slice) {
    if (!__omni_is_slice_ptr(slice)) {
        if (slice && (uintptr_t)slice >= 4096) {
            printf("%s\n", (const char*)slice);
            return;
        }
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
    if (!__omni_is_slice_ptr(slice)) return;
    slice->magic = 0;
    if (slice->data) {
        free(slice->data);
        slice->data = NULL;
    }
    free(slice);
}
