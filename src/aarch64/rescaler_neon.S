/*
 * Area-rescaler row kernels
 * Copyright (c) 2026 Halide Compression, LLC
 *
 * This file is part of wpd.
 */

#include "asm.S"

// The fixed point is 32 bits, so mult_fix is a widening multiply followed by a
// rounding narrow: rshrn's own rounding supplies the 1 << 31.
.macro  mult_fix        out, in, scale, lo, hi
        umull           v\lo\().2d,  v\in\().2s,  v\scale\().s[0]
        umull2          v\hi\().2d,  v\in\().4s,  v\scale\().s[0]
        rshrn           v\out\().2s, v\lo\().2d,  #32
        rshrn2          v\out\().4s, v\hi\().2d,  #32
.endm

// Eight lanes of it, then saturating narrows for the clip to 0..255.
.macro  pack8           a, b, scale
        mult_fix        \a, \a, \scale, 20, 21
        mult_fix        \b, \b, \scale, 22, 23
        uqxtn           v\a\().4h,  v\a\().4s
        uqxtn2          v\a\().8h,  v\b\().4s
        uqxtn           v\a\().8b,  v\a\().8h
.endm

// The same value the vector path produces, for the row's last few lanes.
// x7 carries the rounder; a result past 255 is inverted so strb stores 0xff.
.macro  mult_fix_scalar reg, scale
        umaddl          x\reg, w\reg, \scale, x7
        lsr             x\reg, x\reg, #32
        cmp             w\reg, #255
        csinv           w\reg, w\reg, wzr, ls
.endm

// void ff_rescale_export_direct_neon(uint8_t *dst, const uint32_t *frow,
//                                    int n, uint32_t fy_scale)
function ff_rescale_export_direct_neon, export=1
        fmov            s6,  w3
        mov             x7,  #0x80000000
        cmp             w2,  #8
        b.lt            3f
1:      ld1             {v0.4s, v1.4s}, [x1], #32
        pack8           0, 1, 6
        sub             w2,  w2,  #8
        st1             {v0.8b}, [x0], #8
        cmp             w2,  #8
        b.ge            1b
3:      cmp             w2,  #0
        b.le            9f
4:      ldr             w4,  [x1], #4
        mult_fix_scalar 4, w3
        strb            w4,  [x0], #1
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret
endfunc

// void ff_rescale_export_blend_neon(uint8_t *dst, const uint32_t *irow,
//                                   const uint32_t *frow, int n,
//                                   uint32_t fy_scale, uint32_t wa,
//                                   uint32_t wb)
// The two rows blend with weights that sum to 1 << 32, so the accumulator
// cannot leave 64 bits and umlal can carry the second product.
.macro  blend           out, f, i, lo, hi
        umull           v\lo\().2d,  v\f\().2s,  v7.s[0]
        umull2          v\hi\().2d,  v\f\().4s,  v7.s[0]
        umlal           v\lo\().2d,  v\i\().2s,  v16.s[0]
        umlal2          v\hi\().2d,  v\i\().4s,  v16.s[0]
        rshrn           v\out\().2s, v\lo\().2d, #32
        rshrn2          v\out\().4s, v\hi\().2d, #32
.endm

function ff_rescale_export_blend_neon, export=1
        fmov            s6,  w4
        fmov            s7,  w5
        fmov            s16, w6
        mov             x7,  #0x80000000
        cmp             w3,  #8
        b.lt            3f
1:      ld1             {v0.4s, v1.4s}, [x2], #32
        ld1             {v2.4s, v3.4s}, [x1], #32
        blend           0, 0, 2, 20, 21
        blend           1, 1, 3, 22, 23
        pack8           0, 1, 6
        sub             w3,  w3,  #8
        st1             {v0.8b}, [x0], #8
        cmp             w3,  #8
        b.ge            1b
3:      cmp             w3,  #0
        b.le            9f
4:      ldr             w8,  [x2], #4
        ldr             w9,  [x1], #4
        umaddl          x8,  w8,  w5,  x7
        umaddl          x9,  w9,  w6,  xzr
        add             x8,  x8,  x9
        lsr             x8,  x8,  #32
        mult_fix_scalar 8, w4
        strb            w8,  [x0], #1
        subs            w3,  w3,  #1
        b.gt            4b
9:      ret
endfunc

// void ff_rescale_export_shrink_neon(uint8_t *dst, uint32_t *irow,
//                                    const uint32_t *frow, int n,
//                                    uint32_t yscale, uint32_t fxy_scale)
// The fraction this row carries out of the accumulator is floored, not
// rounded, so it narrows with shrn rather than rshrn.
.macro  carry           out, f, lo, hi
        umull           v\lo\().2d,  v\f\().2s,  v17.s[0]
        umull2          v\hi\().2d,  v\f\().4s,  v17.s[0]
        shrn            v\out\().2s, v\lo\().2d, #32
        shrn2           v\out\().4s, v\hi\().2d, #32
.endm

function ff_rescale_export_shrink_neon, export=1
        fmov            s6,  w5
        fmov            s17, w4
        mov             x7,  #0x80000000
        cmp             w3,  #8
        b.lt            3f
1:      ld1             {v0.4s, v1.4s}, [x2], #32
        ld1             {v2.4s, v3.4s}, [x1]
        carry           4, 0, 20, 21
        carry           5, 1, 22, 23
        sub             v0.4s,   v2.4s,  v4.4s
        sub             v1.4s,   v3.4s,  v5.4s
        st1             {v4.4s, v5.4s}, [x1], #32
        pack8           0, 1, 6
        sub             w3,  w3,  #8
        st1             {v0.8b}, [x0], #8
        cmp             w3,  #8
        b.ge            1b
3:      cmp             w3,  #0
        b.le            9f
4:      ldr             w8,  [x2], #4
        ldr             w9,  [x1]
        umull           x8,  w8,  w4
        lsr             x8,  x8,  #32
        sub             w9,  w9,  w8
        str             w8,  [x1], #4
        mult_fix_scalar 9, w5
        strb            w9,  [x0], #1
        subs            w3,  w3,  #1
        b.gt            4b
9:      ret
endfunc

// void ff_rescale_export_shrink0_neon(uint8_t *dst, uint32_t *irow, int n,
//                                     uint32_t fxy_scale)
function ff_rescale_export_shrink0_neon, export=1
        fmov            s6,  w3
        mov             x7,  #0x80000000
        movi            v4.4s,   #0
        movi            v5.4s,   #0
        cmp             w2,  #8
        b.lt            3f
1:      ld1             {v0.4s, v1.4s}, [x1]
        st1             {v4.4s, v5.4s}, [x1], #32
        pack8           0, 1, 6
        sub             w2,  w2,  #8
        st1             {v0.8b}, [x0], #8
        cmp             w2,  #8
        b.ge            1b
3:      cmp             w2,  #0
        b.le            9f
4:      ldr             w4,  [x1]
        str             wzr, [x1], #4
        mult_fix_scalar 4, w3
        strb            w4,  [x0], #1
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret
endfunc

// void ff_rescale_import_expand_neon(uint32_t *frow, const uint8_t *src,
//                                    int n, int src_width, int channels,
//                                    int x_add, int x_sub)
// Four channels only. They share one accumulator, so a whole pixel is one
// umull/umlal pair; the caller keeps src_width >= 8 and x_add < 1 << 15, and
// accum never leaves 0..x_add, so both weights fit unsigned 16-bit lanes and
// the products cannot reach the wrap the scalar spells out.
//
//   right * x_add + (left - right) * accum == left * accum
//                                             + right * (x_add - accum)
function ff_rescale_import_expand_neon, export=1
        cmp             w2,  #0
        b.le            9f
        ldr             s0,  [x1]
        ldr             s1,  [x1, #4]
        uxtl            v0.8h,   v0.8b            // left
        uxtl            v1.8h,   v1.8b            // right
        mov             w7,  w5                   // accum
1:      sub             w8,  w5,  w7
        orr             w9,  w7,  w8,  lsl #16    // both weights, one fmov
        fmov            s2,  w9
        umull           v3.4s,   v0.4h,  v2.h[0]
        umlal           v3.4s,   v1.4h,  v2.h[1]
        st1             {v3.4s}, [x0], #16
        subs            w2,  w2,  #4
        b.le            9f
        subs            w7,  w7,  w6
        b.pl            1b
        add             x1,  x1,  #4              // the window slides on
        mov             v0.8b,   v1.8b
        ldr             s1,  [x1, #4]
        uxtl            v1.8h,   v1.8b
        add             w7,  w7,  w5
        b               1b
9:      ret
endfunc

// void ff_rescale_import_shrink_neon(uint32_t *frow, const uint8_t *src,
//                                    int n, int x_add, int x_sub,
//                                    uint32_t fx_scale)
// Four channels only. They share one accumulator, so an output pixel is one
// quad of 16-bit lanes and the window's control flow is paid once instead of
// four times. The caller keeps x_add <= x_sub << 7, which caps the window at
// 128 source pixels and so keeps sum, and sum * x_sub, in range.
function ff_rescale_import_shrink_neon, export=1
        cmp             w2,  #0
        b.le            9f
        movi            v0.4h,   #0               // sum
        fmov            s7,  w4                   // x_sub
        fmov            s17, w5                   // fx_scale
        mov             w6,  #0                   // accum
1:      add             w6,  w6,  w3
2:      ldr             s1,  [x1], #4             // base
        uxtl            v1.8h,   v1.8b
        add             v0.4h,   v0.4h,  v1.4h
        subs            w6,  w6,  w4
        b.gt            2b
        neg             w7,  w6
        fmov            s3,  w7                   // the by-element multiplier
        umull           v2.4s,   v1.4h,  v3.h[0]  // frac = base * -accum
        umull           v3.4s,   v0.4h,  v7.h[0]
        sub             v3.4s,   v3.4s,  v2.4s
        st1             {v3.4s}, [x0], #16
        umull           v4.2d,   v2.2s,  v17.s[0] // the fraction carries over
        umull2          v5.2d,   v2.4s,  v17.s[0]
        rshrn           v6.2s,   v4.2d,  #32
        rshrn2          v6.4s,   v5.2d,  #32
        xtn             v0.4h,   v6.4s
        subs            w2,  w2,  #4
        b.gt            1b
9:      ret
endfunc
