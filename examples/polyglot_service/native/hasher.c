#include <stdint.h>

void fast_hash_block(const uint8_t* data, uint64_t len, uint8_t* out_digest) {
    for (uint64_t i = 0; i < len; i++) {
        out_digest[i % 32] ^= data[i];
    }
}
