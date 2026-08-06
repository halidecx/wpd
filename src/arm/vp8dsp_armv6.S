
#include "asm.S"


function ff_vp8_luma_dc_wht_armv6, export=1
        push            {r4-r10, lr}

        ldm             r1,  {r2-r9}
        mov             r10, #0
        mov             lr,  #0
        uadd16          r12, r2,  r8
        usub16          r2,  r2,  r8
        stm             r1!, {r10, lr}
        uadd16          r8,  r4,  r6
        usub16          r4,  r4,  r6
        stm             r1!, {r10, lr}
        uadd16          r6,  r12, r8
        usub16          r12, r12, r8
        stm             r1!, {r10, lr}
        uadd16          r8,  r2,  r4
        usub16          r2,  r2,  r4
        stm             r1!, {r10, lr}

        uadd16          lr,  r3,  r9
        usub16          r3,  r3,  r9
        uadd16          r9,  r5,  r7
        usub16          r5,  r5,  r7

        uadd16          r7,  lr,  r9
        usub16          lr,  lr,  r9
        uadd16          r9,  r3,  r5
        usub16          r3,  r3,  r5

        mov             r1,  #3
        orr             r1,  r1,  #0x30000

        pkhbt           r4,  r6,  r8,  lsl #16
        pkhtb           r6,  r8,  r6,  asr #16
        pkhbt           r5,  r12, r2,  lsl #16
        pkhtb           r12, r2,  r12, asr #16
        pkhbt           r8,  r7,  r9,  lsl #16
        uadd16          r4,  r4,  r1
        uadd16          r5,  r5,  r1
        pkhtb           r7,  r9,  r7,  asr #16
        pkhbt           r2,  lr,  r3,  lsl #16
        pkhtb           lr,  r3,  lr,  asr #16

        uadd16          r9,  r4,  r7
        uadd16          r3,  r5,  lr
        usub16          r4,  r4,  r7
        usub16          r5,  r5,  lr
        uadd16          r7,  r6,  r8
        uadd16          lr,  r12, r2
        usub16          r6,  r6,  r8
        usub16          r12, r12, r2

        uadd16          r8,  r9,  r7
        uadd16          r2,  r3,  lr
        usub16          r9,  r9,  r7
        usub16          r3,  r3,  lr
        uadd16          r7,  r4,  r6
        uadd16          lr,  r5,  r12
        usub16          r4,  r4,  r6
        usub16          r5,  r5,  r12

#if HAVE_ARMV6T2_EXTERNAL
        sbfx            r6,  r8,  #3,  #13
        sbfx            r12, r7,  #3,  #13
        sbfx            r1,  r9,  #3,  #13
        sbfx            r10, r4,  #3,  #13
#else
        sxth            r6,  r8
        sxth            r12, r7
        sxth            r1,  r9
        sxth            r10, r4
        asr             r6,  #3
        asr             r12, #3
        asr             r1,  #3
        asr             r10, #3
#endif

        strh            r6,  [r0], #32
        asr             r8,  r8,  #19
        strh            r12, [r0], #32
        asr             r7,  r7,  #19
        strh            r1,  [r0], #32
        asr             r9,  r9,  #19
        strh            r10, [r0], #32
        asr             r4,  r4,  #19
        strh            r8,  [r0], #32
        asr             r6,  r2,  #19
        strh            r7,  [r0], #32
        asr             r12, lr,  #19
        strh            r9,  [r0], #32
        asr             r1,  r3,  #19
        strh            r4,  [r0], #32
        asr             r10, r5,  #19

#if HAVE_ARMV6T2_EXTERNAL
        sbfx            r2,  r2,  #3,  #13
        sbfx            lr,  lr,  #3,  #13
        sbfx            r3,  r3,  #3,  #13
        sbfx            r5,  r5,  #3,  #13
#else
        sxth            r2,  r2
        sxth            lr,  lr
        sxth            r3,  r3
        sxth            r5,  r5
        asr             r2,  #3
        asr             lr,  #3
        asr             r3,  #3
        asr             r5,  #3
#endif

        strh            r2,  [r0], #32
        strh            lr,  [r0], #32
        strh            r3,  [r0], #32
        strh            r5,  [r0], #32
        strh            r6,  [r0], #32
        strh            r12, [r0], #32
        strh            r1,  [r0], #32
        strh            r10, [r0], #32

        pop             {r4-r10, pc}
endfunc

function ff_vp8_luma_dc_wht_dc_armv6, export=1
        ldrsh           r2,  [r1]
        mov             r3,  #0
        add             r2,  r2,  #3
        strh            r3,  [r1]
        asr             r2,  r2,  #3
    .rept 16
        strh            r2,  [r0], #32
    .endr
        bx              lr
endfunc

function ff_vp8_idct_add_armv6, export=1
        push            {r4-r12, lr}
        sub             sp,  sp,  #32

        movw            r3,  #20091
        movw            r4,  #35468
        mov             r5,  sp
1:
        ldr             r6,  [r1, #8]
        ldr             lr,  [r1, #16]
        ldr             r12, [r1, #24]

        smulwt          r9,  r3,  r6
        smulwb          r7,  r3,  r6
        smulwt          r10, r4,  r6
        smulwb          r8,  r4,  r6
        pkhbt           r7,  r7,  r9,  lsl #16
        smulwt          r11, r3,  r12
        pkhbt           r8,  r8,  r10, lsl #16
        uadd16          r6,  r6,  r7
        smulwb          r9,  r3,  r12
        smulwt          r7,  r4,  r12
        smulwb          r10, r4,  r12

        pkhbt           r9,  r9,  r11, lsl #16
        ldr             r11, [r1]
        pkhbt           r10, r10,  r7, lsl #16
        uadd16          r7,  r12, r9
        uadd16          r6,  r6,  r10
        uadd16          r10, r11, lr
        usub16          r7,  r8,  r7
        usub16          r8,  r11, lr
        uadd16          r9,  r10, r6
        usub16          r10, r10, r6
        uadd16          r6,  r8,  r7
        usub16          r7,  r8,  r7
        mov             r8,  #0
        cmp             sp,  r5
        str             r6,  [r5, #8]
        str             r7,  [r5, #16]
        str             r10, [r5, #24]
        str             r9,  [r5], #4
        str             r8,  [r1, #8]
        str             r8,  [r1, #16]
        str             r8,  [r1, #24]
        str             r8,  [r1], #4
        beq             1b

        mov             r5,  #2
2:
        pop             {r1, r6, r12, lr}
        smulwt          r9,  r3,  r12
        smulwt          r7,  r3,  r1
        smulwt          r10, r4,  r12
        smulwt          r8,  r4,  r1
        pkhbt           r11, r1,  r12, lsl #16
        pkhtb           r1,  r12, r1,  asr #16
        pkhbt           r7,  r7,  r9,  lsl #16
        pkhbt           r8,  r8,  r10, lsl #16
        pkhbt           r9,  r6,  lr,  lsl #16
        pkhtb           r12, lr,  r6,  asr #16
        uadd16          r1,  r7,  r1
        uadd16          r10, r11, r9
        usub16          r9,  r11, r9
        smulwt          r7,  r3,  r12
        smulwb          lr,  r3,  r12
        smulwt          r11, r4,  r12
        smulwb          r6,  r4,  r12
        subs            r5,  r5,  #1
        pkhbt           r7,  lr,  r7,  lsl #16
        pkhbt           r11, r6,  r11, lsl #16
        mov             r6,  #0x4
        orr             r6,  r6,  #0x40000
        uadd16          r12, r7,  r12
        uadd16          r10, r10, r6
        uadd16          r9,  r9,  r6
        usub16          lr,  r8,  r12
        uadd16          r12, r11, r1
        usub16          r1,  r9,  lr
        uadd16          r7,  r10, r12
        usub16          r12, r10, r12
        uadd16          r10, r9,  lr

        asr             lr,  r1,  #3
        asr             r9,  r12, #3
        pkhtb           r8,  lr,  r7,  asr #19
        pkhtb           r11, r9,  r10, asr #19
        ldr             lr,  [r0]
        sxth            r12, r12
        ldr             r9,  [r0, r2]
        sxth            r1,  r1
#if HAVE_ARMV6T2_EXTERNAL
        sbfx            r7,  r7,  #3,  #13
        sbfx            r10, r10, #3,  #13
#else
        sxth            r7,  r7
        sxth            r10, r10
        asr             r7,  #3
        asr             r10, #3
#endif
        pkhbt           r7,  r7,  r1,  lsl #13
        pkhbt           r10, r10, r12, lsl #13

        uxtab16         r7,  r7,  lr
        uxtab16         r10, r10, lr,  ror #8
        uxtab16         r8,  r8,  r9
        uxtab16         r11, r11, r9,  ror #8
        usat16          r7,  #8,  r7
        usat16          r10, #8,  r10
        usat16          r8,  #8,  r8
        usat16          r11, #8,  r11
        orr             r7,  r7,  r10, lsl #8
        orr             r8,  r8,  r11, lsl #8
        str             r8,  [r0, r2]
        str_post        r7,  r0,  r2,  lsl #1

        bne             2b

        pop             {r4-r12, pc}
endfunc

function ff_vp8_idct_dc_add_armv6, export=1
        push            {r4-r6, lr}
        add             r6,  r0,  r2,  lsl #1
        ldrsh           r3,  [r1]
        mov             r4,  #0
        add             r3,  r3,  #4
        strh            r4,  [r1], #32
        asr             r3,  #3
        ldr             r5,  [r0]
        ldr             r4,  [r0, r2]
        pkhbt           r3,  r3,  r3,  lsl #16
        uxtab16         lr,  r3,  r5
        uxtab16         r5,  r3,  r5,  ror #8
        uxtab16         r12, r3,  r4
        uxtab16         r4,  r3,  r4,  ror #8
        usat16          lr,  #8,  lr
        usat16          r5,  #8,  r5
        usat16          r12, #8,  r12
        usat16          r4,  #8,  r4
        orr             lr,  lr,  r5,  lsl #8
        ldr             r5,  [r6]
        orr             r12, r12, r4,  lsl #8
        ldr             r4,  [r6, r2]
        str             lr,  [r0]
        uxtab16         lr,  r3,  r5
        str             r12, [r0, r2]
        uxtab16         r5,  r3,  r5,  ror #8
        uxtab16         r12, r3,  r4
        uxtab16         r4,  r3,  r4,  ror #8
        usat16          lr,  #8,  lr
        usat16          r5,  #8,  r5
        usat16          r12, #8,  r12
        usat16          r4,  #8,  r4
        orr             lr,  lr,  r5,  lsl #8
        orr             r12, r12, r4,  lsl #8
        str             lr,  [r6]
        str             r12, [r6, r2]
        pop             {r4-r6, pc}
endfunc

function ff_vp8_idct_dc_add4uv_armv6, export=1
        push            {r4, lr}

        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  r2,  lsl #2
        sub             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)

        pop             {r4, pc}
endfunc

function ff_vp8_idct_dc_add4y_armv6, export=1
        push            {r4, lr}

        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)
        add             r0,  r0,  #4
        bl              X(ff_vp8_idct_dc_add_armv6)

        pop             {r4, pc}
endfunc


.macro  transpose       o3,  o2,  o1,  o0,  i0,  i1,  i2,  i3
        uxtb16          \o1, \i1
        uxtb16          \o0, \i0
        uxtb16          \o3, \i3
        uxtb16          \o2, \i2
        orr             \o1, \o0, \o1, lsl #8
        orr             \o3, \o2, \o3, lsl #8

        uxtb16          \i1, \i1, ror #8
        uxtb16          \i3, \i3, ror #8
        uxtb16          \i0, \i0, ror #8
        uxtb16          \i2, \i2, ror #8
        orr             \i0, \i0, \i1, lsl #8
        orr             \i2, \i2, \i3, lsl #8

        pkhtb           \o2, \o3, \o1, asr #16
        pkhbt           \o0, \o1, \o3, lsl #16
        pkhtb           \o3, \i2, \i0, asr #16
        pkhbt           \o1, \i0, \i2, lsl #16
.endm

.macro  simple_filter
        uqsub8          r7,  r3,  r6
        uqsub8          r8,  r6,  r3
        uqsub8          r10, r4,  r5
        uqsub8          r9,  r5,  r4
        orr             r7,  r7,  r8
        orr             r9,  r9,  r10
        uhadd8          r7,  r7,  lr
        uqadd8          r9,  r9,  r9
        uqadd8          r7,  r7,  r9
        mvn             r8,  #0
        usub8           r10, r12, r7
        sel             r10, r8,  lr
        cmp             r10, #0
        beq             2f

        eor             r3,  r3,  r2
        eor             r6,  r6,  r2
        eor             r4,  r4,  r2
        eor             r5,  r5,  r2

        qsub8           r3,  r3,  r6
        qsub8           r6,  r5,  r4
        qadd8           r3,  r3,  r6
        lsr             r7,  r2,  #5
        qadd8           r3,  r3,  r6
        sub             r9,  r7,  r2,  lsr #7
        qadd8           r3,  r3,  r6
        and             r3,  r3,  r10

        qadd8           r9,  r3,  r9
        qadd8           r3,  r3,  r7

        shadd8          r9,  r9,  lr
        shadd8          r3,  r3,  lr
        shadd8          r9,  r9,  lr
        shadd8          r3,  r3,  lr
        shadd8          r9,  r9,  lr
        shadd8          r3,  r3,  lr

        qadd8           r4,  r4,  r9
        qsub8           r5,  r5,  r3
        eor             r4,  r4,  r2
        eor             r5,  r5,  r2
.endm

function ff_vp8_v_loop_filter16_simple_armv6, export=1
        push            {r4-r11, lr}

        orr             r2,  r2,  r2,  lsl #16
        mov             r11, #4
        mov             lr,  #0
        orr             r12, r2,  r2,  lsl #8
        mov32           r2,  0x80808080
1:
        ldr_nreg        r3,  r0,  r1,  lsl #1
        ldr_nreg        r4,  r0,  r1
        ldr             r5,  [r0]
        ldr             r6,  [r0, r1]
        simple_filter
T       sub             r7,  r0,  r1
        str             r5,  [r0]
A       str             r4,  [r0, -r1]
T       str             r4,  [r7]
2:
        subs            r11, r11, #1
        add             r0,  r0,  #4
        bne             1b

        pop             {r4-r11, pc}
endfunc

.macro  filter_mask_p
        uqsub8          r6,  r9,  r10
        uqsub8          r7,  r10, r9
        uqsub8          r8,  r10, r11
        uqsub8          r10, r11, r10
        orr             r6,  r6,  r7
        orr             r8,  r8,  r10
        uqsub8          lr,  r6,  r2
        uqsub8          r8,  r8,  r2
        uqsub8          r6,  r11, r12
        orr             lr,  lr,  r8
        uqsub8          r7,  r12, r11
        orr             r6,  r6,  r7
        uqsub8          r7,  r6,  r2
        uqsub8          r8,  r6,  r3
        orr             lr,  lr,  r7
.endm

.macro filter_mask_pq
        uqsub8          r6,  r11, r10
        uqsub8          r7,  r10, r11
        uqsub8          r11, r12, r9
        uqsub8          r12, r9,  r12
        orr             r6,  r6,  r7
        orr             r12, r11, r12
        mov32           r7,  0x7f7f7f7f
        uqadd8          r12, r12, r12
        and             r6,  r7,  r6,  lsr #1
        uqadd8          r12, r12, r6
.endm

.macro  filter_mask_v
        filter_mask_p

        ldr             r10, [r0, r1]
        ldr_post        r9,  r0,  r1,  lsl #1

        filter_mask_pq

        ldr             r11, [r0]

        uqsub8          r7,  r9,  r10
        uqsub8          r6,  r10, r9
        uqsub8          r12, r12, r4
        uqsub8          r9,  r11, r10
        uqsub8          r10, r10, r11
        orr             lr,  lr,  r12
        ldr             r12, [r0, r1]
        orr             r6,  r7,  r6
        orr             r10, r9,  r10
        uqsub8          r9,  r12, r11
        uqsub8          r11, r11, r12
        uqsub8          r7,  r6,  r2
        uqsub8          r10, r10, r2
        uqsub8          r6,  r6,  r3
        orr             r9,  r9,  r11
        orr             lr,  lr,  r7
        orr             lr,  lr,  r10
        uqsub8          r9,  r9,  r2
        orr             lr,  lr,  r9

        mov             r12, #0
        usub8           lr,  r12, lr
        mvn             r11, #0
        sel             lr,  r11, r12
        sub             r0,  r0,  r1,  lsl #1
.endm

.macro  filter_mask_h
        transpose       r12, r11, r10, r9,  r6,  r7,  r8,  lr

        filter_mask_p

        stm             sp,  {r8, r11, r12, lr}
        sub             r0,  r0,  r1,  lsl #2
        add             r0,  r0,  #4

        ldr             r7,  [r0, r1]
        ldr_post        r6,  r0,  r1,  lsl #1
        ldr             lr,  [r0, r1]
        ldr             r8,  [r0]

        transpose       r12, r11, r10, r9,  r6,  r7,  r8,  lr

        uqsub8          r8,  r12, r11
        uqsub8          lr,  r11, r12
        uqsub8          r7,  r9,  r10
        uqsub8          r6,  r10, r9
        uqsub8          r12, r11, r10
        uqsub8          r11, r10, r11
        orr             r8,  r8,  lr
        orr             r6,  r7,  r6
        orr             r11, r12, r11
        ldr             lr,  [sp, #12]
        uqsub8          r8,  r8,  r2
        uqsub8          r7,  r6,  r2
        uqsub8          r11, r11, r2
        orr             lr,  lr,  r8
        uqsub8          r8,  r6,  r3
        orr             lr,  lr,  r7
        ldr             r12, [sp, #8]
        orr             lr,  lr,  r11

        ldr             r11, [sp, #4]

        filter_mask_pq

        mov             r10, #0
        uqsub8          r12, r12, r4
        mvn             r11, #0
        orr             lr,  lr,  r12
        usub8           lr,  r10, lr
        sel             lr,  r11, r10
.endm

.macro  filter          inner
        mov32           r12, 0x80808080
        eor             r11, r7,  r12
        eor             r8,  r8,  r12
        eor             r9,  r9,  r12
        eor             r10, r10, r12

        stm             sp,  {r8-r11}

        qsub8           r7,  r11, r10
        qsub8           r8,  r9,  r8
    .if \inner
        and             r7,  r7,  r6
    .endif
        qadd8           r7,  r7,  r8
        lsr             r10, r12, #5
        qadd8           r7,  r7,  r8
        sub             r9,  r10, r12, lsr #7
        qadd8           r7,  r7,  r8

        and             r7,  r7,  lr
    .if !\inner
        mov             r12, r7
        and             r7,  r7,  r6
    .endif
        qadd8           lr,  r7,  r9
        qadd8           r7,  r7,  r10

        mov             r9,  #0
        shadd8          lr,  lr,  r9
        shadd8          r7,  r7,  r9
        shadd8          lr,  lr,  r9
        shadd8          r7,  r7,  r9
        shadd8          lr,  lr,  r9
        shadd8          r7,  r7,  r9
.endm

.macro  filter_v        inner
        orr             r10, r6,  r8
        ldr_nreg        r7,  r0,  r1,  lsl #1
        usub8           r10, r12, r10
        ldr_nreg        r8,  r0,  r1
        sel             r6,  r12, r11
        ldr             r9,  [r0]
        ldr             r10, [r0, r1]
        filter          \inner
.endm

.macro  filter_h        inner
        orr             r9,  r6,  r8
        usub8           r9,  r12, r9
        sel             r6,  r12, r11

        stm             sp,  {r6, lr}

        ldr_nreg        r12, r0,  r1,  lsl #1
        ldr_nreg        r11, r0,  r1
        ldr             r6,  [r0]
        ldr             lr,  [r0, r1]

        transpose       r10, r9,  r8,  r7,  r12, r11, r6,  lr

        ldm             sp,  {r6, lr}
        filter          \inner
.endm

.macro  filter_inner
        ldm             sp,  {r8, r9}
        lsr             r10, r10, #2
        qadd8           r8,  r8,  lr
        mov             lr,  #0
        qsub8           r9,  r9,  r7
        sadd8           r7,  r7,  r10
        ldr             r10, [sp, #8]
        shadd8          r7,  r7,  lr
        eor             r8,  r8,  r12
        bic             r7,  r7,  r6
        qadd8           r11, r11, r7
        eor             r9,  r9,  r12
        qsub8           r10, r10, r7
        eor             r11, r11, r12
        eor             r10, r10, r12
.endm

.macro  filter_x        c0
        mov             lr,  \c0
        mov             r7,  #63

        sxtb16          r6,  r12
        sxtb16          r10, r12, ror #8
        smlabb          r8,  r6,  lr,  r7
        smlatb          r6,  r6,  lr,  r7
        smlabb          r7,  r10, lr,  r7
        smultb          r10, r10, lr
        ssat            r8,  #8,  r8,  asr #7
        ssat            r6,  #8,  r6,  asr #7
        add             r10, r10, #63
        ssat            r7,  #8,  r7,  asr #7
        ssat            r10, #8,  r10, asr #7

        pkhbt           r6,  r8,  r6,  lsl #16
        pkhbt           r10, r7,  r10, lsl #16
        uxtb16          r6,  r6
        uxtb16          r10, r10

        mov32           lr,  0x80808080

        orr             r10, r6,  r10, lsl #8
        qsub8           r8,  r9,  r10
        qadd8           r10, r11, r10
        eor             r8,  r8,  lr
        eor             r10, r10, lr
.endm

.macro  filter_1
        ldm             sp,  {r8, r9}
        qadd8           r11, r8,  lr
        qsub8           r9,  r9,  r7
        bic             r12, r12, r6
        filter_x        #27
.endm

.macro  filter_2
        ldr             r9,   [sp, #8]
        ldr             r11,  [sp, #12]
        filter_x        #18
.endm

.macro  filter_3
        eor             r9,  r9,  lr
        eor             r11, r11, lr
        filter_x        #9
.endm

function vp8_v_loop_filter_inner_armv6
        mov             r5,  #4
        sub             sp,  sp,  #16

        orr             r2,  r2,  r2,  lsl #16
        orr             r3,  r3,  r3,  lsl #16
        orr             r6,  r6,  r6,  lsl #16
        orr             r4,  r2,  r2,  lsl #8
        orr             r2,  r3,  r3,  lsl #8
        orr             r3,  r6,  r6,  lsl #8
1:
        sub             r0,  r0,  r1,  lsl #2
        ldr             r10, [r0, r1]
        ldr_post        r9,  r0,  r1,  lsl #1
        ldr             r12, [r0, r1]
        ldr_post        r11, r0,  r1,  lsl #1

        filter_mask_v
        cmp             lr,  #0
        beq             2f
        filter_v        inner=1
        filter_inner

A       str             r11, [r0, -r1, lsl #1]
A       str             r8,  [r0, -r1]
T       sub             r0,  r0,  r1,  lsl #1
T       str             r8,  [r0, r1]
T       str_post        r11, r0,  r1,  lsl #1
        str             r9,  [r0]
        str             r10, [r0, r1]
2:
        add             r0,  r0,  #4
        cmp             r5,  #3
        it              eq
        ldreq           r0,  [sp, #16]
        subs            r5,  r5,  #1
        bne             1b

        add             sp,  sp,  #16
        pop             {r0, r4-r11, pc}
endfunc

function ff_vp8_v_loop_filter16_inner_armv6, export=1
        push            {r4-r11, lr}
        add             r12, r0,  #8
        push            {r12}
        ldr             r6,  [sp, #40]
        orr             r2,  r2,  r2,  lsl #16
        b               vp8_v_loop_filter_inner_armv6
endfunc

function ff_vp8_v_loop_filter8uv_inner_armv6, export=1
        push            {r1, r4-r11, lr}
        mov             r1,  r2
        orr             r2,  r3,  r3,  lsl #16
        ldr             r3,  [sp, #40]
        ldr             r6,  [sp, #44]
        b               vp8_v_loop_filter_inner_armv6
endfunc

function vp8_v_loop_filter_armv6
        mov             r5,  #4
        sub             sp,  sp,  #16

        orr             r3,  r3,  r3,  lsl #16
        orr             r6,  r6,  r6,  lsl #16
        orr             r4,  r2,  r2,  lsl #8
        orr             r2,  r3,  r3,  lsl #8
        orr             r3,  r6,  r6,  lsl #8
1:
        sub             r0,  r0,  r1,  lsl #2
        ldr             r10, [r0, r1]
        ldr_post        r9,  r0,  r1,  lsl #1
        ldr             r12, [r0, r1]
        ldr_post        r11, r0,  r1,  lsl #1

        filter_mask_v
        cmp             lr,  #0
        beq             2f

        filter_v        inner=0
        filter_1

        str             r8,  [r0]
A       str             r10, [r0, -r1]
T       sub             r0,  r0,  r1,  lsl #1
T       str             r10, [r0, r1]

        filter_2

A       str             r10, [r0, -r1, lsl #1]
T       str_post        r10, r0,  r1,  lsl #1
        str             r8,  [r0, r1]

        ldr             r9,  [r0, r1,  lsl #1]
        add             r0,  r0,  r1
A       ldr             r11, [r0, -r1, lsl #2]
T       ldr_dpre        r11, r0,  r1,  lsl #2

        filter_3

A       str             r10, [r0, -r1, lsl #2]
T       str_post        r10, r0,  r1,  lsl #2
        str             r8,  [r0, r1]
        sub             r0,  r0,  r1
2:
        add             r0,  r0,  #4
        cmp             r5,  #3
        it              eq
        ldreq           r0,  [sp, #16]
        subs            r5,  r5,  #1
        bne             1b

        add             sp,  sp,  #16
        pop             {r0, r4-r11, pc}
endfunc

function ff_vp8_v_loop_filter16_armv6, export=1
        push            {r4-r11, lr}
        add             r12, r0,  #8
        push            {r12}
        ldr             r6,  [sp, #40]
        orr             r2,  r2,  r2,  lsl #16
        b               vp8_v_loop_filter_armv6
endfunc

function ff_vp8_v_loop_filter8uv_armv6, export=1
        push            {r1, r4-r11, lr}
        mov             r1,  r2
        orr             r2,  r3,  r3,  lsl #16
        ldr             r3,  [sp, #40]
        ldr             r6,  [sp, #44]
        b               vp8_v_loop_filter_armv6
endfunc

function ff_vp8_h_loop_filter16_simple_armv6, export=1
        push            {r4-r11, lr}
        orr             r12, r2,  r2,  lsl #16
        mov32           r2,  0x80808080
        orr             r12, r12, r12, lsl #8

        mov             lr,  #0
        mov             r11, #4
1:
        sub             r0,  r0,  #2
        ldr             r8,  [r0, r1]
        ldr_post        r7,  r0,  r1,  lsl #1
        ldr             r10, [r0, r1]
        ldr_post        r9,  r0,  r1,  lsl #1
        add             r0,  r0,  #2
        transpose       r6,  r5,  r4,  r3,  r7,  r8,  r9,  r10
        simple_filter
        sub             r0,  r0,  r1,  lsl #2
        sub             r0,  r0,  #1

        uxtb16          r6,  r4
        uxtb16          r8,  r5
        uxtb16          r7,  r4,  ror #8
        uxtb16          r9,  r5,  ror #8
        orr             r6,  r6,  r8,  lsl #8
        orr             r7,  r7,  r9,  lsl #8
        lsr             r4,  r6,  #16
        lsr             r5,  r7,  #16

        strh_post       r6,  r0,  r1
        strh_post       r7,  r0,  r1
        strh_post       r4,  r0,  r1
        strh_post       r5,  r0,  r1
        add             r0,  r0,  #1
2:
        subs            r11, r11, #1
        bne             1b

        pop             {r4-r11, pc}
endfunc

function vp8_h_loop_filter_inner_armv6
        mov             r5,  #4
        sub             sp,  sp,  #16

        orr             r3,  r3,  r3,  lsl #16
        orr             r9,  r9,  r9,  lsl #16
        orr             r4,  r2,  r2,  lsl #8
        orr             r2,  r3,  r3,  lsl #8
        orr             r3,  r9,  r9,  lsl #8
        sub             r0,  r0,  #4
1:
        ldr             r7,  [r0, r1]
        ldr_post        r6,  r0,  r1,  lsl #1
        ldr             lr,  [r0, r1]
        ldr_post        r8,  r0,  r1,  lsl #1

        filter_mask_h

        cmp             lr,  #0
        sub             r0,  r0,  #2
        beq             2f

        ldr             r6,  [sp]

        filter_h        inner=1
        filter_inner

        transpose       lr,  r12, r7,  r6,  r11, r8,  r9,  r10

A       str             r6,  [r0, -r1, lsl #1]
A       str             r7,  [r0, -r1]
T       sub             r0,  r0,  r1,  lsl #1
T       str             r7,  [r0, r1]
T       str_post        r6,  r0,  r1,  lsl #1
        str             r12, [r0]
        str             lr,  [r0, r1]
2:
        sub             r0,  r0,  #2
        add             r0,  r0,  r1,  lsl #1
        cmp             r5,  #3
        it              eq
        ldreq           r0,  [sp, #16]
        subs            r5,  r5,  #1
        bne             1b

        add             sp, sp, #16
        pop             {r0, r4-r11, pc}
endfunc

function ff_vp8_h_loop_filter16_inner_armv6, export=1
        push            {r4-r11, lr}
        add             r12, r0,  r1,  lsl #3
        sub             r12, r12, #4
        push            {r12}
        ldr             r9,  [sp, #40]
        orr             r2,  r2,  r2,  lsl #16
        b               vp8_h_loop_filter_inner_armv6
endfunc

function ff_vp8_h_loop_filter8uv_inner_armv6, export=1
        sub             r1,  r1,  #4
        push            {r1, r4-r11, lr}
        mov             r1,  r2
        orr             r2,  r3,  r3,  lsl #16
        ldr             r3,  [sp, #40]
        ldr             r9,  [sp, #44]
        b               vp8_h_loop_filter_inner_armv6
endfunc

function vp8_h_loop_filter_armv6
        mov             r5,  #4
        sub             sp,  sp,  #16

        orr             r3,  r3,  r3,  lsl #16
        orr             r9,  r9,  r9,  lsl #16
        orr             r4,  r2,  r2,  lsl #8
        orr             r2,  r3,  r3,  lsl #8
        orr             r3,  r9,  r9,  lsl #8
1:
        sub             r0,  r0,  #4
        ldr             r7,  [r0, r1]
        ldr_post        r6,  r0,  r1,  lsl #1
        ldr             lr,  [r0, r1]
        ldr_post        r8,  r0,  r1,  lsl #1

        filter_mask_h
        cmp             lr,  #0
        it              eq
        addeq           r0,  r0,  r1,  lsl #1
        beq             2f

        ldr             r6,  [sp]
        sub             r0,  r0,  #2

        filter_h        inner=0
        filter_1

        sub             r0,  r0,  r1,  lsl #1
        uxtb16          r6,  r10
        uxtb16          r7,  r8
        uxtb16          r10, r10, ror #8
        uxtb16          r8,  r8,  ror #8
        orr             r6,  r6,  r7,  lsl #8
        orr             r10, r10, r8,  lsl #8
        lsr             r7,  r6,  #16
        lsr             r8,  r10, #16

        add             r0,  r0,  #1
        strh_post       r6,  r0,  r1
        strh_post       r10, r0,  r1
        strh_post       r7,  r0,  r1
        strh_post       r8,  r0,  r1

        filter_2

        sub             r0,  r0,  r1,  lsl #2
        add             r0,  r0,  #3

        ldrb            r11, [r0, #-5]
        strb            r10, [r0, #-4]
        strb            r8,  [r0, #-1]
        ldrb_post       r9,  r0,  r1

        lsr             r10, r10, #8
        lsr             r8,  r8,  #8

        ldrb            r6,  [r0, #-5]
        strb            r10, [r0, #-4]
        strb            r8,  [r0, #-1]
        ldrb_post       r7,  r0,  r1

        lsr             r10, r10, #8
        lsr             r8,  r8,  #8
        orr             r11, r11, r6,  lsl #8
        orr             r9,  r9,  r7,  lsl #8

        ldrb            r6,  [r0, #-5]
        strb            r10, [r0, #-4]
        strb            r8,  [r0, #-1]
        ldrb_post       r7,  r0,  r1

        lsr             r10, r10, #8
        lsr             r8,  r8,  #8
        orr             r11, r11, r6,  lsl #16
        orr             r9,  r9,  r7,  lsl #16

        ldrb            r6,  [r0, #-5]
        strb            r10, [r0, #-4]
        strb            r8,  [r0, #-1]
        ldrb_post       r7,  r0,  r1
        orr             r11, r11, r6,  lsl #24
        orr             r9,  r9,  r7,  lsl #24

        filter_3

        sub             r0,  r0,  r1,  lsl #2
        strb            r10, [r0, #-5]
        strb_post       r8,  r0,  r1
        lsr             r10, r10, #8
        lsr             r8,  r8,  #8
        strb            r10, [r0, #-5]
        strb_post       r8,  r0,  r1
        lsr             r10, r10, #8
        lsr             r8,  r8,  #8
        strb            r10, [r0, #-5]
        strb_post       r8,  r0,  r1
        lsr             r10, r10, #8
        lsr             r8,  r8,  #8
        strb            r10, [r0, #-5]
        strb_post       r8,  r0,  r1

        sub             r0,  r0,  #2
2:
        cmp             r5,  #3
        it              eq
        ldreq           r0,  [sp, #16]
        subs            r5,  r5,  #1
        bne             1b

        add             sp,  sp,  #16
        pop             {r0, r4-r11, pc}
endfunc

function ff_vp8_h_loop_filter16_armv6, export=1
        push            {r4-r11, lr}
        add             r12, r0,  r1,  lsl #3
        push            {r12}
        ldr             r9,  [sp, #40]
        orr             r2,  r2,  r2,  lsl #16
        b               vp8_h_loop_filter_armv6
endfunc

function ff_vp8_h_loop_filter8uv_armv6, export=1
        push            {r1, r4-r11, lr}
        mov             r1,  r2
        orr             r2,  r3,  r3,  lsl #16
        ldr             r3,  [sp, #40]
        ldr             r9,  [sp, #44]
        b               vp8_h_loop_filter_armv6
endfunc

.ltorg
