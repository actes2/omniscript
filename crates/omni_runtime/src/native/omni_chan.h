#ifndef OMNI_CHAN_H
#define OMNI_CHAN_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>

typedef struct {
    uint8_t* buffer;
    size_t capacity;
    size_t elem_size;
    size_t head;
    size_t tail;
    size_t count;
    bool closed;
    pthread_mutex_t lock;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
} OmniChan;

OmniChan* __omni_chan_make(size_t capacity, size_t elem_size);
bool __omni_chan_send(OmniChan* chan, const void* elem_ptr);
bool __omni_chan_recv(OmniChan* chan, void* out_elem_ptr);
void __omni_chan_close(OmniChan* chan);
void __omni_chan_destroy(OmniChan* chan);

#endif // OMNI_CHAN_H
