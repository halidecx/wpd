
#ifndef WPD_VP56RAC_H
#define WPD_VP56RAC_H

#include "wpd_codec.h"

#if UINTPTR_MAX > 0xffffffffu && defined(__GNUC__)
#define WPD_RAC_64 1
#else
#define WPD_RAC_64 0
#endif

#if WPD_RAC_64

typedef struct VP56RangeCoder {
    uint64_t       value;
    uint32_t       range;
    int            bits;
    const uint8_t *buffer;
    const uint8_t *buf_max;
    const uint8_t *end;
    int            eof;
} VP56RangeCoder;

void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size);
void wpd_vp56_load_final_bytes(VP56RangeCoder *c);

static wpd_always_inline void vp56_rac_refill(VP56RangeCoder *c) {
    if (c->buffer < c->buf_max) {
        c->value = (c->value << 56) |
            (__builtin_bswap64(wpd_r64(c->buffer)) >> 8);
        c->buffer += 7;
        c->bits += 56;
    } else {
        wpd_vp56_load_final_bytes(c);
    }
}

static wpd_always_inline int vp56_rac_get_prob(VP56RangeCoder *c,
                                               uint8_t         prob) {
    /* Load range before the rare refill call to keep it in a register. */
    uint32_t range = c->range;
    if (c->bits < 0)
        vp56_rac_refill(c);
    {
        const int      pos   = c->bits;
        const uint32_t split = (range * prob) >> 8;
        const uint32_t value = (uint32_t)(c->value >> pos);
        const int      bit   = value > split;
        if (bit) {
            range -= split;
            c->value -= (uint64_t)(split + 1) << pos;
        } else {
            range = split + 1;
        }
        {
            const int shift = 7 ^ (31 ^ __builtin_clz(range));
            range <<= shift;
            c->bits -= shift;
        }
        c->range = range - 1;
        return bit;
    }
}

static wpd_always_inline int vp56_rac_get_prob_branchy(VP56RangeCoder *c,
                                                       int             prob) {
    /* Callers branch on this result, so preserve the decoder's branch shape. */
    uint32_t range = c->range;
    if (c->bits < 0)
        vp56_rac_refill(c);
    {
        const int      pos   = c->bits;
        const uint32_t split = (range * prob) >> 8;
        const uint32_t value = (uint32_t)(c->value >> pos);

        if (value > split) {
            const int shift = 7 ^ (31 ^ __builtin_clz(range - split));
            c->value -= (uint64_t)(split + 1) << pos;
            c->range = ((range - split) << shift) - 1;
            c->bits  = pos - shift;
            return 1;
        }
        {
            const int shift = 7 ^ (31 ^ __builtin_clz(split + 1));
            c->range        = ((split + 1) << shift) - 1;
            c->bits         = pos - shift;
            return 0;
        }
    }
}

static wpd_always_inline int vp8_rac_get_signed(VP56RangeCoder *c, int v) {
    if (c->bits < 0)
        vp56_rac_refill(c);
    {
        const int      pos   = c->bits;
        const uint32_t split = c->range >> 1;
        const uint32_t value = (uint32_t)(c->value >> pos);
        const int32_t  mask  = (int32_t)(split - value) >> 31;
        c->bits -= 1;
        c->range += (uint32_t)mask;
        c->range |= 1;
        c->value -= (uint64_t)((split + 1) & (uint32_t)mask) << pos;
        return (v ^ mask) - mask;
    }
}

#else

typedef struct VP56RangeCoder {
    int            high;
    int            bits;
    const uint8_t *buffer;
    const uint8_t *end;
    unsigned int   code_word;
} VP56RangeCoder;

extern const uint8_t wpd_vp56_norm_shift[256];
void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size);

static wpd_always_inline unsigned int vp56_rac_renorm(VP56RangeCoder *c) {
    int          shift     = wpd_vp56_norm_shift[c->high];
    int          bits      = c->bits;
    unsigned int code_word = c->code_word;

    c->high <<= shift;
    code_word <<= shift;
    bits += shift;
    if (bits >= 0 && c->buffer < c->end) {
        code_word |= wpd_bytestream_get_be16(&c->buffer) << bits;
        bits -= 16;
    }
    c->bits = bits;
    return code_word;
}

static wpd_always_inline int vp56_rac_get_prob(VP56RangeCoder *c,
                                               uint8_t         prob) {
    unsigned int code_word = vp56_rac_renorm(c);
    unsigned int low       = 1 + (((c->high - 1) * prob) >> 8);
    unsigned int low_shift = low << 16;
    int          bit       = code_word >= low_shift;

    c->high      = bit ? c->high - low : low;
    c->code_word = bit ? code_word - low_shift : code_word;

    return bit;
}

static wpd_always_inline int vp56_rac_get_prob_branchy(VP56RangeCoder *c,
                                                       int             prob) {
    unsigned long code_word = vp56_rac_renorm(c);
    unsigned      low       = 1 + (((c->high - 1) * prob) >> 8);
    unsigned      low_shift = low << 16;

    if (code_word >= low_shift) {
        c->high -= low;
        c->code_word = code_word - low_shift;
        return 1;
    }

    c->high      = low;
    c->code_word = code_word;
    return 0;
}

static wpd_always_inline int vp8_rac_get_signed(VP56RangeCoder *c, int v) {
    return vp56_rac_get_prob(c, 128) ? -v : v;
}

#endif

static wpd_always_inline int vp8_rac_get(VP56RangeCoder *c) {
    return vp56_rac_get_prob(c, 128);
}

static wpd_unused int vp8_rac_get_uint(VP56RangeCoder *c, int bits) {
    int value = 0;

    while (bits--) { value = (value << 1) | vp8_rac_get(c); }

    return value;
}

static wpd_unused int vp8_rac_get_sint(VP56RangeCoder *c, int bits) {
    int v;

    if (!vp8_rac_get(c))
        return 0;

    v = vp8_rac_get_uint(c, bits);

    if (vp8_rac_get(c))
        v = -v;

    return v;
}

static wpd_always_inline int vp8_rac_get_tree(VP56RangeCoder *c,
                                              const int8_t (*tree)[2],
                                              const uint8_t *probs) {
    int i = 0;

    do { i = tree[i][vp56_rac_get_prob(c, probs[i])]; } while (i > 0);

    return -i;
}

static wpd_always_inline int vp8_rac_get_coeff(VP56RangeCoder *c,
                                               const uint8_t  *prob) {
    int v = 0;

    do { v = (v << 1) + vp56_rac_get_prob(c, *prob++); } while (*prob);

    return v;
}

#endif
