
#include "asm.S"


function ff_pred16x16_hor_neon, export=1
        sub             x2,  x0,  #1
    .rept 16
        ld1r            {v0.16b}, [x2], x1
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_vert_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.16b}, [x2]
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc


function ff_pred4x4_tm_neon, export=1
        // These small blocks are fully unrolled: a taken branch per row costs more.
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        ld1r            {v0.8b},  [x4]
        ld1r            {v1.2s},  [x3]
        sub             x4,  x0,  #1
        usubl           v2.8h,  v1.8b,  v0.8b
        ld1r            {v3.8b},  [x4], x2
        ld1r            {v4.8b},  [x4], x2
        ld1r            {v5.8b},  [x4], x2
        ld1r            {v6.8b},  [x4]
        mov             v3.s[1],  v4.s[0]
        mov             v5.s[1],  v6.s[0]
        uaddw           v16.8h,   v2.8h,  v3.8b
        uaddw           v17.8h,   v2.8h,  v5.8b
        sqxtun          v16.8b,   v16.8h
        sqxtun          v17.8b,   v17.8h
        st1             {v16.s}[0], [x0], x2
        st1             {v16.s}[1], [x0], x2
        st1             {v17.s}[0], [x0], x2
        st1             {v17.s}[1], [x0]
        ret
endfunc

function ff_pred8x8_tm_neon, export=1
        sub             x2,  x0,  x1
        sub             x3,  x2,  #1
        ld1r            {v0.8b},  [x3]
        ld1             {v1.8b},  [x2]
        sub             x3,  x0,  #1
        usubl           v2.8h,  v1.8b,  v0.8b
    .rept 8
        ld1r            {v3.8b},  [x3], x1
        uaddw           v4.8h,  v2.8h,  v3.8b
        sqxtun          v4.8b,  v4.8h
        st1             {v4.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_tm_neon, export=1
        sub             x2,  x0,  x1
        sub             x3,  x2,  #1
        ld1r            {v0.16b}, [x3]
        ld1             {v1.16b}, [x2]
        sub             x3,  x0,  #1
        usubl           v2.8h,  v1.8b,  v0.8b
        usubl2          v3.8h,  v1.16b, v0.16b
    .rept 16
        ld1r            {v4.16b}, [x3], x1
        uaddw           v16.8h,   v2.8h,  v4.8b
        uaddw2          v17.8h,   v3.8h,  v4.16b
        sqxtun          v16.8b,   v16.8h
        sqxtun2         v16.16b,  v17.8h
        st1             {v16.16b}, [x0], x1
    .endr
        ret
endfunc


function ff_pred8x8_dc_neon, export=1
        sub             x2,  x0,  x1
        sub             x3,  x0,  #1
        ld1             {v0.8b},  [x2]
        uaddlv          h0,  v0.8b
        umov            w6,  v0.h[0]
        mov             w4,  #8
    .rept 8
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        add             w4,  w4,  w6
        lsr             w4,  w4,  #4
        dup             v0.8b,  w4
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred8x8_left_dc_neon, export=1
        sub             x3,  x0,  #1
        mov             w4,  #4
    .rept 8
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        lsr             w4,  w4,  #3
        dup             v0.8b,  w4
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred8x8_top_dc_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.8b},  [x2]
        uaddlv          h0,  v0.8b
        umov            w4,  v0.h[0]
        add             w4,  w4,  #4
        lsr             w4,  w4,  #3
        dup             v0.8b,  w4
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_dc_neon, export=1
        // Scalar left-column loads avoid serializing vector lane insertion.
        sub             x2,  x0,  x1
        sub             x3,  x0,  #1
        ld1             {v0.16b}, [x2]
        uaddlv          h0,  v0.16b
        umov            w6,  v0.h[0]
        mov             w4,  #16
    .rept 16
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        add             w4,  w4,  w6
        lsr             w4,  w4,  #5
        dup             v0.16b, w4
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_left_dc_neon, export=1
        sub             x3,  x0,  #1
        mov             w4,  #8
    .rept 16
        ldrb            w5,  [x3]
        add             x3,  x3,  x1
        add             w4,  w4,  w5
    .endr
        lsr             w4,  w4,  #4
        dup             v0.16b, w4
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc

function ff_pred16x16_top_dc_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.16b}, [x2]
        uaddlv          h0,  v0.16b
        umov            w4,  v0.h[0]
        add             w4,  w4,  #8
        lsr             w4,  w4,  #4
        dup             v0.16b, w4
    .rept 16
        st1             {v0.16b}, [x0], x1
    .endr
        ret
endfunc


// uhadd-based avg3 is exact: its dropped bit cannot affect the final rounding.
.macro  edge_filters   seq, a, b, t1, t2
        ext             \t1\().8b, \seq\().8b, \seq\().8b, #1
        ext             \t2\().8b, \seq\().8b, \seq\().8b, #2
        urhadd          \a\().8b,  \seq\().8b, \t1\().8b
        uhadd           \b\().8b,  \seq\().8b, \t2\().8b
        urhadd          \b\().8b,  \b\().8b,   \t1\().8b
.endm

.macro  load_left_rev_top
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        sub             x5,  x0,  #1
        ldrb            w6,  [x5]
        add             x5,  x5,  x2
        ldrb            w7,  [x5]
        add             x5,  x5,  x2
        ldrb            w9,  [x5]
        add             x5,  x5,  x2
        ldrb            w10, [x5]
        ldr             w11, [x4]
        orr             w10, w10, w9,  lsl #8
        orr             w10, w10, w7,  lsl #16
        orr             w10, w10, w6,  lsl #24
        orr             x10, x10, x11, lsl #32
        fmov            d0,  x10
.endm

function ff_pred4x4_dc_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x0,  #1
        ldrb            w5,  [x3]
        ldrb            w6,  [x3, #1]
        ldrb            w7,  [x3, #2]
        ldrb            w9,  [x3, #3]
        add             w5,  w5,  w6
        ldrb            w6,  [x4]
        add             w7,  w7,  w9
        ldrb            w9,  [x4,  x2]
        add             x4,  x4,  x2,  lsl #1
        add             w5,  w5,  w7
        ldrb            w7,  [x4]
        add             w6,  w6,  w9
        ldrb            w9,  [x4,  x2]
        add             w5,  w5,  #4
        add             w7,  w7,  w9
        add             w5,  w5,  w6
        add             w5,  w5,  w7
        lsr             w5,  w5,  #3
        orr             w5,  w5,  w5,  lsl #8
        orr             w5,  w5,  w5,  lsl #16
        str             w5,  [x0]
        str             w5,  [x0,  x2]
        add             x0,  x0,  x2,  lsl #1
        str             w5,  [x0]
        str             w5,  [x0,  x2]
        ret
endfunc

function ff_pred4x4_vert_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        ldr             w6,  [x4]
        ldrb            w7,  [x4, #4]
        ldrb            w9,  [x1]
        orr             x6,  x6,  x7,  lsl #32
        orr             x6,  x6,  x9,  lsl #40
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v2.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_hor_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        sub             x5,  x0,  #1
        ldrb            w6,  [x4]
        ldrb            w7,  [x5]
        add             x5,  x5,  x2
        ldrb            w9,  [x5]
        add             x5,  x5,  x2
        ldrb            w10, [x5]
        add             x5,  x5,  x2
        ldrb            w11, [x5]
        orr             w6,  w6,  w7,  lsl #8
        orr             w6,  w6,  w9,  lsl #16
        orr             w6,  w6,  w10, lsl #24
        orr             x6,  x6,  x11, lsl #32
        orr             x6,  x6,  x11, lsl #40
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        dup             v16.8b, v2.b[0]
        dup             v17.8b, v2.b[1]
        dup             v18.8b, v2.b[2]
        dup             v19.8b, v2.b[3]
        st1             {v16.s}[0], [x0], x2
        st1             {v17.s}[0], [x0], x2
        st1             {v18.s}[0], [x0], x2
        st1             {v19.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_down_left_neon, export=1
        sub             x3,  x0,  x2
        ld1             {v0.s}[0], [x3]
        ld1             {v0.s}[1], [x1]
        dup             v1.8b,  v0.b[7]
        ext             v3.8b,  v0.8b,  v1.8b,  #1
        ext             v4.8b,  v0.8b,  v1.8b,  #2
        uhadd           v2.8b,  v0.8b,  v4.8b
        urhadd          v2.8b,  v2.8b,  v3.8b
        ext             v5.8b,  v2.8b,  v2.8b,  #1
        ext             v6.8b,  v2.8b,  v2.8b,  #2
        ext             v7.8b,  v2.8b,  v2.8b,  #3
        st1             {v2.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v7.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_down_right_neon, export=1
        load_left_rev_top
        add             x6,  x4,  #4
        ld1             {v0.b}[8], [x6]
        ext             v3.16b, v0.16b, v0.16b, #1
        ext             v4.16b, v0.16b, v0.16b, #2
        uhadd           v2.16b, v0.16b, v4.16b
        urhadd          v2.16b, v2.16b, v3.16b
        ext             v5.16b, v2.16b, v2.16b, #1
        ext             v6.16b, v2.16b, v2.16b, #2
        ext             v7.16b, v2.16b, v2.16b, #3
        st1             {v7.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v2.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_vert_left_neon, export=1
        sub             x3,  x0,  x2
        ld1             {v0.s}[0], [x3]
        ld1             {v0.s}[1], [x1]
        edge_filters    v0, v1, v2, v3, v4
        ext             v5.8b,  v1.8b,  v1.8b,  #1
        ins             v5.b[3], v2.b[4]
        ext             v6.8b,  v2.8b,  v2.8b,  #1
        ins             v6.b[3], v2.b[5]
        st1             {v1.s}[0], [x0], x2
        st1             {v2.s}[0], [x0], x2
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_hor_up_neon, export=1
        sub             x5,  x0,  #1
        ldrb            w6,  [x5]
        add             x5,  x5,  x2
        ldrb            w7,  [x5]
        add             x5,  x5,  x2
        ldrb            w9,  [x5]
        add             x5,  x5,  x2
        ldrb            w10, [x5]
        orr             w6,  w6,  w7,  lsl #8
        orr             w6,  w6,  w9,  lsl #16
        orr             w6,  w6,  w10, lsl #24
        orr             x6,  x6,  x10, lsl #32
        orr             x6,  x6,  x10, lsl #40
        fmov            d0,  x6
        edge_filters    v0, v1, v2, v3, v4
        zip1            v5.8b,  v1.8b,  v2.8b
        ext             v6.8b,  v5.8b,  v5.8b,  #2
        ext             v7.8b,  v5.8b,  v5.8b,  #4
        dup             v16.8b, v0.b[3]
        st1             {v5.s}[0], [x0], x2
        st1             {v6.s}[0], [x0], x2
        st1             {v7.s}[0], [x0], x2
        st1             {v16.s}[0], [x0]
        ret
endfunc

function ff_pred4x4_hor_down_neon, export=1
        load_left_rev_top
        edge_filters    v0, v1, v2, v3, v4
        zip1            v5.8b,  v1.8b,  v2.8b
        ext             v6.8b,  v5.8b,  v5.8b,  #2
        ext             v7.8b,  v5.8b,  v5.8b,  #4
        ext             v16.8b, v2.8b,  v2.8b,  #2
        ins             v16.b[0], v1.b[3]
        st1             {v16.s}[0], [x0], x2
        st1             {v7.s}[0],  [x0], x2
        st1             {v6.s}[0],  [x0], x2
        st1             {v5.s}[0],  [x0]
        ret
endfunc

function ff_pred4x4_vert_right_neon, export=1
        sub             x3,  x0,  x2
        sub             x4,  x3,  #1
        sub             x5,  x0,  #1
        ldrb            w6,  [x5]
        add             x5,  x5,  x2
        ldrb            w7,  [x5]
        add             x5,  x5,  x2
        ldrb            w9,  [x5]
        ldr             w10, [x4]
        add             x11, x4,  #4
        ldrb            w11, [x11]
        orr             w9,  w9,  w7,  lsl #8
        orr             w9,  w9,  w6,  lsl #16
        orr             x9,  x9,  x10, lsl #24
        orr             x9,  x9,  x11, lsl #56
        fmov            d0,  x9
        edge_filters    v0, v1, v2, v3, v4
        ext             v5.8b,  v1.8b,  v1.8b,  #3
        ext             v6.8b,  v2.8b,  v2.8b,  #2
        ext             v7.8b,  v1.8b,  v1.8b,  #2
        ins             v7.b[0], v2.b[1]
        ext             v16.8b, v2.8b,  v2.8b,  #1
        ins             v16.b[0], v2.b[0]
        st1             {v5.s}[0],  [x0], x2
        st1             {v6.s}[0],  [x0], x2
        st1             {v7.s}[0],  [x0], x2
        st1             {v16.s}[0], [x0]
        ret
endfunc


function ff_pred8x8_vert_neon, export=1
        sub             x2,  x0,  x1
        ld1             {v0.8b},  [x2]
    .rept 8
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc

function ff_pred8x8_hor_neon, export=1
        sub             x2,  x0,  #1
    .rept 8
        ld1r            {v0.8b},  [x2], x1
        st1             {v0.8b},  [x0], x1
    .endr
        ret
endfunc
