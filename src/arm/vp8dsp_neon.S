
#include "asm.S"
#include "neon.S"

function ff_vp8_luma_dc_wht_neon, export=1
        vld1.16         {q0-q1},  [r1,:128]
        vmov.i16        q15, #0

        vadd.i16        d4,  d0,  d3
        vadd.i16        d6,  d1,  d2
        vst1.16         {q15},    [r1,:128]!
        vsub.i16        d7,  d1,  d2
        vsub.i16        d5,  d0,  d3
        vst1.16         {q15},    [r1,:128]
        vadd.i16        q0,  q2,  q3
        vsub.i16        q1,  q2,  q3

        vmov.i16        q8, #3

        vtrn.32         d0,  d2
        vtrn.32         d1,  d3
        vtrn.16         d0,  d1
        vtrn.16         d2,  d3

        vadd.i16        d0,  d0,  d16

        vadd.i16        d4,  d0,  d3
        vadd.i16        d6,  d1,  d2
        vsub.i16        d7,  d1,  d2
        vsub.i16        d5,  d0,  d3
        vadd.i16        q0,  q2,  q3
        vsub.i16        q1,  q2,  q3

        vshr.s16        q0,  q0,  #3
        vshr.s16        q1,  q1,  #3

        mov             r3,  #32
        vst1.16         {d0[0]},  [r0,:16], r3
        vst1.16         {d1[0]},  [r0,:16], r3
        vst1.16         {d2[0]},  [r0,:16], r3
        vst1.16         {d3[0]},  [r0,:16], r3
        vst1.16         {d0[1]},  [r0,:16], r3
        vst1.16         {d1[1]},  [r0,:16], r3
        vst1.16         {d2[1]},  [r0,:16], r3
        vst1.16         {d3[1]},  [r0,:16], r3
        vst1.16         {d0[2]},  [r0,:16], r3
        vst1.16         {d1[2]},  [r0,:16], r3
        vst1.16         {d2[2]},  [r0,:16], r3
        vst1.16         {d3[2]},  [r0,:16], r3
        vst1.16         {d0[3]},  [r0,:16], r3
        vst1.16         {d1[3]},  [r0,:16], r3
        vst1.16         {d2[3]},  [r0,:16], r3
        vst1.16         {d3[3]},  [r0,:16], r3

        bx              lr
endfunc

function ff_vp8_idct_add_neon, export=1
        vld1.16         {q0-q1},  [r1,:128]
        movw            r3,  #20091
        movt            r3,  #35468/2
        vdup.32         d4,  r3

        vmull.s16       q12, d1,  d4[0]
        vmull.s16       q13, d3,  d4[0]
        vqdmulh.s16     d20, d1,  d4[1]
        vqdmulh.s16     d23, d3,  d4[1]
        vshrn.s32       d21, q12, #16
        vshrn.s32       d22, q13, #16
        vadd.s16        d21, d21, d1
        vadd.s16        d22, d22, d3

        vadd.s16        d16, d0,  d2
        vsub.s16        d17, d0,  d2
        vadd.s16        d18, d21, d23
        vsub.s16        d19, d20, d22
        vadd.s16        q0,  q8,  q9
        vsub.s16        q1,  q8,  q9

        vtrn.32         d0,  d3
        vtrn.32         d1,  d2
        vtrn.16         d0,  d1
        vtrn.16         d3,  d2

        vmov.i16        q15, #0
        vmull.s16       q12, d1,  d4[0]
        vst1.16         {q15},    [r1,:128]!
        vmull.s16       q13, d2,  d4[0]
        vst1.16         {q15},    [r1,:128]
        vqdmulh.s16     d21, d1,  d4[1]
        vqdmulh.s16     d23, d2,  d4[1]
        vshrn.s32       d20, q12, #16
        vshrn.s32       d22, q13, #16
        vadd.i16        d20, d20, d1
        vadd.i16        d22, d22, d2

        vadd.i16        d16, d0,  d3
        vsub.i16        d17, d0,  d3
        vadd.i16        d18, d20, d23
        vld1.32         {d20[]},  [r0,:32], r2
        vsub.i16        d19, d21, d22
        vld1.32         {d22[]},  [r0,:32], r2
        vadd.s16        q0,  q8,  q9
        vld1.32         {d23[]},  [r0,:32], r2
        vsub.s16        q1,  q8,  q9
        vld1.32         {d21[]},  [r0,:32], r2
        vrshr.s16       q0,  q0,  #3
        vtrn.32         q10, q11
        vrshr.s16       q1,  q1,  #3

        sub             r0,  r0,  r2,  lsl #2

        vtrn.32         d0,  d3
        vtrn.32         d1,  d2
        vtrn.16         d0,  d1
        vtrn.16         d3,  d2

        vaddw.u8        q0,  q0,  d20
        vaddw.u8        q1,  q1,  d21
        vqmovun.s16     d0,  q0
        vqmovun.s16     d1,  q1

        vst1.32         {d0[0]},  [r0,:32], r2
        vst1.32         {d0[1]},  [r0,:32], r2
        vst1.32         {d1[1]},  [r0,:32], r2
        vst1.32         {d1[0]},  [r0,:32], r2

        bx              lr
endfunc

function ff_vp8_idct_dc_add_neon, export=1
        mov             r3,  #0
        ldrsh           r12, [r1]
        strh            r3,  [r1]
        vdup.16         q1,  r12
        vrshr.s16       q1,  q1,  #3
        vld1.32         {d0[]},   [r0,:32], r2
        vld1.32         {d1[]},   [r0,:32], r2
        vld1.32         {d0[1]},  [r0,:32], r2
        vld1.32         {d1[1]},  [r0,:32], r2
        vaddw.u8        q2,  q1,  d0
        vaddw.u8        q3,  q1,  d1
        sub             r0,  r0,  r2, lsl #2
        vqmovun.s16     d0,  q2
        vqmovun.s16     d1,  q3
        vst1.32         {d0[0]},  [r0,:32], r2
        vst1.32         {d1[0]},  [r0,:32], r2
        vst1.32         {d0[1]},  [r0,:32], r2
        vst1.32         {d1[1]},  [r0,:32], r2
        bx              lr
endfunc

function ff_vp8_idct_dc_add4uv_neon, export=1
        vmov.i16        d0,  #0
        mov             r3,  #32
        vld1.16         {d16[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d17[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d18[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d19[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        mov             r3,  r0
        vrshr.s16       q8,  q8,  #3
        vld1.8          {d0},     [r0,:64], r2
        vrshr.s16       q9,  q9,  #3
        vld1.8          {d1},     [r0,:64], r2
        vaddw.u8        q10, q8,  d0
        vld1.8          {d2},     [r0,:64], r2
        vaddw.u8        q0,  q8,  d1
        vld1.8          {d3},     [r0,:64], r2
        vaddw.u8        q11, q8,  d2
        vld1.8          {d4},     [r0,:64], r2
        vaddw.u8        q1,  q8,  d3
        vld1.8          {d5},     [r0,:64], r2
        vaddw.u8        q12, q9,  d4
        vld1.8          {d6},     [r0,:64], r2
        vaddw.u8        q2,  q9,  d5
        vld1.8          {d7},     [r0,:64], r2
        vaddw.u8        q13, q9,  d6
        vqmovun.s16     d20, q10
        vaddw.u8        q3,  q9,  d7
        vqmovun.s16     d21, q0
        vqmovun.s16     d22, q11
        vst1.8          {d20},    [r3,:64], r2
        vqmovun.s16     d23, q1
        vst1.8          {d21},    [r3,:64], r2
        vqmovun.s16     d24, q12
        vst1.8          {d22},    [r3,:64], r2
        vqmovun.s16     d25, q2
        vst1.8          {d23},    [r3,:64], r2
        vqmovun.s16     d26, q13
        vst1.8          {d24},    [r3,:64], r2
        vqmovun.s16     d27, q3
        vst1.8          {d25},    [r3,:64], r2
        vst1.8          {d26},    [r3,:64], r2
        vst1.8          {d27},    [r3,:64], r2

        bx              lr
endfunc

function ff_vp8_idct_dc_add4y_neon, export=1
        vmov.i16        d0,  #0
        mov             r3,  #32
        vld1.16         {d16[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d17[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d18[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vld1.16         {d19[]},  [r1,:16]
        vst1.16         {d0[0]},  [r1,:16], r3
        vrshr.s16       q8,  q8,  #3
        vld1.8          {q0},     [r0,:128], r2
        vrshr.s16       q9,  q9,  #3
        vld1.8          {q1},     [r0,:128], r2
        vaddw.u8        q10, q8,  d0
        vld1.8          {q2},     [r0,:128], r2
        vaddw.u8        q0,  q9,  d1
        vld1.8          {q3},     [r0,:128], r2
        vaddw.u8        q11, q8,  d2
        vaddw.u8        q1,  q9,  d3
        vaddw.u8        q12, q8,  d4
        vaddw.u8        q2,  q9,  d5
        vaddw.u8        q13, q8,  d6
        vaddw.u8        q3,  q9,  d7
        sub             r0,  r0,  r2,  lsl #2
        vqmovun.s16     d20, q10
        vqmovun.s16     d21, q0
        vqmovun.s16     d22, q11
        vqmovun.s16     d23, q1
        vqmovun.s16     d24, q12
        vst1.8          {q10},    [r0,:128], r2
        vqmovun.s16     d25, q2
        vst1.8          {q11},    [r0,:128], r2
        vqmovun.s16     d26, q13
        vst1.8          {q12},    [r0,:128], r2
        vqmovun.s16     d27, q3
        vst1.8          {q13},    [r0,:128], r2

        bx              lr
endfunc

.macro  vp8_loop_filter, inner=0, simple=0
    .if \simple
        vabd.u8         q9,  q3,  q4
        vabd.u8         q15, q2,  q5
        vqadd.u8        q9,  q9,  q9
        vshr.u8         q10, q15, #1
        vqadd.u8        q11, q9,  q10
        vmov.i8         q13, #0x80
        vcle.u8         q8,  q11, q14
    .else
        vabd.u8         q12, q2,  q3
        vabd.u8         q13, q5,  q4
        vabd.u8         q10, q0,  q1
        vabd.u8         q11, q1,  q2
        vcle.u8         q8,  q12, q15
        vcle.u8         q9,  q13, q15
        vcle.u8         q10, q10, q15
        vcle.u8         q11, q11, q15
        vand            q8,  q8,  q9
        vabd.u8         q9,  q7,  q6
        vand            q8,  q8,  q11
        vabd.u8         q11, q6,  q5
        vand            q8,  q8,  q10
        vcle.u8         q10, q9,  q15
        vcle.u8         q11, q11, q15
        vabd.u8         q9,  q3,  q4
        vabd.u8         q15, q2,  q5
        vand            q8,  q8,  q10
        vqadd.u8        q9,  q9,  q9
        vand            q8,  q8,  q11
        vshr.u8         q10, q15, #1
        vdup.8          q15, r12
        vqadd.u8        q11, q9,  q10
        vcgt.u8         q12, q12, q15
        vcle.u8         q11, q11, q14
        vcgt.u8         q14, q13, q15
        vand            q8,  q8,  q11
        vmov.i8         q13, #0x80
        vorr            q9,  q12, q14
    .endif


        veor            q3,  q3,  q13
        veor            q4,  q4,  q13

        vmov.i16        q12, #3
        vsubl.s8        q10, d8,  d6
        vsubl.s8        q11, d9,  d7
        veor            q2,  q2,  q13
        veor            q5,  q5,  q13
        vmul.i16        q10, q10, q12
        vmul.i16        q11, q11, q12

        vqsub.s8        q12, q2,  q5
        vmov.i8         q14, #4
        vmov.i8         q15, #3
    .if \inner
        vand            q12, q12, q9
    .endif
        vaddw.s8        q10, q10, d24
        vaddw.s8        q11, q11, d25
        vqmovn.s16      d20, q10
        vqmovn.s16      d21, q11
    .if !\inner && !\simple
        veor            q1,  q1,  q13
        veor            q6,  q6,  q13
    .endif
        vand            q10, q10, q8



    .if \simple
        vqadd.s8        q11, q10, q14
        vqadd.s8        q12, q10, q15
        vshr.s8         q11, q11, #3
        vshr.s8         q12, q12, #3
        vqsub.s8        q4,  q4,  q11
        vqadd.s8        q3,  q3,  q12
        veor            q4,  q4,  q13
        veor            q3,  q3,  q13
        veor            q5,  q5,  q13
        veor            q2,  q2,  q13
    .elseif \inner
        vqadd.s8        q11, q10, q14
        vqadd.s8        q12, q10, q15
        vshr.s8         q11, q11, #3
        vshr.s8         q12, q12, #3
        vqsub.s8        q4,  q4,  q11
        vqadd.s8        q3,  q3,  q12
        vbic            q11, q11, q9
        veor            q4,  q4,  q13
        vrshr.s8        q11, q11, #1
        veor            q3,  q3,  q13
        vqsub.s8        q5,  q5,  q11
        vqadd.s8        q2,  q2,  q11
        veor            q5,  q5,  q13
        veor            q2,  q2,  q13
    .else
        vand            q12, q10, q9
        vqadd.s8        q11, q12, q14
        vqadd.s8        q12, q12, q15
        vshr.s8         q11, q11, #3
        vshr.s8         q12, q12, #3
        vbic            q10, q10, q9
        vqsub.s8        q4,  q4,  q11
        vqadd.s8        q3,  q3,  q12

        vmov.i16        q9,  #63
        vshll.s8        q14, d20, #3
        vshll.s8        q15, d21, #3
        vaddw.s8        q14, q14, d20
        vaddw.s8        q15, q15, d21
        vadd.s16        q8,  q9,  q14
        vadd.s16        q9,  q9,  q15
        vadd.s16        q11, q8,  q14
        vadd.s16        q12, q9,  q15
        vadd.s16        q14, q11, q14
        vadd.s16        q15, q12, q15
        vqshrn.s16      d16, q8,  #7
        vqshrn.s16      d17, q9,  #7
        vqshrn.s16      d22, q11, #7
        vqshrn.s16      d23, q12, #7
        vqshrn.s16      d28, q14, #7
        vqshrn.s16      d29, q15, #7
        vqadd.s8        q1,  q1,  q8
        vqsub.s8        q6,  q6,  q8
        vqadd.s8        q2,  q2,  q11
        vqsub.s8        q5,  q5,  q11
        vqadd.s8        q3,  q3,  q14
        vqsub.s8        q4,  q4,  q14
        veor            q3,  q3,  q13
        veor            q4,  q4,  q13
        veor            q2,  q2,  q13
        veor            q5,  q5,  q13
        veor            q1,  q1,  q13
        veor            q6,  q6,  q13
    .endif
.endm

.macro  vp8_v_loop_filter16 name, inner=0, simple=0
function ff_vp8_v_loop_filter16\name\()_neon, export=1
        vpush           {q4-q7}
        sub             r0,  r0,  r1,  lsl #1+!\simple

    .if !\simple
        ldr             r12, [sp, #64]
        vld1.8          {q0},     [r0,:128], r1
        vld1.8          {q1},     [r0,:128], r1
    .endif
        vld1.8          {q2},     [r0,:128], r1
        vld1.8          {q3},     [r0,:128], r1
        vld1.8          {q4},     [r0,:128], r1
        vld1.8          {q5},     [r0,:128], r1
    .if !\simple
        vld1.8          {q6},     [r0,:128], r1
        vld1.8          {q7},     [r0,:128]
        vdup.8          q15, r3
    .endif
        vdup.8          q14, r2

        vp8_loop_filter inner=\inner, simple=\simple

        sub             r0,  r0,  r1,  lsl #2
    .if !\simple
        sub             r0,  r0,  r1,  lsl #1

        vst1.8          {q1},     [r0,:128], r1
    .endif
        vst1.8          {q2},     [r0,:128], r1
        vst1.8          {q3},     [r0,:128], r1
        vst1.8          {q4},     [r0,:128], r1
        vst1.8          {q5},     [r0,:128], r1
    .if !\simple
        vst1.8          {q6},     [r0,:128]
    .endif

        vpop            {q4-q7}
        bx              lr
endfunc
.endm

vp8_v_loop_filter16
vp8_v_loop_filter16 _inner,  inner=1
vp8_v_loop_filter16 _simple, simple=1

.macro  vp8_v_loop_filter8uv name, inner=0
function ff_vp8_v_loop_filter8uv\name\()_neon, export=1
        vpush           {q4-q7}
        sub             r0,  r0,  r2,  lsl #2
        sub             r1,  r1,  r2,  lsl #2
        ldr             r12, [sp, #64]

        vld1.8          {d0},     [r0,:64], r2
        vld1.8          {d1},     [r1,:64], r2
        vld1.8          {d2},     [r0,:64], r2
        vld1.8          {d3},     [r1,:64], r2
        vld1.8          {d4},     [r0,:64], r2
        vld1.8          {d5},     [r1,:64], r2
        vld1.8          {d6},     [r0,:64], r2
        vld1.8          {d7},     [r1,:64], r2
        vld1.8          {d8},     [r0,:64], r2
        vld1.8          {d9},     [r1,:64], r2
        vld1.8          {d10},    [r0,:64], r2
        vld1.8          {d11},    [r1,:64], r2
        vld1.8          {d12},    [r0,:64], r2
        vld1.8          {d13},    [r1,:64], r2
        vld1.8          {d14},    [r0,:64]
        vld1.8          {d15},    [r1,:64]

        vdup.8          q14, r3
        vdup.8          q15, r12
        ldr             r12, [sp, #68]

        vp8_loop_filter inner=\inner

        sub             r0,  r0,  r2,  lsl #2
        sub             r1,  r1,  r2,  lsl #2
        sub             r0,  r0,  r2,  lsl #1
        sub             r1,  r1,  r2,  lsl #1

        vst1.8          {d2},     [r0,:64], r2
        vst1.8          {d3},     [r1,:64], r2
        vst1.8          {d4},     [r0,:64], r2
        vst1.8          {d5},     [r1,:64], r2
        vst1.8          {d6},     [r0,:64], r2
        vst1.8          {d7},     [r1,:64], r2
        vst1.8          {d8},     [r0,:64], r2
        vst1.8          {d9},     [r1,:64], r2
        vst1.8          {d10},    [r0,:64], r2
        vst1.8          {d11},    [r1,:64], r2
        vst1.8          {d12},    [r0,:64]
        vst1.8          {d13},    [r1,:64]

        vpop            {q4-q7}
        bx              lr
endfunc
.endm

vp8_v_loop_filter8uv
vp8_v_loop_filter8uv _inner, inner=1

.macro  vp8_h_loop_filter16 name, inner=0, simple=0
function ff_vp8_h_loop_filter16\name\()_neon, export=1
        vpush           {q4-q7}
        sub             r0,  r0,  #4
    .if !\simple
        ldr             r12, [sp, #64]
    .endif

        vld1.8          {d0},     [r0], r1
        vld1.8          {d2},     [r0], r1
        vld1.8          {d4},     [r0], r1
        vld1.8          {d6},     [r0], r1
        vld1.8          {d8},     [r0], r1
        vld1.8          {d10},    [r0], r1
        vld1.8          {d12},    [r0], r1
        vld1.8          {d14},    [r0], r1
        vld1.8          {d1},     [r0], r1
        vld1.8          {d3},     [r0], r1
        vld1.8          {d5},     [r0], r1
        vld1.8          {d7},     [r0], r1
        vld1.8          {d9},     [r0], r1
        vld1.8          {d11},    [r0], r1
        vld1.8          {d13},    [r0], r1
        vld1.8          {d15},    [r0], r1

        transpose_8x8   q0,  q1,  q2,  q3,  q4,  q5,  q6,  q7

        vdup.8          q14, r2
    .if !\simple
        vdup.8          q15, r3
    .endif

        vp8_loop_filter inner=\inner, simple=\simple

        sub             r0,  r0,  r1, lsl #4

        transpose_8x8   q0,  q1,  q2,  q3,  q4,  q5,  q6,  q7

        vst1.8          {d0},     [r0],     r1
        vst1.8          {d2},     [r0],     r1
        vst1.8          {d4},     [r0],     r1
        vst1.8          {d6},     [r0],     r1
        vst1.8          {d8},     [r0],     r1
        vst1.8          {d10},    [r0],     r1
        vst1.8          {d12},    [r0],     r1
        vst1.8          {d14},    [r0],     r1
        vst1.8          {d1},     [r0],     r1
        vst1.8          {d3},     [r0],     r1
        vst1.8          {d5},     [r0],     r1
        vst1.8          {d7},     [r0],     r1
        vst1.8          {d9},     [r0],     r1
        vst1.8          {d11},    [r0],     r1
        vst1.8          {d13},    [r0],     r1
        vst1.8          {d15},    [r0]

        vpop            {q4-q7}
        bx              lr
endfunc
.endm

vp8_h_loop_filter16
vp8_h_loop_filter16 _inner,  inner=1
vp8_h_loop_filter16 _simple, simple=1

.macro  vp8_h_loop_filter8uv name, inner=0
function ff_vp8_h_loop_filter8uv\name\()_neon, export=1
        vpush           {q4-q7}
        sub             r0,  r0,  #4
        sub             r1,  r1,  #4
        ldr             r12, [sp, #64]

        vld1.8          {d0},     [r0], r2
        vld1.8          {d1},     [r1], r2
        vld1.8          {d2},     [r0], r2
        vld1.8          {d3},     [r1], r2
        vld1.8          {d4},     [r0], r2
        vld1.8          {d5},     [r1], r2
        vld1.8          {d6},     [r0], r2
        vld1.8          {d7},     [r1], r2
        vld1.8          {d8},     [r0], r2
        vld1.8          {d9},     [r1], r2
        vld1.8          {d10},    [r0], r2
        vld1.8          {d11},    [r1], r2
        vld1.8          {d12},    [r0], r2
        vld1.8          {d13},    [r1], r2
        vld1.8          {d14},    [r0], r2
        vld1.8          {d15},    [r1], r2

        transpose_8x8   q0,  q1,  q2,  q3,  q4,  q5,  q6,  q7

        vdup.8          q14, r3
        vdup.8          q15, r12
        ldr             r12, [sp, #68]

        vp8_loop_filter inner=\inner

        sub             r0,  r0,  r2, lsl #3
        sub             r1,  r1,  r2, lsl #3

        transpose_8x8   q0,  q1,  q2,  q3,  q4,  q5,  q6,  q7

        vst1.8          {d0},     [r0], r2
        vst1.8          {d1},     [r1], r2
        vst1.8          {d2},     [r0], r2
        vst1.8          {d3},     [r1], r2
        vst1.8          {d4},     [r0], r2
        vst1.8          {d5},     [r1], r2
        vst1.8          {d6},     [r0], r2
        vst1.8          {d7},     [r1], r2
        vst1.8          {d8},     [r0], r2
        vst1.8          {d9},     [r1], r2
        vst1.8          {d10},    [r0], r2
        vst1.8          {d11},    [r1], r2
        vst1.8          {d12},    [r0], r2
        vst1.8          {d13},    [r1], r2
        vst1.8          {d14},    [r0]
        vst1.8          {d15},    [r1]

        vpop            {q4-q7}
        bx              lr
endfunc
.endm

vp8_h_loop_filter8uv
vp8_h_loop_filter8uv _inner, inner=1
