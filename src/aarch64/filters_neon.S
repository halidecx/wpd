/*
 * Alpha-plane unfilters
 * Copyright (c) 2026 Halide Compression, LLC
 *
 * This file is part of wpd.
 */

#include "asm.S"

// All three take (const uint8_t *prev, uint8_t *row, int width) and
// reconstruct row in place. A null prev marks the top row, which is
// left-predicted whatever the mode.

//------------------------------------------------------------------------------
// The running total makes every byte depend on the one before it. Adding the
// vector to itself shifted by 1, 2, 4 then 8 bytes turns sixteen of those
// dependencies into four vector adds; only the carry into the next block is
// left on the serial chain.
function ff_horizontal_unfilter_neon, export=1, jumpable=1
        cmp             w2,  #0
        b.le            9f
        ldrb            w3,  [x1]
        cbz             x0,  1f
        ldrb            w4,  [x0]
        add             w3,  w3,  w4
        strb            w3,  [x1]
1:      subs            w2,  w2,  #1
        b.eq            9f
        add             x1,  x1,  #1
        movi            v2.16b,  #0
        dup             v0.16b,  w3
        cmp             w2,  #16
        b.lt            3f
2:      ld1             {v1.16b}, [x1]
.irp    shift, 15, 14, 12, 8
        ext             v3.16b,  v2.16b, v1.16b, #\shift
        add             v1.16b,  v1.16b, v3.16b
.endr
        add             v1.16b,  v1.16b, v0.16b
        dup             v0.16b,  v1.b[15]
        sub             w2,  w2,  #16
        st1             {v1.16b}, [x1], #16
        cmp             w2,  #16
        b.ge            2b
3:      cmp             w2,  #0
        b.le            9f
        umov            w3,  v0.b[0]
4:      ldrb            w4,  [x1]
        add             w3,  w3,  w4
        strb            w3,  [x1], #1
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret
endfunc

//------------------------------------------------------------------------------
function ff_vertical_unfilter_neon, export=1
        cbz             x0,  8f
        cmp             w2,  #32
        b.lt            2f
1:      ld1             {v0.16b, v1.16b}, [x0], #32
        ld1             {v2.16b, v3.16b}, [x1]
        add             v0.16b,  v0.16b, v2.16b
        add             v1.16b,  v1.16b, v3.16b
        sub             w2,  w2,  #32
        st1             {v0.16b, v1.16b}, [x1], #32
        cmp             w2,  #32
        b.ge            1b
2:      cmp             w2,  #16
        b.lt            3f
        ld1             {v0.16b}, [x0], #16
        ld1             {v2.16b}, [x1]
        add             v0.16b,  v0.16b, v2.16b
        sub             w2,  w2,  #16
        st1             {v0.16b}, [x1], #16
3:      cmp             w2,  #0
        b.le            9f
4:      ldrb            w3,  [x0], #1
        ldrb            w4,  [x1]
        add             w3,  w3,  w4
        strb            w3,  [x1], #1
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret
8:      b               X(ff_horizontal_unfilter_neon)
endfunc

//------------------------------------------------------------------------------
// One serially reconstructed pixel: left = in + clip(left + top - top_left).
// The byte wrap rides along as the adds' uxtb, the clamp is branchless so that
// an adversarial plane cannot steer it, and top_left stays in w5 rather than
// being loaded twice.
.macro  gradient_pixel
        ldrb            w4,  [x0], #1
        add             w3,  w4,  w3,  uxtb
        sub             w3,  w3,  w5
        mov             w5,  w4
        ldrb            w4,  [x1]
        bic             w3,  w3,  w3,  asr #31
        cmp             w3,  #255
        csel            w3,  w3,  w7,  lt
        add             w3,  w4,  w3
        strb            w3,  [x1], #1
.endm

// Without the clip and the byte wrap the recurrence telescopes into a plain
// prefix sum, which vectorises. Run that closed form on eight-pixel blocks in
// 16-bit lanes, and keep it only when every intermediate stays inside 0..255,
// so neither the clip nor the wrap could have fired; a dirty block falls back
// to the serial loop above.
//
// Speculating costs a data-dependent branch per block, which on a noisy plane
// mispredicts about as often as it lands. Once GIVE_UP blocks in a row come
// back dirty the next SERIAL_RUN pixels skip the vector work entirely, and the
// row re-arms afterwards: a dirty patch at one end of a row says nothing about
// the other. Each further give-up doubles the run, so a row that is noise all
// the way across stops paying to rediscover that, while one that merely starts
// dirty is back on the fast path after a single short burst.
#define GRADIENT_GIVE_UP    4
#define GRADIENT_SERIAL_RUN 128

function ff_gradient_unfilter_neon, export=1
        cbz             x0,  8f
        cmp             w2,  #0
        b.le            9f
        ldrb            w3,  [x0]
        ldrb            w4,  [x1]
        add             w3,  w3,  w4
        strb            w3,  [x1]
        subs            w2,  w2,  #1
        b.eq            9f
        add             x0,  x0,  #1
        add             x1,  x1,  #1
        mov             w7,  #255
        mov             w8,  #0                  // dirty blocks in a row
        mov             w9,  #GRADIENT_SERIAL_RUN
        movi            v4.16b,  #0
        uxtb            w3,  w3                  // the seed wrapped, and every
        dup             v0.8h,   w3              // lane carries the running left
        cmp             w2,  #8
        b.lt            3f
1:      ld1             {v1.8b}, [x0]
        ldur            d2,  [x0, #-1]
        ld1             {v3.8b}, [x1]
        uxtl            v1.8h,   v1.8b
        uxtl            v2.8h,   v2.8b
        sxtl            v3.8h,   v3.8b           // residuals sign-extend: byte
        sub             v1.8h,   v1.8h,  v2.8h   // 255 is -1, and mod 256 the
        add             v1.8h,   v1.8h,  v3.8h   // check makes them equal
.irp    shift, 14, 12, 8
        ext             v2.16b,  v4.16b, v1.16b, #\shift
        add             v1.8h,   v1.8h,  v2.8h
.endr
        add             v1.8h,   v1.8h,  v0.8h   // speculative out, unclipped
        sub             v2.8h,   v1.8h,  v3.8h   // the predictor each pixel
        orr             v2.16b,  v2.16b, v1.16b  // would clip; nothing outside
        shrn            v2.8b,   v2.8h,  #8      // 0..255 leaves a high byte
        fmov            x4,  d2                  // clear, in either of them
        cbnz            x4,  5f
        xtn             v2.8b,   v1.8h
        dup             v0.8h,   v1.h[7]         // out[7] seeds the next block
        mov             w8,  #0
        add             x0,  x0,  #8
        st1             {v2.8b}, [x1], #8
2:      sub             w2,  w2,  #8
        cmp             w2,  #8
        b.ge            1b
3:      cmp             w2,  #0
        b.le            9f
        umov            w3,  v0.h[0]
        ldurb           w5,  [x0, #-1]
4:      gradient_pixel
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret

        // A dirty block, reconstructed one pixel at a time.
5:      umov            w3,  v0.h[0]
        ldurb           w5,  [x0, #-1]
        mov             w6,  #8
6:      gradient_pixel
        subs            w6,  w6,  #1
        b.gt            6b
        uxtb            w3,  w3
        add             w8,  w8,  #1
        cmp             w8,  #GRADIENT_GIVE_UP
        b.ge            7f
        dup             v0.8h,   w3
        b               2b

        // Enough of them in a row: stop speculating for a while.
7:      sub             w2,  w2,  #8
        mov             w6,  w9
        lsl             w9,  w9,  #1
10:     cmp             w2,  #0
        b.le            9b
        gradient_pixel
        sub             w2,  w2,  #1
        subs            w6,  w6,  #1
        b.gt            10b
        cmp             w2,  #0                  // the run can land on the end
        b.le            9b
        cmp             w2,  #8
        b.lt            4b
        uxtb            w3,  w3
        mov             w8,  #0
        dup             v0.8h,   w3
        b               1b

8:      b               X(ff_horizontal_unfilter_neon)
endfunc
