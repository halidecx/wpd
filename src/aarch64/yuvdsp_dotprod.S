#include "asm.S"

#if HAVE_DOTPROD

const yuv444_dot_coeffs, align=4
        .byte           0, -61, 39, -80         // Y low halves
        .byte           0, 65, 127, 25          // Y high halves
        .byte           0, -99, -101, -60       // U low halves
        .byte           0, -37, -73, 111        // U high halves
        .byte           0, -60, 64, -4          // V low halves
        .byte           0, 111, -93, -18        // V high halves
endconst

.macro  yuv444_dot16 dot, out, coef, ilo, ihi, round
        mov             v4.16b,  \round\().16b
        mov             v5.16b,  \round\().16b
        mov             v6.16b,  \round\().16b
        mov             v7.16b,  \round\().16b
        movi            v16.4s,  #0
        movi            v17.4s,  #0
        movi            v18.4s,  #0
        movi            v19.4s,  #0
        \dot            v4.4s,   v0.16b, \coef\().4b[\ilo]
        \dot            v5.4s,   v1.16b, \coef\().4b[\ilo]
        \dot            v6.4s,   v2.16b, \coef\().4b[\ilo]
        \dot            v7.4s,   v3.16b, \coef\().4b[\ilo]
        \dot            v16.4s,  v0.16b, \coef\().4b[\ihi]
        \dot            v17.4s,  v1.16b, \coef\().4b[\ihi]
        \dot            v18.4s,  v2.16b, \coef\().4b[\ihi]
        \dot            v19.4s,  v3.16b, \coef\().4b[\ihi]
        mla             v4.4s,   v16.4s, v24.4s
        mla             v5.4s,   v17.4s, v24.4s
        mla             v6.4s,   v18.4s, v24.4s
        mla             v7.4s,   v19.4s, v24.4s
        sqshrn          v20.4h,  v4.4s,  #16
        sqshrn2         v20.8h,  v5.4s,  #16
        sqshrn          v21.4h,  v6.4s,  #16
        sqshrn2         v21.8h,  v7.4s,  #16
        sqxtun          \out\().8b,  v20.8h
        sqxtun2         \out\().16b, v21.8h
.endm

.macro  yuv444_dot4 dot, out, coef, ilo, ihi, round
        mov             v4.16b,  \round\().16b
        movi            v16.4s,  #0
        \dot            v4.4s,   v0.16b, \coef\().4b[\ilo]
        \dot            v16.4s,  v0.16b, \coef\().4b[\ihi]
        mla             v4.4s,   v16.4s, v24.4s
        sqshrn          v20.4h,  v4.4s,  #16
        sqxtun          \out\().8b,  v20.8h
.endm

.macro  yuv444_center reg, mask
    .ifnb \mask
        eor             \reg\().16b, \reg\().16b, \mask\().16b
    .endif
.endm

.macro  argb_to_y_dot name, dot, yround, mask
function ff_argb_to_y_\name, export=1
        movrel          x3,  yuv444_dot_coeffs
        ldr             q22, [x3]
        mov             w3,  #260
        dup             v24.4s,  w3
        mov             w3,  #(\yround&0xffff)
        movk            w3,  #(\yround>>16), lsl #16
        dup             v25.4s,  w3
    .ifnb \mask
        movi            v27.16b, #0x80
    .endif
        cmp             w2,  #16
        b.lt            2f
1:      ld1             {v0.16b-v3.16b}, [x1], #64
        sub             w2,  w2,  #16
        yuv444_center   v0,  \mask
        yuv444_center   v1,  \mask
        yuv444_center   v2,  \mask
        yuv444_center   v3,  \mask
        yuv444_dot16    \dot, v28, v22, 0, 1, v25
        st1             {v28.16b}, [x0], #16
        cmp             w2,  #16
        b.ge            1b
2:      cmp             w2,  #4
        b.lt            3f
        ldr             q0,  [x1], #16
        sub             w2,  w2,  #4
        yuv444_center   v0,  \mask
        yuv444_dot4     \dot, v28, v22, 0, 1, v25
        st1             {v28.s}[0], [x0], #4
        cmp             w2,  #4
        b.ge            2b
3:      cbz             w2,  9f
4:      ld1             {v0.s}[0], [x1], #4
        subs            w2,  w2,  #1
        yuv444_center   v0,  \mask
        yuv444_dot4     \dot, v28, v22, 0, 1, v25
        st1             {v28.b}[0], [x0], #1
        b.gt            4b
9:      ret
endfunc
.endm

.macro  argb_to_yuv444_dot name, dot, yround, mask
function ff_argb_to_yuv444_\name, export=1
        movrel          x5,  yuv444_dot_coeffs
        ldr             q22, [x5]
        ldr             d23, [x5, #16]
        mov             w5,  #260
        dup             v24.4s,  w5
        mov             w5,  #(\yround&0xffff)
        movk            w5,  #(\yround>>16), lsl #16
        dup             v25.4s,  w5
        mov             w5,  #0x8000
        movk            w5,  #0x80, lsl #16
        dup             v26.4s,  w5
    .ifnb \mask
        movi            v27.16b, #0x80
    .endif
        cmp             w4,  #16
        b.lt            2f
1:      ld1             {v0.16b-v3.16b}, [x3], #64
        sub             w4,  w4,  #16
        yuv444_center   v0,  \mask
        yuv444_center   v1,  \mask
        yuv444_center   v2,  \mask
        yuv444_center   v3,  \mask
        yuv444_dot16    \dot, v28, v22, 0, 1, v25
        yuv444_dot16    \dot, v29, v22, 2, 3, v26
        yuv444_dot16    \dot, v30, v23, 0, 1, v26
        st1             {v28.16b}, [x0], #16
        st1             {v29.16b}, [x1], #16
        st1             {v30.16b}, [x2], #16
        cmp             w4,  #16
        b.ge            1b
2:      cmp             w4,  #4
        b.lt            3f
        ldr             q0,  [x3], #16
        sub             w4,  w4,  #4
        yuv444_center   v0,  \mask
        yuv444_dot4     \dot, v28, v22, 0, 1, v25
        yuv444_dot4     \dot, v29, v22, 2, 3, v26
        yuv444_dot4     \dot, v30, v23, 0, 1, v26
        st1             {v28.s}[0], [x0], #4
        st1             {v29.s}[0], [x1], #4
        st1             {v30.s}[0], [x2], #4
        cmp             w4,  #4
        b.ge            2b
3:      cbz             w4,  9f
4:      ld1             {v0.s}[0], [x3], #4
        subs            w4,  w4,  #1
        yuv444_center   v0,  \mask
        yuv444_dot4     \dot, v28, v22, 0, 1, v25
        yuv444_dot4     \dot, v29, v22, 2, 3, v26
        yuv444_dot4     \dot, v30, v23, 0, 1, v26
        st1             {v28.b}[0], [x0], #1
        st1             {v29.b}[0], [x1], #1
        st1             {v30.b}[0], [x2], #1
        b.gt            4b
9:      ret
endfunc
.endm

ENABLE_DOTPROD
argb_to_y_dot neon_dotprod, sdot, 8290048, v27
argb_to_yuv444_dot neon_dotprod, sdot, 8290048, v27
DISABLE_DOTPROD

#if HAVE_I8MM
ENABLE_I8MM
argb_to_y_dot neon_i8mm, usdot, 1081344
argb_to_yuv444_dot neon_i8mm, usdot, 1081344
DISABLE_I8MM
#endif

#endif // HAVE_DOTPROD
