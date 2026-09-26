
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

function wpd_pred_add_0_neon, export=1
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

function wpd_blend_row_argb_premult_neon, export=1
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

function wpd_pred_add_1_neon, export=1
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

function wpd_pred_add_2_neon, export=1
        b               pred_add_top_neon
endfunc

function wpd_pred_add_3_neon, export=1
        add             x1,  x1,  #4
        b               pred_add_top_neon
endfunc

function wpd_pred_add_4_neon, export=1
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

function wpd_pred_add_8_neon, export=1
        sub             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

function wpd_pred_add_9_neon, export=1
        add             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

.macro  pred5_pixel
        uhadd           v16.8b,  v0.8b,   v4.8b
        uhadd           v16.8b,  v16.8b,  v2.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function wpd_pred_add_5_neon, export=1
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

function wpd_pred_add_6_neon, export=1
        sub             x1,  x1,  #4
        b               pred_add_avgleft_neon
endfunc

function wpd_pred_add_7_neon, export=1
        b               pred_add_avgleft_neon
endfunc

.macro  pred10_pixel
        uhadd           v16.8b,  v0.8b,   v3.8b
        uhadd           v16.8b,  v5.8b,   v16.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function wpd_pred_add_10_neon, export=1
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

.macro  sum4_uaddlp     dst, src
        uaddlp          \dst\().8h, \src\().16b
        uaddlp          \dst\().4s, \dst\().8h
.endm

/* Select picks T + in when sum|L - TL| <= sum|T - TL|, else L + in, and
 * the sum over L is what chains one pixel to the next. So both of the
 * next pixel's comparisons are made ahead: the one for when this pixel
 * takes T + in (MA, from four pixels at once) and the one for when it
 * takes L + in (s, from B = L + in before the select resolves). The
 * select of the mask itself is then the only step the two share. */
.macro  pred11_pixel    sum4
        cmhs            v22.4s,  v17.4s,  v7.4s         // MB: s <= pb
        bsl             v20.16b, v19.16b, v22.16b       // mask
        add             v0.16b,  v0.16b,  v1.16b        // B = L + in
        uabd            v23.16b, v0.16b,  v2.16b
        \sum4           v7,  v23                        // s for the next
        bit             v0.16b,  v5.16b,  v20.16b
        str             s0,  [x3], #4
.endm

.macro  pred_add_11     sum4
        ldur            s0,  [x3, #-4]
        ldur            s3,  [x1, #-4]
        uabd            v23.8b,  v0.8b,   v3.8b
        \sum4           v7,  v23
        movi            v20.16b, #0
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
        add             v5.16b,  v2.16b,  v1.16b        // A = T + in
        uabd            v16.16b, v2.16b,  v3.16b
        \sum4           v17, v16                        // pb
        ext             v18.16b, v21.16b, v5.16b, #12   // previous pixels' A
        mov             v21.16b, v5.16b
        uabd            v18.16b, v18.16b, v3.16b
        \sum4           v19, v18
        cmhs            v19.4s,  v17.4s,  v19.4s        // MA
.irp    lane, 0, 1, 2
        pred11_pixel    \sum4
        step_pixel      v1,  v2,  v5,  v17
        ext             v19.16b, v19.16b, v19.16b, #4
.endr
        pred11_pixel    \sum4
        cmp             w2,  #4
        b.ge            1b
        /* The tail compares against L itself, so s must match it. */
        uabd            v23.8b,  v0.8b,   v2.8b
        \sum4           v7,  v23
        mov             v3.8b,   v2.8b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        uabd            v16.8b,  v2.8b,   v3.8b
        \sum4           v17, v16
        cmhs            v20.4s,  v17.4s,  v7.4s
        add             v0.8b,   v0.8b,   v1.8b
        add             v5.8b,   v2.8b,   v1.8b
        bit             v0.8b,   v5.8b,   v20.8b
        str             s0,  [x3], #4
        uabd            v23.8b,  v0.8b,   v2.8b
        \sum4           v7,  v23
        mov             v3.8b,   v2.8b
        b.gt            3b
9:      ret
.endm

.macro  sum4_udot       dst, src
        movi            \dst\().4s, #0
        udot            \dst\().4s, \src\().16b, v31.16b
.endm

function wpd_pred_add_11_neon, export=1
        pred_add_11     sum4_uaddlp
endfunc

#if HAVE_DOTPROD
ENABLE_DOTPROD
function wpd_pred_add_11_neon_dotprod, export=1
        movi            v31.16b, #1
        pred_add_11     sum4_udot
endfunc
DISABLE_DOTPROD
#endif

/* clamp(L + T - TL) is L raised by T - TL or lowered by TL - T, only one
 * of which is nonzero, so it takes two saturating steps in bytes and L
 * never has to be widened. */
.macro  pred12_pixel
        uqadd           v0.8b,   v0.8b,   v4.8b
        uqsub           v0.8b,   v0.8b,   v5.8b
        add             v0.8b,   v0.8b,   v1.8b
        str             s0,  [x3], #4
.endm

function wpd_pred_add_12_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
        uqsub           v4.16b,  v2.16b,  v3.16b
        uqsub           v5.16b,  v3.16b,  v2.16b
.irp    lane, 0, 1, 2
        pred12_pixel
        step_pixel      v1,  v4,  v5
.endr
        pred12_pixel
        cmp             w2,  #4
        b.ge            1b
2:      cbz             w2,  9f
3:      ldr             s1,  [x0], #4
        ldur            s3,  [x1, #-4]
        ldr             s2,  [x1], #4
        subs            w2,  w2,  #1
        uqsub           v4.8b,   v2.8b,   v3.8b
        uqsub           v5.8b,   v3.8b,   v2.8b
        pred12_pixel
        b.gt            3b
9:      ret
endfunc

/* avg + (avg - TL) / 2 rounds toward zero, so it is avg raised by
 * (avg - TL) >> 1 when avg >= TL and lowered by (TL - avg) >> 1 when not;
 * both saturate on their own and the compare picks one. */
.macro  pred13_pixel
        uhadd           v16.8b,  v0.8b,   v2.8b
        uhsub           v17.8b,  v16.8b,  v3.8b
        uhsub           v18.8b,  v3.8b,   v16.8b
        cmhs            v19.8b,  v16.8b,  v3.8b
        uqadd           v17.8b,  v16.8b,  v17.8b
        uqsub           v18.8b,  v16.8b,  v18.8b
        bsl             v19.8b,  v17.8b,  v18.8b
        add             v0.8b,   v19.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function wpd_pred_add_13_neon, export=1
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

function wpd_extract_green_neon, export=1
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

/* The predictors over an alpha image's green alone take the residual
 * pixels at x0, whose green they add, the green row above at x1 (its top
 * left at x1[-1]) and write x2 bytes of green from x3, whose left is at
 * x3[-1]. */

/* Left: 16 sums at once, each block's running sums made apart from the
 * carry, so from one block to the next the chain is a single add. */
function wpd_pred_green_1_neon, export=1
        ldurb           w4,  [x3, #-1]
        dup             v0.16b,  w4
        movi            v7.16b,  #0
        cmp             w2,  #16
        b.lt            2f
1:      ldp             q16, q17, [x0], #32
        ldp             q18, q19, [x0], #32
        sub             w2,  w2,  #16
        uzp2            v16.8h,  v16.8h,  v17.8h
        uzp2            v18.8h,  v18.8h,  v19.8h
        uzp1            v1.16b,  v16.16b, v18.16b       // green
        ext             v2.16b,  v7.16b,  v1.16b, #15
        add             v1.16b,  v1.16b,  v2.16b
        ext             v2.16b,  v7.16b,  v1.16b, #14
        add             v1.16b,  v1.16b,  v2.16b
        ext             v2.16b,  v7.16b,  v1.16b, #12
        add             v1.16b,  v1.16b,  v2.16b
        ext             v2.16b,  v7.16b,  v1.16b, #8
        add             v1.16b,  v1.16b,  v2.16b
        dup             v3.16b,  v1.b[15]
        add             v1.16b,  v1.16b,  v0.16b
        add             v0.16b,  v0.16b,  v3.16b
        cmp             w2,  #16
        str             q1,  [x3], #16
        b.ge            1b
        umov            w4,  v0.b[0]
2:      cbz             w2,  9f
3:      ldrb            w5,  [x0, #2]
        add             x0,  x0,  #4
        subs            w2,  w2,  #1
        add             w4,  w4,  w5
        strb            w4,  [x3], #1
        b.gt            3b
9:      ret
endfunc

/* Select on green alone takes T + g when |L - TL| <= |T - TL|, else L + g.
 * That is L inside TL -+ |T - TL|, which a compare and a conditional
 * compare test while both sums are made, so from one pixel to the next
 * there are only those two and the pick. Plain scalar code: the chain is
 * the whole cost, and it runs one pixel at a time. */
.macro  green11_ahead   t, tl, g, lo, hi, took_t
        subs            \lo, \t,  \tl
        cneg            \lo, \lo, mi
        add             \hi, \tl, \lo
        sub             \lo, \tl, \lo
        add             \took_t, \t, \g
        and             \took_t, \took_t, #0xff
.endm

.macro  green11_pick    g, lo, hi, took_t
        add             w12, w4,  \g
        cmp             w4,  \lo
        and             w12, w12, #0xff
        ccmp            w4,  \hi, #0, ge
        csel            w4,  \took_t, w12, le
.endm

function wpd_pred_green_11_neon, export=1
        ldurb           w4,  [x3, #-1]                  // L
        ldurb           w5,  [x1, #-1]                  // TL
        subs            w2,  w2,  #2
        b.lt            2f
1:      ldrb            w6,  [x1]
        ldrb            w13, [x1, #1]
        ldrb            w7,  [x0, #2]
        ldrb            w14, [x0, #6]
        add             x1,  x1,  #2
        add             x0,  x0,  #8
        green11_ahead   w6,  w5,  w7,  w8,  w9,  w10
        green11_ahead   w13, w6,  w14, w15, w16, w17
        green11_pick    w7,  w8,  w9,  w10
        strb            w4,  [x3]
        green11_pick    w14, w15, w16, w17
        mov             w5,  w13
        subs            w2,  w2,  #2
        strb            w4,  [x3, #1]
        add             x3,  x3,  #2
        b.ge            1b
2:      adds            w2,  w2,  #2
        b.eq            9f
        ldrb            w6,  [x1]
        ldrb            w7,  [x0, #2]
        green11_ahead   w6,  w5,  w7,  w8,  w9,  w10
        green11_pick    w7,  w8,  w9,  w10
        strb            w4,  [x3]
9:      ret
endfunc

function wpd_add_green_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
        add             v1.16b,  v1.16b,  v2.16b
        add             v3.16b,  v3.16b,  v2.16b
        cmp             w2,  #16
        st4             {v0.16b-v3.16b}, [x0], #64
        b.ge            1b
2:      cmp             w2,  #8
        b.lt            4f
        ld4             {v0.8b-v3.8b}, [x1], #32
        sub             w2,  w2,  #8
        add             v1.8b,   v1.8b,   v2.8b
        add             v3.8b,   v3.8b,   v2.8b
        st4             {v0.8b-v3.8b}, [x0], #32
4:      cbz             w2,  9f
5:      ldrb            w4,  [x1, #1]
        ldrb            w5,  [x1, #2]
        ldrb            w6,  [x1, #3]
        ldrb            w7,  [x1]
        add             x1,  x1,  #4
        add             w4,  w4,  w5
        add             w6,  w6,  w5
        strb            w7,  [x0]
        strb            w4,  [x0, #1]
        strb            w5,  [x0, #2]
        strb            w6,  [x0, #3]
        add             x0,  x0,  #4
        subs            w2,  w2,  #1
        b.gt            5b
9:      ret
endfunc

function wpd_color_row_neon, export=1
        lsr             w4,  w3,  #24           // green_to_red
        lsr             w5,  w3,  #16           // green_to_blue
        lsr             w6,  w3,  #8            // red_to_blue
        dup             v28.16b, w4
        dup             v29.16b, w5
        dup             v30.16b, w6
        cmp             w2,  #16
        b.lt            2f
1:      ld4             {v0.16b-v3.16b}, [x1], #64
        smull           v4.8h,   v2.8b,  v28.8b
        smull2          v5.8h,   v2.16b, v28.16b
        shrn            v4.8b,   v4.8h,  #5
        shrn2           v4.16b,  v5.8h,  #5
        add             v1.16b,  v1.16b, v4.16b
        smull           v6.8h,   v2.8b,  v29.8b
        smull2          v7.8h,   v2.16b, v29.16b
        smull           v16.8h,  v1.8b,  v30.8b
        smull2          v17.8h,  v1.16b, v30.16b
        shrn            v6.8b,   v6.8h,  #5
        shrn2           v6.16b,  v7.8h,  #5
        shrn            v16.8b,  v16.8h, #5
        shrn2           v16.16b, v17.8h, #5
        add             v3.16b,  v3.16b, v6.16b
        sub             w2,  w2,  #16
        add             v3.16b,  v3.16b, v16.16b
        cmp             w2,  #16
        st4             {v0.16b-v3.16b}, [x0], #64
        b.ge            1b
2:      cbz             w2,  9f
3:      ld4             {v0.b-v3.b}[0], [x1], #4
        smull           v4.8h,   v2.8b,  v28.8b
        shrn            v4.8b,   v4.8h,  #5
        add             v1.8b,   v1.8b,  v4.8b
        smull           v6.8h,   v2.8b,  v29.8b
        smull           v16.8h,  v1.8b,  v30.8b
        shrn            v6.8b,   v6.8h,  #5
        shrn            v16.8b,  v16.8h, #5
        add             v3.8b,   v3.8b,  v6.8b
        add             v3.8b,   v3.8b,  v16.8b
        subs            w2,  w2,  #1
        st4             {v0.b-v3.b}[0], [x0], #4
        b.gt            3b
9:      ret
endfunc

function wpd_map_color32_neon, export=1
        cmp             w3,  #8
        b.lt            2f
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

const blend_scale
        .long           0
        .set            bsi, 1
        .rept           255
        .long           (1 << 24) / bsi
        .set            bsi, bsi + 1
        .endr
endconst

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

function wpd_blend_row_argb_neon, export=1
        movrel          x3,  blend_scale
        movi            v17.8b,  #255
        cmp             w2,  #8
        b.lt            4f
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
