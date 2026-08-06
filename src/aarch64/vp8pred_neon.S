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

// The VP8 4x4 predictors are built from just two edge filters:
//   avg2(a,b)     = (a + b + 1) >> 1          -> urhadd
//   avg3(a,b,c)   = (a + 2b + c + 2) >> 2     -> urhadd(uhadd(a, c), b)
// The uhadd form of avg3 is exact: uhadd truncates (a+c)>>1, and when a+c is
// odd the dropped bit can never carry into bit 2 of the final rounding add.
// Each predictor lays its border samples out as one contiguous sequence, then
// avg2/avg3 of seq, seq<<1 and seq<<2 give every output the block needs.

// v\a = avg2(seq[i], seq[i+1]),  v\b = avg3(seq[i], seq[i+1], seq[i+2])
.macro  edge_filters   seq, a, b, t1, t2
        ext             \t1\().8b, \seq\().8b, \seq\().8b, #1
        ext             \t2\().8b, \seq\().8b, \seq\().8b, #2
        urhadd          \a\().8b,  \seq\().8b, \t1\().8b
        uhadd           \b\().8b,  \seq\().8b, \t2\().8b
        urhadd          \b\().8b,  \b\().8b,   \t1\().8b
.endm

// Gather the left column, top-left and top row as seq = l3,l2,l1,l0,lt,t0,t1,t2
// in d0 (shared by down_right and horizontal_down).
.macro  load_left_rev_top
        sub             x3,  x0,  x2            // top row
        sub             x4,  x3,  #1            // &top[-1]
        sub             x5,  x0,  #1            // &left[0]
        ldrb            w6,  [x5]               // l0
        add             x5,  x5,  x2
        ldrb            w7,  [x5]               // l1
        add             x5,  x5,  x2
        ldrb            w9,  [x5]               // l2
        add             x5,  x5,  x2
        ldrb            w10, [x5]               // l3
        ldr             w11, [x4]               // lt, t0, t1, t2
        orr             w10, w10, w9,  lsl #8
        orr             w10, w10, w7,  lsl #16
        orr             w10, w10, w6,  lsl #24
        orr             x10, x10, x11, lsl #32
        fmov            d0,  x10
.endm

function ff_pred4x4_dc_neon, export=1
        sub             x3,  x0,  x2            // top row
        sub             x4,  x0,  #1            // left column
        ldrb            w5,  [x3]
        ldrb            w6,  [x3, #1]
        ldrb            w7,  [x3, #2]
        ldrb            w9,  [x3, #3]
        add             w5,  w5,  w6
        ldrb            w6,  [x4]
        add             w7,  w7,  w9
        ldrb            w9,  [x4,  x2]
        add             x4,  x4,  x2,  lsl #1
        add             w5,  w5,  w7
        ldrb            w7,  [x4]
        add             w6,  w6,  w9
        ldrb            w9,  [x4,  x2]
        add             w5,  w5,  #4
        add             w7,  w7,  w9
        add             w5,  w5,  w6
        add             w5,  w5,  w7
        lsr             w5,  w5,  #3
        orr             w5,  w5,  w5,  lsl #8   // splat the byte, no SIMD needed
        orr             w5,  w5,  w5,  lsl #16
        str             w5,  [x0]
        str             w5,  [x0,  x2]
        add             x0,  x0,  x2,  lsl #1
        str             w5,  [x0]
        str             w5,  [x0,  x2]
        ret
endfunc

// seq = lt, t0, t1, t2, t3, tr0
function ff_pred4x4_vert_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        ldr             w6,  [x4]               // lt, t0, t1, t2
        ldrb            w7,  [x4, #4]           // t3
        ldrb            w9,  [x1]               // topright[0]
        orr             x6,  x6,  x7,  lsl #32
        orr             x6,  x6,  x9,  lsl #40
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0]
        ret
endfunc

// seq = lt, l0, l1, l2, l3, l3;  row y is avg3 output y broadcast
function ff_pred4x4_hor_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        sub             x5,  x0,  #1
        ldrb            w6,  [x4]               // lt
        ldrb            w7,  [x5]               // l0
        add             x5,  x5,  x2
        ldrb            w9,  [x5]               // l1
        add             x5,  x5,  x2
        ldrb            w10, [x5]               // l2
        add             x5,  x5,  x2
        ldrb            w11, [x5]               // l3
        orr             w6,  w6,  w7,  lsl #8
        orr             w6,  w6,  w9,  lsl #16
        orr             w6,  w6,  w10, lsl #24
        orr             x6,  x6,  x11, lsl #32
        orr             x6,  x6,  x11, lsl #40  // l3 repeated for the last tap
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        dup             v16.8b, v2.b[0]
        dup             v17.8b, v2.b[1]
        dup             v18.8b, v2.b[2]
        dup             v19.8b, v2.b[3]
        st1             {v16.s}[0], [x0], x2
        st1             {v17.s}[0], [x0], x2
        st1             {v18.s}[0], [x0], x2
        st1             {v19.s}[0], [x0]
        ret
endfunc

// seq = t0..t3, tr0..tr3 with tr3 replicated; row y = avg3 outputs y..y+3
function ff_pred4x4_down_left_neon, export=1
        sub             x3,  x0,  x2
        ld1             {v0.s}[0], [x3]         // t0..t3
        ld1             {v0.s}[1], [x1]         // tr0..tr3
        dup             v1.8b,  v0.b[7]         // tr3, the replicated edge
        ext             v3.8b,  v0.8b,  v1.8b,  #1
        ext             v4.8b,  v0.8b,  v1.8b,  #2
        uhadd           v2.8b,  v0.8b,  v4.8b
        urhadd          v2.8b,  v2.8b,  v3.8b
        ext             v5.8b,  v2.8b,  v2.8b,  #1
        ext             v6.8b,  v2.8b,  v2.8b,  #2
        ext             v7.8b,  v2.8b,  v2.8b,  #3
        st1             {v2.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v7.s}[0], [x0]
        ret
endfunc

// seq = l3,l2,l1,l0,lt,t0..t3;  row y = avg3 outputs 3-y .. 6-y
function ff_pred4x4_down_right_neon, export=1
        load_left_rev_top
        add             x6,  x4,  #4
        ld1             {v0.b}[8], [x6]         // t3, the 9th sample
        ext             v3.16b, v0.16b, v0.16b, #1
        ext             v4.16b, v0.16b, v0.16b, #2
        uhadd           v2.16b, v0.16b, v4.16b
        urhadd          v2.16b, v2.16b, v3.16b
        ext             v5.16b, v2.16b, v2.16b, #1
        ext             v6.16b, v2.16b, v2.16b, #2
        ext             v7.16b, v2.16b, v2.16b, #3
        st1             {v7.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v2.s}[0], [x0]
        ret
endfunc

// seq = t0..t3, tr0..tr3;  rows mix avg2 and avg3 outputs
function ff_pred4x4_vert_left_neon, export=1
        sub             x3,  x0,  x2
        ld1             {v0.s}[0], [x3]         // t0..t3
        ld1             {v0.s}[1], [x1]         // tr0..tr3
        edge_filters    v0, v1, v2, v3, v4
        ext             v5.8b,  v1.8b,  v1.8b,  #1
        ins             v5.b[3], v2.b[4]        // A1,A2,A3,B4
        ext             v6.8b,  v2.8b,  v2.8b,  #1
        ins             v6.b[3], v2.b[5]        // B1,B2,B3,B5
        st1             {v1.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0]
        ret
endfunc

// seq = l0..l3 with l3 replicated;  rows are windows of zip(avg2, avg3)
function ff_pred4x4_hor_up_neon, export=1
        sub             x5,  x0,  #1
        ldrb            w6,  [x5]               // l0
        add             x5,  x5,  x2
        ldrb            w7,  [x5]               // l1
        add             x5,  x5,  x2
        ldrb            w9,  [x5]               // l2
        add             x5,  x5,  x2
        ldrb            w10, [x5]               // l3
        orr             w6,  w6,  w7,  lsl #8
        orr             w6,  w6,  w9,  lsl #16
        orr             w6,  w6,  w10, lsl #24
        orr             x6,  x6,  x10, lsl #32
        orr             x6,  x6,  x10, lsl #40  // l3 repeated for the last taps
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        zip1            v5.8b,  v1.8b,  v2.8b   // A0,B0,A1,B1,A2,B2,A3,B3
        ext             v6.8b,  v5.8b,  v5.8b,  #2
        ext             v7.8b,  v5.8b,  v5.8b,  #4
        dup             v16.8b, v0.b[3]         // l3
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v7.s}[0], [x0], x2
        st1             {v16.s}[0], [x0]
        ret
endfunc

// seq = l3,l2,l1,l0,lt,t0,t1,t2;  rows are windows of zip(avg2, avg3)
function ff_pred4x4_hor_down_neon, export=1
        load_left_rev_top
        edge_filters    v0, v1, v2, v3, v4
        zip1            v5.8b,  v1.8b,  v2.8b   // A0,B0,A1,B1,A2,B2,A3,B3
        ext             v6.8b,  v5.8b,  v5.8b,  #2
        ext             v7.8b,  v5.8b,  v5.8b,  #4
        ext             v16.8b, v2.8b,  v2.8b,  #2
        ins             v16.b[0], v1.b[3]       // A3,B3,B4,B5
        st1             {v16.s}[0], [x0], x2
        st1             {v7.s}[0],  [x0], x2
        st1             {v6.s}[0],  [x0], x2
        st1             {v5.s}[0],  [x0]
        ret
endfunc

// seq = l2,l1,l0,lt,t0..t3
function ff_pred4x4_vert_right_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1            // &top[-1]
        sub             x5,  x0,  #1            // &left[0]
        ldrb            w6,  [x5]               // l0
        add             x5,  x5,  x2
        ldrb            w7,  [x5]               // l1
        add             x5,  x5,  x2
        ldrb            w9,  [x5]               // l2
        ldr             w10, [x4]               // lt, t0, t1, t2
        add             x11, x4,  #4
        ldrb            w11, [x11]              // t3
        orr             w9,  w9,  w7,  lsl #8
        orr             w9,  w9,  w6,  lsl #16
        orr             x9,  x9,  x10, lsl #24
        orr             x9,  x9,  x11, lsl #56
        fmov            d0,  x9
        edge_filters    v0, v1, v2, v3, v4
        ext             v5.8b,  v1.8b,  v1.8b,  #3      // A3,A4,A5,A6
        ext             v6.8b,  v2.8b,  v2.8b,  #2      // B2,B3,B4,B5
        ext             v7.8b,  v1.8b,  v1.8b,  #2
        ins             v7.b[0], v2.b[1]                // B1,A3,A4,A5
        ext             v16.8b, v2.8b,  v2.8b,  #1
        ins             v16.b[0], v2.b[0]               // B0,B2,B3,B4
        st1             {v5.s}[0],  [x0], x2
        st1             {v6.s}[0],  [x0], x2
        st1             {v7.s}[0],  [x0], x2
        st1             {v16.s}[0], [x0]
        ret
endfunc


function ff_pred8x8_vert_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.8b},  [x2]
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc
