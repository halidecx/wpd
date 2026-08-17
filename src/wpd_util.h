#ifndef WPD_UTIL_H
#define WPD_UTIL_H

#include "wpd_compat.h"

#include <string.h>

static wpd_always_inline unsigned wpd_clip_uint8(int value) {
    return value & ~255 ? (unsigned)((-value >> 31) & 255) : (unsigned)value;
}

static wpd_always_inline void wpd_w32(void *p, uint32_t v) { memcpy(p, &v, 4); }

#define WPD_WN32A(p, v) wpd_w32(p, v)

#endif
