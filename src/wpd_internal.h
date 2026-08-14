#ifndef WPD_INTERNAL_H
#define WPD_INTERNAL_H

#include "wpd_codec.h"

#include <stddef.h>
#include <string.h>

#define MKTAG(a, b, c, d)                                       \
    ((uint32_t)(a) | (uint32_t)(b) << 8 | (uint32_t)(c) << 16 | \
     (uint32_t)(d) << 24)

/* End offset of a struct field, for the struct_size versioning the public
   structs use. */
#define WPD_FIELD_END(type, field) \
    (offsetof(type, field) + sizeof(((type *)0)->field))

#define CEIL_RSHIFT(v, s) (-((-(v)) >> (s)))

#define WPD_FILE_PADDING 64

/* ICCP, EXIF and XMP, in WPDMetadata bit order. */
#define WPD_METADATA_NB 3

static wpd_always_inline uint32_t rb32(const uint8_t *p) {
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 |
        p[3];
}

static wpd_always_inline void wb32(uint8_t *p, uint32_t v) {
    p[0] = v >> 24;
    p[1] = v >> 16;
    p[2] = v >> 8;
    p[3] = v;
}

static wpd_always_inline void copy32(uint8_t *dst, const uint8_t *src) {
    memcpy(dst, src, 4);
}

static wpd_always_inline int u8_to_s8(uint8_t v) { return (int8_t)v; }

#endif
