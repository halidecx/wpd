#ifndef WPD_BITREADER_H
#define WPD_BITREADER_H

#include "wpd_internal.h"

/* The VP8L bit reader. Everything here is inline: br_bits() sits in the
   lossless pixel loop, so the shift-and-refill machinery it calls has to be
   visible to the compiler at the call site. */

#define BR_MAX_BITS 24
#define BR_LBITS 64
#define BR_WBITS 32

#define VP8L_NEED_MORE 1
#define VP8L_TAIL_MARGIN 64

static const uint32_t br_bit_mask[BR_MAX_BITS + 1] = {
    0,        0x000001, 0x000003, 0x000007, 0x00000f, 0x00001f, 0x00003f,
    0x00007f, 0x0000ff, 0x0001ff, 0x0003ff, 0x0007ff, 0x000fff, 0x001fff,
    0x003fff, 0x007fff, 0x00ffff, 0x01ffff, 0x03ffff, 0x07ffff, 0x0fffff,
    0x1fffff, 0x3fffff, 0x7fffff, 0xffffff};

typedef struct LEBitReader {
    uint64_t       val;
    const uint8_t *buf;
    size_t         len;
    size_t         pos;
    int            bit_pos;
    int            eos;
} LEBitReader;

/* VP8L is LSB-first; refills leave 32 usable bits in a 64-bit window. */
static inline void br_init(LEBitReader *br, const uint8_t *buf, size_t size) {
    size_t   prefetch = WPD_MIN(size, sizeof(br->val));
    uint64_t value    = 0;

    br->buf     = buf;
    br->len     = size;
    br->bit_pos = 0;
    br->eos     = 0;

    for (size_t i = 0; i < prefetch; i++) value |= (uint64_t)buf[i] << (8 * i);
    br->val = value;
    br->pos = prefetch;
}

static wpd_always_inline int br_is_eos(const LEBitReader *br) {
    return br->eos || (br->pos == br->len && br->bit_pos > BR_LBITS);
}

static inline void br_set_eos(LEBitReader *br) {
    /* Reset bit_pos so later prefetch shifts remain defined. */
    br->eos     = 1;
    br->bit_pos = 0;
}

static inline void br_shift_bytes(LEBitReader *br) {
    while (br->bit_pos >= 8 && br->pos < br->len) {
        br->val >>= 8;
        br->val |= (uint64_t)br->buf[br->pos] << (BR_LBITS - 8);
        br->pos++;
        br->bit_pos -= 8;
    }
    if (br_is_eos(br))
        br_set_eos(br);
}

static wpd_always_inline uint32_t br_prefetch(const LEBitReader *br) {
    return (uint32_t)(br->val >> (br->bit_pos & (BR_LBITS - 1)));
}

static wpd_always_inline void br_set_bit_pos(LEBitReader *br, int pos) {
    br->bit_pos = pos;
}

static inline void br_do_fill(LEBitReader *br) {
    if (br->pos + sizeof(br->val) < br->len) {
        br->val >>= BR_WBITS;
        br->bit_pos -= BR_WBITS;
        br->val |= (uint64_t)WPD_RL32(br->buf + br->pos)
            << (BR_LBITS - BR_WBITS);
        br->pos += BR_WBITS / 8;
        return;
    }
    br_shift_bytes(br);
}

static wpd_always_inline void br_fill(LEBitReader *br) {
    if (br->bit_pos >= BR_WBITS)
        br_do_fill(br);
}

static wpd_always_inline unsigned br_bits(LEBitReader *br, int n) {
    if (!br->eos && n <= BR_MAX_BITS) {
        const uint32_t v = br_prefetch(br) & br_bit_mask[n];
        br->bit_pos += n;
        br_shift_bytes(br);
        return v;
    }
    br_set_eos(br);
    return 0;
}

static wpd_always_inline unsigned br_bit(LEBitReader *br) {
    return br_bits(br, 1);
}

/* Points a reader that has not overrun at a longer, possibly moved, copy of
   the same bytes. Everything it tracks is an offset, so nothing else moves. */
static inline void br_extend(LEBitReader *br, const uint8_t *buf, size_t size) {
    br->buf = buf;
    br->len = size;
}

#endif
