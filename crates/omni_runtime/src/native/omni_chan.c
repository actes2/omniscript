#include "omni_chan.h"
#include <stdlib.h>
#include <string.h>

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

    pthread_mutex_init(&chan->lock, NULL);
    pthread_cond_init(&chan->not_empty, NULL);
    pthread_cond_init(&chan->not_full, NULL);

    return chan;
}

bool __omni_chan_send(OmniChan* chan, const void* elem_ptr) {
    if (!chan || !elem_ptr) return false;

    pthread_mutex_lock(&chan->lock);

    while (chan->count == chan->capacity && !chan->closed) {
        pthread_cond_wait(&chan->not_full, &chan->lock);
    }

    if (chan->closed) {
        pthread_mutex_unlock(&chan->lock);
        return false;
    }

    // Copy element into buffer at tail
    uint8_t* dest = chan->buffer + (chan->tail * chan->elem_size);
    memcpy(dest, elem_ptr, chan->elem_size);

    chan->tail = (chan->tail + 1) % chan->capacity;
    chan->count++;

    pthread_cond_signal(&chan->not_empty);
    pthread_mutex_unlock(&chan->lock);
    return true;
}

bool __omni_chan_recv(OmniChan* chan, void* out_elem_ptr) {
    if (!chan || !out_elem_ptr) return false;

    pthread_mutex_lock(&chan->lock);

    while (chan->count == 0 && !chan->closed) {
        pthread_cond_wait(&chan->not_empty, &chan->lock);
    }

    if (chan->count == 0 && chan->closed) {
        pthread_mutex_unlock(&chan->lock);
        return false; // Closed and empty
    }

    // Copy element out of buffer at head
    uint8_t* src = chan->buffer + (chan->head * chan->elem_size);
    memcpy(out_elem_ptr, src, chan->elem_size);

    chan->head = (chan->head + 1) % chan->capacity;
    chan->count--;

    pthread_cond_signal(&chan->not_full);
    pthread_mutex_unlock(&chan->lock);
    return true;
}

bool __omni_chan_send_int(OmniChan* chan, int64_t val) {
    if (!chan) return false;
    return __omni_chan_send(chan, &val);
}

int64_t __omni_chan_recv_int(OmniChan* chan) {
    if (!chan) return 0;
    int64_t val = 0;
    __omni_chan_recv(chan, &val);
    return val;
}

size_t __omni_chan_len(OmniChan* chan) {
    if (!chan) return 0;
    pthread_mutex_lock(&chan->lock);
    size_t count = chan->count;
    pthread_mutex_unlock(&chan->lock);
    return count;
}

void __omni_chan_close(OmniChan* chan) {
    if (!chan) return;
    pthread_mutex_lock(&chan->lock);
    chan->closed = true;
    pthread_cond_broadcast(&chan->not_empty);
    pthread_cond_broadcast(&chan->not_full);
    pthread_mutex_unlock(&chan->lock);
}

void __omni_chan_destroy(OmniChan* chan) {
    if (!chan) return;
    __omni_chan_close(chan);
    pthread_mutex_destroy(&chan->lock);
    pthread_cond_destroy(&chan->not_empty);
    pthread_cond_destroy(&chan->not_full);
    if (chan->buffer) free(chan->buffer);
    free(chan);
}
