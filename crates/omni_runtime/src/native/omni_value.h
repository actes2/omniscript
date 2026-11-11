#ifndef OMNI_VALUE_H
#define OMNI_VALUE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

typedef enum {
    OMNI_VAL_NIL = 0,
    OMNI_VAL_INT = 1,
    OMNI_VAL_FLOAT = 2,
    OMNI_VAL_BOOL = 3,
    OMNI_VAL_STR = 4,
    OMNI_VAL_PTR = 5,
} OmniValType;

typedef struct {
    OmniValType type;
    union {
        int64_t i;
        double f;
        bool b;
        void* ptr;
        struct {
            const char* data;
            size_t len;
        } str;
    } as;
} OmniValue;

// Constructors
OmniValue __omni_val_nil(void);
OmniValue __omni_val_int(int64_t v);
OmniValue __omni_val_float(double v);
OmniValue __omni_val_bool(bool v);
OmniValue __omni_val_str(const char* s);
OmniValue __omni_val_ptr(void* ptr);

// Operators
OmniValue __omni_val_add(OmniValue a, OmniValue b);
OmniValue __omni_val_sub(OmniValue a, OmniValue b);
OmniValue __omni_val_mul(OmniValue a, OmniValue b);
OmniValue __omni_val_div(OmniValue a, OmniValue b);
OmniValue __omni_val_mod(OmniValue a, OmniValue b);

// Comparisons
bool __omni_val_eq(OmniValue a, OmniValue b);
bool __omni_val_lt(OmniValue a, OmniValue b);
bool __omni_val_gt(OmniValue a, OmniValue b);
bool __omni_val_truthy(OmniValue v);

// Standard Output & Sleep
void __omni_val_print(OmniValue v);
void __omni_print_str(const char* s);
void __omni_print_str_inline(const char* s);
void __omni_print_str_raw(const char* s);
void __omni_print_int(int64_t v);
char* __omni_input(const char* prompt);
char* __omni_str_concat(const char* a, const char* b);
char* __omni_int_to_str(int64_t v);
void __omni_sleep_s(int64_t seconds);
void __omni_sleep_ms(int64_t milliseconds);
int64_t __omni_random_int(void);
int64_t __omni_random_range(int64_t min, int64_t max);

// Interpretive C-Frontier Short-Circuit Intrinsics & OS Socket Syscall Primitives
char* __omni_c_http_get(const char* url, int32_t* status_out);
char* __omni_c_json_parse(const char* raw_json);
char* __omni_c_json_get(const char* json_str, const char* key);
char* __omni_obj_get(const char* obj, const char* key);
int32_t __omni_c_to_float(const char* s);
int32_t __omni_c_to_int(const char* s);
void __omni_c_return_str(const char* s);
const char* __omni_c_get_returned_str(void);
void __omni_c_return_ptr(void* p);
void* __omni_c_get_returned_ptr(void);
char* __omni_str_token(const char* s, int32_t index);
char* __omni_str_token_arena(void* arena, const char* s, int32_t index);
char* __omni_os_socket_read_arena(void* arena, int32_t client_fd);
int32_t __omni_os_socket_listen(int32_t port);
int32_t __omni_os_socket_accept(int32_t server_fd);
char* __omni_os_socket_read(int32_t client_fd);
void __omni_os_socket_write_http(int32_t client_fd, int32_t status_code, const char* body);
void __omni_os_socket_close(int32_t fd);
int32_t __omni_os_socket_create(int32_t domain, int32_t type, int32_t protocol);
int32_t __omni_os_socket_bind(int32_t fd, void* addr_or_ip, int32_t default_port);
int32_t __omni_os_socket_listen_fd(int32_t fd, int32_t backlog);
void* __omni_os_socket_accept_tuple(int32_t server_fd);
int32_t __omni_os_socket_connect(int32_t fd, void* addr_or_ip, int32_t default_port);
char* __omni_os_socket_getpeername(int32_t fd);
int32_t __omni_os_socket_send(int32_t fd, const char* data);
char* __omni_os_socket_recv(int32_t fd, int32_t max_bytes);
void* __omni_os_pipe_pair(void);

#endif // OMNI_VALUE_H
