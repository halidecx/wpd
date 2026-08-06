/*
 * NEON inverse predictors for the VP8L predictor transform, ported from
 * libwebp's src/dsp/lossless_neon.c.
 *
 * Every operation here is per byte, so the decoder's [A, R, G, B] pixel
 * order needs no special care; only PRED_MODE_BLACK names a channel, and
 * it builds its constant from bytes.
 *
 * Predictors 5 to 7 and 10 to 13 depend on the pixel to their left, so
 * those run one pixel at a time in lane zero. Predictors 0 to 4, 8 and 9
 * have no such dependency and run sixteen pixels at a time.
 *
 * All of these take (const uint32_t *in, const uint32_t *upper,
 * int num_pixels, uint32_t *out), so x0 = in, x1 = upper, w2 = num_pixels
 * and x3 = out. in and out alias, which is why every block loads its
 * residual before storing the reconstruction. out[-1] holds the pixel to
 * the left, and upper[-1] through upper[num_pixels] may be read.
 */

#include "asm.S"

/*
 * The predictors with a left-pixel dependency keep that recurrence in lane
 * zero and step their inputs down by one pixel instead, so the rotates stay
 * off the loop-carried chain: only the arithmetic that actually produces the
 * next left pixel is on it.
 */
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

/* PRED_MODE_BLACK */
function ff_pred_add_0_neon, export=1
        movi            v0.4s,   #255           // FF 00 00 00 per pixel
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

/*
 * PRED_MODE_L, as a running per-byte prefix sum over four pixels. The
 * left pixel stays in a register across iterations rather than being
 * reloaded from out[-1].
 */
function ff_pred_add_1_neon, export=1
        ldur            s0,  [x3, #-4]
        dup             v0.4s,   v0.s[0]        // the prefix sum adds it to every lane
        movi            v3.16b,  #0
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16          // a | b | c | d
        sub             w2,  w2,  #4
        ext             v2.16b,  v3.16b,  v1.16b, #12   // 0 | a | b | c
        add             v1.16b,  v1.16b,  v2.16b        // a | a+b | b+c | c+d
        ext             v2.16b,  v3.16b,  v1.16b, #8    // 0 | 0 | a | a+b
        add             v1.16b,  v1.16b,  v2.16b        // running prefix
        add             v1.16b,  v1.16b,  v0.16b        // + left
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

/*
 * Predictors that just add a neighbouring pixel from the row above. The
 * stubs bias upper, so the shared body only ever reads upper[i].
 */
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

/* PRED_MODE_T */
function ff_pred_add_2_neon, export=1
        b               pred_add_top_neon
endfunc

/* PRED_MODE_TR */
function ff_pred_add_3_neon, export=1
        add             x1,  x1,  #4
        b               pred_add_top_neon
endfunc

/* PRED_MODE_TL */
function ff_pred_add_4_neon, export=1
        sub             x1,  x1,  #4
        b               pred_add_top_neon
endfunc

/*
 * Predictors that average two of the pixels above. The stubs point x5 at
 * the second one.
 */
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

/* PRED_MODE_AVG_TL_T */
function ff_pred_add_8_neon, export=1
        sub             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

/* PRED_MODE_AVG_T_TR */
function ff_pred_add_9_neon, export=1
        add             x5,  x1,  #4
        b               pred_add_avgtop_neon
endfunc

/* PRED_MODE_AVG_T_AVG_L_TR: average(average(L, TR), T) */
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
1:      ldr             q1,  [x0], #16          // src
        ldur            q4,  [x1, #4]           // TR
        ldr             q2,  [x1], #16          // T
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

/*
 * PRED_MODE_AVG_L_TL and PRED_MODE_AVG_L_T: average(L, one pixel above).
 * The stubs bias upper so the shared body reads a single top pixel.
 */
.macro  pred67_pixel
        uhadd           v16.8b,  v0.8b,   v2.8b
        add             v0.8b,   v16.8b,  v1.8b
        str             s0,  [x3], #4
.endm

function pred_add_avgleft_neon
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16          // src
        ldr             q2,  [x1], #16          // top
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

/* PRED_MODE_AVG_L_TL */
function ff_pred_add_6_neon, export=1
        sub             x1,  x1,  #4
        b               pred_add_avgleft_neon
endfunc

/* PRED_MODE_AVG_L_T */
function ff_pred_add_7_neon, export=1
        b               pred_add_avgleft_neon
endfunc

/* PRED_MODE_AVG_AVG_L_TL_AVG_T_TR */
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
1:      ldr             q1,  [x0], #16          // src
        ldur            q3,  [x1, #-4]          // TL
        ldur            q4,  [x1, #4]           // TR
        ldr             q2,  [x1], #16          // T
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

/*
 * PRED_MODE_SELECT: pick T when sum|L - TL| <= sum|T - TL|, else L. The
 * pairwise adds reduce each pixel's four byte differences into its lane.
 * BIT rather than BSL selects the winner, so the result lands straight in
 * v0 and no register move joins the chain.
 */
.macro  pred11_pixel
        uabd            v17.8b,  v0.8b,   v3.8b
        uaddlp          v17.4h,  v17.8b
        uaddlp          v17.2s,  v17.4h         // pa
        cmhs            v17.2s,  v4.2s,   v17.2s
        add             v0.8b,   v0.8b,   v1.8b // sum of L and the residual
        bit             v0.8b,   v5.8b,   v17.8b
        str             s0,  [x3], #4
.endm

function ff_pred_add_11_neon, export=1
        ldur            s0,  [x3, #-4]
        cmp             w2,  #4
        b.lt            2f
1:      ldr             q1,  [x0], #16          // src
        ldur            q3,  [x1, #-4]          // TL
        ldr             q2,  [x1], #16          // T
        sub             w2,  w2,  #4
        uabd            v4.16b,  v2.16b,  v3.16b
        uaddlp          v4.8h,   v4.16b
        uaddlp          v4.4s,   v4.8h          // pb
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
        uaddlp          v4.2s,   v4.4h          // pb
        add             v5.8b,   v2.8b,   v1.8b
        pred11_pixel
        b.gt            3b
9:      ret
endfunc

/*
 * PRED_MODE_ADD_SUBTRACT_FULL: clamp(L + T - TL). The left pixel is held
 * widened to 16-bit lanes, since it has to meet the signed T - TL
 * difference before the result is clamped back to bytes. One quad's
 * differences span two registers, four 16-bit lanes to a pixel.
 */
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
1:      ldr             q1,  [x0], #16          // src
        ldur            q3,  [x1, #-4]          // TL
        ldr             q2,  [x1], #16          // T
        sub             w2,  w2,  #4
        usubl           v4.8h,   v2.8b,   v3.8b         // T - TL, pixels 0, 1
        usubl2          v5.8h,   v2.16b,  v3.16b        // pixels 2, 3
.irp    diff, v4, v5
.irp    lane, 0, 1
        pred12_pixel    \diff
        ext             v1.16b,  v1.16b,  v1.16b, #4
        /* one pixel of the difference is four 16-bit lanes */
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

/*
 * PRED_MODE_ADD_SUBTRACT_HALF: clamp(avg + (avg - TL) / 2), avg = (L + T) / 2.
 * The halving subtract rounds down while the C reference truncates toward
 * zero, so TL is biased down by one wherever it exceeds avg.
 */
.macro  pred13_pixel
        uhadd           v16.8b,  v0.8b,   v2.8b // avg
        cmhi            v17.8b,  v3.8b,   v16.8b // TL > avg
        add             v17.8b,  v3.8b,   v17.8b // TL biased down
        uhsub           v18.8b,  v16.8b,  v17.8b // (avg - TL) / 2, signed
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
1:      ldr             q1,  [x0], #16          // src
        ldur            q3,  [x1, #-4]          // TL
        ldr             q2,  [x1], #16          // T
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

/*
 * Gather the green byte of each ARGB pixel into a packed plane, for the
 * alpha values a VP8L alpha chunk carries in green. The tail stays scalar
 * so nothing is written past the end of the destination row.
 */
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
