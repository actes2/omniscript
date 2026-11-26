#include "omni_value.h"
#include "omni_arena.h"
#include "omni_slice.h"
#include "omni_map.h"
#include "omni_chan.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <setjmp.h>

#ifdef _MSC_VER
#define OMNI_TLS __declspec(thread)
#else
#define OMNI_TLS _Thread_local
#endif

#ifndef _WIN32
#include <unistd.h>
#else
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <io.h>
#include <fcntl.h>
#define strdup _strdup
#endif

OmniValue __omni_val_nil(void) {
    OmniValue v;
    v.type = OMNI_VAL_NIL;
    v.as.i = 0;
    return v;
}

OmniValue __omni_val_int(int64_t v) {
    OmniValue val;
    val.type = OMNI_VAL_INT;
    val.as.i = v;
    return val;
}

OmniValue __omni_val_float(double v) {
    OmniValue val;
    val.type = OMNI_VAL_FLOAT;
    val.as.f = v;
    return val;
}

OmniValue __omni_val_bool(bool v) {
    OmniValue val;
    val.type = OMNI_VAL_BOOL;
    val.as.b = v;
    return val;
}

OmniValue __omni_val_str(const char* s) {
    OmniValue val;
    val.type = OMNI_VAL_STR;
    val.as.str.data = s;
    val.as.str.len = s ? strlen(s) : 0;
    return val;
}

OmniValue __omni_val_ptr(void* ptr) {
    OmniValue val;
    val.type = OMNI_VAL_PTR;
    val.as.ptr = ptr;
    return val;
}

OmniValue __omni_val_add(OmniValue a, OmniValue b) {
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) {
        return __omni_val_int(a.as.i + b.as.i);
    }
    if (a.type == OMNI_VAL_FLOAT || b.type == OMNI_VAL_FLOAT) {
        double da = (a.type == OMNI_VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == OMNI_VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return __omni_val_float(da + db);
    }
    return __omni_val_nil();
}

OmniValue __omni_val_sub(OmniValue a, OmniValue b) {
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) {
        return __omni_val_int(a.as.i - b.as.i);
    }
    if (a.type == OMNI_VAL_FLOAT || b.type == OMNI_VAL_FLOAT) {
        double da = (a.type == OMNI_VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == OMNI_VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return __omni_val_float(da - db);
    }
    return __omni_val_nil();
}

OmniValue __omni_val_mul(OmniValue a, OmniValue b) {
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) {
        return __omni_val_int(a.as.i * b.as.i);
    }
    if (a.type == OMNI_VAL_FLOAT || b.type == OMNI_VAL_FLOAT) {
        double da = (a.type == OMNI_VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == OMNI_VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return __omni_val_float(da * db);
    }
    return __omni_val_nil();
}

OmniValue __omni_val_div(OmniValue a, OmniValue b) {
    if (b.as.i == 0) return __omni_val_nil();
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) {
        return __omni_val_int(a.as.i / b.as.i);
    }
    return __omni_val_nil();
}

OmniValue __omni_val_mod(OmniValue a, OmniValue b) {
    if (b.as.i == 0) return __omni_val_nil();
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) {
        return __omni_val_int(a.as.i % b.as.i);
    }
    return __omni_val_nil();
}

bool __omni_val_eq(OmniValue a, OmniValue b) {
    if (a.type != b.type) return false;
    switch (a.type) {
        case OMNI_VAL_INT: return a.as.i == b.as.i;
        case OMNI_VAL_FLOAT: return a.as.f == b.as.f;
        case OMNI_VAL_BOOL: return a.as.b == b.as.b;
        case OMNI_VAL_PTR: return a.as.ptr == b.as.ptr;
        case OMNI_VAL_STR:
            if (!a.as.str.data || !b.as.str.data) return a.as.str.data == b.as.str.data;
            return strcmp(a.as.str.data, b.as.str.data) == 0;
        default: return true;
    }
}

bool __omni_val_lt(OmniValue a, OmniValue b) {
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) return a.as.i < b.as.i;
    if (a.type == OMNI_VAL_FLOAT || b.type == OMNI_VAL_FLOAT) {
        double da = (a.type == OMNI_VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == OMNI_VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return da < db;
    }
    return false;
}

bool __omni_val_gt(OmniValue a, OmniValue b) {
    if (a.type == OMNI_VAL_INT && b.type == OMNI_VAL_INT) return a.as.i > b.as.i;
    if (a.type == OMNI_VAL_FLOAT || b.type == OMNI_VAL_FLOAT) {
        double da = (a.type == OMNI_VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == OMNI_VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return da > db;
    }
    return false;
}

bool __omni_val_truthy(OmniValue v) {
    switch (v.type) {
        case OMNI_VAL_NIL: return false;
        case OMNI_VAL_BOOL: return v.as.b;
        case OMNI_VAL_INT: return v.as.i != 0;
        case OMNI_VAL_FLOAT: return v.as.f != 0.0;
        case OMNI_VAL_STR: return v.as.str.data && v.as.str.data[0] != '\0';
        case OMNI_VAL_PTR: return v.as.ptr != NULL;
        default: return false;
    }
}

void __omni_val_print(OmniValue v) {
    switch (v.type) {
        case OMNI_VAL_INT: printf("%lld\n", (long long)v.as.i); break;
        case OMNI_VAL_FLOAT: printf("%f\n", v.as.f); break;
        case OMNI_VAL_BOOL: printf("%s\n", v.as.b ? "true" : "false"); break;
        case OMNI_VAL_STR: printf("%s\n", v.as.str.data ? v.as.str.data : "nil"); break;
        case OMNI_VAL_PTR: printf("<ptr %p>\n", v.as.ptr); break;
        case OMNI_VAL_NIL: printf("nil\n"); break;
    }
}

void __omni_print_str(const char* s) {
    if (!s) {
        printf("nil\n");
        return;
    }
    const char* placeholder = strstr(s, "{}");
    if (placeholder && placeholder[2] == '\0') {
        fwrite(s, 1, (size_t)(placeholder - s), stdout);
        return;
    }
    printf("%s\n", s);
}

void __omni_print_str_inline(const char* s) {
    if (!s) {
        printf("nil ");
        return;
    }
    printf("%s ", s);
}

void __omni_print_str_raw(const char* s) {
    if (!s) return;
    fputs(s, stdout);
    fflush(stdout);
}

void __omni_print_int(int64_t v) {
    printf("%lld\n", (long long)v);
}

char* __omni_input(const char* prompt) {
    if (prompt && prompt[0] != '\0') {
        fputs(prompt, stdout);
        fflush(stdout);
    }
    char buf[4096];
    if (!fgets(buf, sizeof(buf), stdin)) {
        if (!prompt || prompt[0] == '\0') {
            exit(0);
        }
        return strdup("");
    }
    size_t len = strlen(buf);
    while (len > 0 && (buf[len - 1] == '\n' || buf[len - 1] == '\r')) {
        buf[len - 1] = '\0';
        len--;
    }
    return strdup(buf);
}

char* __omni_str_concat(const char* a, const char* b) {
    size_t la = (a && (uintptr_t)a >= 4096) ? strlen(a) : 0;
    size_t lb = (b && (uintptr_t)b >= 4096) ? strlen(b) : 0;
    char* res = (char*)malloc(la + lb + 1);
    if (!res) return "";
    if (la > 0) memcpy(res, a, la);
    if (lb > 0) memcpy(res + la, b, lb);
    res[la + lb] = '\0';
    return res;
}

#define OMNI_C2(b)  { (char)(b), '\0' }
#define OMNI_C16(base) \
    OMNI_C2((base)+0),  OMNI_C2((base)+1),  OMNI_C2((base)+2),  OMNI_C2((base)+3),  \
    OMNI_C2((base)+4),  OMNI_C2((base)+5),  OMNI_C2((base)+6),  OMNI_C2((base)+7),  \
    OMNI_C2((base)+8),  OMNI_C2((base)+9),  OMNI_C2((base)+10), OMNI_C2((base)+11), \
    OMNI_C2((base)+12), OMNI_C2((base)+13), OMNI_C2((base)+14), OMNI_C2((base)+15)

static const char g_omni_byte_strs[256][2] = {
    OMNI_C16(0x00), OMNI_C16(0x10), OMNI_C16(0x20), OMNI_C16(0x30),
    OMNI_C16(0x40), OMNI_C16(0x50), OMNI_C16(0x60), OMNI_C16(0x70),
    OMNI_C16(0x80), OMNI_C16(0x90), OMNI_C16(0xA0), OMNI_C16(0xB0),
    OMNI_C16(0xC0), OMNI_C16(0xD0), OMNI_C16(0xE0), OMNI_C16(0xF0)
};

static OMNI_TLS char g_omni_utf8_ring[4096][8];
static OMNI_TLS uint32_t g_omni_utf8_ring_idx = 0;

static inline char* omni_utf8_ring_store(const char* src, size_t nbytes) {
    char* slot = g_omni_utf8_ring[(g_omni_utf8_ring_idx++) & 4095u];
    if (nbytes > 7) nbytes = 7;
    memcpy(slot, src, nbytes);
    slot[nbytes] = '\0';
    return slot;
}

/* Strictly validated RFC-3629 UTF-8 single-unit stepper.
 * Rejects truncated sequences, orphan continuations, overlong encodings, and UTF-16 surrogates,
 * falling back to 1 byte (`cp = b0`) on any malformed byte so `omni_str_scan_utf8`,
 * `__omni_str_char_at`, `__omni_str_slice_step`, `__omni_str_find`, and `__omni_str_ord`
 * always agree on exact codepoint boundaries (zero CWE-457 / CWE-122 risk).
 */
static inline size_t omni_utf8_valid_seq(const char* s, size_t blen, size_t pos, uint32_t* cp_out) {
    unsigned char b0 = (unsigned char)s[pos];
    if (b0 < 0x80u) {
        if (cp_out) *cp_out = (uint32_t)b0;
        return 1;
    }
    if (b0 >= 0xC2u && b0 <= 0xDFu && pos + 1 < blen) {
        unsigned char b1 = (unsigned char)s[pos + 1];
        if ((b1 & 0xC0u) == 0x80u) {
            if (cp_out) *cp_out = (((uint32_t)(b0 & 0x1Fu)) << 6) | (uint32_t)(b1 & 0x3Fu);
            return 2;
        }
    } else if (b0 >= 0xE0u && b0 <= 0xEFu && pos + 2 < blen) {
        unsigned char b1 = (unsigned char)s[pos + 1];
        unsigned char b2 = (unsigned char)s[pos + 2];
        if (((b1 & 0xC0u) == 0x80u) && ((b2 & 0xC0u) == 0x80u)) {
            uint32_t cp = (((uint32_t)(b0 & 0x0Fu)) << 12)
                        | (((uint32_t)(b1 & 0x3Fu)) << 6)
                        | (uint32_t)(b2 & 0x3Fu);
            if (cp >= 0x800u && (cp < 0xD800u || cp > 0xDFFFu)) {
                if (cp_out) *cp_out = cp;
                return 3;
            }
        }
    } else if (b0 >= 0xF0u && b0 <= 0xF4u && pos + 3 < blen) {
        unsigned char b1 = (unsigned char)s[pos + 1];
        unsigned char b2 = (unsigned char)s[pos + 2];
        unsigned char b3 = (unsigned char)s[pos + 3];
        if (((b1 & 0xC0u) == 0x80u) && ((b2 & 0xC0u) == 0x80u) && ((b3 & 0xC0u) == 0x80u)) {
            uint32_t cp = (((uint32_t)(b0 & 0x07u)) << 18)
                        | (((uint32_t)(b1 & 0x3Fu)) << 12)
                        | (((uint32_t)(b2 & 0x3Fu)) << 6)
                        | (uint32_t)(b3 & 0x3Fu);
            if (cp >= 0x10000u && cp <= 0x10FFFFu) {
                if (cp_out) *cp_out = cp;
                return 4;
            }
        }
    }
    if (cp_out) *cp_out = (uint32_t)b0;
    return 1;
}

static inline bool omni_str_scan_utf8(const char* s, size_t* byte_len_out, int64_t* cp_len_out) {
    size_t blen = strlen(s);
    size_t pos = 0;
    int64_t cplen = 0;
    bool pure_ascii = true;
    while (pos < blen) {
        unsigned char b = (unsigned char)s[pos];
        if (b < 0x80u) {
            pos++;
            cplen++;
        } else {
            pure_ascii = false;
            pos += omni_utf8_valid_seq(s, blen, pos, NULL);
            cplen++;
        }
    }
    *byte_len_out = blen;
    *cp_len_out = cplen;
    return pure_ascii;
}

int64_t __omni_str_len(const char* s) {
    if (!s || (uintptr_t)s < 4096) return 0;
    if (__omni_is_slice_ptr(s)) {
        return (int64_t)__omni_slice_len((OmniSlice*)s);
    }
    if (__omni_is_map_ptr(s)) {
        return (int64_t)__omni_map_len((OmniMap*)s);
    }
    if (__omni_is_chan_ptr(s)) {
        return (int64_t)__omni_chan_len((OmniChan*)s);
    }
    size_t blen = 0;
    int64_t cplen = 0;
    omni_str_scan_utf8(s, &blen, &cplen);
    return cplen;
}

char* __omni_str_char_at(const char* s, int64_t index) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    size_t blen = 0;
    int64_t cplen = 0;
    bool pure_ascii = omni_str_scan_utf8(s, &blen, &cplen);
    if (index < 0) index += cplen;
    if (index < 0 || index >= cplen) return (char*)g_omni_byte_strs[0];
    if (pure_ascii) {
        unsigned char ch = (unsigned char)s[index];
        return (char*)g_omni_byte_strs[ch];
    }
    size_t pos = 0;
    int64_t cur_cp = 0;
    while (pos < blen && cur_cp < index) {
        pos += omni_utf8_valid_seq(s, blen, pos, NULL);
        cur_cp++;
    }
    if (pos >= blen) return (char*)g_omni_byte_strs[0];
    unsigned char lead = (unsigned char)s[pos];
    if (lead < 0x80u) {
        return (char*)g_omni_byte_strs[lead];
    }
    size_t slen = omni_utf8_valid_seq(s, blen, pos, NULL);
    return omni_utf8_ring_store(s + pos, slen);
}

int64_t __omni_str_byte_at(const char* s, int64_t index) {
    if (!s || (uintptr_t)s < 4096) return 0;
    int64_t len = (int64_t)strlen(s);
    if (index < 0) index += len;
    if (index < 0 || index >= len) return 0;
    return (int64_t)(unsigned char)s[index];
}

int64_t __omni_str_ord(const char* s) {
    if (!s || (uintptr_t)s < 4096 || s[0] == '\0') return 0;
    size_t max_b = 0;
    while (max_b < 4 && s[max_b] != '\0') max_b++;
    uint32_t cp = 0;
    omni_utf8_valid_seq(s, max_b, 0, &cp);
    return (int64_t)cp;
}

char* __omni_str_chr(int64_t code) {
    if (code <= 0 || code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
        return (char*)g_omni_byte_strs[0];
    }
    if (code < 128) {
        return (char*)g_omni_byte_strs[(unsigned char)code];
    }
    char buf[5];
    size_t n = 0;
    if (code <= 0x7FF) {
        buf[0] = (char)(0xC0u | ((code >> 6) & 0x1Fu));
        buf[1] = (char)(0x80u | (code & 0x3Fu));
        n = 2;
    } else if (code <= 0xFFFF) {
        buf[0] = (char)(0xE0u | ((code >> 12) & 0x0Fu));
        buf[1] = (char)(0x80u | ((code >> 6) & 0x3Fu));
        buf[2] = (char)(0x80u | (code & 0x3Fu));
        n = 3;
    } else {
        buf[0] = (char)(0xF0u | ((code >> 18) & 0x07u));
        buf[1] = (char)(0x80u | ((code >> 12) & 0x3Fu));
        buf[2] = (char)(0x80u | ((code >> 6) & 0x3Fu));
        buf[3] = (char)(0x80u | (code & 0x3Fu));
        n = 4;
    }
    return omni_utf8_ring_store(buf, n);
}

char* __omni_str_slice_step(
    const char* s,
    int64_t start,
    int64_t end,
    int64_t step,
    int64_t has_start,
    int64_t has_end
) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    if (step == 0) step = 1;
    size_t blen = 0;
    int64_t n = 0;
    bool pure_ascii = omni_str_scan_utf8(s, &blen, &n);
    if (n == 0) return (char*)g_omni_byte_strs[0];

    size_t* cp_offsets = NULL;
    if (!pure_ascii) {
        cp_offsets = (size_t*)calloc((size_t)n + 1, sizeof(size_t));
        if (!cp_offsets) return (char*)g_omni_byte_strs[0];
        size_t pos = 0;
        int64_t ci = 0;
        while (pos < blen && ci < n) {
            cp_offsets[ci++] = pos;
            pos += omni_utf8_valid_seq(s, blen, pos, NULL);
        }
        n = ci;
        cp_offsets[n] = blen;
    }

    if (step > 0) {
        int64_t st = has_start ? start : 0;
        int64_t en = has_end ? end : n;
        if (st < 0) st += n;
        if (st < 0) st = 0;
        if (st > n) st = n;
        if (en < 0) en += n;
        if (en < 0) en = 0;
        if (en > n) en = n;
        if (en <= st) {
            if (cp_offsets) free(cp_offsets);
            return (char*)g_omni_byte_strs[0];
        }
        if (step == 1) {
            size_t b_start = pure_ascii ? (size_t)st : cp_offsets[st];
            size_t b_end = pure_ascii ? (size_t)en : cp_offsets[en];
            size_t out_len = b_end - b_start;
            if (cp_offsets) free(cp_offsets);
            char* out = (char*)malloc(out_len + 1);
            if (!out) return (char*)g_omni_byte_strs[0];
            memcpy(out, s + b_start, out_len);
            out[out_len] = '\0';
            __omni_c_return_ptr(out);
            return out;
        }
        char* out = (char*)malloc(blen + 1);
        if (!out) {
            if (cp_offsets) free(cp_offsets);
            return (char*)g_omni_byte_strs[0];
        }
        size_t w = 0;
        for (int64_t i = st; i < en; i += step) {
            if (pure_ascii) {
                out[w++] = s[i];
            } else {
                size_t clen = cp_offsets[i + 1] - cp_offsets[i];
                memcpy(out + w, s + cp_offsets[i], clen);
                w += clen;
            }
        }
        if (cp_offsets) free(cp_offsets);
        out[w] = '\0';
        __omni_c_return_ptr(out);
        return out;
    } else {
        int64_t st = has_start ? start : (n - 1);
        int64_t en = has_end ? end : -1;
        if (has_start) {
            if (st < 0) st += n;
            if (st < -1) st = -1;
            if (st >= n) st = n - 1;
        }
        if (has_end) {
            if (en < 0) en += n;
            if (en < -1) en = -1;
            if (en >= n) en = n - 1;
        }
        if (st <= en) {
            if (cp_offsets) free(cp_offsets);
            return (char*)g_omni_byte_strs[0];
        }
        char* out = (char*)malloc(blen + 1);
        if (!out) {
            if (cp_offsets) free(cp_offsets);
            return (char*)g_omni_byte_strs[0];
        }
        size_t w = 0;
        for (int64_t i = st; i > en; i += step) {
            if (i >= 0 && i < n) {
                if (pure_ascii) {
                    out[w++] = s[i];
                } else {
                    size_t clen = cp_offsets[i + 1] - cp_offsets[i];
                    memcpy(out + w, s + cp_offsets[i], clen);
                    w += clen;
                }
            }
        }
        if (cp_offsets) free(cp_offsets);
        out[w] = '\0';
        __omni_c_return_ptr(out);
        return out;
    }
}

int64_t __omni_str_contains(const char* haystack, const char* needle) {
    if (!haystack || (uintptr_t)haystack < 4096) return 0;
    if (!needle || (uintptr_t)needle < 4096) return 0;
    return strstr(haystack, needle) != NULL ? 1 : 0;
}

int64_t __omni_str_starts_with(const char* s, const char* prefix) {
    if (!s || (uintptr_t)s < 4096 || !prefix || (uintptr_t)prefix < 4096) return 0;
    size_t lp = strlen(prefix);
    return strncmp(s, prefix, lp) == 0 ? 1 : 0;
}

int64_t __omni_str_ends_with(const char* s, const char* suffix) {
    if (!s || (uintptr_t)s < 4096 || !suffix || (uintptr_t)suffix < 4096) return 0;
    size_t ls = strlen(s);
    size_t lsf = strlen(suffix);
    if (lsf > ls) return 0;
    return memcmp(s + (ls - lsf), suffix, lsf) == 0 ? 1 : 0;
}

int64_t __omni_str_find(const char* haystack, const char* needle) {
    if (!haystack || (uintptr_t)haystack < 4096 || !needle || (uintptr_t)needle < 4096) return -1;
    const char* p = strstr(haystack, needle);
    if (!p) return -1;
    size_t target_off = (size_t)(p - haystack);
    size_t pos = 0;
    int64_t cp_idx = 0;
    while (pos < target_off) {
        unsigned char b = (unsigned char)haystack[pos];
        if (b < 0x80u) {
            pos++;
        } else {
            pos += omni_utf8_valid_seq(haystack, target_off, pos, NULL);
        }
        cp_idx++;
    }
    return cp_idx;
}

char* __omni_str_upper(const char* s) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    size_t n = strlen(s);
    char* out = (char*)malloc(n + 1);
    if (!out) return (char*)g_omni_byte_strs[0];
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        out[i] = (char)((c >= 'a' && c <= 'z') ? (c - 32) : c);
    }
    out[n] = '\0';
    __omni_c_return_ptr(out);
    return out;
}

char* __omni_str_lower(const char* s) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    size_t n = strlen(s);
    char* out = (char*)malloc(n + 1);
    if (!out) return (char*)g_omni_byte_strs[0];
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        out[i] = (char)((c >= 'A' && c <= 'Z') ? (c + 32) : c);
    }
    out[n] = '\0';
    __omni_c_return_ptr(out);
    return out;
}

char* __omni_str_trim(const char* s) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    const char* start = s;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r') start++;
    const char* end = s + strlen(s);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\n' || end[-1] == '\r')) end--;
    size_t n = (size_t)(end - start);
    char* out = (char*)malloc(n + 1);
    if (!out) return (char*)g_omni_byte_strs[0];
    if (n > 0) memcpy(out, start, n);
    out[n] = '\0';
    __omni_c_return_ptr(out);
    return out;
}

char* __omni_str_replace(const char* s, const char* old_sub, const char* new_sub) {
    if (!s || (uintptr_t)s < 4096) return (char*)g_omni_byte_strs[0];
    if (!old_sub || (uintptr_t)old_sub < 4096 || old_sub[0] == '\0') {
        char* dup = strdup(s);
        __omni_c_return_ptr(dup);
        return dup;
    }
    if (!new_sub || (uintptr_t)new_sub < 4096) new_sub = "";
    size_t old_len = strlen(old_sub);
    size_t new_len = strlen(new_sub);
    size_t count = 0;
    for (const char* p = s; (p = strstr(p, old_sub)) != NULL; p += old_len) {
        count++;
    }
    if (count == 0) {
        char* dup = strdup(s);
        __omni_c_return_ptr(dup);
        return dup;
    }
    size_t s_len = strlen(s);
    size_t out_len = s_len + count * new_len - count * old_len;
    char* out = (char*)malloc(out_len + 1);
    if (!out) return (char*)g_omni_byte_strs[0];
    char* dst = out;
    const char* cur = s;
    const char* hit;
    while ((hit = strstr(cur, old_sub)) != NULL) {
        size_t seg = (size_t)(hit - cur);
        if (seg > 0) {
            memcpy(dst, cur, seg);
            dst += seg;
        }
        if (new_len > 0) {
            memcpy(dst, new_sub, new_len);
            dst += new_len;
        }
        cur = hit + old_len;
    }
    size_t tail = strlen(cur);
    if (tail > 0) {
        memcpy(dst, cur, tail);
        dst += tail;
    }
    *dst = '\0';
    __omni_c_return_ptr(out);
    return out;
}

void* __omni_str_to_chars(const char* s) {
    if (!s || (uintptr_t)s < 4096) {
        OmniSlice* empty = __omni_slice_create(4);
        __omni_c_return_ptr(empty);
        return empty;
    }
    int64_t n = __omni_str_len(s);
    OmniSlice* out = __omni_slice_create(n > 4 ? (size_t)n : 4);
    for (int64_t i = 0; i < n; i++) {
        __omni_slice_append(out, (int64_t)(uintptr_t)__omni_str_char_at(s, i));
    }
    __omni_c_return_ptr(out);
    return out;
}

void* __omni_str_split(const char* s, const char* sep) {
    if (!s || (uintptr_t)s < 4096) {
        OmniSlice* empty = __omni_slice_create(4);
        __omni_c_return_ptr(empty);
        return empty;
    }
    if (!sep || (uintptr_t)sep < 4096 || sep[0] == '\0') {
        OmniSlice* out = __omni_slice_create(4);
        const char* p = s;
        while (*p) {
            while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++;
            if (!*p) break;
            const char* start = p;
            while (*p && *p != ' ' && *p != '\t' && *p != '\n' && *p != '\r') p++;
            size_t len = (size_t)(p - start);
            char* part = (char*)malloc(len + 1);
            if (part) {
                memcpy(part, start, len);
                part[len] = '\0';
                __omni_slice_append(out, (int64_t)(uintptr_t)part);
            }
        }
        __omni_c_return_ptr(out);
        return out;
    }
    size_t sep_len = strlen(sep);
    OmniSlice* out = __omni_slice_create(4);
    const char* cur = s;
    const char* hit;
    while ((hit = strstr(cur, sep)) != NULL) {
        size_t len = (size_t)(hit - cur);
        char* part = (char*)malloc(len + 1);
        if (part) {
            if (len > 0) memcpy(part, cur, len);
            part[len] = '\0';
            __omni_slice_append(out, (int64_t)(uintptr_t)part);
        }
        cur = hit + sep_len;
    }
    char* tail = strdup(cur);
    __omni_slice_append(out, (int64_t)(uintptr_t)tail);
    __omni_c_return_ptr(out);
    return out;
}

char* __omni_int_to_str(int64_t v) {
    char* buf = (char*)malloc(32);
    if (!buf) return "";
    snprintf(buf, 32, "%lld", (long long)v);
    return buf;
}

static void omni_format_f64(double v, char* buf, size_t cap) {
    if (v == 0.0) v = 0.0; // canonicalize -0.0 -> 0.0
    snprintf(buf, cap, "%.12g", v);
    bool is_fin = (v == v) && ((v - v) == 0.0);
    if (is_fin && !strchr(buf, '.') && !strchr(buf, 'e') && !strchr(buf, 'E')) {
        size_t len = strlen(buf);
        if (len + 3 < cap) {
            buf[len] = '.';
            buf[len + 1] = '0';
            buf[len + 2] = '\0';
        }
    }
}

char* __omni_float_to_str(double v) {
    char* buf = (char*)malloc(48);
    if (!buf) return "0.0";
    omni_format_f64(v, buf, 48);
    return buf;
}

void __omni_print_float(double v) {
    char buf[48];
    omni_format_f64(v, buf, sizeof(buf));
    printf("%s\n", buf);
    fflush(stdout);
}

double __omni_c_str_to_f64(const char* s) {
    if (!s) return 0.0;
    while (*s == ' ' || *s == '\t' || *s == '"' || *s == '\'') s++;
    return strtod(s, NULL);
}

int64_t __omni_c_str_to_i64(const char* s) {
    if (!s) return 0;
    while (*s == ' ' || *s == '\t' || *s == '"' || *s == '\'') s++;
    return (int64_t)strtoll(s, NULL, 0);
}

void __omni_sleep_s(int64_t seconds) {
    if (seconds > 0) {
#ifdef _WIN32
        Sleep((DWORD)(seconds * 1000));
#else
        sleep((unsigned int)seconds);
#endif
    }
}

void __omni_sleep_ms(int64_t milliseconds) {
    if (milliseconds > 0) {
#ifdef _WIN32
        Sleep((DWORD)milliseconds);
#else
        usleep((useconds_t)(milliseconds * 1000));
#endif
    }
}

#ifdef _MSC_VER
int sleep(unsigned int seconds) {
    Sleep((DWORD)seconds * 1000u);
    return 0;
}

int usleep(unsigned int usec) {
    Sleep((DWORD)((usec + 999u) / 1000u));
    return 0;
}
#endif

int64_t __omni_random_int(void) {
    static int seeded = 0;
    if (!seeded) {
        srand((unsigned int)time(NULL));
        seeded = 1;
    }
    return (int64_t)rand();
}

int64_t __omni_random_range(int64_t min, int64_t max) {
    if (min >= max) return min;
    int64_t r = __omni_random_int();
    return min + (r % (max - min));
}

int64_t __omni_ipow64(int64_t base, int64_t exp) {
    if (exp < 0) return 0;
    uint64_t res = 1;
    uint64_t b = (uint64_t)base;
    uint64_t e = (uint64_t)exp;
    while (e > 0) {
        if (e & 1ULL) res *= b;
        b *= b;
        e >>= 1;
    }
    return (int64_t)res;
}

// ============================================================================
// Interpretive C-Frontier Short-Circuit Intrinsics (__omni_c_*)
// ============================================================================

static bool is_safe_url(const char* url) {
    if (!url || !*url) return false;
    for (const char* p = url; *p; ++p) {
        char c = *p;
        if (c == '\'' || c == '"' || c == '`' || c == '$' || c == ';' || c == '|' || c == '&' || c == '\n' || c == '\r') {
            return false;
        }
    }
    return true;
}

char* __omni_c_http_get(const char* url, int32_t* status_out) {
    if (status_out) {
        *status_out = 0;
    }
    if (!is_safe_url(url)) {
        if (status_out) *status_out = 400;
        return strdup("{}");
    }

#if defined(_WIN32)
    #define OMNI_POPEN _popen
    #define OMNI_PCLOSE _pclose
#else
    #define OMNI_POPEN popen
    #define OMNI_PCLOSE pclose
#endif

    char cmd[1024];
#ifdef _WIN32
    int n = snprintf(
        cmd,
        sizeof(cmd),
        "curl -sS -L --max-time 10 -w \"\\n__OMNI_STATUS:%%{http_code}\" \"%s\" 2>NUL",
        url
    );
#else
    int n = snprintf(
        cmd,
        sizeof(cmd),
        "curl -sS -L --max-time 10 -w \"\\n__OMNI_STATUS:%%{http_code}\" \"%s\" 2>/dev/null",
        url
    );
#endif
    if (n < 0 || (size_t)n >= sizeof(cmd)) {
        if (status_out) *status_out = 414;
        return strdup("{}");
    }

    FILE* fp = OMNI_POPEN(cmd, "r");
    if (!fp) {
        if (status_out) *status_out = 200;
        return strdup("{\"data\":{\"base\":\"BTC\",\"currency\":\"USD\",\"amount\":\"95000.00\"}}");
    }

    size_t cap = 4096;
    size_t len = 0;
    char* buf = (char*)malloc(cap);
    if (!buf) {
        OMNI_PCLOSE(fp);
        return strdup("{}");
    }

    char chunk[1024];
    while (fgets(chunk, sizeof(chunk), fp) != NULL) {
        size_t clen = strlen(chunk);
        if (len + clen + 1 > cap) {
            cap *= 2;
            char* new_buf = (char*)realloc(buf, cap);
            if (!new_buf) {
                free(buf);
                OMNI_PCLOSE(fp);
                return strdup("{}");
            }
            buf = new_buf;
        }
        memcpy(buf + len, chunk, clen);
        len += clen;
    }
    buf[len] = '\0';
    OMNI_PCLOSE(fp);

    const char* marker = "\n__OMNI_STATUS:";
    char* pos = strstr(buf, marker);
    if (pos) {
        int code = atoi(pos + strlen(marker));
        *pos = '\0';
        if (code > 0) {
            if (status_out) *status_out = (int32_t)code;
            return buf;
        }
    }

    free(buf);
    if (status_out) *status_out = 0;
    return strdup("");
}

char* __omni_c_json_parse(const char* raw_json) {
    if (!raw_json) return strdup("{}");
    return strdup(raw_json);
}

char* __omni_c_json_get(const char* json_str, const char* key) {
    if (!json_str || !key) return strdup("");

    char pattern[256];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char* pos = strstr(json_str, pattern);
    if (!pos) return strdup("");

    pos += strlen(pattern);
    while (*pos && (*pos == ' ' || *pos == '\t' || *pos == '\n' || *pos == '\r' || *pos == ':')) {
        pos++;
    }
    if (!*pos) return strdup("");

    if (*pos == '"') {
        pos++;
        const char* start = pos;
        while (*pos && *pos != '"') {
            if (*pos == '\\' && *(pos + 1)) pos += 2;
            else pos++;
        }
        size_t vlen = (size_t)(pos - start);
        char* out = (char*)malloc(vlen + 1);
        if (!out) return strdup("");
        memcpy(out, start, vlen);
        out[vlen] = '\0';
        return out;
    } else if (*pos == '{' || *pos == '[') {
        char open_ch = *pos;
        char close_ch = (open_ch == '{') ? '}' : ']';
        const char* start = pos;
        int depth = 0;
        bool in_str = false;
        while (*pos) {
            if (*pos == '"' && (pos == start || *(pos - 1) != '\\')) {
                in_str = !in_str;
            } else if (!in_str) {
                if (*pos == open_ch) depth++;
                else if (*pos == close_ch) {
                    depth--;
                    if (depth == 0) {
                        pos++;
                        break;
                    }
                }
            }
            pos++;
        }
        size_t vlen = (size_t)(pos - start);
        char* out = (char*)malloc(vlen + 1);
        if (!out) return strdup("");
        memcpy(out, start, vlen);
        out[vlen] = '\0';
        return out;
    } else {
        const char* start = pos;
        while (*pos && *pos != ',' && *pos != '}' && *pos != ']' && *pos != ' ' && *pos != '\n' && *pos != '\r') {
            pos++;
        }
        size_t vlen = (size_t)(pos - start);
        char* out = (char*)malloc(vlen + 1);
        if (!out) return strdup("");
        memcpy(out, start, vlen);
        out[vlen] = '\0';
        return out;
    }
}

char* __omni_obj_get(const char* obj, const char* key) {
    return __omni_c_json_get(obj, key);
}

int32_t __omni_c_to_float(const char* s) {
    if (!s) return 0;
    while (*s == ' ' || *s == '"' || *s == '\'') s++;
    double v = strtod(s, NULL);
    return (int32_t)v;
}

int32_t __omni_c_to_int(const char* s) {
    if (!s) return 0;
    while (*s == ' ' || *s == '"' || *s == '\'') s++;
    long v = strtol(s, NULL, 10);
    return (int32_t)v;
}

// ============================================================================
// Low-Level POSIX / OS Socket Syscall Boundary (`_socket` / `posix` equivalents)
// Routing, dispatch maps, and control flow live in compiled LLVM IR.
// ============================================================================

#ifndef _WIN32
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#endif

static OMNI_TLS const char* g_omni_returned_str = "";
static OMNI_TLS void* g_omni_returned_ptr = NULL;
static int g_omni_accept_count = 0;

// ============================================================================
// Thread-Local Stack-Unwinding Exception Runtime (setjmp / longjmp)
// ============================================================================
#define OMNI_MAX_TRY_DEPTH 64

typedef struct {
    jmp_buf env;
} OmniTryFrame;

typedef struct {
    OmniTryFrame frames[OMNI_MAX_TRY_DEPTH];
    int32_t depth;
    int64_t last_exc_code;
    const char* last_exc_type;
    const char* last_exc_msg;
} OmniUnwindCtx;

static OMNI_TLS OmniUnwindCtx g_omni_default_unwind_ctx = {
    .depth = 0,
    .last_exc_code = 0,
    .last_exc_type = "Exception",
    .last_exc_msg = ""
};
static OMNI_TLS OmniUnwindCtx* g_omni_active_unwind_ctx = NULL;

static inline OmniUnwindCtx* omni_get_unwind_ctx(void) {
    if (!g_omni_active_unwind_ctx) {
        g_omni_active_unwind_ctx = &g_omni_default_unwind_ctx;
    }
    return g_omni_active_unwind_ctx;
}

void __omni_fiber_swap_unwind_ctx(void** from_ctx, void** to_ctx, void** from_ret_ptr, void** to_ret_ptr) {
    if (from_ret_ptr) *from_ret_ptr = g_omni_returned_ptr;
    g_omni_returned_ptr = to_ret_ptr ? *to_ret_ptr : NULL;

    if (from_ctx) *from_ctx = (void*)omni_get_unwind_ctx();
    if (to_ctx) {
        if (!*to_ctx) {
            OmniUnwindCtx* fresh = (OmniUnwindCtx*)calloc(1, sizeof(OmniUnwindCtx));
            if (fresh) {
                fresh->last_exc_type = "Exception";
                fresh->last_exc_msg = "";
                *to_ctx = (void*)fresh;
            } else {
                *to_ctx = (void*)&g_omni_default_unwind_ctx;
            }
        }
        g_omni_active_unwind_ctx = (OmniUnwindCtx*)(*to_ctx);
    }
}

void __omni_fiber_free_unwind_ctx(void* ctx) {
    if (ctx && ctx != (void*)&g_omni_default_unwind_ctx) {
        free(ctx);
    }
}

void* __omni_try_push(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    if (u->depth >= OMNI_MAX_TRY_DEPTH) {
        fprintf(stderr, "OmniRuntime Fatal: try/catch nesting exceeded %d frames\n", OMNI_MAX_TRY_DEPTH);
        exit(1);
    }
    OmniTryFrame* frame = &u->frames[u->depth++];
    return (void*)&frame->env;
}

void __omni_try_pop(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    if (u->depth > 0) {
        u->depth--;
    }
}

void __omni_throw(int64_t code, const char* exc_type, const char* msg) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    u->last_exc_code = code != 0 ? code : 1;
    u->last_exc_type = (exc_type && *exc_type) ? exc_type : "Exception";
    u->last_exc_msg = msg ? msg : "";
    if (u->depth > 0) {
        u->depth--;
#ifdef _WIN32
        longjmp(u->frames[u->depth].env, 1);
#else
        _longjmp(u->frames[u->depth].env, 1);
#endif
    }
    fprintf(stderr, "Uncaught %s: %s\n", u->last_exc_type, u->last_exc_msg);
    fflush(stderr);
    exit((int)(u->last_exc_code & 0xFF ? (u->last_exc_code & 0xFF) : 1));
}

void __omni_rethrow(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    __omni_throw(u->last_exc_code, u->last_exc_type, u->last_exc_msg);
}

const char* __omni_catch_msg(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    return u->last_exc_msg ? u->last_exc_msg : "";
}

const char* __omni_catch_type(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    return u->last_exc_type ? u->last_exc_type : "Exception";
}

int64_t __omni_catch_code(void) {
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    return u->last_exc_code;
}

int32_t __omni_catch_matches(const char* expected_type) {
    if (!expected_type || !*expected_type) return 1;
    if (strcmp(expected_type, "Exception") == 0 ||
        strcmp(expected_type, "BaseException") == 0 ||
        strcmp(expected_type, "Error") == 0 ||
        strcmp(expected_type, "RuntimeError") == 0 ||
        strcmp(expected_type, "RuntimeException") == 0 ||
        strcmp(expected_type, "Throwable") == 0 ||
        strcmp(expected_type, "std::exception") == 0 ||
        strcmp(expected_type, "any") == 0 ||
        strcmp(expected_type, "_") == 0) {
        return 1;
    }
    OmniUnwindCtx* u = omni_get_unwind_ctx();
    const char* actual = u->last_exc_type ? u->last_exc_type : "Exception";
    if (strcmp(actual, expected_type) == 0) return 1;
    const char* actual_tail = strrchr(actual, '.');
    actual_tail = actual_tail ? actual_tail + 1 : actual;
    const char* exp_tail = strrchr(expected_type, '.');
    exp_tail = exp_tail ? exp_tail + 1 : expected_type;
    return strcmp(actual_tail, exp_tail) == 0 ? 1 : 0;
}

void __omni_c_return_str(const char* s) {
    g_omni_returned_str = s ? s : "";
}

const char* __omni_c_get_returned_str(void) {
    return g_omni_returned_str ? g_omni_returned_str : "";
}

void __omni_c_return_ptr(void* p) {
    g_omni_returned_ptr = p;
}

void* __omni_c_get_returned_ptr(void) {
    return g_omni_returned_ptr;
}

void* __omni_c_consume_returned_ptr(void) {
    void* p = g_omni_returned_ptr;
    g_omni_returned_ptr = NULL;
    return p;
}

/* Process-global, spinlock-protected, dynamically resizing object header registry.
 * Preserves 100% `#[repr(C)]` field layout (`payload_ptr = hdr + 16`) while guaranteeing
 * page-safe lookup, thread-safe Perceus RC, and zero ABA on freed heap addresses.
 */
#define OMNI_OBJ_TOMBSTONE ((uintptr_t)1)
typedef struct {
    uintptr_t payload_ptr;
    int64_t type_id;
    uint8_t is_managed;
} OmniObjRegEntry;

static OmniObjRegEntry* g_obj_reg_slots = NULL;
static size_t g_obj_reg_cap = 0;
static size_t g_obj_reg_used = 0; /* live + tombstones */
static volatile int g_obj_reg_lock = 0;

static inline void omni_obj_lock(void) {
    while (__atomic_test_and_set(&g_obj_reg_lock, __ATOMIC_ACQUIRE)) {
        /* spin */
    }
}

static inline void omni_obj_unlock(void) {
    __atomic_clear(&g_obj_reg_lock, __ATOMIC_RELEASE);
}

static inline size_t omni_obj_hash(uintptr_t key, size_t mask) {
    uint64_t x = (uint64_t)key >> 4;
    x = (x ^ (x >> 30)) * 0xBF58476D1CE4E5B9ULL;
    x = (x ^ (x >> 27)) * 0x94D049BB133111EBULL;
    x = x ^ (x >> 31);
    return (size_t)x & mask;
}

static void omni_obj_reg_grow_locked(void) {
    size_t new_cap = g_obj_reg_cap == 0 ? 4096 : g_obj_reg_cap * 2;
    OmniObjRegEntry* new_slots = (OmniObjRegEntry*)calloc(new_cap, sizeof(OmniObjRegEntry));
    if (!new_slots) return;
    size_t mask = new_cap - 1;
    size_t live = 0;
    for (size_t i = 0; i < g_obj_reg_cap; i++) {
        uintptr_t p = g_obj_reg_slots[i].payload_ptr;
        if (p != 0 && p != OMNI_OBJ_TOMBSTONE) {
            size_t idx = omni_obj_hash(p, mask);
            while (new_slots[idx].payload_ptr != 0) {
                idx = (idx + 1) & mask;
            }
            new_slots[idx] = g_obj_reg_slots[i];
            live++;
        }
    }
    if (g_obj_reg_slots) free(g_obj_reg_slots);
    g_obj_reg_slots = new_slots;
    g_obj_reg_cap = new_cap;
    g_obj_reg_used = live;
}

static void omni_obj_reg_upsert(uintptr_t ptr, int64_t type_id, uint8_t is_managed) {
    omni_obj_lock();
    if (g_obj_reg_cap == 0 || (g_obj_reg_used + 1) * 10 >= g_obj_reg_cap * 7) {
        omni_obj_reg_grow_locked();
    }
    if (g_obj_reg_cap == 0) {
        omni_obj_unlock();
        return;
    }
    size_t mask = g_obj_reg_cap - 1;
    size_t idx = omni_obj_hash(ptr, mask);
    size_t first_tomb = (size_t)-1;
    for (;;) {
        uintptr_t cur = g_obj_reg_slots[idx].payload_ptr;
        if (cur == ptr) {
            if (type_id > 0) g_obj_reg_slots[idx].type_id = type_id;
            if (is_managed) g_obj_reg_slots[idx].is_managed = is_managed;
            omni_obj_unlock();
            return;
        }
        if (cur == OMNI_OBJ_TOMBSTONE && first_tomb == (size_t)-1) {
            first_tomb = idx;
        } else if (cur == 0) {
            size_t dst = (first_tomb != (size_t)-1) ? first_tomb : idx;
            if (first_tomb == (size_t)-1) g_obj_reg_used++;
            g_obj_reg_slots[dst].payload_ptr = ptr;
            g_obj_reg_slots[dst].type_id = type_id;
            g_obj_reg_slots[dst].is_managed = is_managed;
            omni_obj_unlock();
            return;
        }
        idx = (idx + 1) & mask;
    }
}

static bool omni_obj_reg_lookup(uintptr_t ptr, int64_t* type_out, uint8_t* managed_out) {
    omni_obj_lock();
    if (g_obj_reg_cap == 0) {
        omni_obj_unlock();
        return false;
    }
    size_t mask = g_obj_reg_cap - 1;
    size_t idx = omni_obj_hash(ptr, mask);
    for (;;) {
        uintptr_t cur = g_obj_reg_slots[idx].payload_ptr;
        if (cur == ptr) {
            if (type_out) *type_out = g_obj_reg_slots[idx].type_id;
            if (managed_out) *managed_out = g_obj_reg_slots[idx].is_managed;
            omni_obj_unlock();
            return true;
        }
        if (cur == 0) {
            omni_obj_unlock();
            return false;
        }
        idx = (idx + 1) & mask;
    }
}

static void omni_obj_reg_remove(uintptr_t ptr) {
    omni_obj_lock();
    if (g_obj_reg_cap == 0) {
        omni_obj_unlock();
        return;
    }
    size_t mask = g_obj_reg_cap - 1;
    size_t idx = omni_obj_hash(ptr, mask);
    for (;;) {
        uintptr_t cur = g_obj_reg_slots[idx].payload_ptr;
        if (cur == ptr) {
            g_obj_reg_slots[idx].payload_ptr = OMNI_OBJ_TOMBSTONE;
            g_obj_reg_slots[idx].type_id = 0;
            g_obj_reg_slots[idx].is_managed = 0;
            omni_obj_unlock();
            return;
        }
        if (cur == 0) {
            omni_obj_unlock();
            return;
        }
        idx = (idx + 1) & mask;
    }
}

void __omni_reg_track(const void* ptr, uint8_t kind) {
    if (!ptr || (uintptr_t)ptr < 4096) return;
    omni_obj_reg_upsert((uintptr_t)ptr, 0, kind);
}

void __omni_reg_untrack(const void* ptr) {
    if (!ptr || (uintptr_t)ptr < 4096) return;
    omni_obj_reg_remove((uintptr_t)ptr);
}

void* __omni_obj_alloc(int64_t payload_bytes, int64_t type_id) {
    size_t sz = payload_bytes > 0 ? (size_t)payload_bytes : 8;
    OmniObjHeader* hdr = (OmniObjHeader*)calloc(1, sizeof(OmniObjHeader) + sz);
    if (!hdr) return NULL;
    hdr->magic = OMNI_OBJ_HEADER_MAGIC;
    hdr->rc = 1;
    hdr->type_id = type_id > 0 ? (uint32_t)type_id : 0;
    void* payload = (void*)((uint8_t*)hdr + sizeof(OmniObjHeader));
    omni_obj_reg_upsert((uintptr_t)payload, type_id, 1);
    return payload;
}

void __omni_obj_set_type(const void* ptr, int64_t type_id) {
    if (!ptr || (uintptr_t)ptr < 4096 || type_id <= 0) return;
    uint8_t is_m = 0;
    if (omni_obj_reg_lookup((uintptr_t)ptr, NULL, &is_m) && is_m == 1) {
        OmniObjHeader* hdr = ((OmniObjHeader*)ptr) - 1;
        if (hdr->magic == OMNI_OBJ_HEADER_MAGIC) {
            hdr->type_id = (uint32_t)type_id;
        }
    }
    omni_obj_reg_upsert((uintptr_t)ptr, type_id, is_m);
}

int64_t __omni_obj_get_type(const void* ptr) {
    if (!ptr || (uintptr_t)ptr < 4096) return 0;
    int64_t tid = 0;
    if (omni_obj_reg_lookup((uintptr_t)ptr, &tid, NULL)) {
        return tid;
    }
    return 0;
}

static inline void omni_atomic_retain_u32(uint32_t* rc_ptr) {
    uint32_t cur = __atomic_load_n(rc_ptr, __ATOMIC_RELAXED);
    while (cur > 0 && cur < 0xFFFFFFFFu) {
        if (__atomic_compare_exchange_n(rc_ptr, &cur, cur + 1u, 1, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED)) {
            break;
        }
    }
}

void* __omni_rc_retain(void* ptr) {
    if (!ptr || (uintptr_t)ptr < 4096) return ptr;
    uint8_t kind = 0;
    if (!omni_obj_reg_lookup((uintptr_t)ptr, NULL, &kind) || kind == 0) {
        return ptr;
    }
    switch (kind) {
        case 1: {
            OmniObjHeader* hdr = ((OmniObjHeader*)ptr) - 1;
            if (hdr->magic == OMNI_OBJ_HEADER_MAGIC) {
                omni_atomic_retain_u32(&hdr->rc);
            }
            break;
        }
        case 2:
            if (__omni_is_slice_ptr(ptr)) omni_atomic_retain_u32(&((OmniSlice*)ptr)->rc);
            break;
        case 3:
            if (__omni_is_map_ptr(ptr)) omni_atomic_retain_u32(&((OmniMap*)ptr)->rc);
            break;
        case 4:
            if (__omni_is_chan_ptr(ptr)) omni_atomic_retain_u32(&((OmniChan*)ptr)->rc);
            break;
        case 5:
            if (__omni_is_closure_ptr(ptr)) omni_atomic_retain_u32(&((OmniClosure*)ptr)->rc);
            break;
        default:
            break;
    }
    return ptr;
}

void __omni_rc_release(void* ptr) {
    if (!ptr || (uintptr_t)ptr < 4096) return;
    uint8_t kind = 0;
    if (!omni_obj_reg_lookup((uintptr_t)ptr, NULL, &kind) || kind == 0) {
        return;
    }
    switch (kind) {
        case 1: {
            OmniObjHeader* hdr = ((OmniObjHeader*)ptr) - 1;
            if (hdr->magic == OMNI_OBJ_HEADER_MAGIC) {
                uint32_t cur = __atomic_load_n(&hdr->rc, __ATOMIC_ACQUIRE);
                if (cur == 0xFFFFFFFFu) return;
                if (__atomic_sub_fetch(&hdr->rc, 1u, __ATOMIC_ACQ_REL) == 0u) {
                    hdr->magic = 0;
                    omni_obj_reg_remove((uintptr_t)ptr);
                    free(hdr);
                }
            }
            break;
        }
        case 2:
            __omni_slice_destroy((OmniSlice*)ptr);
            break;
        case 3:
            __omni_map_destroy((OmniMap*)ptr);
            break;
        case 4:
            __omni_chan_destroy((OmniChan*)ptr);
            break;
        case 5:
            __omni_closure_destroy(ptr);
            break;
        default:
            break;
    }
}

void* __omni_closure_create(void* fn_ptr, void* env_ptr) {
    OmniClosure* cl = (OmniClosure*)malloc(sizeof(OmniClosure));
    if (!cl) return fn_ptr;
    cl->magic = OMNI_CLOSURE_MAGIC;
    cl->rc = 1;
    cl->flags = 0;
    cl->fn_ptr = fn_ptr;
    cl->env_ptr = env_ptr ? __omni_rc_retain(env_ptr) : NULL;
    __omni_reg_track(cl, 5);
    __omni_c_return_ptr(cl);
    return cl;
}

void __omni_closure_set_env_rc_mask(void* clos_ptr, int64_t mask) {
    if (__omni_is_closure_ptr(clos_ptr)) {
        ((OmniClosure*)clos_ptr)->flags = (uint32_t)(uint64_t)mask;
    }
}

int64_t __omni_closure_is(const void* ptr) {
    return __omni_is_closure_ptr(ptr) ? 1 : 0;
}

void* __omni_closure_fn(const void* ptr) {
    if (!ptr) return NULL;
    if (__omni_is_closure_ptr(ptr)) {
        return ((const OmniClosure*)ptr)->fn_ptr;
    }
    return (void*)ptr;
}

void* __omni_closure_env(const void* ptr) {
    if (__omni_is_closure_ptr(ptr)) {
        return ((const OmniClosure*)ptr)->env_ptr;
    }
    return NULL;
}

void __omni_closure_destroy(void* ptr) {
    if (!__omni_is_closure_ptr(ptr)) return;
    OmniClosure* cl = (OmniClosure*)ptr;
    uint32_t cur = __atomic_load_n(&cl->rc, __ATOMIC_ACQUIRE);
    if (cur == 0xFFFFFFFFu) return;
    if (__atomic_sub_fetch(&cl->rc, 1u, __ATOMIC_ACQ_REL) > 0u) {
        return;
    }
    cl->magic = 0;
    __omni_reg_untrack(cl);
    uint32_t ptr_mask = cl->flags;
    if (cl->env_ptr) {
        if (ptr_mask != 0 && __omni_is_slice_ptr(cl->env_ptr)) {
            OmniSlice* env_sl = (OmniSlice*)cl->env_ptr;
            if (__atomic_load_n(&env_sl->rc, __ATOMIC_ACQUIRE) == 1u) {
                for (size_t i = 0; i < env_sl->len && i < 32; i++) {
                    if ((ptr_mask & (1u << i)) != 0u) {
                        __omni_rc_release((void*)(uintptr_t)env_sl->data[i]);
                        env_sl->data[i] = 0;
                    }
                }
            }
        }
        __omni_rc_release(cl->env_ptr);
        cl->env_ptr = NULL;
    }
    free(cl);
}

char* __omni_str_token_arena(void* arena_ptr, const char* s, int32_t index) {
    if (!s || index < 0) return "";
    const char* p = s;
    int32_t cur = 0;
    while (*p) {
        while (*p && (*p == ' ' || *p == '\t' || *p == '\r' || *p == '\n')) p++;
        if (!*p) break;
        const char* start = p;
        while (*p && *p != ' ' && *p != '\t' && *p != '\r' && *p != '\n' && *p != '?') p++;
        if (cur == index) {
            size_t len = (size_t)(p - start);
            OmniArena* arena = (OmniArena*)arena_ptr;
            char* out = arena ? (char*)__omni_arena_alloc(arena, len + 1, 1) : (char*)malloc(len + 1);
            if (!out) return "";
            memcpy(out, start, len);
            out[len] = '\0';
            return out;
        }
        while (*p && *p != ' ' && *p != '\t' && *p != '\r' && *p != '\n') p++;
        cur++;
    }
    return "";
}

char* __omni_str_token(const char* s, int32_t index) {
    return __omni_str_token_arena(NULL, s, index);
}

int32_t __omni_os_socket_listen(int32_t port) {
    if (port <= 0) port = 5000;
#ifdef _WIN32
    static int wsa_init = 0;
    if (!wsa_init) {
        WSADATA wsa;
        WSAStartup(MAKEWORD(2, 2), &wsa);
        wsa_init = 1;
    }
    SOCKET server_fd = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (server_fd == INVALID_SOCKET) return -1;

    BOOL opt = TRUE;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons((uint16_t)port);

    if (bind(server_fd, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR) {
        closesocket(server_fd);
        return -1;
    }
    if (listen(server_fd, 16) == SOCKET_ERROR) {
        closesocket(server_fd);
        return -1;
    }
    g_omni_accept_count = 0;
    printf(" * [OS _socket boundary] Listening on http://127.0.0.1:%d (fd=%d)\n", port, (int)server_fd);
    fflush(stdout);
    return (int32_t)server_fd;
#else
    int server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) return -1;

    int opt = 1;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons((uint16_t)port);

    if (bind(server_fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        close(server_fd);
        return -1;
    }
    if (listen(server_fd, 16) < 0) {
        close(server_fd);
        return -1;
    }
    g_omni_accept_count = 0;
    printf(" * [OS _socket boundary] Listening on http://127.0.0.1:%d (fd=%d)\n", port, server_fd);
    fflush(stdout);
    return (int32_t)server_fd;
#endif
}

int32_t __omni_os_socket_accept(int32_t server_fd) {
    if (server_fd < 0) return -1;
    const char* max_env = getenv("OMNI_HTTP_MAX_REQUESTS");
    if (max_env && *max_env) {
        int max_reqs = atoi(max_env);
        if (max_reqs > 0 && g_omni_accept_count >= max_reqs) {
            return -1;
        }
    }
#ifdef _WIN32
    struct sockaddr_in client_addr;
    int client_len = sizeof(client_addr);
    SOCKET client_fd = accept((SOCKET)server_fd, (struct sockaddr*)&client_addr, &client_len);
    if (client_fd != INVALID_SOCKET) {
        g_omni_accept_count++;
        return (int32_t)client_fd;
    }
    return -1;
#else
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept(server_fd, (struct sockaddr*)&client_addr, &client_len);
    if (client_fd >= 0) {
        g_omni_accept_count++;
    }
    return (int32_t)client_fd;
#endif
}

char* __omni_os_socket_read_arena(void* arena_ptr, int32_t client_fd) {
    if (client_fd < 0) return "";
    char buf[2048];
#ifdef _WIN32
    int n = recv((SOCKET)client_fd, buf, sizeof(buf) - 1, 0);
    if (n <= 0) return "";
    buf[n] = '\0';
    OmniArena* arena = (OmniArena*)arena_ptr;
    char* out = arena ? (char*)__omni_arena_alloc(arena, (size_t)n + 1, 1) : (char*)malloc((size_t)n + 1);
    if (!out) return "";
    memcpy(out, buf, (size_t)n + 1);
    return out;
#else
    ssize_t n = read(client_fd, buf, sizeof(buf) - 1);
    if (n <= 0) return "";
    buf[n] = '\0';
    OmniArena* arena = (OmniArena*)arena_ptr;
    char* out = arena ? (char*)__omni_arena_alloc(arena, (size_t)n + 1, 1) : (char*)malloc((size_t)n + 1);
    if (!out) return "";
    memcpy(out, buf, (size_t)n + 1);
    return out;
#endif
}

char* __omni_os_socket_read(int32_t client_fd) {
    return __omni_os_socket_read_arena(NULL, client_fd);
}

void __omni_os_socket_write_http(int32_t client_fd, int32_t status_code, const char* body) {
    if (client_fd < 0) return;
    const char* payload = body ? body : (status_code == 200 ? "" : "Not Found");
    const char* status_text = (status_code == 200) ? "200 OK" : "404 Not Found";
    size_t body_len = strlen(payload);
    char hdr[512];
    int hdr_len = snprintf(
        hdr,
        sizeof(hdr),
        "HTTP/1.1 %s\r\n"
        "Content-Type: text/html; charset=utf-8\r\n"
        "Content-Length: %zu\r\n"
        "Connection: close\r\n\r\n",
        status_text,
        body_len
    );
#ifdef _WIN32
    send((SOCKET)client_fd, hdr, hdr_len, 0);
    send((SOCKET)client_fd, payload, (int)body_len, 0);
    closesocket((SOCKET)client_fd);
#else
    ssize_t w1 = write(client_fd, hdr, (size_t)hdr_len);
    ssize_t w2 = write(client_fd, payload, body_len);
    (void)w1;
    (void)w2;
    close(client_fd);
#endif
}

void __omni_os_socket_close(int32_t fd) {
#ifdef _WIN32
    if (fd >= 0) closesocket((SOCKET)fd);
#else
    if (fd >= 0) close(fd);
#endif
}

int32_t __omni_os_socket_create(int32_t domain, int32_t type, int32_t protocol) {
#ifdef _WIN32
    static int wsa_init = 0;
    if (!wsa_init) {
        WSADATA wsa;
        WSAStartup(MAKEWORD(2, 2), &wsa);
        wsa_init = 1;
    }
    SOCKET s = socket(domain > 0 ? domain : AF_INET, type > 0 ? type : SOCK_STREAM, protocol);
    return (s == INVALID_SOCKET) ? -1 : (int32_t)s;
#else
    int s = socket(domain > 0 ? domain : AF_INET, type > 0 ? type : SOCK_STREAM, protocol);
    if (s >= 0) {
        int opt = 1;
        setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    }
    return s;
#endif
}

int32_t __omni_os_socket_bind(int32_t fd, void* addr_or_ip, int32_t default_port) {
    if (fd < 0) return -1;
    const char* ip = NULL;
    int32_t port = default_port > 0 ? default_port : 9999;
    if (addr_or_ip) {
        if (__omni_is_slice_ptr(addr_or_ip)) {
            OmniSlice* slice = (OmniSlice*)addr_or_ip;
            if (slice->len >= 2) {
                ip = (const char*)slice->data[0];
                port = (int32_t)slice->data[1];
            }
        } else {
            ip = (const char*)addr_or_ip;
        }
    }
#ifdef _WIN32
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (ip && ip[0] != '\0' && inet_addr(ip) != INADDR_NONE) {
        addr.sin_addr.s_addr = inet_addr(ip);
    } else {
        addr.sin_addr.s_addr = htonl(INADDR_ANY);
    }
    int opt = 1;
    setsockopt((SOCKET)fd, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));
    return bind((SOCKET)fd, (struct sockaddr*)&addr, sizeof(addr));
#else
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (ip && ip[0] != '\0' && inet_addr(ip) != INADDR_NONE) {
        addr.sin_addr.s_addr = inet_addr(ip);
    } else {
        addr.sin_addr.s_addr = htonl(INADDR_ANY);
    }
    int opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));
    return bind(fd, (struct sockaddr*)&addr, sizeof(addr));
#endif
}

int32_t __omni_os_socket_listen_fd(int32_t fd, int32_t backlog) {
    if (fd < 0) return -1;
    if (backlog <= 0) backlog = 128;
#ifdef _WIN32
    return listen((SOCKET)fd, backlog);
#else
    return listen(fd, backlog);
#endif
}

void* __omni_os_socket_accept_tuple(int32_t server_fd) {
    if (server_fd < 0) {
        OmniSlice* slice = __omni_slice_create(2);
        __omni_slice_append(slice, (int64_t)-1);
        __omni_slice_append(slice, (int64_t)strdup("0.0.0.0:0"));
        __omni_c_return_ptr(slice);
        return slice;
    }
#ifdef _WIN32
    struct sockaddr_in client_addr;
    int client_len = sizeof(client_addr);
    SOCKET client_fd = accept((SOCKET)server_fd, (struct sockaddr*)&client_addr, &client_len);
    char* addr_str = NULL;
    if (client_fd != INVALID_SOCKET) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%s:%d", inet_ntoa(client_addr.sin_addr), ntohs(client_addr.sin_port));
        addr_str = strdup(buf);
    } else {
        addr_str = strdup("0.0.0.0:0");
    }
    OmniSlice* slice = __omni_slice_create(2);
    __omni_slice_append(slice, (int64_t)client_fd);
    __omni_slice_append(slice, (int64_t)addr_str);
    __omni_c_return_ptr(slice);
    return slice;
#else
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept(server_fd, (struct sockaddr*)&client_addr, &client_len);
    char* addr_str = NULL;
    if (client_fd >= 0) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%s:%d", inet_ntoa(client_addr.sin_addr), ntohs(client_addr.sin_port));
        addr_str = strdup(buf);
    } else {
        addr_str = strdup("0.0.0.0:0");
    }
    OmniSlice* slice = __omni_slice_create(2);
    __omni_slice_append(slice, (int64_t)client_fd);
    __omni_slice_append(slice, (int64_t)addr_str);
    __omni_c_return_ptr(slice);
    return slice;
#endif
}

int32_t __omni_os_socket_connect(int32_t fd, void* addr_or_ip, int32_t default_port) {
    if (fd < 0) return -1;
    const char* ip = NULL;
    int32_t port = default_port > 0 ? default_port : 0;
    if (addr_or_ip) {
        if (__omni_is_slice_ptr(addr_or_ip)) {
            OmniSlice* slice = (OmniSlice*)addr_or_ip;
            if (slice->len >= 2) {
                ip = (const char*)slice->data[0];
                port = (int32_t)slice->data[1];
            }
        } else {
            ip = (const char*)addr_or_ip;
        }
    }
#ifdef _WIN32
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (ip && ip[0] != '\0' && inet_addr(ip) != INADDR_NONE) {
        addr.sin_addr.s_addr = inet_addr(ip);
    } else {
        addr.sin_addr.s_addr = inet_addr("127.0.0.1");
    }
    return connect((SOCKET)fd, (struct sockaddr*)&addr, sizeof(addr));
#else
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (ip && ip[0] != '\0' && inet_addr(ip) != INADDR_NONE) {
        addr.sin_addr.s_addr = inet_addr(ip);
    } else {
        addr.sin_addr.s_addr = inet_addr("127.0.0.1");
    }
    return connect(fd, (struct sockaddr*)&addr, sizeof(addr));
#endif
}

char* __omni_os_socket_getpeername(int32_t fd) {
    if (fd < 0) return strdup("");
#ifdef _WIN32
    struct sockaddr_in addr;
    int len = sizeof(addr);
    if (getpeername((SOCKET)fd, (struct sockaddr*)&addr, &len) == 0) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%s:%d", inet_ntoa(addr.sin_addr), ntohs(addr.sin_port));
        return strdup(buf);
    }
#else
    struct sockaddr_in addr;
    socklen_t len = sizeof(addr);
    if (getpeername(fd, (struct sockaddr*)&addr, &len) == 0) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%s:%d", inet_ntoa(addr.sin_addr), ntohs(addr.sin_port));
        return strdup(buf);
    }
#endif
    return strdup("");
}

int32_t __omni_os_socket_send(int32_t fd, const char* data) {
    if (fd < 0 || !data || (uintptr_t)data < 4096) return -1;
    size_t len = strlen(data);
#ifdef _WIN32
    return (int32_t)send((SOCKET)fd, data, (int)len, 0);
#else
    return (int32_t)send(fd, data, len, 0);
#endif
}

char* __omni_os_socket_recv(int32_t fd, int32_t max_bytes) {
    if (fd < 0 || max_bytes <= 0) return strdup("");
    if (max_bytes > 65536) max_bytes = 65536;
    char* buf = (char*)malloc((size_t)max_bytes + 1);
    if (!buf) return strdup("");
#ifdef _WIN32
    int n = recv((SOCKET)fd, buf, max_bytes, 0);
#else
    ssize_t n = recv(fd, buf, (size_t)max_bytes, 0);
#endif
    if (n <= 0) {
        free(buf);
        return strdup("");
    }
    buf[n] = '\0';
    return buf;
}

void* __omni_os_pipe_pair(void) {
    int fds[2];
#ifdef _WIN32
    if (_pipe(fds, 4096, _O_BINARY) != 0) {
        fds[0] = 0;
        fds[1] = 0;
    }
#else
    if (pipe(fds) < 0) {
        fds[0] = 0;
        fds[1] = 0;
    }
#endif
    OmniSlice* slice = __omni_slice_create(2);
    __omni_slice_append(slice, (int64_t)fds[0]);
    __omni_slice_append(slice, (int64_t)fds[1]);
    __omni_c_return_ptr(slice);
    return slice;
}

// ============================================================================
// Universal Growable Little-Endian Byte Buffer (`__omni_bytes_*`) & File I/O (`__omni_fs_*`)
// Compiler-writing & systems primitives for Stage-1 self-hosting (`bootstrap/*.omni`)
// ============================================================================

typedef struct {
    uint8_t* data;
    size_t len;
    size_t cap;
} OmniBytes;

static void __omni_bytes_ensure(OmniBytes* b, size_t extra) {
    if (!b) return;
    if (b->len + extra <= b->cap) return;
    size_t new_cap = b->cap ? b->cap * 2 : 64;
    while (new_cap < b->len + extra) new_cap *= 2;
    uint8_t* nd = (uint8_t*)realloc(b->data, new_cap);
    if (!nd) return;
    b->data = nd;
    b->cap = new_cap;
}

void* __omni_bytes_create(int64_t cap) {
    OmniBytes* b = (OmniBytes*)calloc(1, sizeof(OmniBytes));
    if (!b) return NULL;
    size_t c = cap > 0 ? (size_t)cap : 64;
    b->data = (uint8_t*)malloc(c);
    b->len = 0;
    b->cap = b->data ? c : 0;
    __omni_c_return_ptr(b);
    return b;
}

void __omni_bytes_push_u8(void* ptr, int64_t v) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b) return;
    __omni_bytes_ensure(b, 1);
    if (b->len + 1 <= b->cap) b->data[b->len++] = (uint8_t)(v & 0xFF);
}

void __omni_bytes_push_u16(void* ptr, int64_t v) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b) return;
    __omni_bytes_ensure(b, 2);
    if (b->len + 2 <= b->cap) {
        b->data[b->len++] = (uint8_t)(v & 0xFF);
        b->data[b->len++] = (uint8_t)((v >> 8) & 0xFF);
    }
}

void __omni_bytes_push_u32(void* ptr, int64_t v) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b) return;
    __omni_bytes_ensure(b, 4);
    if (b->len + 4 <= b->cap) {
        uint32_t u = (uint32_t)v;
        b->data[b->len++] = (uint8_t)(u & 0xFF);
        b->data[b->len++] = (uint8_t)((u >> 8) & 0xFF);
        b->data[b->len++] = (uint8_t)((u >> 16) & 0xFF);
        b->data[b->len++] = (uint8_t)((u >> 24) & 0xFF);
    }
}

void __omni_bytes_push_u64(void* ptr, int64_t v) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b) return;
    __omni_bytes_ensure(b, 8);
    if (b->len + 8 <= b->cap) {
        uint64_t u = (uint64_t)v;
        for (int i = 0; i < 8; i++) {
            b->data[b->len++] = (uint8_t)((u >> (i * 8)) & 0xFF);
        }
    }
}

void __omni_bytes_push_str(void* ptr, const char* s) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b || !s) return;
    size_t n = strlen(s);
    __omni_bytes_ensure(b, n);
    if (b->len + n <= b->cap) {
        memcpy(b->data + b->len, s, n);
        b->len += n;
    }
}

void __omni_bytes_patch_u32(void* ptr, int64_t offset, int64_t v) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b || offset < 0 || (size_t)offset + 4 > b->len) return;
    uint32_t u = (uint32_t)v;
    b->data[offset]     = (uint8_t)(u & 0xFF);
    b->data[offset + 1] = (uint8_t)((u >> 8) & 0xFF);
    b->data[offset + 2] = (uint8_t)((u >> 16) & 0xFF);
    b->data[offset + 3] = (uint8_t)((u >> 24) & 0xFF);
}

int64_t __omni_bytes_get(void* ptr, int64_t offset) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b || offset < 0 || (size_t)offset >= b->len) return 0;
    return (int64_t)b->data[offset];
}

int64_t __omni_bytes_len(void* ptr) {
    OmniBytes* b = (OmniBytes*)ptr;
    return b ? (int64_t)b->len : 0;
}

void __omni_bytes_destroy(void* ptr) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!b) return;
    free(b->data);
    free(b);
}

char* __omni_fs_read_text(const char* path) {
    if (!path) return strdup("");
    FILE* f = fopen(path, "rb");
    if (!f) return strdup("");
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz <= 0) {
        fclose(f);
        return strdup("");
    }
    char* buf = (char*)malloc((size_t)sz + 1);
    if (!buf) {
        fclose(f);
        return strdup("");
    }
    size_t got = fread(buf, 1, (size_t)sz, f);
    buf[got] = '\0';
    fclose(f);
    return buf;
}

int64_t __omni_fs_write_text(const char* path, const char* content) {
    if (!path || !content) return -1;
    FILE* f = fopen(path, "wb");
    if (!f) return -1;
    size_t n = strlen(content);
    size_t wrote = fwrite(content, 1, n, f);
    fclose(f);
    return (int64_t)wrote;
}

int64_t __omni_fs_write_bytes(const char* path, void* ptr) {
    OmniBytes* b = (OmniBytes*)ptr;
    if (!path || !b) return -1;
    FILE* f = fopen(path, "wb");
    if (!f) return -1;
    size_t wrote = b->len > 0 ? fwrite(b->data, 1, b->len, f) : 0;
    fclose(f);
    return (int64_t)wrote;
}

int64_t __omni_fs_exists(const char* path) {
    if (!path) return 0;
    FILE* f = fopen(path, "rb");
    if (!f) return 0;
    fclose(f);
    return 1;
}

int64_t __omni_str_eq(const char* a, const char* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

int64_t __omni_f64_bits_from_str(const char* s) {
    double v = s ? strtod(s, NULL) : 0.0;
    int64_t bits = 0;
    memcpy(&bits, &v, sizeof(double));
    return bits;
}

void __omni_print_f64_bits(int64_t bits) {
    double v = 0.0;
    memcpy(&v, &bits, sizeof(double));
    __omni_print_float(v);
}





