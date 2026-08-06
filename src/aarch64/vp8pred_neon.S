/*
 * Copyright (c) 2009 Mans Rullgard <mans@mansr.com>
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

#include "asm.S"

// These blocks are small enough that a taken branch per row costs more than
// the row itself, so every predictor below is fully unrolled.

function ff_pred16x16_hor_neon, export=1
        sub             x2,  x0,  #1
    .rept 16
        ld1r            {v0.16b}, [x2], x1
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_vert_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.16b}, [x2]
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc

// TrueMotion: dst[x] = clip_uint8(left + top[x] - top[-1]). The top minus
// top-left difference is invariant over the block, so it is widened once and
// each row costs a widening add of the left sample plus a saturating narrow,
// which performs the clip for free.

function ff_pred4x4_tm_neon, export=1
        sub             x3,  x0,  x2            // top row
        sub             x4,  x3,  #1
        ld1r            {v0.8b},  [x4]          // top[-1]
        ld1r            {v1.2s},  [x3]          // top[0..3], twice
        sub             x4,  x0,  #1            // left column
        usubl           v2.8h,  v1.8b,  v0.8b
        ld1r            {v3.8b},  [x4], x2
        ld1r            {v4.8b},  [x4], x2
        ld1r            {v5.8b},  [x4], x2
        ld1r            {v6.8b},  [x4]
        mov             v3.s[1],  v4.s[0]       // rows 0 and 1
        mov             v5.s[1],  v6.s[0]       // rows 2 and 3
        uaddw           v16.8h,   v2.8h,  v3.8b
        uaddw           v17.8h,   v2.8h,  v5.8b
        sqxtun          v16.8b,   v16.8h
        sqxtun          v17.8b,   v17.8h
        st1             {v16.s}[0], [x0], x2
        st1             {v16.s}[1], [x0], x2
        st1             {v17.s}[0], [x0], x2
        st1             {v17.s}[1], [x0]
        ret
endfunc

function ff_pred8x8_tm_neon, export=1
        sub             x2,  x0,  x1            // top row
        sub             x3,  x2,  #1
        ld1r            {v0.8b},  [x3]          // top[-1]
        ld1             {v1.8b},  [x2]          // top[0..7]
        sub             x3,  x0,  #1            // left column
        usubl           v2.8h,  v1.8b,  v0.8b
    .rept 8
        ld1r            {v3.8b},  [x3], x1
        uaddw           v4.8h,  v2.8h,  v3.8b
        sqxtun          v4.8b,  v4.8h
        st1             {v4.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_tm_neon, export=1
        sub             x2,  x0,  x1            // top row
        sub             x3,  x2,  #1
        ld1r            {v0.16b}, [x3]          // top[-1]
        ld1             {v1.16b}, [x2]          // top[0..15]
        sub             x3,  x0,  #1            // left column
        usubl           v2.8h,  v1.8b,  v0.8b
        usubl2          v3.8h,  v1.16b, v0.16b
    .rept 16
        ld1r            {v4.16b}, [x3], x1
        uaddw           v16.8h,   v2.8h,  v4.8b
        uaddw2          v17.8h,   v3.8h,  v4.16b
        sqxtun          v16.8b,   v16.8h
        sqxtun2         v16.16b,  v17.8h
        st1             {v16.16b}, [x0], x1
    .endr
        ret
endfunc

// DC: rounded mean of the top row and left column. The left column is summed
// in a general register -- gathering it into a vector lane by lane serializes
// on the destination register and costs far more than the scalar loads.

function ff_pred8x8_dc_neon, export=1
        sub             x2,  x0,  x1            // top row
        sub             x3,  x0,  #1            // left column
        ld1             {v0.8b},  [x2]
        uaddlv          h0,  v0.8b
        umov            w6,  v0.h[0]            // hoisted to hide its latency
        mov             w4,  #8                 // rounding term
    .rept 8
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        add             w4,  w4,  w6
        lsr             w4,  w4,  #4
        dup             v0.8b,  w4
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_dc_neon, export=1
        sub             x2,  x0,  x1            // top row
        sub             x3,  x0,  #1            // left column
        ld1             {v0.16b}, [x2]
        uaddlv          h0,  v0.16b
        umov            w6,  v0.h[0]            // hoisted to hide its latency
        mov             w4,  #16                // rounding term
    .rept 16
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        add             w4,  w4,  w6
        lsr             w4,  w4,  #5
        dup             v0.16b, w4
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc
