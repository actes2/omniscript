#include "omni_map.h"
#include "omni_slice.h"
#include "omni_value.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <inttypes.h>

#ifdef _WIN32
#define strdup _strdup
#endif

static uint64_t fnv1a_hash(const char* str) {
    uint64_t hash = 14695981039346656037ULL;
    for (const unsigned char* p = (const unsigned char*)str; *p; p++) {
        hash ^= (uint64_t)(*p);
        hash *= 1099511628211ULL;
    }
    return hash;
}

static size_t next_power_of_two(size_t n) {
    if (n < 8) return 8;
    size_t p = 1;
    while (p < n) {
        p <<= 1;
    }
    return p;
}

OmniMap* __omni_map_create(size_t initial_cap) {
    OmniMap* map = (OmniMap*)malloc(sizeof(OmniMap));
    if (!map) return NULL;

    size_t cap = next_power_of_two(initial_cap);
    map->buckets = (OmniMapBucket*)calloc(cap, sizeof(OmniMapBucket));
    if (!map->buckets) {
        free(map);
        return NULL;
    }

    map->magic = OMNI_MAP_MAGIC;
    map->rc = 1;
    map->flags = 0;
    map->capacity = cap;
    map->count = 0;
    map->tombstones = 0;
    __omni_reg_track(map, 3);
    return map;
}

static void map_resize(OmniMap* map, size_t new_cap) {
    OmniMapBucket* old_buckets = map->buckets;
    size_t old_cap = map->capacity;

    OmniMapBucket* new_buckets = (OmniMapBucket*)calloc(new_cap, sizeof(OmniMapBucket));
    if (!new_buckets) return;

    map->buckets = new_buckets;
    map->capacity = new_cap;
    map->count = 0;
    map->tombstones = 0;

    size_t mask = new_cap - 1;

    for (size_t i = 0; i < old_cap; i++) {
        if (old_buckets[i].occupied && !old_buckets[i].deleted) {
            uint64_t h = old_buckets[i].hash;
            size_t idx = (size_t)(h & mask);

            for (size_t j = 0; j < new_cap; j++) {
                size_t cur = (idx + j) & mask;
                if (!new_buckets[cur].occupied) {
                    // Direct pointer transfer without redundant strdup or memory re-allocation
                    new_buckets[cur].key = old_buckets[i].key;
                    new_buckets[cur].val = old_buckets[i].val;
                    new_buckets[cur].hash = h;
                    new_buckets[cur].occupied = true;
                    new_buckets[cur].deleted = false;
                    new_buckets[cur].val_kind = old_buckets[i].val_kind;
                    map->count++;
                    break;
                }
            }
        } else if (old_buckets[i].occupied && old_buckets[i].deleted && old_buckets[i].key) {
            free(old_buckets[i].key);
        }
    }

    free(old_buckets);
}

void __omni_map_set_typed(OmniMap* map, const char* key, int64_t val, int64_t kind) {
    if (!map || !key) return;

    // Load factor considers both active count and dead tombstones to prevent O(N) probing degradation
    if ((map->count + map->tombstones + 1) * 10 >= map->capacity * 7) {
        size_t new_cap = (map->count * 2 >= map->capacity) ? map->capacity * 2 : map->capacity;
        map_resize(map, new_cap);
    }

    uint64_t h = fnv1a_hash(key);
    size_t mask = map->capacity - 1;
    size_t idx = (size_t)(h & mask);
    size_t first_deleted = (size_t)-1;
    uint8_t k8 = (uint8_t)(kind & 0xFF);

    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (map->buckets[cur].occupied) {
            if (map->buckets[cur].deleted) {
                if (first_deleted == (size_t)-1) {
                    first_deleted = cur;
                }
            } else if (map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
                // Key already exists, update value and kind in place
                map->buckets[cur].val = val;
                map->buckets[cur].val_kind = k8;
                return;
            }
        } else {
            // Found unoccupied slot
            size_t insert_idx = (first_deleted != (size_t)-1) ? first_deleted : cur;
            if (insert_idx == first_deleted && map->tombstones > 0) {
                map->tombstones--;
            }
            map->buckets[insert_idx].key = strdup(key);
            map->buckets[insert_idx].val = val;
            map->buckets[insert_idx].hash = h;
            map->buckets[insert_idx].occupied = true;
            map->buckets[insert_idx].deleted = false;
            map->buckets[insert_idx].val_kind = k8;
            map->count++;
            return;
        }
    }

    if (first_deleted != (size_t)-1) {
        if (map->tombstones > 0) map->tombstones--;
        map->buckets[first_deleted].key = strdup(key);
        map->buckets[first_deleted].val = val;
        map->buckets[first_deleted].hash = h;
        map->buckets[first_deleted].occupied = true;
        map->buckets[first_deleted].deleted = false;
        map->buckets[first_deleted].val_kind = k8;
        map->count++;
    }
}

void __omni_map_set(OmniMap* map, const char* key, int64_t val) {
    __omni_map_set_typed(map, key, val, OMNI_MAP_VAL_INT);
}

int64_t __omni_map_get(OmniMap* map, const char* key) {
    if (!map || !key || map->count == 0) return 0;

    uint64_t h = fnv1a_hash(key);
    size_t mask = map->capacity - 1;
    size_t idx = (size_t)(h & mask);

    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (!map->buckets[cur].occupied) {
            return 0; // Not found
        }
        if (!map->buckets[cur].deleted && map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
            return map->buckets[cur].val;
        }
    }
    return 0;
}

bool __omni_map_contains(OmniMap* map, const char* key) {
    if (!map || !key || map->count == 0) return false;

    uint64_t h = fnv1a_hash(key);
    size_t mask = map->capacity - 1;
    size_t idx = (size_t)(h & mask);

    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (!map->buckets[cur].occupied) {
            return false;
        }
        if (!map->buckets[cur].deleted && map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
            return true;
        }
    }
    return false;
}

void __omni_map_delete(OmniMap* map, const char* key) {
    if (!map || !key || map->count == 0) return;

    uint64_t h = fnv1a_hash(key);
    size_t mask = map->capacity - 1;
    size_t idx = (size_t)(h & mask);

    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (!map->buckets[cur].occupied) {
            return;
        }
        if (!map->buckets[cur].deleted && map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
            map->buckets[cur].deleted = true;
            if (map->buckets[cur].key) {
                free(map->buckets[cur].key);
                map->buckets[cur].key = NULL;
            }
            if (map->count > 0) map->count--;
            map->tombstones++;
            return;
        }
    }
}

size_t __omni_map_len(OmniMap* map) {
    if (!__omni_is_map_ptr(map)) return 0;
    return map->count;
}

void __omni_map_destroy(OmniMap* map) {
    if (!__omni_is_map_ptr(map)) return;
    uint32_t cur = __atomic_load_n(&map->rc, __ATOMIC_ACQUIRE);
    if (cur == 0xFFFFFFFFu) return;
    if (__atomic_sub_fetch(&map->rc, 1u, __ATOMIC_ACQ_REL) > 0u) {
        return;
    }
    map->magic = 0;
    __omni_reg_untrack(map);
    if (map->buckets) {
        for (size_t i = 0; i < map->capacity; i++) {
            if (map->buckets[i].occupied && !map->buckets[i].deleted && map->buckets[i].key) {
                free(map->buckets[i].key);
            }
        }
        free(map->buckets);
        map->buckets = NULL;
    }
    free(map);
}

void __omni_map_set_int_typed(OmniMap* map, int64_t key, int64_t val, int64_t kind) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    __omni_map_set_typed(map, buf, val, kind);
}

void __omni_map_set_int(OmniMap* map, int64_t key, int64_t val) {
    __omni_map_set_int_typed(map, key, val, OMNI_MAP_VAL_INT);
}

int64_t __omni_map_get_int(OmniMap* map, int64_t key) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    return __omni_map_get(map, buf);
}

bool __omni_map_contains_int(OmniMap* map, int64_t key) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    return __omni_map_contains(map, buf);
}

void __omni_map_delete_int(OmniMap* map, int64_t key) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    __omni_map_delete(map, buf);
}

static OmniMapBucket* map_find_bucket(OmniMap* map, const char* key) {
    if (!__omni_is_map_ptr(map) || !key || map->count == 0) return NULL;
    uint64_t h = fnv1a_hash(key);
    size_t mask = map->capacity - 1;
    size_t idx = (size_t)(h & mask);
    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (!map->buckets[cur].occupied) return NULL;
        if (!map->buckets[cur].deleted && map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
            return &map->buckets[cur];
        }
    }
    return NULL;
}

int64_t __omni_map_get_kind(OmniMap* map, const char* key) {
    OmniMapBucket* b = map_find_bucket(map, key);
    if (!b) return -1;
    return (int64_t)b->val_kind;
}

int64_t __omni_map_get_int_kind(OmniMap* map, int64_t key) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    return __omni_map_get_kind(map, buf);
}

typedef struct {
    char* data;
    size_t len;
    size_t cap;
} JsonBuf;

static void jbuf_append(JsonBuf* jb, const char* s, size_t n) {
    if (jb->len + n + 1 > jb->cap) {
        size_t ncap = jb->cap ? jb->cap * 2 : 128;
        while (ncap < jb->len + n + 1) ncap *= 2;
        char* nd = (char*)realloc(jb->data, ncap);
        if (!nd) return;
        jb->data = nd;
        jb->cap = ncap;
    }
    memcpy(jb->data + jb->len, s, n);
    jb->len += n;
    jb->data[jb->len] = '\0';
}

static void jbuf_cstr(JsonBuf* jb, const char* s) {
    jbuf_append(jb, s, strlen(s));
}

static void jbuf_json_str(JsonBuf* jb, const char* s) {
    jbuf_append(jb, "\"", 1);
    if (s) {
        for (const unsigned char* p = (const unsigned char*)s; *p; p++) {
            if (*p == '"') jbuf_append(jb, "\\\"", 2);
            else if (*p == '\\') jbuf_append(jb, "\\\\", 2);
            else if (*p == '\n') jbuf_append(jb, "\\n", 2);
            else if (*p == '\r') jbuf_append(jb, "\\r", 2);
            else if (*p == '\t') jbuf_append(jb, "\\t", 2);
            else jbuf_append(jb, (const char*)p, 1);
        }
    }
    jbuf_append(jb, "\"", 1);
}

static void map_append_json(JsonBuf* jb, OmniMap* map, int depth) {
    if (!__omni_is_map_ptr(map) || depth > 16) {
        jbuf_cstr(jb, "{}");
        return;
    }
    jbuf_append(jb, "{", 1);
    bool first = true;
    for (size_t i = 0; i < map->capacity; i++) {
        if (!map->buckets[i].occupied || map->buckets[i].deleted || !map->buckets[i].key) continue;
        if (!first) jbuf_append(jb, ", ", 2);
        first = false;
        const char* k = map->buckets[i].key;
        if (k[0] == '\x01' && k[1] == 'i' && k[2] == ':') {
            jbuf_json_str(jb, k + 3);
        } else {
            jbuf_json_str(jb, k);
        }
        jbuf_append(jb, ": ", 2);
        int64_t v = map->buckets[i].val;
        uint8_t kind = map->buckets[i].val_kind;
        char num[64];
        if (kind == OMNI_MAP_VAL_STR) {
            if (v && (uintptr_t)v >= 4096) jbuf_json_str(jb, (const char*)(uintptr_t)v);
            else jbuf_cstr(jb, "null");
        } else if (kind == OMNI_MAP_VAL_FLOAT) {
            double dv = 0.0;
            memcpy(&dv, &v, sizeof(double));
            snprintf(num, sizeof(num), "%g", dv);
            jbuf_cstr(jb, num);
        } else if (kind == OMNI_MAP_VAL_BOOL) {
            jbuf_cstr(jb, v ? "true" : "false");
        } else if (kind == OMNI_MAP_VAL_NULL || v == -581005069656919553LL || v == -581005069656919554LL) {
            jbuf_cstr(jb, "null");
        } else if (kind == OMNI_MAP_VAL_MAP && __omni_is_map_ptr((const void*)(uintptr_t)v)) {
            map_append_json(jb, (OmniMap*)(uintptr_t)v, depth + 1);
        } else {
            snprintf(num, sizeof(num), "%" PRId64, v);
            jbuf_cstr(jb, num);
        }
    }
    jbuf_append(jb, "}", 1);
}

char* __omni_map_to_json(OmniMap* map) {
    JsonBuf jb = { NULL, 0, 0 };
    map_append_json(&jb, map, 0);
    if (!jb.data) return strdup("{}");
    return jb.data;
}

void __omni_map_print(OmniMap* map) {
    char* s = __omni_map_to_json(map);
    printf("%s\n", s ? s : "{}");
    fflush(stdout);
    free(s);
}

void __omni_map_print_val(OmniMap* map, const char* key) {
    OmniMapBucket* b = map_find_bucket(map, key);
    if (!b) {
        printf("undefined\n");
        fflush(stdout);
        return;
    }
    int64_t v = b->val;
    switch (b->val_kind) {
        case OMNI_MAP_VAL_STR:
            if (v && (uintptr_t)v >= 4096) printf("%s\n", (const char*)(uintptr_t)v);
            else printf("null\n");
            break;
        case OMNI_MAP_VAL_FLOAT: {
            double dv = 0.0;
            memcpy(&dv, &v, sizeof(double));
            __omni_print_float(dv);
            return;
        }
        case OMNI_MAP_VAL_BOOL:
            printf("%s\n", v ? "true" : "false");
            break;
        case OMNI_MAP_VAL_NULL:
            printf("null\n");
            break;
        case OMNI_MAP_VAL_MAP:
            if (__omni_is_map_ptr((const void*)(uintptr_t)v)) {
                __omni_map_print((OmniMap*)(uintptr_t)v);
                return;
            }
            printf("%" PRId64 "\n", v);
            break;
        default:
            if (v == -581005069656919553LL || v == -581005069656919554LL) {
                printf("null\n");
            } else {
                printf("%" PRId64 "\n", v);
            }
            break;
    }
    fflush(stdout);
}

void __omni_map_print_int_val(OmniMap* map, int64_t key) {
    char buf[36];
    snprintf(buf, sizeof(buf), "\x01i:%" PRId64, key);
    __omni_map_print_val(map, buf);
}

void* __omni_map_keys(OmniMap* map) {
    size_t cap = (__omni_is_map_ptr(map) && map->count > 0) ? map->count : 4;
    OmniSlice* sl = __omni_slice_create(cap);
    if (!sl || !__omni_is_map_ptr(map)) return sl;
    for (size_t i = 0; i < map->capacity; i++) {
        if (map->buckets[i].occupied && !map->buckets[i].deleted && map->buckets[i].key) {
            const char* k = map->buckets[i].key;
            if (k[0] == '\x01' && k[1] == 'i' && k[2] == ':') k += 3;
            __omni_slice_append(sl, (int64_t)(uintptr_t)strdup(k));
        }
    }
    return sl;
}

void* __omni_map_values(OmniMap* map) {
    size_t cap = (__omni_is_map_ptr(map) && map->count > 0) ? map->count : 4;
    OmniSlice* sl = __omni_slice_create(cap);
    if (!sl || !__omni_is_map_ptr(map)) return sl;
    for (size_t i = 0; i < map->capacity; i++) {
        if (map->buckets[i].occupied && !map->buckets[i].deleted && map->buckets[i].key) {
            __omni_slice_append(sl, map->buckets[i].val);
        }
    }
    return sl;
}

int64_t __omni_map_get_or(OmniMap* map, const char* key, int64_t default_val) {
    OmniMapBucket* b = map_find_bucket(map, key);
    return b ? b->val : default_val;
}

int64_t __omni_map_setdefault_typed(OmniMap* map, const char* key, int64_t default_val, int64_t kind) {
    OmniMapBucket* b = map_find_bucket(map, key);
    if (b) return b->val;
    __omni_map_set_typed(map, key, default_val, kind);
    return default_val;
}

void __omni_map_update(OmniMap* dst, OmniMap* src) {
    if (!__omni_is_map_ptr(dst) || !__omni_is_map_ptr(src)) return;
    for (size_t i = 0; i < src->capacity; i++) {
        if (src->buckets[i].occupied && !src->buckets[i].deleted && src->buckets[i].key) {
            __omni_map_set_typed(dst, src->buckets[i].key, src->buckets[i].val, (int64_t)src->buckets[i].val_kind);
        }
    }
}

int64_t __omni_map_pop(OmniMap* map, const char* key, int64_t default_val) {
    OmniMapBucket* b = map_find_bucket(map, key);
    if (!b) return default_val;
    int64_t v = b->val;
    __omni_map_delete(map, key);
    return v;
}
