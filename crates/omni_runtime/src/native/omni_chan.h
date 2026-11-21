#ifndef OMNI_CHAN_H
#define OMNI_CHAN_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

#if defined(_WIN32)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
typedef SRWLOCK omni_mutex_t;
typedef CONDITION_VARIABLE omni_cond_t;
#else
#include <pthread.h>
typedef pthread_mutex_t omni_mutex_t;
typedef pthread_cond_t omni_cond_t;
#endif

#define OMNI_CHAN_MAGIC 0x4348414E4F4D4EFFULL

typedef struct {
    uint64_t magic;
    uint32_t rc;
    uint32_t flags;
    uint8_t* buffer;
    size_t capacity;
    size_t elem_size;
    size_t head;
    size_t tail;
    size_t count;
    bool closed;
    omni_mutex_t lock;
    omni_cond_t not_empty;
    omni_cond_t not_full;
} OmniChan;

static inline bool __omni_is_chan_ptr(const void* ptr) {
    const unsigned char* p = (const unsigned char*)ptr;
    if (!p || (uintptr_t)p < 4096) return false;
#if UINTPTR_MAX > 0xFFFFFFFFu
    if (((uintptr_t)p >> 48) != 0u) return false;
#endif
    if ((((uintptr_t)p & 7u) != 0u) || p[0] != 0xFF) return false;
    return ((const OmniChan*)ptr)->magic == OMNI_CHAN_MAGIC;
}

OmniChan* __omni_chan_make(size_t capacity, size_t elem_size);
bool __omni_chan_send(OmniChan* chan, const void* elem_ptr);
bool __omni_chan_recv(OmniChan* chan, void* out_elem_ptr);
bool __omni_chan_send_int(OmniChan* chan, int64_t val);
int64_t __omni_chan_recv_int(OmniChan* chan);
size_t __omni_chan_len(OmniChan* chan);
void __omni_chan_close(OmniChan* chan);
void __omni_chan_destroy(OmniChan* chan);

#endif // OMNI_CHAN_H
