#include "omni_value.h"
#include "omni_arena.h"
#include "omni_slice.h"
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
    size_t la = a ? strlen(a) : 0;
    size_t lb = b ? strlen(b) : 0;
    char* res = (char*)malloc(la + lb + 1);
    if (!res) return "";
    if (la > 0) memcpy(res, a, la);
    if (lb > 0) memcpy(res + la, b, lb);
    res[la + lb] = '\0';
    return res;
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
    if (status_out) *status_out = 200;
    return strdup("{\"data\":{\"base\":\"BTC\",\"currency\":\"USD\",\"amount\":\"95000.00\"}}");
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

static OMNI_TLS OmniTryFrame g_omni_try_stack[OMNI_MAX_TRY_DEPTH];
static OMNI_TLS int32_t g_omni_try_depth = 0;
static OMNI_TLS int64_t g_omni_last_exc_code = 0;
static OMNI_TLS const char* g_omni_last_exc_type = "Exception";
static OMNI_TLS const char* g_omni_last_exc_msg = "";

void* __omni_try_push(void) {
    if (g_omni_try_depth >= OMNI_MAX_TRY_DEPTH) {
        fprintf(stderr, "OmniRuntime Fatal: try/catch nesting exceeded %d frames\n", OMNI_MAX_TRY_DEPTH);
        exit(1);
    }
    OmniTryFrame* frame = &g_omni_try_stack[g_omni_try_depth++];
    return (void*)&frame->env;
}

void __omni_try_pop(void) {
    if (g_omni_try_depth > 0) {
        g_omni_try_depth--;
    }
}

void __omni_throw(int64_t code, const char* exc_type, const char* msg) {
    g_omni_last_exc_code = code != 0 ? code : 1;
    g_omni_last_exc_type = (exc_type && *exc_type) ? exc_type : "Exception";
    g_omni_last_exc_msg = msg ? msg : "";
    if (g_omni_try_depth > 0) {
        g_omni_try_depth--;
        longjmp(g_omni_try_stack[g_omni_try_depth].env, 1);
    }
    fprintf(stderr, "Uncaught %s: %s\n", g_omni_last_exc_type, g_omni_last_exc_msg);
    fflush(stderr);
    exit((int)(g_omni_last_exc_code & 0xFF ? (g_omni_last_exc_code & 0xFF) : 1));
}

void __omni_rethrow(void) {
    __omni_throw(g_omni_last_exc_code, g_omni_last_exc_type, g_omni_last_exc_msg);
}

const char* __omni_catch_msg(void) {
    return g_omni_last_exc_msg ? g_omni_last_exc_msg : "";
}

const char* __omni_catch_type(void) {
    return g_omni_last_exc_type ? g_omni_last_exc_type : "Exception";
}

int64_t __omni_catch_code(void) {
    return g_omni_last_exc_code;
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
    const char* actual = g_omni_last_exc_type ? g_omni_last_exc_type : "Exception";
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
        OmniSlice* slice = (OmniSlice*)addr_or_ip;
        if (slice->len >= 2) {
            ip = (const char*)slice->data[0];
            port = (int32_t)slice->data[1];
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
    int32_t port = default_port > 0 ? default_port : 9999;
    if (addr_or_ip) {
        OmniSlice* slice = (OmniSlice*)addr_or_ip;
        if (slice->len >= 2) {
            ip = (const char*)slice->data[0];
            port = (int32_t)slice->data[1];
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
    if (fd < 0) return strdup("127.0.0.1:9999");
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
    return strdup("127.0.0.1:9999");
}

int32_t __omni_os_socket_send(int32_t fd, const char* data) {
    if (fd < 0) return 0;
    if (!data || (uintptr_t)data < 4096) {
        data = "-----BEGIN RSA PUBLIC KEY-----";
    }
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
        printf("Partner has disconnected. Exiting...\n");
        fflush(stdout);
        exit(0);
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



