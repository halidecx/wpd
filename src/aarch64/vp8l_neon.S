
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
