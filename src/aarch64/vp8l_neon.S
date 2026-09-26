
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

/* Two rows at once, the lower (in x4, out x5) under the upper (in x0,
 * above x1, out x3), for the predictors that read no top right. A lone
 * row is one chain of pixels, each waiting on its left, and leaves the
 * core idle for most of a pixel's latency; the lower row a block behind
 * the upper is a second chain to fill that wait. The upper row's blocks
 * are gathered in v4 as they are made, which is all the lower row needs
 * of its above, and v25 keeps the block before for its top left. */
.macro  pair_start_a
        ldr             q1,  [x0], #16
        ldur            q3,  [x1, #-4]
        ldr             q2,  [x1], #16
        sub             w2,  w2,  #4
.endm

.macro  pair_start_b    tl
        ldr             q24, [x4], #16
        ext             \tl\().16b, v25.16b, v4.16b,  #12
        mov             v25.16b, v4.16b
        add             x6,  x6,  #16
.endm

/* From the upper row's first block alone to the lower row's last; each
 * mode's pieces take the sum macro, which only select uses. */
.macro  pair_blocks     m, sum4
        p\m\()_start_a  \sum4
.irp    lane, 0, 1, 2
        p\m\()_pixel_a  \sum4
        p\m\()_step_a
.endr
        p\m\()_pixel_a  \sum4
        str             q4,  [x3], #16
        cmp             w2,  #4
        b.lt            3f
2:      p\m\()_start_b  \sum4
        p\m\()_start_a  \sum4
.irp    lane, 0, 1, 2
        p\m\()_pixel_a  \sum4
        p\m\()_pixel_b  \sum4
        p\m\()_step_a
        p\m\()_step_b
.endr
        p\m\()_pixel_a  \sum4
        p\m\()_pixel_b  \sum4
        p\m\()_end_b
        str             q4,  [x3], #16
        cmp             w2,  #4
        b.ge            2b
3:      p\m\()_start_b  \sum4
.irp    lane, 0, 1, 2
        p\m\()_pixel_b  \sum4
        p\m\()_step_b
.endr
        p\m\()_pixel_b  \sum4
        p\m\()_end_b
.endm

.macro  p11_pixel       sum4, l, in, t, aval, s, pb, ma, mask
        cmhs            v22.4s,  \pb\().4s,  \s\().4s
        bsl             \mask\().16b, \ma\().16b, v22.16b
        add             \l\().16b,  \l\().16b,  \in\().16b
        uabd            v23.16b, \l\().16b,  \t\().16b
        \sum4           \s,  v23
        bit             \l\().16b,  \aval\().16b, \mask\().16b
.endm

.macro  p11_start_a     sum4
        pair_start_a
        add             v5.16b,  v2.16b,  v1.16b
        uabd            v16.16b, v2.16b,  v3.16b
        \sum4           v17, v16
        ext             v18.16b, v21.16b, v5.16b, #12
        mov             v21.16b, v5.16b
        uabd            v18.16b, v18.16b, v3.16b
        \sum4           v19, v18
        cmhs            v19.4s,  v17.4s,  v19.4s
.endm

.macro  p11_start_b     sum4
        pair_start_b    v3
        add             v26.16b, v25.16b, v24.16b
        uabd            v16.16b, v25.16b, v3.16b
        \sum4           v28, v16
        ext             v18.16b, v8.16b,  v26.16b, #12
        mov             v8.16b,  v26.16b
        uabd            v18.16b, v18.16b, v3.16b
        \sum4           v29, v18
        cmhs            v29.4s,  v28.4s,  v29.4s
.endm

.macro  p11_pixel_a     sum4
        p11_pixel       \sum4, v0,  v1,  v2,  v5,  v7,  v17, v19, v20
        ext             v4.16b,  v4.16b,  v0.16b,  #4
.endm

.macro  p11_pixel_b     sum4
        p11_pixel       \sum4, v6,  v24, v25, v26, v27, v28, v29, v30
        str             s6,  [x5], #4
.endm

.macro  p11_step_a
        step_pixel      v1,  v2,  v5,  v17
        ext             v19.16b, v19.16b, v19.16b, #4
.endm

.macro  p11_step_b
        step_pixel      v24, v25, v26, v28
        ext             v29.16b, v29.16b, v29.16b, #4
.endm

.macro  p11_end_b
        ext             v25.16b, v25.16b, v25.16b, #4
.endm

/* A pixel on its own, from the left in v0, the top left in v3 and the
 * distance between them in v7. */
.macro  p11_tail        sum4, in, up, out, n
        ldr             s1,  [\in], #4
        ldr             s2,  [\up], #4
        subs            \n,  \n,  #1
        uabd            v16.8b,  v2.8b,   v3.8b
        \sum4           v17, v16
        cmhs            v20.4s,  v17.4s,  v7.4s
        add             v0.8b,   v0.8b,   v1.8b
        add             v5.8b,   v2.8b,   v1.8b
        bit             v0.8b,   v5.8b,   v20.8b
        str             s0,  [\out], #4
        uabd            v23.8b,  v0.8b,   v2.8b
        \sum4           v7,  v23
        mov             v3.8b,   v2.8b
.endm

.macro  pred_add_11_pair sum4
        str             d8,  [sp, #-16]!
        mov             x6,  x3
        ldur            s0,  [x3, #-4]
        ldur            s3,  [x1, #-4]
        ldur            s6,  [x5, #-4]
        dup             v25.4s,  v0.s[0]
        cmp             w2,  #4
        b.lt            4f
        uabd            v23.8b,  v0.8b,   v3.8b
        \sum4           v7,  v23
        uabd            v23.8b,  v6.8b,   v25.8b
        \sum4           v27, v23
        movi            v20.16b, #0
        movi            v30.16b, #0
        pair_blocks     11,  \sum4
        mov             v3.8b,   v2.8b
4:      cbz             w2,  9f
        mov             w7,  w2
        uabd            v23.8b,  v0.8b,   v3.8b
        \sum4           v7,  v23
5:      p11_tail        \sum4, x0,  x1,  x3,  w2
        b.gt            5b
        mov             v0.8b,   v6.8b
        dup             v3.4s,   v25.s[3]
        uabd            v23.8b,  v0.8b,   v3.8b
        \sum4           v7,  v23
6:      p11_tail        \sum4, x4,  x6,  x5,  w7
        b.gt            6b
9:      ldr             d8,  [sp], #16
        ret
.endm

function wpd_pred_add_11_pair_neon, export=1
        pred_add_11_pair sum4_uaddlp
endfunc

#if HAVE_DOTPROD
ENABLE_DOTPROD
function wpd_pred_add_11_pair_neon_dotprod, export=1
        movi            v31.16b, #1
        pred_add_11_pair sum4_udot
endfunc
DISABLE_DOTPROD
#endif

.macro  p12_px          l, in, up, down
        uqadd           \l\().8b,  \l\().8b,  \up\().8b
        uqsub           \l\().8b,  \l\().8b,  \down\().8b
        add             \l\().8b,  \l\().8b,  \in\().8b
.endm

.macro  p12_start_a     sum4
        pair_start_a
        uqsub           v16.16b, v2.16b,  v3.16b
        uqsub           v17.16b, v3.16b,  v2.16b
.endm

.macro  p12_start_b     sum4
        pair_start_b    v3
        uqsub           v26.16b, v25.16b, v3.16b
        uqsub           v27.16b, v3.16b,  v25.16b
.endm

.macro  p12_pixel_a     sum4
        p12_px          v0,  v1,  v16, v17
        ext             v4.16b,  v4.16b,  v0.16b,  #4
.endm

.macro  p12_pixel_b     sum4
        p12_px          v6,  v24, v26, v27
        str             s6,  [x5], #4
.endm

.macro  p12_step_a
        step_pixel      v1,  v16, v17
.endm

.macro  p12_step_b
        step_pixel      v24, v26, v27
.endm

.macro  p12_end_b
.endm

.macro  p12_tail        in, up, out, n, l
        ldr             s1,  [\in], #4
        ldur            s3,  [\up, #-4]
        ldr             s2,  [\up], #4
        subs            \n,  \n,  #1
        uqsub           v16.8b,  v2.8b,   v3.8b
        uqsub           v17.8b,  v3.8b,   v2.8b
        p12_px          v\l, v1,  v16, v17
        str             s\l, [\out], #4
.endm

.macro  p13_px          l, in, t, tl
        uhadd           v16.8b,  \l\().8b,  \t\().8b
        uhsub           v17.8b,  v16.8b,  \tl\().8b
        uhsub           v18.8b,  \tl\().8b, v16.8b
        cmhs            v19.8b,  v16.8b,  \tl\().8b
        uqadd           v17.8b,  v16.8b,  v17.8b
        uqsub           v18.8b,  v16.8b,  v18.8b
        bsl             v19.8b,  v17.8b,  v18.8b
        add             \l\().8b,  v19.8b,  \in\().8b
.endm

.macro  p13_start_a     sum4
        pair_start_a
.endm

.macro  p13_start_b     sum4
        pair_start_b    v26
.endm

.macro  p13_pixel_a     sum4
        p13_px          v0,  v1,  v2,  v3
        ext             v4.16b,  v4.16b,  v0.16b,  #4
.endm

.macro  p13_pixel_b     sum4
        p13_px          v6,  v24, v25, v26
        str             s6,  [x5], #4
.endm

.macro  p13_step_a
        step_pixel      v1,  v2,  v3
.endm

.macro  p13_step_b
        step_pixel      v24, v25, v26
.endm

.macro  p13_end_b
        ext             v25.16b, v25.16b, v25.16b, #4
.endm

.macro  p13_tail        in, up, out, n, l
        ldr             s1,  [\in], #4
        ldur            s3,  [\up, #-4]
        ldr             s2,  [\up], #4
        subs            \n,  \n,  #1
        p13_px          v\l, v1,  v2,  v3
        str             s\l, [\out], #4
.endm

/* The top lefts of both rows' tails are read back, the lower row's from
 * the upper row's output. */
.macro  pred_add_pair   m
        mov             x6,  x3
        ldur            s0,  [x3, #-4]
        ldur            s6,  [x5, #-4]
        dup             v25.4s,  v0.s[0]
        cmp             w2,  #4
        b.lt            4f
        pair_blocks     \m
4:      cbz             w2,  9f
        mov             w7,  w2
5:      p\m\()_tail     x0,  x1,  x3,  w2,  0
        b.gt            5b
6:      p\m\()_tail     x4,  x6,  x5,  w7,  6
        b.gt            6b
9:      ret
.endm

function wpd_pred_add_12_pair_neon, export=1
        pred_add_pair   12
endfunc

function wpd_pred_add_13_pair_neon, export=1
        pred_add_pair   13
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
 * there are only those two and the pick. */
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

/* Runs w16 pixels one at a time, two to an iteration. */
.macro  green11_serial
        subs            w16, w16, #2
        b.lt            21f
20:     ldrb            w6,  [x1]
        ldrb            w13, [x1, #1]
        ldrb            w7,  [x0, #2]
        ldrb            w14, [x0, #6]
        add             x1,  x1,  #2
        add             x0,  x0,  #8
        green11_ahead   w6,  w5,  w7,  w8,  w9,  w10
        green11_ahead   w13, w6,  w14, w15, w11, w17
        green11_pick    w7,  w8,  w9,  w10
        strb            w4,  [x3]
        green11_pick    w14, w15, w11, w17
        mov             w5,  w13
        subs            w16, w16, #2
        strb            w4,  [x3, #1]
        add             x3,  x3,  #2
        b.ge            20b
21:     tbz             w16, #0, 22f                    // -1: one left
        ldrb            w6,  [x1], #1
        ldrb            w7,  [x0, #2]
        add             x0,  x0,  #4
        green11_ahead   w6,  w5,  w7,  w8,  w9,  w10
        green11_pick    w7,  w8,  w9,  w10
        mov             w5,  w6
        strb            w4,  [x3], #1
22:
.endm

/* Where every pixel of a block takes T + g, each one's L is its left
 * neighbour's T + g, so the whole block's picks can be checked at once from
 * those sums; flat alpha passes nearly always. A block that fails is redone
 * one pixel at a time, from its first: starting at the first pixel to take
 * L instead would hang the serial chain on the check. As with the gradient
 * unfilter, GIVE_UP failed blocks in a row stop the speculation for the next
 * SERIAL_RUN pixels, doubling with each further give-up in the call. */
#define GREEN11_GIVE_UP    4
#define GREEN11_SERIAL_RUN 128

function wpd_pred_green_11_neon, export=1
        stp             x19, x20, [sp, #-16]!
        ldurb           w4,  [x3, #-1]                  // L
        ldurb           w5,  [x1, #-1]                  // TL
        mov             w19, #0                         // failed blocks in a row
        mov             w20, #GREEN11_SERIAL_RUN
        cmp             w2,  #16
        b.lt            3f
        dup             v0.16b,  w4
1:      ldp             q16, q17, [x0]
        ldp             q18, q19, [x0, #32]
        ldr             q2,  [x1]                       // T
        ldur            q3,  [x1, #-1]                  // TL
        uzp2            v16.8h,  v16.8h,  v17.8h
        uzp2            v18.8h,  v18.8h,  v19.8h
        uzp1            v1.16b,  v16.16b, v18.16b       // g
        add             v1.16b,  v1.16b,  v2.16b        // T + g
        ext             v4.16b,  v0.16b,  v1.16b, #15   // L, had all taken T
        uabd            v5.16b,  v2.16b,  v3.16b
        uabd            v6.16b,  v4.16b,  v3.16b
        cmhi            v6.16b,  v6.16b,  v5.16b        // took L instead
        shrn            v6.8b,   v6.8h,   #4
        fmov            x15, d6
        cbnz            x15, 5f
        str             q1,  [x3]
        mov             v0.16b,  v1.16b
        mov             w19, #0
        add             x0,  x0,  #64
        add             x1,  x1,  #16
        add             x3,  x3,  #16
2:      sub             w2,  w2,  #16
        cmp             w2,  #16
        b.ge            1b
        umov            w4,  v0.b[15]
        ldurb           w5,  [x1, #-1]
3:      cmp             w2,  #0
        b.le            9f
        mov             w16, w2
        green11_serial
9:      ldp             x19, x20, [sp], #16
        ret

5:      mov             w16, #16
        ldurb           w4,  [x3, #-1]
        ldurb           w5,  [x1, #-1]
        green11_serial
        add             w19, w19, #1
        cmp             w19, #GREEN11_GIVE_UP
        b.ge            7f
        dup             v0.16b,  w4
        b               2b

        // Enough of them in a row: stop speculating for a while.
7:      sub             w2,  w2,  #16
        cmp             w2,  w20
        csel            w16, w2,  w20, lt
        sub             w2,  w2,  w16
        lsl             w20, w20, #1
        green11_serial
        cmp             w2,  #16
        b.lt            3b
        mov             w19, #0
        dup             v0.16b,  w4
        b               1b
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

// Looks up the eight pixels held two to a register in \a..\d, and stores
// them.
.macro  map8            a, b, c, d
        ubfx            x12, \a,  #16, #8
        ubfx            x13, \a,  #48, #8
        ubfx            x14, \b,  #16, #8
        ubfx            x15, \b,  #48, #8
        ldr             w12, [x2, x12, lsl #2]
        ldr             w13, [x2, x13, lsl #2]
        ldr             w14, [x2, x14, lsl #2]
        ldr             w15, [x2, x15, lsl #2]
        orr             x12, x12, x13, lsl #32
        orr             x14, x14, x15, lsl #32
        stp             x12, x14, [x0]
        ubfx            x12, \c,  #16, #8
        ubfx            x13, \c,  #48, #8
        ubfx            x14, \d,  #16, #8
        ubfx            x15, \d,  #48, #8
        ldr             w12, [x2, x12, lsl #2]
        ldr             w13, [x2, x13, lsl #2]
        ldr             w14, [x2, x14, lsl #2]
        ldr             w15, [x2, x15, lsl #2]
        orr             x12, x12, x13, lsl #32
        orr             x14, x14, x15, lsl #32
        stp             x12, x14, [x0, #16]
        add             x0,  x0,  #32
.endm

.macro  map_load16
        ldp             x4,  x5,  [x1]
        ldp             x6,  x7,  [x1, #16]
        ldp             x8,  x9,  [x1, #32]
        ldp             x10, x11, [x1, #48]
        add             x1,  x1,  #64
        sub             w3,  w3,  #16
.endm

// Sets eq if the sixteen pixels in x4..x11 are all the same.
.macro  map_same16
        ror             x13, x4,  #32
        cmp             x4,  x13
        ccmp            x4,  x5,  #0, eq
        ccmp            x4,  x6,  #0, eq
        ccmp            x4,  x7,  #0, eq
        ccmp            x4,  x8,  #0, eq
        ccmp            x4,  x9,  #0, eq
        ccmp            x4,  x10, #0, eq
        ccmp            x4,  x11, #0, eq
.endm

/* Sixteen pixels of one index are one lookup, splatted. The branch that
 * finds them cannot be predicted, though, and each switch between runs
 * and mixed pixels costs about as much as looking up ten blocks, which
 * photos cut to a palette, full of short runs, would lose on. So a run
 * is only followed from a block whose pixels MAP_AHEAD and one past it
 * match it too, a sign the run goes on for a while. */
#define MAP_AHEAD 158

function wpd_map_color32_neon, export=1
        cmp             w3,  #MAP_AHEAD + 2
        b.lt            3f
1:      map_load16
        ldr             x12, [x1, #(MAP_AHEAD - 16) * 4]
        map_same16
        ccmp            x4,  x12, #0, eq
        b.eq            4f
2:      map8            x4,  x5,  x6,  x7
        map8            x8,  x9,  x10, x11
        cmp             w3,  #MAP_AHEAD + 2
        b.ge            1b
        b               3f
4:      ubfx            x12, x4,  #16, #8
        add             x12, x2,  x12, lsl #2
        ld1r            {v0.4s}, [x12]
        cmp             w3,  #16
        stp             q0,  q0,  [x0]
        stp             q0,  q0,  [x0, #32]
        add             x0,  x0,  #64
        b.lt            6f
        map_load16
        map_same16
        b.eq            4b
        b               2b
3:      cmp             w3,  #16
        b.lt            6f
5:      map_load16
        map8            x4,  x5,  x6,  x7
        map8            x8,  x9,  x10, x11
        cmp             w3,  #16
        b.ge            5b
6:      cmp             w3,  #8
        b.lt            7f
        ldp             x4,  x5,  [x1]
        ldp             x6,  x7,  [x1, #16]
        add             x1,  x1,  #32
        sub             w3,  w3,  #8
        map8            x4,  x5,  x6,  x7
7:      cbz             w3,  9f
8:      ldrb            w4,  [x1, #2]
        add             x1,  x1,  #4
        subs            w3,  w3,  #1
        ldr             w4,  [x2, x4, lsl #2]
        str             w4,  [x0], #4
        b.gt            8b
9:      ret
endfunc

/* An alpha image's palette indices packed two to a byte, the first pixel
 * in the low nibble, looked up among the sixteen alphas at x2: w3 blocks
 * of sixteen pixels, from eight bytes each. */
function wpd_expand_alpha_nibbles_neon, export=1
        ld1             {v31.16b}, [x2]
        movi            v30.16b, #15
        subs            w3,  w3,  #4
        b.lt            2f
1:      ldp             q0,  q1,  [x1], #32
        subs            w3,  w3,  #4
        and             v2.16b,  v0.16b,  v30.16b
        ushr            v3.16b,  v0.16b,  #4
        and             v4.16b,  v1.16b,  v30.16b
        ushr            v5.16b,  v1.16b,  #4
        tbl             v2.16b,  {v31.16b}, v2.16b
        tbl             v3.16b,  {v31.16b}, v3.16b
        tbl             v4.16b,  {v31.16b}, v4.16b
        tbl             v5.16b,  {v31.16b}, v5.16b
        st2             {v2.16b, v3.16b}, [x0], #32
        st2             {v4.16b, v5.16b}, [x0], #32
        b.ge            1b
2:      adds            w3,  w3,  #4
        b.eq            9f
3:      ldr             d0,  [x1], #8
        subs            w3,  w3,  #1
        and             v2.8b,   v0.8b,   v30.8b
        ushr            v3.8b,   v0.8b,   #4
        tbl             v2.8b,   {v31.16b}, v2.8b
        tbl             v3.8b,   {v31.16b}, v3.8b
        st2             {v2.8b,  v3.8b},  [x0], #16
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

/* Eight pixels from x1 over x0, advancing both; v17 holds 255 per byte. */
.macro  blend8
        ld4             {v0.8b-v3.8b}, [x1], #32
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
        st4             {v4.8b-v7.8b}, [x0], #32
.endm

/* Animation frames are mostly binary alpha, so sixteen pixels are first
 * sorted by their alphas alone: an opaque run is stored without reading the
 * canvas, a clear one touches nothing, and a mix of the two is a select.
 * Only a run with a partial alpha goes through the division. */
function wpd_blend_row_argb_neon, export=1
        movrel          x3,  blend_scale
        cmp             w2,  #16
        b.lt            20f
        movi            v31.16b, #255
        movi            v30.4s,  #0xff, lsl #24
1:      ldp             q0,  q1,  [x1]
        ldp             q2,  q3,  [x1, #32]
        uzp1            v16.8h,  v0.8h,   v1.8h
        uzp1            v17.8h,  v2.8h,   v3.8h
        uzp1            v16.16b, v16.16b, v17.16b
        cmeq            v17.16b, v16.16b, v31.16b
        cmeq            v18.16b, v16.16b, #0
        orr             v19.16b, v17.16b, v18.16b
        shrn            v19.8b,  v19.8h,  #4
        shrn            v17.8b,  v17.8h,  #4
        fmov            x4,  d19
        fmov            x5,  d17
        cmn             x4,  #1
        b.ne            5f              // a partial alpha
        cmn             x5,  #1
        b.eq            2f              // all opaque
        cbz             x5,  3f         // all clear
        shl             v20.4s,  v0.4s,  #24
        shl             v21.4s,  v1.4s,  #24
        shl             v22.4s,  v2.4s,  #24
        shl             v23.4s,  v3.4s,  #24
        ldp             q4,  q5,  [x0]
        ldp             q6,  q7,  [x0, #32]
        cmeq            v20.4s,  v20.4s, v30.4s
        cmeq            v21.4s,  v21.4s, v30.4s
        cmeq            v22.4s,  v22.4s, v30.4s
        cmeq            v23.4s,  v23.4s, v30.4s
        bif             v0.16b,  v4.16b,  v20.16b
        bif             v1.16b,  v5.16b,  v21.16b
        bif             v2.16b,  v6.16b,  v22.16b
        bif             v3.16b,  v7.16b,  v23.16b
2:      stp             q0,  q1,  [x0]
        stp             q2,  q3,  [x0, #32]
3:      add             x1,  x1,  #64
        add             x0,  x0,  #64
        sub             w2,  w2,  #16
        cmp             w2,  #16
        b.ge            1b
        b               20f
5:      movi            v17.8b,  #255
        blend8
        blend8
        movi            v31.16b, #255
        movi            v30.4s,  #0xff, lsl #24
        sub             w2,  w2,  #16
        cmp             w2,  #16
        b.ge            1b
20:     cmp             w2,  #8
        b.lt            4f
        movi            v17.8b,  #255
        blend8
        sub             w2,  w2,  #8
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
