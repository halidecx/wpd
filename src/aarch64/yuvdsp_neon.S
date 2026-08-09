
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

function ff_dispatch_alpha_neon, export=1
        cmp             w2,  #16
        b.lt            2f
1:      ldr             q0,  [x1], #16
        ld4             {v1.16b-v4.16b}, [x0]
        sub             w2,  w2,  #16
        mov             v1.16b,  v2.16b
        mov             v2.16b,  v3.16b
        mov             v3.16b,  v4.16b
        st4             {v0.16b-v3.16b}, [x0], #64
        cmp             w2,  #16
        b.ge            1b
2:      cbz             w2,  9f
3:      ldrb            w3,  [x1], #1
        subs            w2,  w2,  #1
        strb            w3,  [x0], #4
        b.gt            3b
9:      ret
endfunc

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
