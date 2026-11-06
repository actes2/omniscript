#include "omni_map.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <inttypes.h>

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

    map->capacity = cap;
    map->count = 0;
    map->tombstones = 0;
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

void __omni_map_set(OmniMap* map, const char* key, int64_t val) {
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

    for (size_t i = 0; i < map->capacity; i++) {
        size_t cur = (idx + i) & mask;
        if (map->buckets[cur].occupied) {
            if (map->buckets[cur].deleted) {
                if (first_deleted == (size_t)-1) {
                    first_deleted = cur;
                }
            } else if (map->buckets[cur].hash == h && strcmp(map->buckets[cur].key, key) == 0) {
                // Key already exists, update value in place
                map->buckets[cur].val = val;
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
        map->count++;
    }
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
    if (!map) return 0;
    return map->count;
}

void __omni_map_destroy(OmniMap* map) {
    if (!map) return;
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

void __omni_map_set_int(OmniMap* map, int64_t key, int64_t val) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%" PRId64, key);
    __omni_map_set(map, buf, val);
}

int64_t __omni_map_get_int(OmniMap* map, int64_t key) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%" PRId64, key);
    return __omni_map_get(map, buf);
}

bool __omni_map_contains_int(OmniMap* map, int64_t key) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%" PRId64, key);
    return __omni_map_contains(map, buf);
}
