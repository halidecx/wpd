/**
 * @file
 * VP5 and VP6 compatible video decoder (common features)
 *
 * Copyright (C) 2006  Aurelien Jacobs <aurel@gnuage.org>
 *
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * FFmpeg is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with FFmpeg; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA
 */

#ifndef WPD_VP56RAC_H
#define WPD_VP56RAC_H

#include "wpd_codec.h"

typedef struct {
    int            high;
    int            bits;
    const uint8_t *buffer;
    const uint8_t *end;
    unsigned int   code_word;
} VP56RangeCoder;

/**
 * vp56 specific range coder implementation
 */

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

#if 0
#include "arm/vp56_arith.h"
#elif 0
#include "x86/vp56_arith.h"
#endif

#ifndef vp56_rac_get_prob
#define vp56_rac_get_prob vp56_rac_get_prob
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
#endif

#ifndef vp56_rac_get_prob_branchy
// branchy variant, to be used where there's a branch based on the bit decoded
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
#endif

static wpd_always_inline int vp56_rac_get(VP56RangeCoder *c) {
    unsigned int code_word = vp56_rac_renorm(c);
    /* equiprobable */
    int          low       = (c->high + 1) >> 1;
    unsigned int low_shift = low << 16;
    int          bit       = code_word >= low_shift;
    if (bit) {
        c->high -= low;
        code_word -= low_shift;
    } else {
        c->high = low;
    }

    c->code_word = code_word;
    return bit;
}

// rounding is different than vp56_rac_get, is vp56_rac_get wrong?
static wpd_always_inline int vp8_rac_get(VP56RangeCoder *c) {
    return vp56_rac_get_prob(c, 128);
}

static wpd_unused int vp8_rac_get_uint(VP56RangeCoder *c, int bits) {
    int value = 0;

    while (bits--) { value = (value << 1) | vp8_rac_get(c); }

    return value;
}

// fixme: add 1 bit to all the calls to this?
static wpd_unused int vp8_rac_get_sint(VP56RangeCoder *c, int bits) {
    int v;

    if (!vp8_rac_get(c))
        return 0;

    v = vp8_rac_get_uint(c, bits);

    if (vp8_rac_get(c))
        v = -v;

    return v;
}

/**
 * This is identical to vp8_rac_get_tree except for the possibility of starting
 * on a node other than the root node, needed for coeff decode where this is
 * used to save a bit after a 0 token (by disallowing EOB to immediately follow.)
 */
static wpd_always_inline int vp8_rac_get_tree_with_offset(
    VP56RangeCoder *c, const int8_t (*tree)[2], const uint8_t *probs, int i) {
    do { i = tree[i][vp56_rac_get_prob(c, probs[i])]; } while (i > 0);

    return -i;
}

// how probabilities are associated with decisions is different I think
// well, the new scheme fits in the old but this way has one fewer branches per decision
static wpd_always_inline int vp8_rac_get_tree(VP56RangeCoder *c,
                                              const int8_t (*tree)[2],
                                              const uint8_t *probs) {
    return vp8_rac_get_tree_with_offset(c, tree, probs, 0);
}

// DCTextra
static wpd_always_inline int vp8_rac_get_coeff(VP56RangeCoder *c,
                                               const uint8_t  *prob) {
    int v = 0;

    do { v = (v << 1) + vp56_rac_get_prob(c, *prob++); } while (*prob);

    return v;
}

#endif /* WPD_VP56RAC_H */
