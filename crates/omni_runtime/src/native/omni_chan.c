#include "omni_chan.h"
#include "omni_fiber.h"
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
static inline void omni_mutex_init(omni_mutex_t* m) { InitializeSRWLock(m); }
static inline void omni_mutex_lock(omni_mutex_t* m) { AcquireSRWLockExclusive(m); }
static inline void omni_mutex_unlock(omni_mutex_t* m) { ReleaseSRWLockExclusive(m); }
static inline void omni_mutex_destroy(omni_mutex_t* m) { (void)m; }
static inline void omni_cond_init(omni_cond_t* c) { InitializeConditionVariable(c); }
static inline void omni_cond_wait(omni_cond_t* c, omni_mutex_t* m) {
    SleepConditionVariableSRW(c, m, INFINITE, 0);
}
static inline void omni_cond_signal(omni_cond_t* c) { WakeConditionVariable(c); }
static inline void omni_cond_broadcast(omni_cond_t* c) { WakeAllConditionVariable(c); }
static inline void omni_cond_destroy(omni_cond_t* c) { (void)c; }
#else
static inline void omni_mutex_init(omni_mutex_t* m) { pthread_mutex_init(m, NULL); }
static inline void omni_mutex_lock(omni_mutex_t* m) { pthread_mutex_lock(m); }
static inline void omni_mutex_unlock(omni_mutex_t* m) { pthread_mutex_unlock(m); }
static inline void omni_mutex_destroy(omni_mutex_t* m) { pthread_mutex_destroy(m); }
static inline void omni_cond_init(omni_cond_t* c) { pthread_cond_init(c, NULL); }
static inline void omni_cond_wait(omni_cond_t* c, omni_mutex_t* m) { pthread_cond_wait(c, m); }
static inline void omni_cond_signal(omni_cond_t* c) { pthread_cond_signal(c); }
static inline void omni_cond_broadcast(omni_cond_t* c) { pthread_cond_broadcast(c); }
static inline void omni_cond_destroy(omni_cond_t* c) { pthread_cond_destroy(c); }
#endif

OmniChan* __omni_chan_make(size_t capacity, size_t elem_size) {
    if (capacity == 0) capacity = 1; // Unbuffered channels default to slot size 1
    if (elem_size == 0) elem_size = sizeof(uint64_t);

    OmniChan* chan = (OmniChan*)malloc(sizeof(OmniChan));
    if (!chan) return NULL;

    chan->buffer = (uint8_t*)malloc(capacity * elem_size);
    if (!chan->buffer) {
        free(chan);
        return NULL;
    }

    chan->capacity = capacity;
    chan->elem_size = elem_size;
    chan->head = 0;
    chan->tail = 0;
    chan->count = 0;
    chan->closed = false;

    omni_mutex_init(&chan->lock);
    omni_cond_init(&chan->not_empty);
    omni_cond_init(&chan->not_full);

    return chan;
}

bool __omni_chan_send(OmniChan* chan, const void* elem_ptr) {
    if (!chan || !elem_ptr) return false;

    omni_mutex_lock(&chan->lock);

    while (chan->count == chan->capacity && !chan->closed) {
        if (__omni_fiber_is_in_fiber()) {
            omni_mutex_unlock(&chan->lock);
            __omni_fiber_yield();
            omni_mutex_lock(&chan->lock);
        } else {
            omni_cond_wait(&chan->not_full, &chan->lock);
        }
    }

    if (chan->closed) {
        omni_mutex_unlock(&chan->lock);
        return false;
    }

    // Copy element into buffer at tail
    uint8_t* dest = chan->buffer + (chan->tail * chan->elem_size);
    memcpy(dest, elem_ptr, chan->elem_size);

    chan->tail = (chan->tail + 1) % chan->capacity;
    chan->count++;

    omni_cond_signal(&chan->not_empty);
    omni_mutex_unlock(&chan->lock);
    return true;
}

bool __omni_chan_recv(OmniChan* chan, void* out_elem_ptr) {
    if (!chan || !out_elem_ptr) return false;

    omni_mutex_lock(&chan->lock);

    while (chan->count == 0 && !chan->closed) {
        if (__omni_fiber_is_in_fiber()) {
            omni_mutex_unlock(&chan->lock);
            __omni_fiber_yield();
            omni_mutex_lock(&chan->lock);
        } else {
            omni_cond_wait(&chan->not_empty, &chan->lock);
        }
    }

    if (chan->count == 0 && chan->closed) {
        omni_mutex_unlock(&chan->lock);
        return false; // Closed and empty
    }

    // Copy element out of buffer at head
    uint8_t* src = chan->buffer + (chan->head * chan->elem_size);
    memcpy(out_elem_ptr, src, chan->elem_size);

    chan->head = (chan->head + 1) % chan->capacity;
    chan->count--;

    omni_cond_signal(&chan->not_full);
    omni_mutex_unlock(&chan->lock);
    return true;
}

bool __omni_chan_send_int(OmniChan* chan, int64_t val) {
    if (!chan) return false;
    return __omni_chan_send(chan, &val);
}

int64_t __omni_chan_recv_int(OmniChan* chan) {
    if (!chan) return 0;
    int64_t val = 0;
    if (__omni_chan_recv(chan, &val)) {
        return val;
    }
    return 0;
}

size_t __omni_chan_len(OmniChan* chan) {
    if (!chan) return 0;
    omni_mutex_lock(&chan->lock);
    size_t count = chan->count;
    omni_mutex_unlock(&chan->lock);
    return count;
}

void __omni_chan_close(OmniChan* chan) {
    if (!chan) return;
    omni_mutex_lock(&chan->lock);
    chan->closed = true;
    omni_cond_broadcast(&chan->not_empty);
    omni_cond_broadcast(&chan->not_full);
    omni_mutex_unlock(&chan->lock);
}

void __omni_chan_destroy(OmniChan* chan) {
    if (!chan) return;
    omni_mutex_lock(&chan->lock);
    chan->closed = true;
    omni_cond_broadcast(&chan->not_empty);
    omni_cond_broadcast(&chan->not_full);
    omni_mutex_unlock(&chan->lock);

    omni_mutex_destroy(&chan->lock);
    omni_cond_destroy(&chan->not_empty);
    omni_cond_destroy(&chan->not_full);
    if (chan->buffer) {
        free(chan->buffer);
        chan->buffer = NULL;
    }
    free(chan);
}
