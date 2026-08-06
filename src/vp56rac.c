/*
 * VP5/6/8 decoder
 * Copyright (c) 2010 Jason Garrett-Glaser <darkshikari@gmail.com>
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

#include "vp56rac.h"
#include "wpd_codec.h"

#if WPD_RAC_64

void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size) {
    c->value   = 0;
    c->range   = 255 - 1;
    c->bits    = -8;
    c->buffer  = buf;
    c->end     = buf + buf_size;
    c->buf_max = buf_size >= 8 ? buf + buf_size - 8 + 1 : buf;
    c->eof     = 0;
}

// tail refill, one byte at a time; past the end zero bits are shifted in,
// matching what the encoder's flush wrote
void wpd_vp56_load_final_bytes(VP56RangeCoder *c) {
    if (c->buffer < c->end) {
        c->value = (c->value << 8) | *c->buffer++;
        c->bits += 8;
    } else if (!c->eof) {
        c->value <<= 8;
        c->bits += 8;
        c->eof = 1;
    } else {
        c->bits = 0; // to avoid undefined behaviour with shifts
    }
}

#else /* !WPD_RAC_64 */

const uint8_t wpd_vp56_norm_shift[256] = {
    8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
};

void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size) {
    c->high      = 255;
    c->bits      = -16;
    c->buffer    = buf;
    c->end       = buf + buf_size;
    c->code_word = wpd_bytestream_get_be24(&c->buffer);
}

#endif /* WPD_RAC_64 */
