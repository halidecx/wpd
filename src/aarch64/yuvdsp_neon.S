
#include "asm.S"

const yuv_coeffs, align=4
        .short          19077, 26149, 6419, 13320
        .short          282, 14234, 8708, 17685
endconst

const pack_rgba_tbl, align=4
        .byte           1, 2, 3, 0, 5, 6, 7, 4
        .byte           9, 10, 11, 8, 13, 14, 15, 12
endconst

.macro  finish_r dst
        add             v\dst\().8h, v3.8h, v4.8h
        sub             v\dst\().8h, v\dst\().8h, v29.8h
        sshr            v\dst\().8h, v\dst\().8h, #6
        sqxtun          v\dst\().8b, v\dst\().8h
.endm

.macro  finish_g dst
        sub             v\dst\().8h, v3.8h, v5.8h
        sub             v\dst\().8h, v\dst\().8h, v6.8h
        add             v\dst\().8h, v\dst\().8h, v30.8h
        sshr            v\dst\().8h, v\dst\().8h, #6
        sqxtun          v\dst\().8b, v\dst\().8h
.endm

.macro  finish_b dst
        uqadd           v\dst\().8h, v3.8h, v7.8h
        uqsub           v\dst\().8h, v\dst\().8h, v31.8h
        ushr            v\dst\().8h, v\dst\().8h, #6
        uqxtn           v\dst\().8b, v\dst\().8h
.endm

.macro  recon_uv top, cur, top_off, bottom_off
        ldr             q0,  [\top]
        ldur            q1,  [\top, #1]
        ldr             q2,  [\cur]
        ldur            q3,  [\cur, #1]

        uaddl           v4.8h,   v0.8b,  v1.8b
        uaddw           v4.8h,   v4.8h,  v2.8b
        uaddw           v4.8h,   v4.8h,  v3.8b
        ushll           v5.8h,   v0.8b,  #1
        add             v5.8h,   v5.8h,  v4.8h
        ushll           v6.8h,   v3.8b,  #1
        add             v5.8h,   v5.8h,  v6.8h
        ushll           v6.8h,   v1.8b,  #1
        add             v6.8h,   v6.8h,  v4.8h
        ushll           v7.8h,   v2.8b,  #1
        add             v6.8h,   v6.8h,  v7.8h
        shrn            v5.8b,   v5.8h,  #3
        shrn            v6.8b,   v6.8h,  #3
        urhadd          v4.8b,   v0.8b,  v6.8b
        urhadd          v7.8b,   v1.8b,  v5.8b
        zip1            v4.16b,  v4.16b, v7.16b
        str             q4,  [sp, #\top_off]
        urhadd          v4.8b,   v2.8b,  v5.8b
        urhadd          v7.8b,   v3.8b,  v6.8b
        zip1            v4.16b,  v4.16b, v7.16b
        str             q4,  [sp, #\bottom_off]

        uaddl2          v4.8h,   v0.16b, v1.16b
        uaddw2          v4.8h,   v4.8h,  v2.16b
        uaddw2          v4.8h,   v4.8h,  v3.16b
        ushll2          v5.8h,   v0.16b, #1
        add             v5.8h,   v5.8h,  v4.8h
        ushll2          v6.8h,   v3.16b, #1
        add             v5.8h,   v5.8h,  v6.8h
        ushll2          v6.8h,   v1.16b, #1
        add             v6.8h,   v6.8h,  v4.8h
        ushll2          v7.8h,   v2.16b, #1
        add             v6.8h,   v6.8h,  v7.8h
        shrn            v5.8b,   v5.8h,  #3
        shrn            v6.8b,   v6.8h,  #3
        ext             v4.16b,  v0.16b, v0.16b, #8
        urhadd          v4.8b,   v4.8b,  v6.8b
        ext             v7.16b,  v1.16b, v1.16b, #8
        urhadd          v7.8b,   v7.8b,  v5.8b
        zip1            v4.16b,  v4.16b, v7.16b
        str             q4,  [sp, #(\top_off + 16)]
        ext             v4.16b,  v2.16b, v2.16b, #8
        urhadd          v4.8b,   v4.8b,  v5.8b
        ext             v7.16b,  v3.16b, v3.16b, #8
        urhadd          v7.8b,   v7.8b,  v6.8b
        zip1            v4.16b,  v4.16b, v7.16b
        str             q4,  [sp, #(\bottom_off + 16)]
.endm

.macro  convert8 y, dst, uoff, voff, layout
        ldr             d0,  [\y], #8
        ldr             d1,  [sp, #\uoff]
        ldr             d2,  [sp, #\voff]
        uxtl            v0.8h,   v0.8b
        uxtl            v1.8h,   v1.8b
        uxtl            v2.8h,   v2.8b
        shl             v0.8h,   v0.8h,  #7
        shl             v1.8h,   v1.8h,  #7
        shl             v2.8h,   v2.8h,  #7
        sqdmulh         v3.8h,   v0.8h,  v24.8h
        sqdmulh         v4.8h,   v2.8h,  v25.8h
.ifc \layout,argb
        finish_r        17
.endif
.ifc \layout,rgba
        finish_r        16
.endif
.ifc \layout,bgra
        finish_r        18
.endif
.ifc \layout,rgb
        finish_r        16
.endif
.ifc \layout,bgr
        finish_r        18
.endif
        sqdmulh         v5.8h,   v1.8h,  v26.8h
        sqdmulh         v6.8h,   v2.8h,  v27.8h
.ifc \layout,argb
        finish_g        18
.else
        finish_g        17
.endif
        sqdmulh         v7.8h,   v1.8h,  v28.8h
        add             v7.8h,   v7.8h,  v1.8h
.ifc \layout,argb
        finish_b        19
        movi            v16.8b,  #255
        st4             {v16.8b-v19.8b}, [\dst], #32
.endif
.ifc \layout,rgba
        finish_b        18
        movi            v19.8b,  #255
        st4             {v16.8b-v19.8b}, [\dst], #32
.endif
.ifc \layout,bgra
        finish_b        16
        movi            v19.8b,  #255
        st4             {v16.8b-v19.8b}, [\dst], #32
.endif
.ifc \layout,rgb
        finish_b        18
        st3             {v16.8b-v18.8b}, [\dst], #24
.endif
.ifc \layout,bgr
        finish_b        16
        st3             {v16.8b-v18.8b}, [\dst], #24
.endif
.endm

.macro  upsample_block layout
function ff_upsample_block_\layout\()_neon, export=1
        ldr             w8,  [sp]
        sub             sp,  sp,  #128
        movrel          x9,  yuv_coeffs
        ld1r            {v24.8h}, [x9], #2
        ld1r            {v25.8h}, [x9], #2
        ld1r            {v26.8h}, [x9], #2
        ld1r            {v27.8h}, [x9], #2
        ld1r            {v28.8h}, [x9], #2
        ld1r            {v29.8h}, [x9], #2
        ld1r            {v30.8h}, [x9], #2
        ld1r            {v31.8h}, [x9]
1:      recon_uv        x2, x4, 0, 32
        recon_uv        x3, x5, 64, 96
        convert8        x0, x6, 0, 64, \layout
        convert8        x0, x6, 8, 72, \layout
        convert8        x0, x6, 16, 80, \layout
        convert8        x0, x6, 24, 88, \layout
        cbz             x1,  2f
        convert8        x1, x7, 32, 96, \layout
        convert8        x1, x7, 40, 104, \layout
        convert8        x1, x7, 48, 112, \layout
        convert8        x1, x7, 56, 120, \layout
2:      add             x2,  x2,  #16
        add             x3,  x3,  #16
        add             x4,  x4,  #16
        add             x5,  x5,  #16
        subs            w8,  w8,  #1
        b.gt            1b
        add             sp,  sp,  #128
        ret
endfunc
.endm

upsample_block argb
upsample_block rgba
upsample_block bgra
upsample_block rgb
upsample_block bgr

.macro dispatch_alpha pos, off
function ff_dispatch_alpha_\pos\()_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ldr             q4,  [x1], #16
        ld4             {v0.16b-v3.16b}, [x0]
        sub             w2,  w2,  #16
.if \off == 0
        mov             v0.16b,  v4.16b
.else
        mov             v3.16b,  v4.16b
.endif
        st4             {v0.16b-v3.16b}, [x0], #64
        cmp             w2,  #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w3,  [x1], #1
        subs            w2,  w2,  #1
        strb            w3,  [x0, #\off]
        add             x0,  x0,  #4
        b.gt            3b
9:      ret
endfunc
.endm

dispatch_alpha first, 0
dispatch_alpha last,  3

function ff_pack_rgba_neon, export=1
        cmp             w2,  #16
        b.lt            2f
        movrel          x3,  pack_rgba_tbl
        ldr             q7,  [x3]
1:      ldp             q0,  q1,  [x1], #32
        ldp             q2,  q3,  [x1], #32
        sub             w2,  w2,  #16
        tbl             v0.16b,  {v0.16b}, v7.16b
        tbl             v1.16b,  {v1.16b}, v7.16b
        tbl             v2.16b,  {v2.16b}, v7.16b
        tbl             v3.16b,  {v3.16b}, v7.16b
        stp             q0,  q1,  [x0], #32
        stp             q2,  q3,  [x0], #32
        cmp             w2,  #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w3,  [x1, #1]
        ldrb            w4,  [x1, #2]
        ldrb            w5,  [x1, #3]
        ldrb            w6,  [x1]
        add             x1,  x1,  #4
        subs            w2,  w2,  #1
        strb            w3,  [x0], #1
        strb            w4,  [x0], #1
        strb            w5,  [x0], #1
        strb            w6,  [x0], #1
        b.gt            3b
9:      ret
endfunc

function ff_pack_bgra_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ldp             q0,  q1,  [x1], #32
        ldp             q2,  q3,  [x1], #32
        sub             w2,  w2,  #16
        rev32           v0.16b,  v0.16b
        rev32           v1.16b,  v1.16b
        rev32           v2.16b,  v2.16b
        rev32           v3.16b,  v3.16b
        stp             q0,  q1,  [x0], #32
        stp             q2,  q3,  [x0], #32
        cmp             w2,  #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w3,  [x1, #3]
        ldrb            w4,  [x1, #2]
        ldrb            w5,  [x1, #1]
        ldrb            w6,  [x1]
        add             x1,  x1,  #4
        subs            w2,  w2,  #1
        strb            w3,  [x0], #1
        strb            w4,  [x0], #1
        strb            w5,  [x0], #1
        strb            w6,  [x0], #1
        b.gt            3b
9:      ret
endfunc

.macro  pack24 name, p0, p1, p2
function ff_pack_\name\()_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
.if \p0 == 1
        st3             {v1.16b-v3.16b}, [x0], #48
.else
        mov             v4.16b,  v1.16b
        mov             v1.16b,  v3.16b
        mov             v3.16b,  v4.16b
        st3             {v1.16b-v3.16b}, [x0], #48
.endif
        cmp             w2,  #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w3,  [x1, #\p0]
        ldrb            w4,  [x1, #\p1]
        ldrb            w5,  [x1, #\p2]
        add             x1,  x1,  #4
        subs            w2,  w2,  #1
        strb            w3,  [x0], #1
        strb            w4,  [x0], #1
        strb            w5,  [x0], #1
        b.gt            3b
9:      ret
endfunc
.endm

pack24 rgb, 1, 2, 3
pack24 bgr, 3, 2, 1

// Both output bytes of these two formats are a high field of one input byte
// and the top bits of the next, which is what sri does in one instruction:
// it keeps the destination's leading bits and shifts the source in beneath
// them. 565 needs green twice, its two fields landing at different shifts, so
// the second one gets a shl first.
.macro  pack16_rgb565 sz, a, r, g, b
        shl             v\a\().\sz,  v\g\().\sz, #3
        sri             v\r\().\sz,  v\g\().\sz, #5
        mov             v\g\().16b,  v\a\().16b
        sri             v\g\().\sz,  v\b\().\sz, #3
.endm

.macro  pack16_rgba4444 sz, a, r, g, b
        sri             v\r\().\sz,  v\g\().\sz, #4
        mov             v\g\().16b,  v\b\().16b
        sri             v\g\().\sz,  v\a\().\sz, #4
.endm

// The swapped variants lay the very same two bytes down the other way round,
// so they reuse the field macros and only reverse the store. st2 wants an
// ascending register pair, hence the copy into v3, which the pack has already
// finished with.
.macro  pack16_store sz, bytes, swap
    .if \swap
        mov             v3.16b, v1.16b
        st2             {v2.\sz, v3.\sz}, [x0], #\bytes
    .else
        st2             {v1.\sz, v2.\sz}, [x0], #\bytes
    .endif
.endm

.macro  pack16 name, kind, swap
function ff_pack_\name\()_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
        pack16_\kind    16b, 0, 1, 2, 3
        pack16_store    16b, 32, \swap
        cmp             w2,  #16
        b.ge            1b
2:      cmp             w2,  #8
        b.lt            3f
        ld4             {v0.8b-v3.8b}, [x1], #32
        sub             w2,  w2,  #8
        pack16_\kind    8b, 0, 1, 2, 3
        pack16_store    8b, 16, \swap
3:      cbz             w2,  9f
4:      ld4             {v0.b, v1.b, v2.b, v3.b}[0], [x1], #4
        subs            w2,  w2,  #1
        pack16_\kind    8b, 0, 1, 2, 3
    .if \swap
        mov             v3.16b, v1.16b
        st2             {v2.b, v3.b}[0], [x0], #2
    .else
        st2             {v1.b, v2.b}[0], [x0], #2
    .endif
        b.gt            4b
9:      ret
endfunc
.endm

pack16 rgb565,   rgb565,   0
pack16 rgba4444, rgba4444, 0
pack16 bgr565,   rgb565,   1
pack16 bgra4444, rgba4444, 1

// Every channel is a nibble expanded to eight bits by a multiply by 17, and
// the alpha multiplier is that expansion doubled up. Folding the 257 out of
// it leaves a product that stays inside sixteen bits, so libwebp's truncating
// divide by 255 -- a 32-bit product shifted down by 16 -- becomes
// (p + (p >> 8)) >> 8, which is one usra and the narrowing shift.
.macro  premultiply_mulhi ch, alpha, wide
        umull           v18.8h,  v\ch\().8b,  v\alpha\().8b
    .if \wide
        umull2          v19.8h,  v\ch\().16b, v\alpha\().16b
    .endif
        usra            v18.8h,  v18.8h,  #8
        shrn            v\ch\().8b,  v18.8h, #8
    .if \wide
        usra            v19.8h,  v19.8h,  #8
        shrn2           v\ch\().16b, v19.8h, #8
    .endif
.endm

// 'rg' and 'ba' name the two byte streams ld2 has separated and 'org'/'oba'
// where each one's result goes, so the swapped layout is the same work with
// the two exchanged: alpha still rides the low nibble of the ba byte either
// way, and st2 still writes the lower-numbered register first.
.macro  premultiply4444 sz, wide, rg, ba, org, oba
        and             v2.16b,  v\ba\().16b, v16.16b
        mul             v3.16b,  v2.16b, v17.16b
        ushr            v\org\().\sz,  v\rg\().\sz, #4
        and             v6.16b,  v\rg\().16b, v16.16b
        ushr            v\oba\().\sz,  v\ba\().\sz, #4
        mul             v\org\().16b,  v\org\().16b, v17.16b
        mul             v6.16b,  v6.16b, v17.16b
        mul             v\oba\().16b,  v\oba\().16b, v17.16b
        premultiply_mulhi \org, 3, \wide
        premultiply_mulhi 6, 3, \wide
        premultiply_mulhi \oba, 3, \wide
        sri             v\org\().\sz,  v6.\sz, #4
        bit             v\oba\().16b,  v\ba\().16b, v16.16b
.endm

.macro  premultiply_row_4444 name, rg, ba, org, oba
function ff_premultiply_row_4444\name\()_neon, export=1
        movi            v16.16b, #15
        movi            v17.16b, #17
        cmp             w1,  #16
        b.lt            2f
1:      ld2             {v0.16b, v1.16b}, [x0]
        sub             w1,  w1,  #16
        premultiply4444 16b, 1, \rg, \ba, \org, \oba
        st2             {v4.16b, v5.16b}, [x0], #32
        cmp             w1,  #16
        b.ge            1b
2:      cmp             w1,  #8
        b.lt            3f
        ld2             {v0.8b, v1.8b}, [x0]
        sub             w1,  w1,  #8
        premultiply4444 8b, 0, \rg, \ba, \org, \oba
        st2             {v4.8b, v5.8b}, [x0], #16
3:      cbz             w1,  9f
4:      ld2             {v0.b, v1.b}[0], [x0]
        subs            w1,  w1,  #1
        premultiply4444 8b, 0, \rg, \ba, \org, \oba
        st2             {v4.b, v5.b}[0], [x0], #2
        b.gt            4b
9:      ret
endfunc
.endm

premultiply_row_4444 , 0, 1, 4, 5
premultiply_row_4444 _swap, 1, 0, 5, 4

// Y = (16839 R + 33059 G + 6420 B + 32768 + (16 << 16)) >> 16. Splitting each
// coefficient as 256 * hi + lo keeps either partial sum inside sixteen bits,
// so the whole dot product is umlal on bytes; the rounding term divides evenly
// into the high half as 4224, and the low half folds back in through usra.
.macro  argb_to_y_group sz, wide
        umull           v4.8h,   v1.8b,  v20.8b
        umlal           v4.8h,   v2.8b,  v21.8b
        umlal           v4.8h,   v3.8b,  v22.8b
        umull           v6.8h,   v1.8b,  v23.8b
        umlal           v6.8h,   v2.8b,  v24.8b
        umlal           v6.8h,   v3.8b,  v25.8b
        usra            v4.8h,   v6.8h,  #8
        add             v4.8h,   v4.8h,  v26.8h
        shrn            v4.8b,   v4.8h,  #8
    .if \wide
        umull2          v5.8h,   v1.16b, v20.16b
        umlal2          v5.8h,   v2.16b, v21.16b
        umlal2          v5.8h,   v3.16b, v22.16b
        umull2          v7.8h,   v1.16b, v23.16b
        umlal2          v7.8h,   v2.16b, v24.16b
        umlal2          v7.8h,   v3.16b, v25.16b
        usra            v5.8h,   v7.8h,  #8
        add             v5.8h,   v5.8h,  v26.8h
        shrn2           v4.16b,  v5.8h,  #8
    .endif
.endm

function ff_argb_to_y_neon, export=1
        movi            v20.16b, #65
        movi            v21.16b, #129
        movi            v22.16b, #25
        movi            v23.16b, #199
        movi            v24.16b, #35
        movi            v25.16b, #20
        mov             w3,  #4224
        dup             v26.8h,  w3
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
        argb_to_y_group 16b, 1
        st1             {v4.16b}, [x0], #16
        cmp             w2,  #16
        b.ge            1b
2:      cmp             w2,  #8
        b.lt            3f
        ld4             {v0.8b-v3.8b}, [x1], #32
        sub             w2,  w2,  #8
        argb_to_y_group 8b, 0
        st1             {v4.8b}, [x0], #8
3:      cbz             w2,  9f
4:      ld4             {v0.b, v1.b, v2.b, v3.b}[0], [x1], #4
        subs            w2,  w2,  #1
        argb_to_y_group 8b, 0
        st1             {v4.b}[0], [x0], #1
        b.gt            4b
9:      ret
endfunc

const yuv444_coeffs, align=4
        .short          -9719, -19081, 28800, 28800, -24116, -4684, 0, 0
endconst

.macro  argb_to_uv444_widen hi
    .if \hi
        ushll2          v16.8h,  v1.16b, #0
        ushll2          v17.8h,  v2.16b, #0
        ushll2          v18.8h,  v3.16b, #0
    .else
        ushll           v16.8h,  v1.8b,  #0
        ushll           v17.8h,  v2.8b,  #0
        ushll           v18.8h,  v3.8b,  #0
    .endif
.endm

.macro  argb_to_uv444_calc dst, cr, cg, cb
        smull           v4.4s,   v16.4h, v8.h[\cr]
        smlal           v4.4s,   v17.4h, v8.h[\cg]
        smlal           v4.4s,   v18.4h, v8.h[\cb]
        smull2          v5.4s,   v16.8h, v8.h[\cr]
        smlal2          v5.4s,   v17.8h, v8.h[\cg]
        smlal2          v5.4s,   v18.8h, v8.h[\cb]
        add             v4.4s,   v4.4s,  v28.4s
        add             v5.4s,   v5.4s,  v28.4s
        sqshrn          v\dst\().4h, v4.4s, #16
        sqshrn2         v\dst\().8h, v5.4s, #16
        sqxtun          v\dst\().8b, v\dst\().8h
.endm

.macro  argb_to_uv444_group hi
        argb_to_uv444_widen \hi
        argb_to_uv444_calc 6, 0, 1, 2
        argb_to_uv444_calc 7, 3, 4, 5
.endm

// Scaling the inputs by four and shifting by YUV_FIX + 2, as clip_uv does, is
// the same as leaving them alone and shifting by YUV_FIX, so the rounding and
// the 128 offset collapse into one constant of 32768 + (128 << 16). The sum
// stays within [16, 240] for any input, so neither narrowing saturates.
function ff_argb_to_yuv444_neon, export=1
        str             d8,  [sp, #-16]!
        movi            v20.16b, #65
        movi            v21.16b, #129
        movi            v22.16b, #25
        movi            v23.16b, #199
        movi            v24.16b, #35
        movi            v25.16b, #20
        mov             w5,  #4224
        dup             v26.8h,  w5
        movrel          x5,  yuv444_coeffs
        ld1             {v8.8h}, [x5]
        mov             w5,  #0x8000
        movk            w5,  #0x80, lsl #16
        dup             v28.4s,  w5
        cmp             w4,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x3], #64
        sub             w4,  w4,  #16
        argb_to_y_group 16b, 1
        st1             {v4.16b}, [x0], #16
        argb_to_uv444_group 0
        st1             {v6.8b}, [x1], #8
        st1             {v7.8b}, [x2], #8
        argb_to_uv444_group 1
        st1             {v6.8b}, [x1], #8
        st1             {v7.8b}, [x2], #8
        cmp             w4,  #16
        b.ge            1b
2:      cmp             w4,  #8
        b.lt            3f
        ld4             {v0.8b-v3.8b}, [x3], #32
        sub             w4,  w4,  #8
        argb_to_y_group 8b, 0
        st1             {v4.8b}, [x0], #8
        argb_to_uv444_group 0
        st1             {v6.8b}, [x1], #8
        st1             {v7.8b}, [x2], #8
3:      cbz             w4,  9f
4:      ld4             {v0.b, v1.b, v2.b, v3.b}[0], [x3], #4
        subs            w4,  w4,  #1
        argb_to_y_group 8b, 0
        st1             {v4.b}[0], [x0], #1
        argb_to_uv444_group 0
        st1             {v6.b}[0], [x1], #1
        st1             {v7.b}[0], [x2], #1
        b.gt            4b
9:      ldr             d8,  [sp], #16
        ret
endfunc

.macro  premultiply_ch ch, alpha
        umull           v4.8h,   v\ch\().8b, v\alpha\().8b
        ushr            v5.8h,   v4.8h,   #8
        add             v4.8h,   v4.8h,   v5.8h
        add             v4.8h,   v4.8h,   v7.8h
        shrn            v\ch\().8b, v4.8h, #8
.endm

.macro  premultiply_loop alpha, c0, c1, c2
        movi            v7.8h,   #1
1:      ld4             {v0.8b-v3.8b}, [x0]
        premultiply_ch  \c0, \alpha
        premultiply_ch  \c1, \alpha
        premultiply_ch  \c2, \alpha
        sub             w2,  w2,  #8
        st4             {v0.8b-v3.8b}, [x0], #32
        cmp             w2,  #8
        b.ge            1b
        b               3f
.endm

function ff_premultiply_row_neon, export=1
        cmp             w2,  #8
        b.lt            3f
        cbz             w1,  2f
        premultiply_loop 0, 1, 2, 3
2:      premultiply_loop 3, 0, 1, 2
3:      cbz             w2,  9f
        add             x3,  x0,  #3
        cbz             w1,  4f
        mov             x3,  x0
        add             x0,  x0,  #1
4:      ldrb            w4,  [x3], #4
        mov             w5,  #32897
        mul             w4,  w4,  w5
.irp    off, 0, 1, 2
        ldrb            w5,  [x0, #\off]
        mul             w5,  w5,  w4
        lsr             w5,  w5,  #23
        strb            w5,  [x0, #\off]
.endr
        add             x0,  x0,  #4
        subs            w2,  w2,  #1
        b.gt            4b
9:      ret
endfunc

#define UV_BLOCK  192                   // 16 columns of both rows, for the tail
#define UV_OUT    320
#define UV_FRAME  336

const argb_to_uv_consts, align=4
        .short          1020, 1020, 1020, 1020, 1020, 1020, 1020, 1020
        .float          524288.0, 524288.0, 524288.0, 524288.0
        .long           33685504, 33685504, 33685504, 33685504
        .short          -9719, -19081, 28800, 28800, -24116, -4684, 0, 0
endconst

// Eight gamma_to_linear entries. NEON has no gather, so the indices leave the
// vector in one umov and ubfx picks them apart; packing four results into an x
// register keeps this down to two stores per group.
.macro  gamma8  src, half, off
        umov            x7,  v\src\().d[\half]
.irp    quad, 0, 1
        ubfx            x8,  x7,  #(32 * \quad +  0), #8
        ubfx            x9,  x7,  #(32 * \quad +  8), #8
        ubfx            x10, x7,  #(32 * \quad + 16), #8
        ubfx            x11, x7,  #(32 * \quad + 24), #8
        ldrh            w8,  [x12, x8,  lsl #1]
        ldrh            w9,  [x12, x9,  lsl #1]
        ldrh            w10, [x12, x10, lsl #1]
        ldrh            w11, [x12, x11, lsl #1]
        orr             x8,  x8,  x9,  lsl #16
        orr             x10, x10, x11, lsl #16
        orr             x8,  x8,  x10, lsl #32
        str             x8,  [sp, #(\off + 8 * \quad)]
.endr
.endm

.macro  gamma16 src, off
        gamma8          \src, 0, (\off)
        gamma8          \src, 1, (\off + 16)
.endm

// One channel of eight output pixels: the four samples each averages are two
// neighbouring columns of both rows, so addp folds them without ever splitting
// even from odd columns. linear_to_gamma then interpolates between two entries
// of a table whose values are bytes, which tbl reaches directly once the
// differences are precomputed alongside it; 512 * v0 is a multiple of the
// rounding step, so only the interpolated part goes through the shift.
.macro  uv_channel  off_top, off_bot, dst
        ldp             q1,  q2,  [sp, #(\off_top)]
        ldp             q3,  q5,  [sp, #(\off_bot)]
        addp            v6.8h,   v1.8h,  v2.8h
        addp            v7.8h,   v3.8h,  v5.8h
        add             v6.8h,   v6.8h,  v7.8h
        cbz             w13, 6f
        umull           v7.4s,   v1.4h,  v29.4h
        umull2          v17.4s,  v1.8h,  v29.8h
        addp            v7.4s,   v7.4s,  v17.4s
        umull           v17.4s,  v3.4h,  v31.4h
        umull2          v19.4s,  v3.8h,  v31.8h
        addp            v17.4s,  v17.4s, v19.4s
        add             v7.4s,   v7.4s,  v17.4s
        umull           v17.4s,  v2.4h,  v30.4h
        umull2          v19.4s,  v2.8h,  v30.8h
        addp            v17.4s,  v17.4s, v19.4s
        umull           v19.4s,  v5.4h,  v23.4h
        umull2          v22.4s,  v5.8h,  v23.8h
        addp            v19.4s,  v19.4s, v22.4s
        add             v17.4s,  v17.4s, v19.4s
        mul             v7.4s,   v7.4s,  v20.4s
        mul             v17.4s,  v17.4s, v21.4s
        ushr            v7.4s,   v7.4s,  #17
        ushr            v17.4s,  v17.4s, #17
        uzp1            v7.8h,   v7.8h,  v17.8h
        bit             v6.16b,  v7.16b, v18.16b
6:      ushr            v1.8h,   v6.8h,  #9
        bic             v6.8h,   #0xfe,  lsl #8
        xtn             v2.8b,   v1.8h
        tbl             v3.8b,   {v24.16b, v25.16b}, v2.8b
        tbl             v2.8b,   {v26.16b, v27.16b}, v2.8b
        uxtl            v2.8h,   v2.8b
        mul             v2.8h,   v2.8h,  v6.8h
        urshr           v2.8h,   v2.8h,  #7
        ushll           v\dst\().8h, v3.8b, #2
        add             v\dst\().8h, v\dst\().8h, v2.8h
.endm

.macro  uv_matrix dst, first, rnd
        mov             v2.16b,  \rnd\().16b
        mov             v3.16b,  \rnd\().16b
        smlal           v2.4s,   v0.4h,  v15.h[\first]
        smlal2          v3.4s,   v0.8h,  v15.h[\first]
        smlal           v2.4s,   v4.4h,  v15.h[(\first + 1)]
        smlal2          v3.4s,   v4.8h,  v15.h[(\first + 1)]
        smlal           v2.4s,   v16.4h, v15.h[(\first + 2)]
        smlal2          v3.4s,   v16.8h, v15.h[(\first + 2)]
        sshr            v2.4s,   v2.4s,  #2
        sshr            v3.4s,   v3.4s,  #2
        sqshrun         v2.4h,   v2.4s,  #16
        sqshrun2        v2.8h,   v3.4s,  #16
        sqxtun          \dst\().8b,  v2.8h
.endm

// Sixteen columns of both rows, eight output pixels. A block that is opaque,
// fully transparent, or whose alpha the caller does not want kept is averaged
// unweighted; when no lane of the eight needs the weighted average, which is
// every block of an opaque image, the whole of it is skipped.
.macro  argb_to_uv_body src, stride
        ld4             {v0.16b-v3.16b}, [\src]
        add             x11, \src, \stride
        ld4             {v4.16b-v7.16b}, [x11]
        gamma16         1,  0
        gamma16         5,  32
        gamma16         2,  64
        gamma16         6,  96
        gamma16         3,  128
        gamma16         7,  160
        uaddlp          v16.8h,  v0.16b
        uadalp          v16.8h,  v4.16b
        ldr             q17, [x14]
        cmeq            v18.8h,  v16.8h, v17.8h
        cmeq            v19.8h,  v16.8h, #0
        orr             v18.16b, v18.16b, v19.16b
        bic             v18.16b, v28.16b, v18.16b
        umaxv           h19, v18.8h
        fmov            w13, s19
        cbz             w13, 5f
        ushll           v20.4s,  v16.4h, #0
        ushll2          v21.4s,  v16.8h, #0
        movi            v22.4s,  #1
        umax            v20.4s,  v20.4s, v22.4s
        umax            v21.4s,  v21.4s, v22.4s
        ucvtf           v20.4s,  v20.4s
        ucvtf           v21.4s,  v21.4s
        ldr             q22, [x14, #16]
        // 2^19 over a total in [1, 1020] is never within a float rounding
        // error of an integer from below, so truncating agrees with the
        // integer divide the C does.
        fdiv            v20.4s,  v22.4s, v20.4s
        fdiv            v21.4s,  v22.4s, v21.4s
        fcvtzu          v20.4s,  v20.4s
        fcvtzu          v21.4s,  v21.4s
        uxtl            v29.8h,  v0.8b
        uxtl2           v30.8h,  v0.16b
        uxtl            v31.8h,  v4.8b
        uxtl2           v23.8h,  v4.16b
5:      uv_channel      0,   32,  0
        uv_channel      64,  96,  4
        uv_channel      128, 160, 16
        ldr             q1,  [x14, #32]
        uv_matrix       v20, 0, v1
        uv_matrix       v21, 3, v1
.endm

function ff_argb_to_uv_neon, export=1
        str             d15, [sp, #-16]!
        sub             sp,  sp,  #UV_FRAME
        movrel          x12, X(wpd_gamma_to_linear_tab)
        movrel          x14, argb_to_uv_consts
        // The inverse table is 33 byte-sized entries; loading it twice, one
        // entry apart, builds both the values and their differences, which is
        // what the interpolation wants and what tbl can index.
        movrel          x6,  X(wpd_linear_to_gamma_tab)
        ld1             {v16.8h-v19.8h}, [x6]
        xtn             v24.8b,  v16.8h
        xtn2            v24.16b, v17.8h
        xtn             v25.8b,  v18.8h
        xtn2            v25.16b, v19.8h
        add             x6,  x6,  #2
        ld1             {v16.8h-v19.8h}, [x6]
        xtn             v26.8b,  v16.8h
        xtn2            v26.16b, v17.8h
        xtn             v27.8b,  v18.8h
        xtn2            v27.16b, v19.8h
        sub             v26.16b, v26.16b, v24.16b
        sub             v27.16b, v27.16b, v25.16b
        ldr             q15, [x14, #48]
        dup             v28.4s,  w5
        cmeq            v28.4s,  v28.4s, #0
        not             v28.16b, v28.16b
        lsr             w15, w4,  #4
        cbz             w15, 3f
1:      argb_to_uv_body x2, x3
        st1             {v20.8b}, [x0], #8
        st1             {v21.8b}, [x1], #8
        add             x2,  x2,  #64
        subs            w15, w15, #1
        b.gt            1b
3:      ands            w4,  w4,  #15
        b.eq            9f
        // Fewer than sixteen columns left: gather them into a zeroed block,
        // with the last column doubled up when the width is odd, which is
        // exactly how the scalar path folds a lone column onto itself.
        movi            v0.16b,  #0
        stp             q0,  q0,  [sp, #(UV_BLOCK)]
        stp             q0,  q0,  [sp, #(UV_BLOCK + 32)]
        stp             q0,  q0,  [sp, #(UV_BLOCK + 64)]
        stp             q0,  q0,  [sp, #(UV_BLOCK + 96)]
        add             x6,  sp,  #UV_BLOCK
        mov             w7,  w4
2:      ldr             w8,  [x2]
        ldr             w9,  [x2, x3]
        add             x2,  x2,  #4
        subs            w7,  w7,  #1
        str             w8,  [x6]
        str             w9,  [x6, #64]
        add             x6,  x6,  #4
        b.gt            2b
        tbz             w4,  #0,  4f
        ldur            w8,  [x6, #-4]
        ldr             w9,  [x6, #60]
        str             w8,  [x6]
        str             w9,  [x6, #64]
4:      add             x2,  sp,  #UV_BLOCK
        mov             x3,  #64
        argb_to_uv_body x2, x3
        str             d20, [sp, #(UV_OUT)]
        str             d21, [sp, #(UV_OUT + 8)]
        add             w4,  w4,  #1
        lsr             w4,  w4,  #1
        add             x6,  sp,  #UV_OUT
        add             x7,  sp,  #(UV_OUT + 8)
8:      ldrb            w8,  [x6], #1
        ldrb            w9,  [x7], #1
        subs            w4,  w4,  #1
        strb            w8,  [x0], #1
        strb            w9,  [x1], #1
        b.gt            8b
9:      add             sp,  sp,  #UV_FRAME
        ldr             d15, [sp], #16
        ret
endfunc
