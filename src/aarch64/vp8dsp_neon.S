
#include "asm.S"
#include "neon.S"

function ff_vp8_luma_dc_wht_neon, export=1
        ld1             {v0.4h - v3.4h}, [x1]
        movi            v30.8h, #0

        add             v4.4h,  v0.4h,  v3.4h
        add             v6.4h,  v1.4h,  v2.4h
        st1             {v30.8h}, [x1], #16
        sub             v7.4h,  v1.4h,  v2.4h
        sub             v5.4h,  v0.4h,  v3.4h
        st1             {v30.8h}, [x1]
        add             v0.4h,  v4.4h,  v6.4h
        add             v1.4h,  v5.4h,  v7.4h
        sub             v2.4h,  v4.4h,  v6.4h
        sub             v3.4h,  v5.4h,  v7.4h

        movi            v16.4h, #3

        transpose_4x4H  v0, v1, v2, v3, v4, v5, v6, v7

        add             v0.4h,  v0.4h,  v16.4h

        add             v4.4h,  v0.4h,  v3.4h
        add             v6.4h,  v1.4h,  v2.4h
        sub             v7.4h,  v1.4h,  v2.4h
        sub             v5.4h,  v0.4h,  v3.4h
        add             v0.4h,  v4.4h,  v6.4h
        add             v1.4h,  v5.4h,  v7.4h
        sub             v2.4h,  v4.4h,  v6.4h
        sub             v3.4h,  v5.4h,  v7.4h

        sshr            v0.4h,  v0.4h,  #3
        sshr            v1.4h,  v1.4h,  #3
        sshr            v2.4h,  v2.4h,  #3
        sshr            v3.4h,  v3.4h,  #3

        mov             x3,  #32
        st1             {v0.h}[0],  [x0], x3
        st1             {v1.h}[0],  [x0], x3
        st1             {v2.h}[0],  [x0], x3
        st1             {v3.h}[0],  [x0], x3
        st1             {v0.h}[1],  [x0], x3
        st1             {v1.h}[1],  [x0], x3
        st1             {v2.h}[1],  [x0], x3
        st1             {v3.h}[1],  [x0], x3
        st1             {v0.h}[2],  [x0], x3
        st1             {v1.h}[2],  [x0], x3
        st1             {v2.h}[2],  [x0], x3
        st1             {v3.h}[2],  [x0], x3
        st1             {v0.h}[3],  [x0], x3
        st1             {v1.h}[3],  [x0], x3
        st1             {v2.h}[3],  [x0], x3
        st1             {v3.h}[3],  [x0], x3

        ret
endfunc

function ff_vp8_idct_add_neon, export=1
        ld1             {v0.8b - v3.8b},  [x1]
        mov             w4,  #20091
        movk            w4,  #35468/2, lsl #16
        dup             v4.2s, w4

        smull           v26.4s, v1.4h,  v4.h[0]
        smull           v27.4s, v3.4h,  v4.h[0]
        sqdmulh         v20.4h, v1.4h,  v4.h[1]
        sqdmulh         v23.4h, v3.4h,  v4.h[1]
        shrn            v21.4h, v26.4s, #16
        shrn            v22.4h, v27.4s, #16
        add             v21.4h, v21.4h, v1.4h
        add             v22.4h, v22.4h, v3.4h

        add             v16.4h,  v0.4h,   v2.4h
        sub             v17.4h,  v0.4h,   v2.4h

        add             v18.4h,  v21.4h,  v23.4h
        sub             v19.4h,  v20.4h,  v22.4h

        add             v0.4h,   v16.4h,  v18.4h
        add             v1.4h,   v17.4h,  v19.4h
        sub             v3.4h,   v16.4h,  v18.4h
        sub             v2.4h,   v17.4h,  v19.4h

        transpose_4x4H  v0, v1, v2, v3, v24, v5, v6, v7

        movi            v29.8h, #0
        smull           v26.4s,     v1.4h,  v4.h[0]
        st1             {v29.8h},   [x1],   #16
        smull           v27.4s,     v3.4h,  v4.h[0]
        st1             {v29.16b},  [x1]
        sqdmulh         v21.4h,     v1.4h,  v4.h[1]
        sqdmulh         v23.4h,     v3.4h,  v4.h[1]
        shrn            v20.4h,     v26.4s, #16
        shrn            v22.4h,     v27.4s, #16
        add             v20.4h,     v20.4h, v1.4h
        add             v22.4h,     v22.4h, v3.4h
        add             v16.4h,     v0.4h,  v2.4h
        sub             v17.4h,     v0.4h,  v2.4h

        add             v18.4h,     v20.4h, v23.4h
        ld1             {v24.s}[0], [x0],   x2
        sub             v19.4h, v21.4h, v22.4h
        ld1             {v25.s}[0], [x0],   x2
        add             v0.4h,      v16.4h, v18.4h
        add             v1.4h,      v17.4h, v19.4h
        ld1             {v26.s}[0], [x0],   x2
        sub             v3.4h,      v16.4h, v18.4h
        sub             v2.4h,      v17.4h, v19.4h
        ld1             {v27.s}[0], [x0],   x2
        srshr           v0.4h,      v0.4h,  #3
        srshr           v1.4h,      v1.4h,  #3
        srshr           v2.4h,      v2.4h,  #3
        srshr           v3.4h,      v3.4h,  #3

        sub             x0,  x0,  x2,  lsl #2

        transpose_4x4H  v0, v1, v2, v3, v5, v6, v7, v16

        uaddw           v0.8h,  v0.8h, v24.8b
        uaddw           v1.8h,  v1.8h, v25.8b
        uaddw           v2.8h,  v2.8h, v26.8b
        uaddw           v3.8h,  v3.8h, v27.8b
        sqxtun          v0.8b,  v0.8h
        sqxtun          v1.8b,  v1.8h
        sqxtun          v2.8b,  v2.8h
        sqxtun          v3.8b,  v3.8h

        st1             {v0.s}[0],  [x0], x2
        st1             {v1.s}[0],  [x0], x2
        st1             {v2.s}[0],  [x0], x2
        st1             {v3.s}[0],  [x0], x2

        ret
endfunc

function ff_vp8_idct_dc_add4uv_neon, export=1
        movi            v0.4h,  #0
        mov             x3,     #32
        ld1r            {v16.4h},  [x1]
        st1             {v0.h}[0], [x1], x3
        ld1r            {v17.4h},  [x1]
        st1             {v0.h}[0], [x1], x3
        ld1r            {v18.4h},  [x1]
        st1             {v0.h}[0], [x1], x3
        ld1r            {v19.4h},  [x1]
        st1             {v0.h}[0], [x1], x3
        ins             v16.d[1],  v17.d[0]
        ins             v18.d[1],  v19.d[0]
        mov             x3,  x0
        srshr           v16.8h,    v16.8h,  #3
        ld1             {v0.8b},   [x0], x2
        srshr           v18.8h,    v18.8h,  #3
        ld1             {v1.8b},   [x0], x2
        uaddw           v20.8h,    v16.8h, v0.8b
        ld1             {v2.8b},   [x0], x2
        uaddw           v0.8h,     v16.8h, v1.8b
        ld1             {v3.8b},   [x0], x2
        uaddw           v22.8h,    v16.8h, v2.8b
        ld1             {v4.8b},   [x0], x2
        uaddw           v2.8h,     v16.8h, v3.8b
        ld1             {v5.8b},   [x0], x2
        uaddw           v24.8h,    v18.8h, v4.8b
        ld1             {v6.8b},   [x0], x2
        uaddw           v4.8h,     v18.8h, v5.8b
        ld1             {v7.8b},   [x0], x2
        uaddw           v26.8h,    v18.8h, v6.8b
        sqxtun          v20.8b,    v20.8h
        uaddw           v6.8h,     v18.8h, v7.8b
        sqxtun          v21.8b,    v0.8h
        sqxtun          v22.8b,    v22.8h
        st1             {v20.8b},  [x3], x2
        sqxtun          v23.8b,    v2.8h
        st1             {v21.8b},  [x3], x2
        sqxtun          v24.8b,    v24.8h
        st1             {v22.8b},  [x3], x2
        sqxtun          v25.8b,    v4.8h
        st1             {v23.8b},  [x3], x2
        sqxtun          v26.8b,    v26.8h
        st1             {v24.8b},  [x3], x2
        sqxtun          v27.8b,    v6.8h
        st1             {v25.8b},  [x3], x2
        st1             {v26.8b},  [x3], x2
        st1             {v27.8b},  [x3], x2

        ret
endfunc

function ff_vp8_idct_dc_add4y_neon, export=1
        movi            v0.16b,  #0
        mov             x3,  #32
        ld1r            {v16.4h},    [x1]
        st1             {v0.h}[0],   [x1], x3
        ld1r            {v17.4h},    [x1]
        st1             {v0.h}[0],   [x1], x3
        zip1            v16.2d,      v16.2d, v17.2d
        ld1r            {v18.4h},    [x1]
        st1             {v0.h}[0],   [x1], x3
        ld1r            {v19.4h},    [x1]
        st1             {v0.h}[0],   [x1], x3
        zip1            v18.2d,      v18.2d, v19.2d
        srshr           v16.8h,      v16.8h,  #3
        ld1             {v0.16b},     [x0], x2
        srshr           v18.8h,       v18.8h,  #3
        ld1             {v1.16b},     [x0], x2
        uaddw           v20.8h,       v16.8h,  v0.8b
        ld1             {v2.16b},     [x0], x2
        uaddw2          v0.8h,        v18.8h,   v0.16b
        ld1             {v3.16b},     [x0], x2
        uaddw           v21.8h, v16.8h,  v1.8b
        uaddw2          v1.8h,  v18.8h,  v1.16b
        uaddw           v22.8h, v16.8h,  v2.8b
        uaddw2          v2.8h,  v18.8h,  v2.16b
        uaddw           v23.8h, v16.8h,  v3.8b
        uaddw2          v3.8h,  v18.8h,  v3.16b
        sub             x0,  x0,  x2,  lsl #2
        sqxtun          v20.8b,  v20.8h
        sqxtun2         v20.16b, v0.8h
        sqxtun          v21.8b,  v21.8h
        sqxtun2         v21.16b, v1.8h
        sqxtun          v22.8b,  v22.8h
        st1             {v20.16b},    [x0], x2
        sqxtun2         v22.16b, v2.8h
        st1             {v21.16b},    [x0], x2
        sqxtun          v23.8b,  v23.8h
        st1             {v22.16b},    [x0], x2
        sqxtun2         v23.16b, v3.8h
        st1             {v23.16b},    [x0], x2

        ret
endfunc

// Computing the four row addresses up front keeps each row an independent
// ldr/str pair; lane inserts serialize and lane stores cost extra.
function ff_vp8_idct_dc_add_neon, export=1
        ld1r            {v2.8h},  [x1]
        add             x3,  x0,  x2
        strh            wzr,      [x1]
        add             x4,  x0,  x2,  lsl #1
        srshr           v2.8h,  v2.8h,  #3
        add             x5,  x3,  x2,  lsl #1
        ldr             s16,  [x0]
        ldr             s17,  [x3]
        ldr             s18,  [x4]
        ldr             s19,  [x5]
        uaddw           v16.8h,  v2.8h,  v16.8b
        uaddw           v17.8h,  v2.8h,  v17.8b
        uaddw           v18.8h,  v2.8h,  v18.8b
        uaddw           v19.8h,  v2.8h,  v19.8b
        sqxtun          v16.8b,  v16.8h
        sqxtun          v17.8b,  v17.8h
        sqxtun          v18.8b,  v18.8h
        sqxtun          v19.8b,  v19.8h
        str             s16,  [x0]
        str             s17,  [x3]
        str             s18,  [x4]
        str             s19,  [x5]
        ret
endfunc

.macro  vp8_loop_filter, inner=0, simple=0, hev_thresh
    .if \simple
        uabd            v17.16b, v3.16b,  v4.16b
        uabd            v23.16b, v2.16b,  v5.16b
        uqadd           v17.16b, v17.16b, v17.16b
        ushr            v18.16b, v23.16b, #1
        uqadd           v19.16b, v17.16b,  v18.16b
        movi            v21.16b, #0x80
        cmhs            v16.16b, v22.16b, v19.16b
    .else
        uabd            v20.16b, v2.16b,  v3.16b
        uabd            v21.16b, v5.16b,  v4.16b
        uabd            v18.16b, v0.16b,  v1.16b
        uabd            v19.16b, v1.16b,  v2.16b
        cmhs            v16.16b, v23.16b, v20.16b
        cmhs            v17.16b, v23.16b, v21.16b
        cmhs            v18.16b, v23.16b, v18.16b
        cmhs            v19.16b, v23.16b, v19.16b
        and             v16.16b, v17.16b, v16.16b
        uabd            v17.16b, v7.16b,  v6.16b
        and             v16.16b, v16.16b, v19.16b
        uabd            v19.16b, v6.16b,  v5.16b
        and             v16.16b, v16.16b, v18.16b
        cmhs            v18.16b, v23.16b, v17.16b
        cmhs            v19.16b, v23.16b, v19.16b
        uabd            v17.16b, v3.16b,  v4.16b
        uabd            v23.16b, v2.16b,  v5.16b
        and             v16.16b, v16.16b, v18.16b
        uqadd           v17.16b, v17.16b, v17.16b
        and             v16.16b, v16.16b, v19.16b
        ushr            v18.16b, v23.16b, #1
        dup             v23.16b, \hev_thresh
        uqadd           v19.16b, v17.16b, v18.16b
        cmhi            v20.16b, v20.16b, v23.16b
        cmhs            v19.16b, v22.16b, v19.16b
        cmhi            v22.16b, v21.16b, v23.16b
        and             v16.16b, v16.16b, v19.16b
        movi            v21.16b, #0x80
        orr             v17.16b, v20.16b, v22.16b
    .endif


        eor             v3.16b, v3.16b, v21.16b
        eor             v4.16b, v4.16b, v21.16b

        movi            v20.8h, #3
        ssubl           v18.8h, v4.8b,  v3.8b
        ssubl2          v19.8h, v4.16b, v3.16b
        eor             v2.16b, v2.16b, v21.16b
        eor             v5.16b, v5.16b, v21.16b
        mul             v18.8h, v18.8h, v20.8h
        mul             v19.8h, v19.8h, v20.8h

        sqsub           v20.16b, v2.16b, v5.16b
        movi            v22.16b, #4
        movi            v23.16b, #3
    .if \inner
        and             v20.16b, v20.16b, v17.16b
    .endif
        saddw           v18.8h,  v18.8h, v20.8b
        saddw2          v19.8h,  v19.8h, v20.16b
        sqxtn           v18.8b,  v18.8h
        sqxtn2          v18.16b, v19.8h
    .if !\inner && !\simple
        eor             v1.16b,  v1.16b,  v21.16b
        eor             v6.16b,  v6.16b,  v21.16b
    .endif
        and             v18.16b, v18.16b, v16.16b


    .if \simple
        sqadd           v19.16b, v18.16b, v22.16b
        sqadd           v20.16b, v18.16b, v23.16b
        sshr            v19.16b, v19.16b, #3
        sshr            v20.16b, v20.16b, #3
        sqsub           v4.16b,  v4.16b,  v19.16b
        sqadd           v3.16b,  v3.16b,  v20.16b
        eor             v4.16b,  v4.16b,  v21.16b
        eor             v3.16b,  v3.16b,  v21.16b
        eor             v5.16b,  v5.16b,  v21.16b
        eor             v2.16b,  v2.16b,  v21.16b
    .elseif \inner
        sqadd           v19.16b, v18.16b, v22.16b
        sqadd           v20.16b, v18.16b, v23.16b
        sshr            v19.16b, v19.16b, #3
        sshr            v20.16b, v20.16b, #3
        sqsub           v4.16b,  v4.16b,  v19.16b
        sqadd           v3.16b,  v3.16b,  v20.16b
        bic             v19.16b, v19.16b, v17.16b
        eor             v4.16b,  v4.16b,  v21.16b
        srshr           v19.16b, v19.16b, #1
        eor             v3.16b,  v3.16b,  v21.16b
        sqsub           v5.16b,  v5.16b,  v19.16b
        sqadd           v2.16b,  v2.16b,  v19.16b
        eor             v5.16b,  v5.16b,  v21.16b
        eor             v2.16b,  v2.16b,  v21.16b
    .else
        and             v20.16b, v18.16b, v17.16b
        sqadd           v19.16b, v20.16b, v22.16b
        sqadd           v20.16b, v20.16b, v23.16b
        sshr            v19.16b, v19.16b, #3
        sshr            v20.16b, v20.16b, #3
        bic             v18.16b, v18.16b, v17.16b
        sqsub           v4.16b,  v4.16b,  v19.16b
        sqadd           v3.16b,  v3.16b,  v20.16b

        movi            v17.8h,  #63
        sshll           v22.8h,  v18.8b, #3
        sshll2          v23.8h,  v18.16b, #3
        saddw           v22.8h,  v22.8h, v18.8b
        saddw2          v23.8h,  v23.8h, v18.16b
        add             v16.8h,  v17.8h, v22.8h
        add             v17.8h,  v17.8h, v23.8h
        add             v19.8h,  v16.8h, v22.8h
        add             v20.8h,  v17.8h, v23.8h
        add             v22.8h,  v19.8h, v22.8h
        add             v23.8h,  v20.8h, v23.8h
        sqshrn          v16.8b,  v16.8h,  #7
        sqshrn2         v16.16b, v17.8h, #7
        sqshrn          v19.8b,  v19.8h, #7
        sqshrn2         v19.16b, v20.8h, #7
        sqshrn          v22.8b,  v22.8h, #7
        sqshrn2         v22.16b, v23.8h, #7
        sqadd           v1.16b,  v1.16b,  v16.16b
        sqsub           v6.16b,  v6.16b,  v16.16b
        sqadd           v2.16b,  v2.16b,  v19.16b
        sqsub           v5.16b,  v5.16b,  v19.16b
        sqadd           v3.16b,  v3.16b,  v22.16b
        sqsub           v4.16b,  v4.16b,  v22.16b
        eor             v3.16b,  v3.16b,  v21.16b
        eor             v4.16b,  v4.16b,  v21.16b
        eor             v2.16b,  v2.16b,  v21.16b
        eor             v5.16b,  v5.16b,  v21.16b
        eor             v1.16b,  v1.16b,  v21.16b
        eor             v6.16b,  v6.16b,  v21.16b
    .endif
.endm

.macro  vp8_v_loop_filter16 name, inner=0, simple=0
function ff_vp8_v_loop_filter16\name\()_neon, export=1
        sub             x0,  x0,  x1,  lsl #1+!\simple

    .if !\simple
        ld1             {v0.16b},     [x0], x1
        ld1             {v1.16b},     [x0], x1
    .endif
        ld1             {v2.16b},     [x0], x1
        ld1             {v3.16b},     [x0], x1
        ld1             {v4.16b},     [x0], x1
        ld1             {v5.16b},     [x0], x1
    .if !\simple
        ld1             {v6.16b},     [x0], x1
        ld1             {v7.16b},     [x0]
        dup             v23.16b, w3
    .endif
        dup             v22.16b, w2

        vp8_loop_filter inner=\inner, simple=\simple, hev_thresh=w4

        sub             x0,  x0,  x1,  lsl #2
    .if !\simple
        sub             x0,  x0,  x1,  lsl #1

        st1             {v1.16b},     [x0], x1
    .endif
        st1             {v2.16b},     [x0], x1
        st1             {v3.16b},     [x0], x1
        st1             {v4.16b},     [x0], x1
        st1             {v5.16b},     [x0], x1
    .if !\simple
        st1             {v6.16b},     [x0]
    .endif

        ret
endfunc
.endm

vp8_v_loop_filter16
vp8_v_loop_filter16 _inner,  inner=1
vp8_v_loop_filter16 _simple, simple=1

.macro  vp8_v_loop_filter8uv name, inner=0
function ff_vp8_v_loop_filter8uv\name\()_neon, export=1
        sub             x0,  x0,  x2,  lsl #2
        sub             x1,  x1,  x2,  lsl #2
        ld1             {v0.d}[0],     [x0], x2
        ld1             {v0.d}[1],     [x1], x2
        ld1             {v1.d}[0],     [x0], x2
        ld1             {v1.d}[1],     [x1], x2
        ld1             {v2.d}[0],     [x0], x2
        ld1             {v2.d}[1],     [x1], x2
        ld1             {v3.d}[0],     [x0], x2
        ld1             {v3.d}[1],     [x1], x2
        ld1             {v4.d}[0],     [x0], x2
        ld1             {v4.d}[1],     [x1], x2
        ld1             {v5.d}[0],     [x0], x2
        ld1             {v5.d}[1],     [x1], x2
        ld1             {v6.d}[0],     [x0], x2
        ld1             {v6.d}[1],     [x1], x2
        ld1             {v7.d}[0],     [x0]
        ld1             {v7.d}[1],     [x1]

        dup             v22.16b, w3
        dup             v23.16b, w4

        vp8_loop_filter inner=\inner, hev_thresh=w5

        sub             x0,  x0,  x2,  lsl #2
        sub             x1,  x1,  x2,  lsl #2
        sub             x0,  x0,  x2,  lsl #1
        sub             x1,  x1,  x2,  lsl #1


        st1             {v1.d}[0],     [x0], x2
        st1             {v1.d}[1],     [x1], x2
        st1             {v2.d}[0],     [x0], x2
        st1             {v2.d}[1],     [x1], x2
        st1             {v3.d}[0],     [x0], x2
        st1             {v3.d}[1],     [x1], x2
        st1             {v4.d}[0],     [x0], x2
        st1             {v4.d}[1],     [x1], x2
        st1             {v5.d}[0],     [x0], x2
        st1             {v5.d}[1],     [x1], x2
        st1             {v6.d}[0],     [x0]
        st1             {v6.d}[1],     [x1]

        ret
endfunc
.endm

vp8_v_loop_filter8uv
vp8_v_loop_filter8uv _inner, inner=1

.macro  vp8_h_loop_filter16 name, inner=0
function ff_vp8_h_loop_filter16\name\()_neon, export=1

        sub             x0,  x0,  #4
        ld1             {v0.d}[0], [x0], x1
        ld1             {v1.d}[0], [x0], x1
        ld1             {v2.d}[0], [x0], x1
        ld1             {v3.d}[0], [x0], x1
        ld1             {v4.d}[0], [x0], x1
        ld1             {v5.d}[0], [x0], x1
        ld1             {v6.d}[0], [x0], x1
        ld1             {v7.d}[0], [x0], x1
        ld1             {v0.d}[1], [x0], x1
        ld1             {v1.d}[1], [x0], x1
        ld1             {v2.d}[1], [x0], x1
        ld1             {v3.d}[1], [x0], x1
        ld1             {v4.d}[1], [x0], x1
        ld1             {v5.d}[1], [x0], x1
        ld1             {v6.d}[1], [x0], x1
        ld1             {v7.d}[1], [x0], x1

        transpose_8x16B v0,  v1,  v2,  v3,  v4,  v5,  v6,  v7, v30, v31

        dup             v22.16b, w2
        dup             v23.16b, w3

        vp8_loop_filter inner=\inner, hev_thresh=w4

        sub             x0,  x0,  x1, lsl #4

        transpose_8x16B v0,  v1,  v2,  v3,  v4,  v5,  v6,  v7, v30, v31

        st1             {v0.d}[0], [x0], x1
        st1             {v1.d}[0], [x0], x1
        st1             {v2.d}[0], [x0], x1
        st1             {v3.d}[0], [x0], x1
        st1             {v4.d}[0], [x0], x1
        st1             {v5.d}[0], [x0], x1
        st1             {v6.d}[0], [x0], x1
        st1             {v7.d}[0], [x0], x1
        st1             {v0.d}[1], [x0], x1
        st1             {v1.d}[1], [x0], x1
        st1             {v2.d}[1], [x0], x1
        st1             {v3.d}[1], [x0], x1
        st1             {v4.d}[1], [x0], x1
        st1             {v5.d}[1], [x0], x1
        st1             {v6.d}[1], [x0], x1
        st1             {v7.d}[1], [x0]

        ret
endfunc
.endm

vp8_h_loop_filter16
vp8_h_loop_filter16 _inner,  inner=1

// The simple filter only reads p1..q1 and only writes p0/q0, so load 4 columns
// per row and transpose 16x4 instead of touching and transposing an 8x16 block.
function ff_vp8_h_loop_filter16_simple_neon, export=1
        dup             v22.16b, w2
        sub             x0,  x0,  #2
        add             x2,  x0,  x1
        add             x3,  x0,  x1, lsl #1
        add             x4,  x2,  x1, lsl #1
        lsl             x5,  x1,  #2
        add             x6,  x0,  #1
        add             x7,  x2,  #1
        add             x8,  x3,  #1
        add             x9,  x4,  #1

        ld1             {v16.s}[0], [x0], x5
        ld1             {v17.s}[0], [x2], x5
        ld1             {v18.s}[0], [x3], x5
        ld1             {v19.s}[0], [x4], x5
        ld1             {v16.s}[1], [x0], x5
        ld1             {v17.s}[1], [x2], x5
        ld1             {v18.s}[1], [x3], x5
        ld1             {v19.s}[1], [x4], x5
        ld1             {v16.s}[2], [x0], x5
        ld1             {v17.s}[2], [x2], x5
        ld1             {v18.s}[2], [x3], x5
        ld1             {v19.s}[2], [x4], x5
        ld1             {v16.s}[3], [x0]
        ld1             {v17.s}[3], [x2]
        ld1             {v18.s}[3], [x3]
        ld1             {v19.s}[3], [x4]

        trn1            v24.16b, v16.16b, v17.16b
        trn2            v25.16b, v16.16b, v17.16b
        trn1            v26.16b, v18.16b, v19.16b
        trn2            v27.16b, v18.16b, v19.16b
        trn1            v2.8h,   v24.8h,  v26.8h
        trn2            v4.8h,   v24.8h,  v26.8h
        trn1            v3.8h,   v25.8h,  v27.8h
        trn2            v5.8h,   v25.8h,  v27.8h

        vp8_loop_filter simple=1

        zip1            v24.16b, v3.16b, v4.16b
        zip2            v25.16b, v3.16b, v4.16b

        st1             {v24.h}[0], [x6], x5
        st1             {v24.h}[1], [x7], x5
        st1             {v24.h}[2], [x8], x5
        st1             {v24.h}[3], [x9], x5
        st1             {v24.h}[4], [x6], x5
        st1             {v24.h}[5], [x7], x5
        st1             {v24.h}[6], [x8], x5
        st1             {v24.h}[7], [x9], x5
        st1             {v25.h}[0], [x6], x5
        st1             {v25.h}[1], [x7], x5
        st1             {v25.h}[2], [x8], x5
        st1             {v25.h}[3], [x9], x5
        st1             {v25.h}[4], [x6], x5
        st1             {v25.h}[5], [x7], x5
        st1             {v25.h}[6], [x8], x5
        st1             {v25.h}[7], [x9]

        ret
endfunc

// Transposes the 16x16 byte block held in v0-v15, clobbering v16-v31.
.macro  transpose_16x16B
        trn1            v16.16b, v0.16b,  v1.16b
        trn2            v17.16b, v0.16b,  v1.16b
        trn1            v18.16b, v2.16b,  v3.16b
        trn2            v19.16b, v2.16b,  v3.16b
        trn1            v20.16b, v4.16b,  v5.16b
        trn2            v21.16b, v4.16b,  v5.16b
        trn1            v22.16b, v6.16b,  v7.16b
        trn2            v23.16b, v6.16b,  v7.16b
        trn1            v24.16b, v8.16b,  v9.16b
        trn2            v25.16b, v8.16b,  v9.16b
        trn1            v26.16b, v10.16b, v11.16b
        trn2            v27.16b, v10.16b, v11.16b
        trn1            v28.16b, v12.16b, v13.16b
        trn2            v29.16b, v12.16b, v13.16b
        trn1            v30.16b, v14.16b, v15.16b
        trn2            v31.16b, v14.16b, v15.16b

        trn1            v0.8h,   v16.8h,  v18.8h
        trn2            v2.8h,   v16.8h,  v18.8h
        trn1            v1.8h,   v17.8h,  v19.8h
        trn2            v3.8h,   v17.8h,  v19.8h
        trn1            v4.8h,   v20.8h,  v22.8h
        trn2            v6.8h,   v20.8h,  v22.8h
        trn1            v5.8h,   v21.8h,  v23.8h
        trn2            v7.8h,   v21.8h,  v23.8h
        trn1            v8.8h,   v24.8h,  v26.8h
        trn2            v10.8h,  v24.8h,  v26.8h
        trn1            v9.8h,   v25.8h,  v27.8h
        trn2            v11.8h,  v25.8h,  v27.8h
        trn1            v12.8h,  v28.8h,  v30.8h
        trn2            v14.8h,  v28.8h,  v30.8h
        trn1            v13.8h,  v29.8h,  v31.8h
        trn2            v15.8h,  v29.8h,  v31.8h

        trn1            v16.4s,  v0.4s,   v4.4s
        trn2            v20.4s,  v0.4s,   v4.4s
        trn1            v17.4s,  v1.4s,   v5.4s
        trn2            v21.4s,  v1.4s,   v5.4s
        trn1            v18.4s,  v2.4s,   v6.4s
        trn2            v22.4s,  v2.4s,   v6.4s
        trn1            v19.4s,  v3.4s,   v7.4s
        trn2            v23.4s,  v3.4s,   v7.4s
        trn1            v24.4s,  v8.4s,   v12.4s
        trn2            v28.4s,  v8.4s,   v12.4s
        trn1            v25.4s,  v9.4s,   v13.4s
        trn2            v29.4s,  v9.4s,   v13.4s
        trn1            v26.4s,  v10.4s,  v14.4s
        trn2            v30.4s,  v10.4s,  v14.4s
        trn1            v27.4s,  v11.4s,  v15.4s
        trn2            v31.4s,  v11.4s,  v15.4s

        trn1            v0.2d,   v16.2d,  v24.2d
        trn2            v8.2d,   v16.2d,  v24.2d
        trn1            v1.2d,   v17.2d,  v25.2d
        trn2            v9.2d,   v17.2d,  v25.2d
        trn1            v2.2d,   v18.2d,  v26.2d
        trn2            v10.2d,  v18.2d,  v26.2d
        trn1            v3.2d,   v19.2d,  v27.2d
        trn2            v11.2d,  v19.2d,  v27.2d
        trn1            v4.2d,   v20.2d,  v28.2d
        trn2            v12.2d,  v20.2d,  v28.2d
        trn1            v5.2d,   v21.2d,  v29.2d
        trn2            v13.2d,  v21.2d,  v29.2d
        trn1            v6.2d,   v22.2d,  v30.2d
        trn2            v14.2d,  v22.2d,  v30.2d
        trn1            v7.2d,   v23.2d,  v31.2d
        trn2            v15.2d,  v23.2d,  v31.2d
.endm

// One vertical edge, with the four columns already transposed into rows.
// v16 = 0x80, v17 = #4, v18 = #3.
.macro  vp8_simple_filter p1, p0, q0, q1, flim, t0, t1, t2
        uabd            \t0\().16b, \p0\().16b, \q0\().16b
        uabd            \t1\().16b, \p1\().16b, \q1\().16b
        uqadd           \t0\().16b, \t0\().16b, \t0\().16b
        ushr            \t1\().16b, \t1\().16b, #1
        uqadd           \t0\().16b, \t0\().16b, \t1\().16b
        cmhs            \t0\().16b, \flim\().16b, \t0\().16b

        eor             \p1\().16b, \p1\().16b, v16.16b
        eor             \p0\().16b, \p0\().16b, v16.16b
        eor             \q0\().16b, \q0\().16b, v16.16b
        eor             \q1\().16b, \q1\().16b, v16.16b

        // 3*(q0-p0) saturates monotonically, so the byte-wise chain matches
        // clamping the widened sum
        sqsub           \t1\().16b, \q0\().16b, \p0\().16b
        sqsub           \t2\().16b, \p1\().16b, \q1\().16b
        sqadd           \t2\().16b, \t2\().16b, \t1\().16b
        sqadd           \t2\().16b, \t2\().16b, \t1\().16b
        sqadd           \t2\().16b, \t2\().16b, \t1\().16b
        and             \t2\().16b, \t2\().16b, \t0\().16b

        sqadd           \t0\().16b, \t2\().16b, v17.16b
        sqadd           \t1\().16b, \t2\().16b, v18.16b
        sshr            \t0\().16b, \t0\().16b, #3
        sshr            \t1\().16b, \t1\().16b, #3
        sqsub           \q0\().16b, \q0\().16b, \t0\().16b
        sqadd           \p0\().16b, \p0\().16b, \t1\().16b

        eor             \p1\().16b, \p1\().16b, v16.16b
        eor             \p0\().16b, \p0\().16b, v16.16b
        eor             \q0\().16b, \q0\().16b, v16.16b
        eor             \q1\().16b, \q1\().16b, v16.16b
.endm

// Filters all four vertical edges of a luma macroblock in one pass: the
// four edges cover columns -2..13, so the whole block is read and written
// full width instead of four strided lane-wise passes over it.
function ff_vp8_h_loop_filter_simple_mb_neon, export=1
        stp             d8,  d9,  [sp, #-64]!
        stp             d10, d11, [sp, #16]
        stp             d12, d13, [sp, #32]
        stp             d14, d15, [sp, #48]

        sub             x0,  x0,  #2
        add             x4,  x0,  x1
        add             x5,  x0,  x1, lsl #1
        add             x6,  x4,  x1, lsl #1
        lsl             x7,  x1,  #2
        mov             x8,  x0
        mov             x9,  x4
        mov             x10, x5
        mov             x11, x6

        ld1             {v0.16b},  [x0], x7
        ld1             {v1.16b},  [x4], x7
        ld1             {v2.16b},  [x5], x7
        ld1             {v3.16b},  [x6], x7
        ld1             {v4.16b},  [x0], x7
        ld1             {v5.16b},  [x4], x7
        ld1             {v6.16b},  [x5], x7
        ld1             {v7.16b},  [x6], x7
        ld1             {v8.16b},  [x0], x7
        ld1             {v9.16b},  [x4], x7
        ld1             {v10.16b}, [x5], x7
        ld1             {v11.16b}, [x6], x7
        ld1             {v12.16b}, [x0]
        ld1             {v13.16b}, [x4]
        ld1             {v14.16b}, [x5]
        ld1             {v15.16b}, [x6]

        transpose_16x16B

        movi            v16.16b, #0x80
        movi            v17.16b, #4
        movi            v18.16b, #3
        dup             v19.16b, w2
        dup             v20.16b, w3

        vp8_simple_filter v0,  v1,  v2,  v3,  v19, v21, v22, v23
        vp8_simple_filter v4,  v5,  v6,  v7,  v20, v24, v25, v26
        vp8_simple_filter v8,  v9,  v10, v11, v20, v27, v28, v29
        vp8_simple_filter v12, v13, v14, v15, v20, v30, v31, v19

        transpose_16x16B

        st1             {v0.16b},  [x8], x7
        st1             {v1.16b},  [x9], x7
        st1             {v2.16b},  [x10], x7
        st1             {v3.16b},  [x11], x7
        st1             {v4.16b},  [x8], x7
        st1             {v5.16b},  [x9], x7
        st1             {v6.16b},  [x10], x7
        st1             {v7.16b},  [x11], x7
        st1             {v8.16b},  [x8], x7
        st1             {v9.16b},  [x9], x7
        st1             {v10.16b}, [x10], x7
        st1             {v11.16b}, [x11], x7
        st1             {v12.16b}, [x8]
        st1             {v13.16b}, [x9]
        st1             {v14.16b}, [x10]
        st1             {v15.16b}, [x11]

        ldp             d10, d11, [sp, #16]
        ldp             d12, d13, [sp, #32]
        ldp             d14, d15, [sp, #48]
        ldp             d8,  d9,  [sp], #64

        ret
endfunc

.macro  vp8_h_loop_filter8uv name, inner=0
function ff_vp8_h_loop_filter8uv\name\()_neon, export=1
        sub             x0,  x0,  #4
        sub             x1,  x1,  #4

        ld1             {v0.d}[0],     [x0], x2
        ld1             {v0.d}[1],     [x1], x2
        ld1             {v1.d}[0],     [x0], x2
        ld1             {v1.d}[1],     [x1], x2
        ld1             {v2.d}[0],     [x0], x2
        ld1             {v2.d}[1],     [x1], x2
        ld1             {v3.d}[0],     [x0], x2
        ld1             {v3.d}[1],     [x1], x2
        ld1             {v4.d}[0],     [x0], x2
        ld1             {v4.d}[1],     [x1], x2
        ld1             {v5.d}[0],     [x0], x2
        ld1             {v5.d}[1],     [x1], x2
        ld1             {v6.d}[0],     [x0], x2
        ld1             {v6.d}[1],     [x1], x2
        ld1             {v7.d}[0],     [x0], x2
        ld1             {v7.d}[1],     [x1], x2

        transpose_8x16B v0,  v1,  v2,  v3,  v4,  v5,  v6,  v7, v30, v31

        dup             v22.16b, w3
        dup             v23.16b, w4

        vp8_loop_filter inner=\inner, hev_thresh=w5

        sub             x0,  x0,  x2, lsl #3
        sub             x1,  x1,  x2, lsl #3

        transpose_8x16B v0,  v1,  v2,  v3,  v4,  v5,  v6,  v7, v30, v31

        st1             {v0.d}[0],     [x0], x2
        st1             {v0.d}[1],     [x1], x2
        st1             {v1.d}[0],     [x0], x2
        st1             {v1.d}[1],     [x1], x2
        st1             {v2.d}[0],     [x0], x2
        st1             {v2.d}[1],     [x1], x2
        st1             {v3.d}[0],     [x0], x2
        st1             {v3.d}[1],     [x1], x2
        st1             {v4.d}[0],     [x0], x2
        st1             {v4.d}[1],     [x1], x2
        st1             {v5.d}[0],     [x0], x2
        st1             {v5.d}[1],     [x1], x2
        st1             {v6.d}[0],     [x0], x2
        st1             {v6.d}[1],     [x1], x2
        st1             {v7.d}[0],     [x0]
        st1             {v7.d}[1],     [x1]

        ret

endfunc
.endm

vp8_h_loop_filter8uv
vp8_h_loop_filter8uv _inner, inner=1
