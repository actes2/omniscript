#include "omni_value.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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
    printf("%s\n", s ? s : "nil");
}

void __omni_print_int(int64_t v) {
    printf("%ld\n", v);
}
