#ifndef WPD_UTIL_H
#define WPD_UTIL_H

#include "wpd.h"
#include "wpd_compat.h"

#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WPD_MIN(a, b) ((a) > (b) ? (b) : (a))
#define WPD_MAX(a, b) ((a) > (b) ? (a) : (b))
#define WPD_ABS(a) ((a) >= 0 ? (a) : -(a))

#define WPD_ERROR(e) (-(e))
#define WPD_ERROR_INVALID_DATA (-1094995529)
#define WPD_ERROR_TOO_LARGE (-558319938)

static wpd_always_inline unsigned wpd_clip_uint8(int value) {
    return value & ~255 ? (unsigned)((-value >> 31) & 255) : (unsigned)value;
}
static wpd_always_inline unsigned wpd_clip_uintp2(int value, int bits) {
    int max = (1 << bits) - 1;
    return value < 0 ? 0 : value > max ? max : value;
}

#define WPD_ALLOC_ALIGNMENT 64

static wpd_always_inline void *wpd_align_ptr(void *p) {
    uintptr_t v = (uintptr_t)p + (WPD_ALLOC_ALIGNMENT - 1);
    return (void *)(v & ~(uintptr_t)(WPD_ALLOC_ALIGNMENT - 1));
}

static wpd_always_inline uint64_t wpd_r64(const void *p) {
    uint64_t v;
    memcpy(&v, p, 8);
    return v;
}
static wpd_always_inline void wpd_w32(void *p, uint32_t v) { memcpy(p, &v, 4); }
static wpd_always_inline void wpd_w64(void *p, uint64_t v) { memcpy(p, &v, 8); }

#define WPD_WN32A(p, v) wpd_w32(p, v)
#define WPD_WN64(p, v) wpd_w64(p, v)
#define WPD_RL16(p) ((uint16_t)((p)[0] | (p)[1] << 8))
#define WPD_RL24(p) ((uint32_t)((p)[0] | (p)[1] << 8 | (p)[2] << 16))
#define WPD_RL32(p) \
    ((uint32_t)((p)[0] | (p)[1] << 8 | (p)[2] << 16 | (uint32_t)(p)[3] << 24))
#define WPD_COPY64(d, s) wpd_w64(d, wpd_r64(s))
#define WPD_COPY128(d, s) memcpy(d, s, 16)
#define WPD_ZERO64(d) memset(d, 0, 8)
#define WPD_ZERO128(d) memset(d, 0, 16)
#define WPD_SWAP64(a, b)             \
    do {                             \
        uint64_t wpd_v = wpd_r64(a); \
        WPD_COPY64(a, b);            \
        wpd_w64(b, wpd_v);           \
    } while (0)

static wpd_always_inline unsigned wpd_bytestream_get_be16(const uint8_t **p) {
    unsigned v = (unsigned)(*p)[0] << 8 | (*p)[1];
    *p += 2;
    return v;
}
static wpd_always_inline unsigned wpd_bytestream_get_be24(const uint8_t **p) {
    unsigned v = (unsigned)(*p)[0] << 16 | (unsigned)(*p)[1] << 8 | (*p)[2];
    *p += 3;
    return v;
}

void *wpd_mallocz(size_t size);
void  wpd_free(void *pointer);
void  wpd_freep(void *pointer);
void  wpd_log(void *context, int level, const char *format, ...);
int   wpd_check_image_size(unsigned width, unsigned height);

#endif
