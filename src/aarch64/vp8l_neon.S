
#include "asm.S"

.macro  step_pixel      a, b, c, d
        ext             \a\().16b, \a\().16b, \a\().16b, #4
    .ifnb \b
        ext             \b\().16b, \b\().16b, \b\().16b, #4
    .endif
    .ifnb \c
        ext             \c\().16b, \c\().16b, \c\().16b, #4
    .endif
    .ifnb \d
        ext             \d\().16b, \d\().16b, \d\().16b, #4
    .endif
.endm

function ff_pred_add_0_neon, export=1
        movi            v0.4s,   #255
        cmp             w2,  #16
        b.lt            2f
1:      ldp             q16, q17, [x0], #32
        ldp             q18, q19, [x0], #32
        sub             w2,  w2,  #16
        add             v16.16b, v16.16b, v0.16b
        add             v17.16b, v17.16b, v0.16b
        add             v18.16b, v18.16b, v0.16b
        add             v19.16b, v19.16b, v0.16b
        cmp             w2,  #16
        stp             q16, q17, [x3], #32
        stp             q18, q19, [x3], #32
        b.ge            1b
2:      cmp             w2,  #4
        b.lt            4f
3:      ldr             q16, [x0], #16
        sub             w2,  w2,  #4
        add             v16.16b, v16.16b, v0.16b
        cmp             w2,  #4
        str             q16, [x3], #16
        b.ge            3b
4:      cbz             w2,  9f
5:      ldr             s16, [x0], #4
        subs            w2,  w2,  #1
        add             v16.8b,  v16.8b,  v0.8b
        str             s16, [x3], #4
        b.gt            5b
9:      ret
endfunc

.macro  blend_premult_ch dst, src, tmp
        uxtl            v\tmp\().8h, v\dst\().8b
        mul             v\tmp\().8h, v\tmp\().8h, v16.8h
        ushr            v\tmp\().8h, v\tmp\().8h, #8
        xtn             v\dst\().8b, v\tmp\().8h
        add             v\dst\().8b, v\dst\().8b, v\src\().8b
.endm

function ff_blend_row_argb_premult_neon, export=1
        cmp             w2,  #8
        b.lt            2f
        movi            v17.8h,  #1, lsl #8
1:      ld4             {v0.8b-v3.8b}, [x1], #32
        ld4             {v4.8b-v7.8b}, [x0]
        uxtl            v16.8h, v0.8b
        sub             v16.8h,  v17.8h, v16.8h
        blend_premult_ch 4, 0, 18
        blend_premult_ch 5, 1, 19
        blend_premult_ch 6, 2, 20
        blend_premult_ch 7, 3, 21
        sub             w2,  w2,  #8
        st4             {v4.8b-v7.8b}, [x0], #32
        cmp             w2,  #8
        b.ge            1b
2:      cbz             w2,  9f
        mov             w5,  #256
3:      ldrb            w3,  [x1]
        sub             w3,  w5,  w3
.irp    off, 0, 1, 2, 3
        ldrb            w4,  [x0, #\off]
        ldrb            w6,  [x1, #\off]
        lsl             w6,  w6,  #8
        madd            w4,  w4,  w3,  w6
        lsr             w4,  w4,  #8
        strb            w4,  [x0, #\off]
.endr
        add             x1,  x1,  #4
        add             x0,  x0,  #4
        subs            w2,  w2,  #1
        b.gt            3b
9:      ret
endfunc

function ff_pred_add_1_neon, export=1
        // Retain left in a lane; its recurrence prevents wider independent work.
        ldur            s0,  [x3, #-4]
        dup             v0.4s,   v0.s[0]
        movi            v3.16b,  #0
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        sub             w2,  w2,  #4
        ext             v2.16b,  v3.16b,  v1.16b, #12
        add             v1.16b,  v1.16b,  v2.16b
        ext             v2.16b,  v3.16b,  v1.16b, #8
        add             v1.16b,  v1.16b,  v2.16b
        add             v1.16b,  v1.16b,  v0.16b
        cmp             w2,  #4
        str             q1,  [x3], #16
        dup             v0.4s,   v1.s[3]
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        subs            w2,  w2,  #1
        add             v0.8b,   v1.8b,   v0.8b
        str             s0,  [x3], #4
        b.gt            3b
9:      ret
endfunc

function pred_add_top_neon
        cmp             w2,  #16
        b.lt            2f
1:      ldp             q0,  q1,  [x1], #32
        ldp             q2,  q3,  [x1], #32
        ldp             q16, q17, [x0], #32
        ldp             q18, q19, [x0], #32
        sub             w2,  w2,  #16
        add             v16.16b, v16.16b, v0.16b
        add             v17.16b, v17.16b, v1.16b
        add             v18.16b, v18.16b, v2.16b
        add             v19.16b, v19.16b, v3.16b
        cmp             w2,  #16
        stp             q16, q17, [x3], #32
        stp             q18, q19, [x3], #32
        b.ge            1b
2:      cmp             w2,  #4
        b.lt            4f
3:      ldr             q0,  [x1], #16
        ldr             q16, [x0], #16
        sub             w2,  w2,  #4
        add             v16.16b, v16.16b, v0.16b
        cmp             w2,  #4
        str             q16, [x3], #16
        b.ge            3b
4:      cbz             w2,  9f
5:      ldr             s0,  [x1], #4
        ldr             s16, [x0], #4
        subs            w2,  w2,  #1
        add             v16.8b,  v16.8b,  v0.8b
        str             s16, [x3], #4
        b.gt            5b
9:      ret
endfunc

function ff_pred_add_2_neon, export=1
        b               pred_add_top_neon
endfunc

function ff_pred_add_3_neon, export=1
        add             x1,  x1,  #4
        b               pred_add_top_neon
endfunc

function ff_pred_add_4_neon, export=1
        sub             x1,  x1,  #4
        b               pred_add_top_neon
endfunc

function pred_add_avgtop_neon
        cmp             w2,  #16
        b.lt            2f
1:      ldp             q0,  q1,  [x1], #32
        ldp             q2,  q3,  [x1], #32
        ldp             q4,  q5,  [x5], #32
        ldp             q6,  q7,  [x5], #32
        ldp             q16, q17, [x0], #32
        ldp             q18, q19, [x0], #32
        sub             w2,  w2,  #16
        uhadd           v0.16b,  v0.16b,  v4.16b
        uhadd           v1.16b,  v1.16b,  v5.16b
        uhadd           v2.16b,  v2.16b,  v6.16b
        uhadd           v3.16b,  v3.16b,  v7.16b
        add             v16.16b, v16.16b, v0.16b
        add             v17.16b, v17.16b, v1.16b
        add             v18.16b, v18.16b, v2.16b
        add             v19.16b, v19.16b, v3.16b
        cmp             w2,  #16
        stp             q16, q17, [x3], #32
        stp             q18, q19, [x3], #32
        b.ge            1b
2:      cmp             w2,  #4
        b.lt            4f
3:      ldr             q0,  [x1], #16
        ldr             q4,  [x5], #16
        ldr             q16, [x0], #16
        sub             w2,  w2,  #4
        uhadd           v0.16b,  v0.16b,  v4.16b
        add             v16.16b, v16.16b, v0.16b
        cmp             w2,  #4
        str             q16, [x3], #16
        b.ge            3b
4:      cbz             w2,  9f
5:      ldr             s0,  [x1], #4
        ldr             s4,  [x5], #4
        ldr             s16, [x0], #4
        subs            w2,  w2,  #1
        uhadd           v0.8b,   v0.8b,   v4.8b
        add             v16.8b,  v16.8b,  v0.8b
        str             s16, [x3], #4
        b.gt            5b
9:      ret
endfunc

function ff_pred_add_8_neon, export=1
        sub             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

function ff_pred_add_9_neon, export=1
        add             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

.macro  pred5_pixel
        uhadd           v16.8b,  v0.8b,   v4.8b
        uhadd           v16.8b,  v16.8b,  v2.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function ff_pred_add_5_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q4,  [x1, #4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
.irp    lane, 0, 1, 2, 3
        pred5_pixel
        step_pixel      v1,  v2,  v4
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s4,  [x1, #4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        pred5_pixel
        b.gt            3b
9:      ret
endfunc

.macro  pred67_pixel
        uhadd           v16.8b,  v0.8b,   v2.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function pred_add_avgleft_neon
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
.irp    lane, 0, 1, 2, 3
        pred67_pixel
        step_pixel      v1,  v2
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        pred67_pixel
        b.gt            3b
9:      ret
endfunc

function ff_pred_add_6_neon, export=1
        sub             x1,  x1,  #4
        b               pred_add_avgleft_neon
endfunc

function ff_pred_add_7_neon, export=1
        b               pred_add_avgleft_neon
endfunc

.macro  pred10_pixel
        uhadd           v16.8b,  v0.8b,   v3.8b
        uhadd           v16.8b,  v5.8b,   v16.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function ff_pred_add_10_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldur            q4,  [x1, #4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
        uhadd           v5.16b,  v2.16b,  v4.16b
.irp    lane, 0, 1, 2, 3
        pred10_pixel
        step_pixel      v1,  v3,  v5
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s3,  [x1, #-4]
        ldur            s4,  [x1, #4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        uhadd           v5.8b,   v2.8b,   v4.8b
        pred10_pixel
        b.gt            3b
9:      ret
endfunc

.macro  pred11_pixel
        uabd            v17.8b,  v0.8b,   v3.8b
        uaddlp          v17.4h,  v17.8b
        uaddlp          v17.2s,  v17.4h
        cmhs            v17.2s,  v4.2s,   v17.2s
        add             v0.8b,   v0.8b,   v1.8b
        bit             v0.8b,   v5.8b,   v17.8b
        str             s0,  [x3], #4
.endm

function ff_pred_add_11_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
        uabd            v4.16b,  v2.16b,  v3.16b
        uaddlp          v4.8h,   v4.16b
        uaddlp          v4.4s,   v4.8h
        add             v5.16b,  v2.16b,  v1.16b
.irp    lane, 0, 1, 2, 3
        pred11_pixel
        step_pixel      v1,  v3,  v4,  v5
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s3,  [x1, #-4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        uabd            v4.8b,   v2.8b,   v3.8b
        uaddlp          v4.4h,   v4.8b
        uaddlp          v4.2s,   v4.4h
        add             v5.8b,   v2.8b,   v1.8b
        pred11_pixel
        b.gt            3b
9:      ret
endfunc

.macro  pred12_pixel    diff
        add             v16.8h,  v0.8h,   \diff\().8h
        sqxtun          v16.8b,  v16.8h
        add             v16.8b,  v16.8b,  v1.8b
        str             s16, [x3], #4
        uxtl            v0.8h,   v16.8b
.endm

function ff_pred_add_12_neon, export=1
        ldur            s0,  [x3, #-4]
        uxtl            v0.8h,   v0.8b
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
        usubl           v4.8h,   v2.8b,   v3.8b
        usubl2          v5.8h,   v2.16b,  v3.16b
.irp    diff, v4, v5
.irp    lane, 0, 1
        pred12_pixel    \diff
        ext             v1.16b,  v1.16b,  v1.16b, #4
        ext             \diff\().16b, \diff\().16b, \diff\().16b, #8
.endr
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s3,  [x1, #-4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        usubl           v4.8h,   v2.8b,   v3.8b
        pred12_pixel    v4
        b.gt            3b
9:      ret
endfunc

// Bias TL before uhsub so its rounded difference matches C's truncation toward zero.
.macro  pred13_pixel
        uhadd           v16.8b,  v0.8b,   v2.8b
        cmhi            v17.8b,  v3.8b,   v16.8b
        add             v17.8b,  v3.8b,   v17.8b
        uhsub           v18.8b,  v16.8b,  v17.8b
        uxtl            v19.8h,  v16.8b
        saddw           v19.8h,  v19.8h,  v18.8b
        sqxtun          v20.8b,  v19.8h
        add             v0.8b,   v20.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function ff_pred_add_13_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
.irp    lane, 0, 1, 2, 3
        pred13_pixel
        step_pixel      v1,  v2,  v3
.endr
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s3,  [x1, #-4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        pred13_pixel
        b.gt            3b
9:      ret
endfunc

function ff_extract_green_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
        cmp             w2,  #16
        st1             {v2.16b}, [x0], #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w4,  [x1, #2]
        add             x1,  x1,  #4
        subs            w2,  w2,  #1
        strb            w4,  [x0], #1
        b.gt            3b
9:      ret
endfunc

// NEON has no gather, so index in general registers; ubfx beats a per-pixel
// byte load, and pairing results into stp halves the stores.
function ff_map_color32_neon, export=1
        cmp             w3,  #8
        b.lt            2f
        // dst may alias src, so every load of a block precedes its stores.
1:      ldp             x4,  x5,  [x1]
        ldp             x6,  x7,  [x1, #16]
        ubfx            x8,  x4,  #16, #8
        ubfx            x9,  x4,  #48, #8
        ubfx            x10, x5,  #16, #8
        ubfx            x11, x5,  #48, #8
        ubfx            x12, x6,  #16, #8
        ubfx            x13, x6,  #48, #8
        ubfx            x14, x7,  #16, #8
        ubfx            x15, x7,  #48, #8
        ldr             w8,  [x2,  x8, lsl #2]
        ldr             w9,  [x2,  x9, lsl #2]
        ldr             w10, [x2, x10, lsl #2]
        ldr             w11, [x2, x11, lsl #2]
        ldr             w12, [x2, x12, lsl #2]
        ldr             w13, [x2, x13, lsl #2]
        ldr             w14, [x2, x14, lsl #2]
        ldr             w15, [x2, x15, lsl #2]
        orr             x8,  x8,  x9,  lsl #32
        orr             x10, x10, x11, lsl #32
        orr             x12, x12, x13, lsl #32
        orr             x14, x14, x15, lsl #32
        add             x1,  x1,  #32
        sub             w3,  w3,  #8
        stp             x8,  x10, [x0]
        stp             x12, x14, [x0, #16]
        add             x0,  x0,  #32
        cmp             w3,  #8
        b.ge            1b
2:      cbz             w3,  9f
3:      ldrb            w4,  [x1, #2]
        add             x1,  x1,  #4
        subs            w3,  w3,  #1
        ldr             w4,  [x2, x4, lsl #2]
        str             w4,  [x0], #4
        b.gt            3b
9:      ret
endfunc

// blend_scale[a] = (1 << 24) / a; entry 0 is never selected.
const blend_scale
        .long           0
        .set            bsi, 1
        .rept           255
        .long           (1 << 24) / bsi
        .set            bsi, bsi + 1
        .endr
endconst

// Blend one plane of 8 pixels. v0 = src alpha, v24 = dst factor,
// v26/v27 = 2^24/blend_alpha, v16 = transparent lanes.
.macro  blend_ch        dc, sc
        umull           v28.8h,  \sc\().8b,  v0.8b
        umull           v29.8h,  \dc\().8b,  v24.8b
        uaddl           v30.4s,  v28.4h,  v29.4h
        uaddl2          v31.4s,  v28.8h,  v29.8h
        mul             v30.4s,  v30.4s,  v26.4s
        mul             v31.4s,  v31.4s,  v27.4s
        ushr            v30.4s,  v30.4s,  #24
        ushr            v31.4s,  v31.4s,  #24
        uzp1            v30.8h,  v30.8h,  v31.8h
        xtn             v28.8b,  v30.8h
        bit             v28.8b,  \dc\().8b,  v16.8b
        mov             \dc\().8b,  v28.8b
.endm

.macro  blend_scale_lane idx
        umov            w4,  v23.h[\idx]
        ldr             w4,  [x3, x4, lsl #2]
    .if \idx < 4
        mov             v26.s[\idx], w4
    .else
        mov             v27.s[\idx - 4], w4
    .endif
.endm

.macro  blend_tail_ch   off
        ldrb            w9,  [x1, #\off]
        ldrb            w10, [x0, #\off]
        mul             w9,  w9,  w4
        madd            w9,  w10, w6,  w9
        mul             w9,  w9,  w8
        lsr             w9,  w9,  #24
        strb            w9,  [x0, #\off]
.endm

function ff_blend_row_argb_neon, export=1
        movrel          x3,  blend_scale
        movi            v17.8b,  #255
        cmp             w2,  #8
        b.lt            4f
        // Planar lanes keep the per-pixel alpha broadcast-free.
1:      ld4             {v0.8b-v3.8b}, [x1], #32
        ld4             {v4.8b-v7.8b}, [x0]
        cmeq            v16.8b,  v0.8b,  #0
        cmeq            v18.8b,  v0.8b,  v17.8b
        orr             v19.8b,  v16.8b, v18.8b
        fmov            x4,  d19
        cmn             x4,  #1
        b.eq            2f              // no lane needs the blend
        uxtl            v20.8h,  v0.8b
        movi            v21.8h,  #1,  lsl #8
        sub             v21.8h,  v21.8h, v20.8h
        uxtl            v22.8h,  v4.8b
        mul             v21.8h,  v21.8h, v22.8h
        ushr            v21.8h,  v21.8h, #8
        add             v23.8h,  v20.8h, v21.8h
        xtn             v24.8b,  v21.8h
.irp    idx, 0, 1, 2, 3, 4, 5, 6, 7
        blend_scale_lane \idx
.endr
        blend_ch        v5,  v1
        blend_ch        v6,  v2
        blend_ch        v7,  v3
        xtn             v28.8b,  v23.8h
        bit             v28.8b,  v4.8b,  v16.8b
        mov             v4.8b,   v28.8b
        // Opaque lanes take src verbatim; a no-op on the blended lanes.
2:      bit             v4.8b,   v0.8b,   v18.8b
        bit             v5.8b,   v1.8b,   v18.8b
        bit             v6.8b,   v2.8b,   v18.8b
        bit             v7.8b,   v3.8b,   v18.8b
        sub             w2,  w2,  #8
        st4             {v4.8b-v7.8b}, [x0], #32
        cmp             w2,  #8
        b.ge            1b
4:      cbz             w2,  9f
        mov             w11, #256
5:      ldrb            w4,  [x1]
        cmp             w4,  #255
        b.eq            7f
        cbz             w4,  8f
        ldrb            w5,  [x0]
        sub             w6,  w11, w4
        mul             w6,  w6,  w5
        lsr             w6,  w6,  #8
        add             w7,  w4,  w6
        ldr             w8,  [x3, x7, lsl #2]
        blend_tail_ch   1
        blend_tail_ch   2
        blend_tail_ch   3
        strb            w7,  [x0]
        b               8f
7:      ldr             w4,  [x1]
        str             w4,  [x0]
8:      add             x1,  x1,  #4
        add             x0,  x0,  #4
        subs            w2,  w2,  #1
        b.gt            5b
9:      ret
endfunc
