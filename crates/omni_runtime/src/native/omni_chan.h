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

typedef struct {
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

OmniChan* __omni_chan_make(size_t capacity, size_t elem_size);
bool __omni_chan_send(OmniChan* chan, const void* elem_ptr);
bool __omni_chan_recv(OmniChan* chan, void* out_elem_ptr);
bool __omni_chan_send_int(OmniChan* chan, int64_t val);
int64_t __omni_chan_recv_int(OmniChan* chan);
size_t __omni_chan_len(OmniChan* chan);
void __omni_chan_close(OmniChan* chan);
void __omni_chan_destroy(OmniChan* chan);

#endif // OMNI_CHAN_H
