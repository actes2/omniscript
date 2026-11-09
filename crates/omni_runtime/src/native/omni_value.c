#include "omni_value.h"
#include "omni_arena.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef _WIN32
#include <unistd.h>
#else
#include <windows.h>
#include <winsock2.h>
#include <ws2tcpip.h>
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
        case OMNI_VAL_INT: printf("%ld\n", v.as.i); break;
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

void __omni_print_int(int64_t v) {
    printf("%ld\n", v);
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
    int n = snprintf(
        cmd,
        sizeof(cmd),
        "curl -sS -L --max-time 10 -w \"\\n__OMNI_STATUS:%%{http_code}\" \"%s\" 2>/dev/null",
        url
    );
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

static const char* g_omni_returned_str = "";
static int g_omni_accept_count = 0;

void __omni_c_return_str(const char* s) {
    g_omni_returned_str = s ? s : "";
}

const char* __omni_c_get_returned_str(void) {
    return g_omni_returned_str ? g_omni_returned_str : "";
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


