#ifndef OMNI_MAP_H
#define OMNI_MAP_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct {
    char* key;
    int64_t val;
    uint64_t hash;
    bool occupied;
    bool deleted;
} OmniMapBucket;

typedef struct {
    OmniMapBucket* buckets;
    size_t capacity;
    size_t count;
} OmniMap;

OmniMap* __omni_map_create(size_t initial_cap);
void __omni_map_set(OmniMap* map, const char* key, int64_t val);
int64_t __omni_map_get(OmniMap* map, const char* key);
bool __omni_map_contains(OmniMap* map, const char* key);
void __omni_map_delete(OmniMap* map, const char* key);
size_t __omni_map_len(OmniMap* map);
void __omni_map_destroy(OmniMap* map);

// Integer key convenience helpers
void __omni_map_set_int(OmniMap* map, int64_t key, int64_t val);
int64_t __omni_map_get_int(OmniMap* map, int64_t key);
bool __omni_map_contains_int(OmniMap* map, int64_t key);

#endif // OMNI_MAP_H
