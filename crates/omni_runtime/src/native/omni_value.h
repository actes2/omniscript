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

// Standard Output
void __omni_val_print(OmniValue v);
void __omni_print_str(const char* s);
void __omni_print_int(int64_t v);

#endif // OMNI_VALUE_H
