
%include "ext/x86/x86util.asm"

SECTION_RODATA 32

pw_27:    times 8 dw 27
pw_63:    times 8 dw 63

; 32 bytes wide so the avx2 filters can use them as memory operands
pb_3:     times 32 db 3
pb_4:     times 32 db 4
pb_16:    times 32 db 16
pb_64:    times 32 db 64
pb_F8:    times 32 db 0xF8
pb_FE:    times 32 db 0xFE
pb_27_63: times 8 db 27, 63
pb_18_63: times 8 db 18, 63
pb_9_63:  times 8 db  9, 63

pb_1:     times 16 db 1
pb_80:    times 16 db 0x80
pw_9:     times 8 dw 9
pw_18:    times 8 dw 18

SECTION .text


%macro READ_16x4_INTERLEAVED 12
    lea           %12, [r0+8*r2]

    movd          m%1, [%8+%10*4]
    movd          m%3, [%12+%10*4]
    movd          m%2, [%8+%10*2]
    movd          m%4, [%12+%10*2]
    movd          m%6, [%8+%10]
    movd          m%5, [%12+%10]
    movd          m%7, [%12]
    add           %12, %11
    punpcklbw     m%1, m%3
    movd          m%3, [%8]
    punpcklbw     m%2, m%4
    punpcklbw     m%6, m%5
    punpcklbw     m%3, m%7
    punpcklbw     m%2, m%6

    movd         m%5, [%9+%10*4]
    movd         m%4, [%12+%10*4]
    movd         m%7, [%9]
    movd         m%6, [%12]
    punpcklbw    m%5, m%4
    punpcklbw    m%7, m%6
    punpcklbw    m%1, m%5
    punpcklbw    m%3, m%7
    movd         m%4, [%9+%11]
    movd         m%6, [%12+%11]
    movd         m%5, [%9+%11*2]
    movd         m%7, [%12+%11*2]
    punpcklbw    m%4, m%6
    punpcklbw    m%5, m%7
    punpcklbw    m%4, m%5
%endmacro

%macro WRITE_4x4D 10
    movd    [%5+%8*4], m%1
    movd         [%5], m%2
    movd    [%7+%8*4], m%3
    movd         [%7], m%4

    psrldq        m%1, 4
    psrldq        m%2, 4
    psrldq        m%3, 4
    psrldq        m%4, 4
    movd    [%6+%8*4], m%1
    movd         [%6], m%2
%if %10 == 16
    movd    [%6+%9*4], m%3
%endif
    movd      [%7+%9], m%4

    psrldq        m%1, 4
    psrldq        m%2, 4
%if %10 == 8
    movd    [%5+%8*2], m%1
    movd          %5d, m%3
%endif
    psrldq        m%3, 4
    psrldq        m%4, 4
%if %10 == 16
    movd    [%5+%8*2], m%1
%endif
    movd      [%6+%9], m%2
    movd    [%7+%8*2], m%3
    movd    [%7+%9*2], m%4
    add            %7, %9

    psrldq        m%1, 4
    psrldq        m%2, 4
    psrldq        m%3, 4
    psrldq        m%4, 4
%if %10 == 8
    mov     [%7+%8*4], %5d
    movd    [%6+%8*2], m%1
%else
    movd      [%5+%8], m%1
%endif
    movd    [%6+%9*2], m%2
    movd    [%7+%8*2], m%3
    movd    [%7+%9*2], m%4
%endmacro

%macro WRITE_8W 5
    movd            %2d, %1
    psrldq           %1, 4
    mov       [%3+%4*4], %2w
    shr              %2, 16
    add              %3, %5
    mov       [%3+%4*4], %2w

    movd            %2d, %1
    psrldq           %1, 4
    add              %3, %4
    mov       [%3+%4*2], %2w
    shr              %2, 16
    mov       [%3+%4  ], %2w

    movd            %2d, %1
    psrldq           %1, 4
    mov       [%3     ], %2w
    shr              %2, 16
    mov       [%3+%5  ], %2w

    movd            %2d, %1
    add              %3, %5
    mov       [%3+%5  ], %2w
    shr              %2, 16
    mov       [%3+%5*2], %2w
%endmacro

%macro SIMPLE_LOOPFILTER 2
cglobal vp8_%1_loop_filter_simple, 3, %2, 8, dst, stride, flim, cntr
%if cpuflag(ssse3)
    pxor           m0, m0
%endif
    SPLATB_REG     m7, flim, m0

    DEFINE_ARGS dst1, mstride, stride, dst3, dst2
    mov       strideq, mstrideq
    neg      mstrideq
%ifidn %1, h
    lea         dst1q, [dst1q+4*strideq-2]
%endif

%ifidn %1, v
    mova           m0, [dst1q+mstrideq*2]
    mova           m1, [dst1q+mstrideq]
    mova           m2, [dst1q]
    mova           m3, [dst1q+ strideq]
%else
    lea         dst2q, [dst1q+ strideq]

    READ_16x4_INTERLEAVED 0, 1, 2, 3, 4, 5, 6, dst1q, dst2q, mstrideq, strideq, dst3q
    TRANSPOSE4x4W         0, 1, 2, 3, 4
%endif

    mova           m5, m2
    mova           m6, m1
    psubusb        m1, m2
    psubusb        m2, m6
    por            m1, m2
    paddusb        m1, m1

    mova           m4, m3
    mova           m2, m0
    psubusb        m3, m0
    psubusb        m0, m4
    por            m3, m0
    mova           m0, [pb_80]
    pxor           m2, m0
    pxor           m4, m0
    psubsb         m2, m4
    pand           m3, [pb_FE]
    psrlq          m3, 1
    paddusb        m3, m1
    psubusb        m3, m7
    pxor           m1, m1
    pcmpeqb        m3, m1

    mova           m4, m5
    pxor           m5, m0
    pxor           m0, m6
    psubsb         m5, m0
    paddsb         m2, m5
    paddsb         m2, m5
    paddsb         m2, m5
    pand           m2, m3

    mova           m3, [pb_F8]
    mova           m1, m2
    paddsb         m2, [pb_4]
    paddsb         m1, [pb_3]
    pand           m2, m3
    pand           m1, m3

    pxor           m0, m0
    pxor           m3, m3
    pcmpgtb        m0, m2
    psubb          m3, m2
    psrlq          m2, 3
    psrlq          m3, 3
    pand           m3, m0
    pandn          m0, m2
    psubusb        m4, m0
    paddusb        m4, m3

    pxor           m0, m0
    pxor           m3, m3
    pcmpgtb        m0, m1
    psubb          m3, m1
    psrlq          m1, 3
    psrlq          m3, 3
    pand           m3, m0
    pandn          m0, m1
    paddusb        m6, m0
    psubusb        m6, m3

%ifidn %1, v
    mova      [dst1q], m4
    mova [dst1q+mstrideq], m6
%else
    inc        dst1q
    SBUTTERFLY    bw, 6, 4, 0

    WRITE_8W       m6, dst2q, dst1q, mstrideq, strideq
    lea         dst2q, [dst3q+mstrideq+1]
    WRITE_8W       m4, dst3q, dst2q, mstrideq, strideq
%endif

    RET
%endmacro

INIT_XMM sse2
SIMPLE_LOOPFILTER v, 3
SIMPLE_LOOPFILTER h, 5
INIT_XMM ssse3
SIMPLE_LOOPFILTER v, 3
SIMPLE_LOOPFILTER h, 5


; SIMPLE_FILTER p1, p0, q0, q1, flim, t0, t1, t2, t3, pb_80
; Updates p0 and q0 in place, leaving p1 and q1 unchanged.
%macro SIMPLE_FILTER 10
    psubusb        %6, %2, %3
    psubusb        %7, %3, %2
    por            %6, %6, %7
    paddusb        %6, %6, %6           ; 2 * |p0 - q0|

    psubusb        %7, %1, %4
    psubusb        %8, %4, %1
    por            %7, %7, %8
    pand           %7, %7, [pb_FE]
    psrlq          %7, 1                ; |p1 - q1| >> 1
    paddusb        %6, %6, %7
    psubusb        %6, %6, %5
    pxor           %7, %7, %7
    pcmpeqb        %6, %6, %7           ; 2*|p0-q0| + |p1-q1|/2 <= flim

    pxor           %7, %1, %10
    pxor           %8, %4, %10
    psubsb         %7, %7, %8
    pxor           %8, %3, %10
    pxor           %9, %2, %10
    psubsb         %8, %8, %9
    paddsb         %7, %7, %8
    paddsb         %7, %7, %8
    paddsb         %7, %7, %8           ; f = p1 - q1 + 3 * (q0 - p0)
    pand           %7, %7, %6

    paddsb         %8, %7, [pb_4]
    paddsb         %7, %7, [pb_3]
    pand           %8, %8, [pb_F8]      ; clearing the low 3 bits lets the
    pand           %7, %7, [pb_F8]      ; shifts below stay inside each byte

    psrlq          %8, 3
    psrlq          %7, 3
    pxor           %8, %8, [pb_16]
    pxor           %7, %7, [pb_16]
    psubb          %8, %8, [pb_16]
    psubb          %7, %7, [pb_16]

    pxor           %3, %3, %10
    psubsb         %3, %3, %8
    pxor           %3, %3, %10          ; q0 -= (f + 4) >> 3
    pxor           %2, %2, %10
    paddsb         %2, %2, %7
    pxor           %2, %2, %10          ; p0 += (f + 3) >> 3
%endmacro

; Two horizontal edges four rows apart, one per lane, with per-lane flim.
%macro V_SIMPLE_FILTER_PAIR 1
    movu          xm0, [dstq+mstrideq*2]
    vinserti128    m0, m0, [dst4q+mstrideq*2], 1
    movu          xm1, [dstq+mstrideq]
    vinserti128    m1, m1, [dst4q+mstrideq], 1
    movu          xm2, [dstq]
    vinserti128    m2, m2, [dst4q], 1
    movu          xm3, [dstq+strideq]
    vinserti128    m3, m3, [dst4q+strideq], 1

    SIMPLE_FILTER  m0, m1, m2, m3, %1, m6, m7, m8, m9, m10

    movu   [dstq+mstrideq], xm1
    vextracti128 [dst4q+mstrideq], m1, 1
    movu           [dstq], xm2
    vextracti128  [dst4q], m2, 1
%endmacro

; All four horizontal luma edges, paired in the two 128-bit lanes.
INIT_YMM avx2
cglobal vp8_v_loop_filter_simple_mb, 4, 4, 11, dst, stride, mbedge, bedge
    movd          xm4, mbedged
    movd          xm5, bedged
    vpbroadcastb  xm4, xm4
    vpbroadcastb   m5, xm5              ; edges 2 and 3, both inner
    vinserti128    m4, m4, xm5, 1       ; edge 0 is the macroblock edge
    vbroadcasti128 m10, [pb_80]

    DEFINE_ARGS dst, stride, mstride, dst4
    mov      mstrideq, strideq
    neg      mstrideq
    lea         dst4q, [dstq+strideq*4]

    V_SIMPLE_FILTER_PAIR m4

    lea          dstq, [dst4q+strideq*4]
    lea         dst4q, [dstq+strideq*4]

    V_SIMPLE_FILTER_PAIR m5
    RET


; Transposes m0-m7 packed as [row i | row i+8], clobbering m8-m15.
; The output is packed as [col j | col j+8], making this its own inverse.
%macro TRANSPOSE_16x16B 0
    punpcklbw      m8, m0, m1
    punpckhbw      m9, m0, m1
    punpcklbw     m10, m2, m3
    punpckhbw     m11, m2, m3
    punpcklbw     m12, m4, m5
    punpckhbw     m13, m4, m5
    punpcklbw     m14, m6, m7
    punpckhbw     m15, m6, m7

    punpcklwd      m0, m8, m10
    punpckhwd      m1, m8, m10
    punpcklwd      m2, m12, m14
    punpckhwd      m3, m12, m14
    punpcklwd      m4, m9, m11
    punpckhwd      m5, m9, m11
    punpcklwd      m6, m13, m15
    punpckhwd      m7, m13, m15

    punpckldq      m8, m0, m2
    punpckhdq      m9, m0, m2
    punpckldq     m10, m1, m3
    punpckhdq     m11, m1, m3
    punpckldq     m12, m4, m6
    punpckhdq     m13, m4, m6
    punpckldq     m14, m5, m7
    punpckhdq     m15, m5, m7

    ; each register now holds two columns per lane; 0xd8 reorders the qwords to
    ; [0 2 1 3], which joins the two row halves of a column into one lane
    punpcklqdq     m0, m8, m12
    punpckhqdq     m1, m8, m12
    punpcklqdq     m2, m9, m13
    punpckhqdq     m3, m9, m13
    punpcklqdq     m4, m10, m14
    punpckhqdq     m5, m10, m14
    punpcklqdq     m6, m11, m15
    punpckhqdq     m7, m11, m15

    vpermq         m0, m0, 0xd8
    vpermq         m1, m1, 0xd8
    vpermq         m2, m2, 0xd8
    vpermq         m3, m3, 0xd8
    vpermq         m4, m4, 0xd8
    vpermq         m5, m5, 0xd8
    vpermq         m6, m6, 0xd8
    vpermq         m7, m7, 0xd8
%endmacro

%macro LOAD_16x16B 0
    movu          xm0, [dstq]
    movu          xm1, [dstq+strideq]
    movu          xm2, [dstq+strideq*2]
    movu          xm3, [dst4q+mstrideq]
    movu          xm4, [dst4q]
    movu          xm5, [dst4q+strideq]
    movu          xm6, [dst4q+strideq*2]
    movu          xm7, [dst8q+mstrideq]
    vinserti128    m0, m0, [dst8q], 1
    vinserti128    m1, m1, [dst8q+strideq], 1
    vinserti128    m2, m2, [dst8q+strideq*2], 1
    vinserti128    m3, m3, [dst12q+mstrideq], 1
    vinserti128    m4, m4, [dst12q], 1
    vinserti128    m5, m5, [dst12q+strideq], 1
    vinserti128    m6, m6, [dst12q+strideq*2], 1
    vinserti128    m7, m7, [dst16q+mstrideq], 1
%endmacro

%macro STORE_16x16B 0
    movu         [dstq], xm0
    movu [dstq+strideq], xm1
    movu [dstq+strideq*2], xm2
    movu [dst4q+mstrideq], xm3
    movu        [dst4q], xm4
    movu [dst4q+strideq], xm5
    movu [dst4q+strideq*2], xm6
    movu [dst8q+mstrideq], xm7
    vextracti128 [dst8q], m0, 1
    vextracti128 [dst8q+strideq], m1, 1
    vextracti128 [dst8q+strideq*2], m2, 1
    vextracti128 [dst12q+mstrideq], m3, 1
    vextracti128 [dst12q], m4, 1
    vextracti128 [dst12q+strideq], m5, 1
    vextracti128 [dst12q+strideq*2], m6, 1
    vextracti128 [dst16q+mstrideq], m7, 1
%endmacro

; All four vertical luma edges using one 16x16 load and transpose.
INIT_YMM avx2
cglobal vp8_h_loop_filter_simple_mb, 4, 9, 16, dst, stride, mbedge, bedge
    ; the transpose clobbers m8-m15, so the limits stay in gprs until after it
    DEFINE_ARGS dst, stride, mbedge, bedge, mstride, dst4, dst8, dst12, dst16
    mov      mstrideq, strideq
    neg      mstrideq
    sub          dstq, 2
    lea         dst4q, [dstq+strideq*4]
    lea         dst8q, [dstq+strideq*8]
    lea        dst12q, [dst4q+strideq*8]
    lea        dst16q, [dst8q+strideq*8]

    LOAD_16x16B
    TRANSPOSE_16x16B

    movd         xm10, mbedged
    movd          xm9, bedged
    vpbroadcastb xm10, xm10
    vpbroadcastb   m9, xm9               ; edges 1 and 3, both inner
    vinserti128   m10, m10, xm9, 1       ; edge 0 is the macroblock edge, edge 2
                                         ; rides along in the other lane
    vbroadcasti128 m8, [pb_80]

    SIMPLE_FILTER  m0, m1, m2, m3, m10, m11, m12, m13, m14, m8
    SIMPLE_FILTER  m4, m5, m6, m7,  m9, m11, m12, m13, m14, m8

    TRANSPOSE_16x16B
    STORE_16x16B
    RET


%macro LOAD_TRANSPOSED_16x16B 0
    movu          xm0, [tmpq]
    movu          xm1, [tmpq+16]
    movu          xm2, [tmpq+32]
    movu          xm3, [tmpq+48]
    movu          xm4, [tmpq+64]
    movu          xm5, [tmpq+80]
    movu          xm6, [tmpq+96]
    movu          xm7, [tmpq+112]
    vinserti128    m0, m0, [tmpq+128], 1
    vinserti128    m1, m1, [tmpq+144], 1
    vinserti128    m2, m2, [tmpq+160], 1
    vinserti128    m3, m3, [tmpq+176], 1
    vinserti128    m4, m4, [tmpq+192], 1
    vinserti128    m5, m5, [tmpq+208], 1
    vinserti128    m6, m6, [tmpq+224], 1
    vinserti128    m7, m7, [tmpq+240], 1
%endmacro

%macro STORE_TRANSPOSED_16x16B 0
    movu       [tmpq], xm0
    movu    [tmpq+16], xm1
    movu    [tmpq+32], xm2
    movu    [tmpq+48], xm3
    movu    [tmpq+64], xm4
    movu    [tmpq+80], xm5
    movu    [tmpq+96], xm6
    movu   [tmpq+112], xm7
    vextracti128 [tmpq+128], m0, 1
    vextracti128 [tmpq+144], m1, 1
    vextracti128 [tmpq+160], m2, 1
    vextracti128 [tmpq+176], m3, 1
    vextracti128 [tmpq+192], m4, 1
    vextracti128 [tmpq+208], m5, 1
    vextracti128 [tmpq+224], m6, 1
    vextracti128 [tmpq+240], m7, 1
%endmacro

INIT_YMM avx2
cglobal vp8_h_loop_filter16y_mb_transpose, 3, 8, 16, dst, stride, tmp, dst4, dst8, dst12, dst16, mstride
    mov      mstrideq, strideq
    neg      mstrideq
    sub          dstq, 4
    lea         dst4q, [dstq+strideq*4]
    lea         dst8q, [dstq+strideq*8]
    lea        dst12q, [dst4q+strideq*8]
    lea        dst16q, [dst8q+strideq*8]
    LOAD_16x16B
    TRANSPOSE_16x16B
    STORE_TRANSPOSED_16x16B
    RET

INIT_YMM avx2
cglobal vp8_h_loop_filter16y_mb_itranspose, 3, 8, 16, dst, stride, tmp, dst4, dst8, dst12, dst16, mstride
    mov      mstrideq, strideq
    neg      mstrideq
    sub          dstq, 4
    lea         dst4q, [dstq+strideq*4]
    lea         dst8q, [dstq+strideq*8]
    lea        dst12q, [dst4q+strideq*8]
    lea        dst16q, [dst8q+strideq*8]
    LOAD_TRANSPOSED_16x16B
    TRANSPOSE_16x16B
    STORE_16x16B
    RET

%macro LOAD_8UVx16B 0
    movu          xm0, [dstUq]
    movu          xm1, [dstUq+strideq]
    movu          xm2, [dstUq+strideq*2]
    movu          xm3, [dstU4q+mstrideq]
    movu          xm4, [dstU4q]
    movu          xm5, [dstU4q+strideq]
    movu          xm6, [dstU4q+strideq*2]
    movu          xm7, [dstU8q+mstrideq]
    vinserti128    m0, m0, [dstVq], 1
    vinserti128    m1, m1, [dstVq+strideq], 1
    vinserti128    m2, m2, [dstVq+strideq*2], 1
    vinserti128    m3, m3, [dstV4q+mstrideq], 1
    vinserti128    m4, m4, [dstV4q], 1
    vinserti128    m5, m5, [dstV4q+strideq], 1
    vinserti128    m6, m6, [dstV4q+strideq*2], 1
    vinserti128    m7, m7, [dstV8q+mstrideq], 1
%endmacro

%macro STORE_8UVx16B 0
    movu          [dstUq], xm0
    movu  [dstUq+strideq], xm1
    movu [dstUq+strideq*2], xm2
    movu [dstU4q+mstrideq], xm3
    movu         [dstU4q], xm4
    movu [dstU4q+strideq], xm5
    movu [dstU4q+strideq*2], xm6
    movu [dstU8q+mstrideq], xm7
    vextracti128 [dstVq], m0, 1
    vextracti128 [dstVq+strideq], m1, 1
    vextracti128 [dstVq+strideq*2], m2, 1
    vextracti128 [dstV4q+mstrideq], m3, 1
    vextracti128 [dstV4q], m4, 1
    vextracti128 [dstV4q+strideq], m5, 1
    vextracti128 [dstV4q+strideq*2], m6, 1
    vextracti128 [dstV8q+mstrideq], m7, 1
%endmacro

INIT_YMM avx2
cglobal vp8_h_loop_filter8uv_mb_transpose, 4, 9, 16, dstU, dstV, stride, tmp, dstU4, dstV4, dstU8, dstV8, mstride
    mov      mstrideq, strideq
    neg      mstrideq
    sub         dstUq, 4
    sub         dstVq, 4
    lea        dstU4q, [dstUq+strideq*4]
    lea        dstV4q, [dstVq+strideq*4]
    lea        dstU8q, [dstU4q+strideq*4]
    lea        dstV8q, [dstV4q+strideq*4]
    LOAD_8UVx16B
    TRANSPOSE_16x16B
    STORE_TRANSPOSED_16x16B
    RET

INIT_YMM avx2
cglobal vp8_h_loop_filter8uv_mb_itranspose, 4, 9, 16, dstU, dstV, stride, tmp, dstU4, dstV4, dstU8, dstV8, mstride
    mov      mstrideq, strideq
    neg      mstrideq
    sub         dstUq, 4
    sub         dstVq, 4
    lea        dstU4q, [dstUq+strideq*4]
    lea        dstV4q, [dstVq+strideq*4]
    lea        dstU8q, [dstU4q+strideq*4]
    lea        dstV8q, [dstV4q+strideq*4]
    LOAD_TRANSPOSED_16x16B
    TRANSPOSE_16x16B
    STORE_8UVx16B
    RET


%macro INNER_LOOPFILTER 2
%define stack_size 0
%ifndef m8
%ifidn %1, v
%define stack_size mmsize * -4
%else
%define stack_size mmsize * -5
%endif
%endif

%if %2 == 8
cglobal vp8_%1_loop_filter8uv_inner, 6, 6, 13, stack_size, dst, dst8, stride, flimE, flimI, hevthr
%else
cglobal vp8_%1_loop_filter16y_inner, 5, 5, 13, stack_size, dst, stride, flimE, flimI, hevthr
%endif

%if cpuflag(ssse3)
    pxor             m7, m7
%endif

%ifndef m8
    SPLATB_REG       m0, flimEq, m7
    SPLATB_REG       m1, flimIq, m7
    SPLATB_REG       m2, hevthrq, m7

%define m_flimE    [rsp]
%define m_flimI    [rsp+mmsize]
%define m_hevthr   [rsp+mmsize*2]
%define m_maskres  [rsp+mmsize*3]
%define m_p0backup [rsp+mmsize*3]
%define m_q0backup [rsp+mmsize*4]

    mova        m_flimE, m0
    mova        m_flimI, m1
    mova       m_hevthr, m2
%else
%define m_flimE    m9
%define m_flimI    m10
%define m_hevthr   m11
%define m_maskres  m12
%define m_p0backup m12
%define m_q0backup m8

    SPLATB_REG  m_flimE, flimEq, m7
    SPLATB_REG  m_flimI, flimIq, m7
    SPLATB_REG m_hevthr, hevthrq, m7
%endif

%if %2 == 8
    DEFINE_ARGS dst1, dst8, mstride, stride, dst2
%else
    DEFINE_ARGS dst1, mstride, stride, dst2, dst8
%endif
    mov         strideq, mstrideq
    neg        mstrideq
%ifidn %1, h
    lea           dst1q, [dst1q+strideq*4-4]
%if %2 == 8
    lea           dst8q, [dst8q+strideq*4-4]
%endif
%endif

    lea           dst2q, [dst1q+strideq]
%ifidn %1, v
%if %2 == 8
%define movrow movh
%else
%define movrow mova
%endif
    movrow           m0, [dst1q+mstrideq*4]
    movrow           m1, [dst2q+mstrideq*4]
    movrow           m2, [dst1q+mstrideq*2]
    movrow           m5, [dst2q]
    movrow           m6, [dst2q+ strideq*1]
    movrow           m7, [dst2q+ strideq*2]
%if %2 == 8
    movhps           m0, [dst8q+mstrideq*4]
    movhps           m2, [dst8q+mstrideq*2]
    add           dst8q, strideq
    movhps           m1, [dst8q+mstrideq*4]
    movhps           m5, [dst8q]
    movhps           m6, [dst8q+ strideq  ]
    movhps           m7, [dst8q+ strideq*2]
    add           dst8q, mstrideq
%endif
%else
%if %2 == 16
    lea           dst8q, [dst1q+ strideq*8]
%endif

    movh             m0, [dst1q+mstrideq*4]
    movh             m1, [dst8q+mstrideq*4]
    movh             m2, [dst1q+mstrideq*2]
    movh             m5, [dst8q+mstrideq*2]
    movh             m3, [dst1q+mstrideq  ]
    movh             m6, [dst8q+mstrideq  ]
    movh             m4, [dst1q]
    movh             m7, [dst8q]
    punpcklbw        m0, m1
    punpcklbw        m2, m5
    punpcklbw        m3, m6
    punpcklbw        m4, m7

    add           dst8q, strideq
    movh             m1, [dst2q+mstrideq*4]
    movh             m6, [dst8q+mstrideq*4]
    movh             m5, [dst2q]
    movh             m7, [dst8q]
    punpcklbw        m1, m6
    punpcklbw        m5, m7
    movh             m6, [dst2q+ strideq  ]
    movh             m7, [dst8q+ strideq  ]
    punpcklbw        m6, m7

    TRANSPOSE4x4B     0, 1, 2, 3, 7
%ifdef m8
    SWAP              1, 8
%else
    mova     m_q0backup, m1
%endif
    movh             m7, [dst2q+ strideq*2]
    movh             m1, [dst8q+ strideq*2]
    punpcklbw        m7, m1
    TRANSPOSE4x4B     4, 5, 6, 7, 1
    SBUTTERFLY       dq, 0, 4, 1
    SBUTTERFLY       dq, 2, 6, 1
    SBUTTERFLY       dq, 3, 7, 1
%ifdef m8
    SWAP              1, 8
    SWAP              2, 8
%else
    mova             m1, m_q0backup
    mova     m_q0backup, m2
%endif
    SBUTTERFLY       dq, 1, 5, 2
%ifdef m12
    SWAP              5, 12
%else
    mova     m_p0backup, m5
%endif
    SWAP              1, 4
    SWAP              2, 4
    SWAP              6, 3
    SWAP              5, 3
%endif

    mova             m4, m1
    SWAP              4, 1
    psubusb          m4, m0
    psubusb          m0, m1
    por              m0, m4

    mova             m4, m2
    SWAP              4, 2
    psubusb          m4, m1
    psubusb          m1, m2
    por              m1, m4

    mova             m4, m6
    SWAP              4, 6
    psubusb          m4, m7
    psubusb          m7, m6
    por              m7, m4

    mova             m4, m5
    SWAP              4, 5
    psubusb          m4, m6
    psubusb          m6, m5
    por              m6, m4

    pmaxub           m0, m1
    pmaxub           m6, m7
    pmaxub           m0, m6

    SWAP              7, 3
%ifidn %1, v
    movrow           m3, [dst1q+mstrideq  ]
%if %2 == 8
    movhps           m3, [dst8q+mstrideq  ]
%endif
%elifdef m12
    SWAP              3, 12
%else
    mova             m3, m_p0backup
%endif

    mova             m1, m2
    SWAP              1, 2
    mova             m6, m3
    SWAP              3, 6
    psubusb          m1, m3
    psubusb          m6, m2
    por              m1, m6
    pmaxub           m0, m1
    SWAP              1, 4

    SWAP              6, 4
%ifidn %1, v
    movrow           m4, [dst1q]
%if %2 == 8
    movhps           m4, [dst8q]
%endif
%elifdef m8
    SWAP              4, 8
%else
    mova             m4, m_q0backup
%endif
    mova             m1, m4
    SWAP              1, 4
    mova             m7, m5
    SWAP              7, 5
    psubusb          m1, m5
    psubusb          m7, m4
    por              m1, m7
    pxor             m7, m7
    pmaxub           m0, m1
    pmaxub           m6, m1
    psubusb          m0, m_flimI
    psubusb          m6, m_hevthr
    pcmpeqb          m0, m7
    pcmpeqb          m6, m7
%ifdef m12
    SWAP              6, 12
%else
    mova      m_maskres, m6
%endif

    mova             m1, m3
    SWAP              1, 3
    mova             m6, m4
    SWAP              6, 4
    psubusb          m1, m4
    psubusb          m6, m3
    por              m1, m6
    paddusb          m1, m1

    mova             m7, m2
    SWAP              7, 2
    mova             m6, m5
    SWAP              6, 5
    psubusb          m7, m5
    psubusb          m6, m2
    por              m7, m6
    pxor             m6, m6
    pand             m7, [pb_FE]
    psrlq            m7, 1
    paddusb          m7, m1
    psubusb          m7, m_flimE
    pcmpeqb          m7, m6
    pand             m0, m7

%ifdef m8
    mova             m8, [pb_80]
%define m_pb_80 m8
%else
%define m_pb_80 [pb_80]
%endif
    mova             m1, m4
    mova             m7, m3
    pxor             m1, m_pb_80
    pxor             m7, m_pb_80
    psubsb           m1, m7
    mova             m6, m2
    mova             m7, m5
    pxor             m6, m_pb_80
    pxor             m7, m_pb_80
    psubsb           m6, m7
    mova             m7, m_maskres
    pandn            m7, m6
    paddsb           m7, m1
    paddsb           m7, m1
    paddsb           m7, m1

    pand             m7, m0
    mova             m1, [pb_F8]
    mova             m6, m7
    paddsb           m7, [pb_3]
    paddsb           m6, [pb_4]
    pand             m7, m1
    pand             m6, m1
    psrlq            m7, 3
    psrlq            m6, 3
    pxor             m7, [pb_16]
    pxor             m6, [pb_16]
    psubb            m7, [pb_16]
    psubb            m6, [pb_16]

    pxor             m3, m_pb_80
    paddsb           m3, m7
    pxor             m3, m_pb_80
    pxor             m4, m_pb_80
    psubsb           m4, m6
    pxor             m4, m_pb_80

    pxor             m6, m_pb_80
    pxor             m0, m0
    pavgb            m6, m0
    psubb            m6, [pb_64]

%ifdef m12
    SWAP              0, 12
%else
    mova             m0, m_maskres
%endif
    pand             m6, m0
    pxor             m2, m_pb_80
    paddsb           m2, m6
    pxor             m2, m_pb_80
    pxor             m5, m_pb_80
    psubsb           m5, m6
    pxor             m5, m_pb_80

%ifidn %1, v
    movrow [dst1q+mstrideq*2], m2
    movrow [dst1q+mstrideq  ], m3
    movrow      [dst1q], m4
    movrow [dst1q+ strideq  ], m5
%if %2 == 8
    movhps [dst8q+mstrideq*2], m2
    movhps [dst8q+mstrideq  ], m3
    movhps      [dst8q], m4
    movhps [dst8q+ strideq  ], m5
%endif
%else
    add           dst1q, 2
    add           dst2q, 2

    TRANSPOSE4x4B     2, 3, 4, 5, 6

    lea           dst8q, [dst8q+mstrideq  +2]
    WRITE_4x4D        2, 3, 4, 5, dst1q, dst2q, dst8q, mstrideq, strideq, %2
%endif

    RET
%endmacro

INIT_XMM sse2
INNER_LOOPFILTER v, 16
INNER_LOOPFILTER h, 16
INNER_LOOPFILTER v,  8
INNER_LOOPFILTER h,  8

INIT_XMM ssse3
INNER_LOOPFILTER v, 16
INNER_LOOPFILTER h, 16
INNER_LOOPFILTER v,  8
INNER_LOOPFILTER h,  8


%macro MBEDGE_LOOPFILTER 2
%define stack_size 0
%ifndef m8
%define stack_size mmsize * -7
%endif

%if %2 == 8
cglobal vp8_%1_loop_filter8uv_mbedge, 6, 6, 15, stack_size, dst1, dst8, stride, flimE, flimI, hevthr
%else
cglobal vp8_%1_loop_filter16y_mbedge, 5, 5, 15, stack_size, dst1, stride, flimE, flimI, hevthr
%endif

%if cpuflag(ssse3)
    pxor             m7, m7
%endif

%ifndef m8
    SPLATB_REG       m0, flimEq, m7
    SPLATB_REG       m1, flimIq, m7
    SPLATB_REG       m2, hevthrq, m7

%define m_flimE    [rsp]
%define m_flimI    [rsp+mmsize]
%define m_hevthr   [rsp+mmsize*2]
%define m_maskres  [rsp+mmsize*3]
%define m_limres   [rsp+mmsize*4]
%define m_p0backup [rsp+mmsize*3]
%define m_q0backup [rsp+mmsize*4]
%define m_p2backup [rsp+mmsize*5]
%define m_q2backup [rsp+mmsize*6]

    mova        m_flimE, m0
    mova        m_flimI, m1
    mova       m_hevthr, m2
%else
%define m_flimE    m9
%define m_flimI    m10
%define m_hevthr   m11
%define m_maskres  m12
%define m_limres   m8
%define m_p0backup m12
%define m_q0backup m8
%define m_p2backup m13
%define m_q2backup m14

    SPLATB_REG  m_flimE, flimEq, m7
    SPLATB_REG  m_flimI, flimIq, m7
    SPLATB_REG m_hevthr, hevthrq, m7
%endif

%if %2 == 8
    DEFINE_ARGS dst1, dst8, mstride, stride, dst2
%else
    DEFINE_ARGS dst1, mstride, stride, dst2, dst8
%endif
    mov         strideq, mstrideq
    neg        mstrideq
%ifidn %1, h
    lea           dst1q, [dst1q+strideq*4-4]
%if %2 == 8
    lea           dst8q, [dst8q+strideq*4-4]
%endif
%endif

    lea           dst2q, [dst1q+ strideq  ]
%ifidn %1, v
%if %2 == 8
%define movrow movh
%else
%define movrow mova
%endif
    movrow           m0, [dst1q+mstrideq*4]
    movrow           m1, [dst2q+mstrideq*4]
    movrow           m2, [dst1q+mstrideq*2]
    movrow           m5, [dst2q]
    movrow           m6, [dst2q+ strideq  ]
    movrow           m7, [dst2q+ strideq*2]
%if %2 == 8
    movhps           m0, [dst8q+mstrideq*4]
    movhps           m2, [dst8q+mstrideq*2]
    add           dst8q, strideq
    movhps           m1, [dst8q+mstrideq*4]
    movhps           m5, [dst8q]
    movhps           m6, [dst8q+ strideq  ]
    movhps           m7, [dst8q+ strideq*2]
    add           dst8q, mstrideq
%endif
%else
%if %2 == 16
    lea           dst8q, [dst1q+ strideq*8  ]
%endif

    movh             m0, [dst1q+mstrideq*4]
    movh             m1, [dst8q+mstrideq*4]
    movh             m2, [dst1q+mstrideq*2]
    movh             m5, [dst8q+mstrideq*2]
    movh             m3, [dst1q+mstrideq  ]
    movh             m6, [dst8q+mstrideq  ]
    movh             m4, [dst1q]
    movh             m7, [dst8q]
    punpcklbw        m0, m1
    punpcklbw        m2, m5
    punpcklbw        m3, m6
    punpcklbw        m4, m7

    add           dst8q, strideq
    movh             m1, [dst2q+mstrideq*4]
    movh             m6, [dst8q+mstrideq*4]
    movh             m5, [dst2q]
    movh             m7, [dst8q]
    punpcklbw        m1, m6
    punpcklbw        m5, m7
    movh             m6, [dst2q+ strideq  ]
    movh             m7, [dst8q+ strideq  ]
    punpcklbw        m6, m7

    TRANSPOSE4x4B     0, 1, 2, 3, 7
%ifdef m8
    SWAP              1, 8
%else
    mova     m_q0backup, m1
%endif
    movh             m7, [dst2q+ strideq*2]
    movh             m1, [dst8q+ strideq*2]
    punpcklbw        m7, m1
    TRANSPOSE4x4B     4, 5, 6, 7, 1
    SBUTTERFLY       dq, 0, 4, 1
    SBUTTERFLY       dq, 2, 6, 1
    SBUTTERFLY       dq, 3, 7, 1
%ifdef m8
    SWAP              1, 8
    SWAP              2, 8
%else
    mova             m1, m_q0backup
    mova     m_q0backup, m2
%endif
    SBUTTERFLY       dq, 1, 5, 2
%ifdef m12
    SWAP              5, 12
%else
    mova     m_p0backup, m5
%endif
    SWAP              1, 4
    SWAP              2, 4
    SWAP              6, 3
    SWAP              5, 3
%endif

    mova             m4, m1
    SWAP              4, 1
    psubusb          m4, m0
    psubusb          m0, m1
    por              m0, m4

    mova             m4, m2
    SWAP              4, 2
    psubusb          m4, m1
    mova     m_p2backup, m1
    psubusb          m1, m2
    por              m1, m4

    mova             m4, m6
    SWAP              4, 6
    psubusb          m4, m7
    psubusb          m7, m6
    por              m7, m4

    mova             m4, m5
    SWAP              4, 5
    psubusb          m4, m6
    mova     m_q2backup, m6
    psubusb          m6, m5
    por              m6, m4

    pmaxub           m0, m1
    pmaxub           m6, m7
    pmaxub           m0, m6

    SWAP              7, 3
%ifidn %1, v
    movrow           m3, [dst1q+mstrideq  ]
%if %2 == 8
    movhps           m3, [dst8q+mstrideq  ]
%endif
%elifdef m12
    SWAP              3, 12
%else
    mova             m3, m_p0backup
%endif

    mova             m1, m2
    SWAP              1, 2
    mova             m6, m3
    SWAP              3, 6
    psubusb          m1, m3
    psubusb          m6, m2
    por              m1, m6
    pmaxub           m0, m1
    SWAP              1, 4

    SWAP              6, 4
%ifidn %1, v
    movrow           m4, [dst1q]
%if %2 == 8
    movhps           m4, [dst8q]
%endif
%elifdef m8
    SWAP              4, 8
%else
    mova             m4, m_q0backup
%endif
    mova             m1, m4
    SWAP              1, 4
    mova             m7, m5
    SWAP              7, 5
    psubusb          m1, m5
    psubusb          m7, m4
    por              m1, m7
    pxor             m7, m7
    pmaxub           m0, m1
    pmaxub           m6, m1
    psubusb          m0, m_flimI
    psubusb          m6, m_hevthr
    pcmpeqb          m0, m7
    pcmpeqb          m6, m7
%ifdef m12
    SWAP              6, 12
%else
    mova      m_maskres, m6
%endif

    mova             m1, m3
    SWAP              1, 3
    mova             m6, m4
    SWAP              6, 4
    psubusb          m1, m4
    psubusb          m6, m3
    por              m1, m6
    paddusb          m1, m1

    mova             m7, m2
    SWAP              7, 2
    mova             m6, m5
    SWAP              6, 5
    psubusb          m7, m5
    psubusb          m6, m2
    por              m7, m6
    pxor             m6, m6
    pand             m7, [pb_FE]
    psrlq            m7, 1
    paddusb          m7, m1
    psubusb          m7, m_flimE
    pcmpeqb          m7, m6
    pand             m0, m7

%ifdef m8
    mova             m8, [pb_80]
%define m_pb_80 m8
%else
%define m_pb_80 [pb_80]
%endif
    mova             m1, m4
    mova             m7, m3
    pxor             m1, m_pb_80
    pxor             m7, m_pb_80
    psubsb           m1, m7
    mova             m6, m2
    mova             m7, m5
    pxor             m6, m_pb_80
    pxor             m7, m_pb_80
    psubsb           m6, m7
    mova             m7, m_maskres
    paddsb           m6, m1
    paddsb           m6, m1
    paddsb           m6, m1
    pand             m6, m0
%ifdef m8
    mova       m_limres, m6
    pand       m_limres, m7
%else
    mova             m0, m6
    pand             m0, m7
    mova       m_limres, m0
%endif
    pandn            m7, m6

    mova             m1, [pb_F8]
    mova             m6, m7
    paddsb           m7, [pb_3]
    paddsb           m6, [pb_4]
    pand             m7, m1
    pand             m6, m1
    psrlq            m7, 3
    psrlq            m6, 3
    pxor             m7, [pb_16]
    pxor             m6, [pb_16]
    psubb            m7, [pb_16]
    psubb            m6, [pb_16]

    pxor             m3, [pb_80]
    paddsb           m3, m7
    pxor             m3, [pb_80]
    pxor             m4, [pb_80]
    psubsb           m4, m6
    pxor             m4, [pb_80]

%if cpuflag(ssse3)
    mova             m7, [pb_1]
%else
    mova             m7, [pw_63]
%endif
%ifdef m8
    SWAP              1, 8
%else
    mova             m1, m_limres
%endif
    pxor             m0, m0
    mova             m6, m1
    pcmpgtb          m0, m1
%if cpuflag(ssse3)
    punpcklbw        m6, m7
    punpckhbw        m1, m7
%else
    punpcklbw        m6, m0
    punpckhbw        m1, m0
%endif
%if cpuflag(ssse3)
    mova             m7, [pb_27_63]
%ifndef m8
    mova       m_limres, m1
%endif
%ifdef m10
    SWAP              0, 10
%endif
    mova             m0, m7
    pmaddubsw        m7, m6
    SWAP              6, 7
    pmaddubsw        m0, m1
    SWAP              1, 0
%ifdef m10
    SWAP              0, 10
%endif
%else
    mova      m_maskres, m6
    mova       m_limres, m1
    pmullw          m6, [pw_27]
    pmullw          m1, [pw_27]
    paddw           m6, m7
    paddw           m1, m7
%endif
    psraw           m6, 7
    psraw           m1, 7
    packsswb        m6, m1
    pxor            m3, [pb_80]
    paddsb          m3, m6
    pxor            m3, [pb_80]
    pxor            m4, [pb_80]
    psubsb          m4, m6
    pxor            m4, [pb_80]
%if cpuflag(ssse3)
    mova            m6, [pb_18_63]
%endif

%if cpuflag(ssse3)
    SWAP             6, 7
%ifdef m10
    SWAP             1, 10
%else
    mova            m1, m_limres
%endif
    mova            m0, m7
    pmaddubsw       m7, m6
    SWAP             6, 7
    pmaddubsw       m0, m1
    SWAP             1, 0
%ifdef m10
    SWAP             0, 10
%endif
%else
    mova            m6, m_maskres
    mova            m1, m_limres
    pmullw          m6, [pw_18]
    pmullw          m1, [pw_18]
    paddw           m6, m7
    paddw           m1, m7
%endif
    psraw           m6, 7
    psraw           m1, 7
    packsswb        m6, m1
    pxor            m2, [pb_80]
    paddsb          m2, m6
    pxor            m2, [pb_80]
    pxor            m5, [pb_80]
    psubsb          m5, m6
    pxor            m5, [pb_80]
%if cpuflag(ssse3)
    mova            m6, [pb_9_63]
%endif

%if cpuflag(ssse3)
    SWAP             6, 7
%ifdef m10
    SWAP             1, 10
%else
    mova            m1, m_limres
%endif
    mova            m0, m7
    pmaddubsw       m7, m6
    SWAP             6, 7
    pmaddubsw       m0, m1
    SWAP             1, 0
%else
%ifdef m8
    SWAP             6, 12
    SWAP             1, 8
%else
    mova            m6, m_maskres
    mova            m1, m_limres
%endif
    pmullw          m6, [pw_9]
    pmullw          m1, [pw_9]
    paddw           m6, m7
    paddw           m1, m7
%endif
    psraw           m6, 7
    psraw           m1, 7
    packsswb        m6, m1
%ifdef m8
    pxor     m_p2backup, [pb_80]
    paddsb   m_p2backup, m6
    pxor     m_p2backup, [pb_80]
    pxor     m_q2backup, [pb_80]
    psubsb   m_q2backup, m6
    pxor     m_q2backup, [pb_80]
    SWAP             1, 13
    SWAP             6, 14
%else
    mova            m1, m_p2backup
    mova            m0, m_q2backup
    pxor            m1, [pb_80]
    paddsb          m1, m6
    pxor            m1, [pb_80]
    pxor            m0, [pb_80]
    psubsb          m0, m6
    pxor            m0, [pb_80]
    mova            m6, m0
%endif

%ifidn %1, v
    movrow [dst2q+mstrideq*4], m1
    movrow [dst1q+mstrideq*2], m2
    movrow [dst1q+mstrideq  ], m3
    movrow     [dst1q], m4
    movrow     [dst2q], m5
    movrow [dst2q+ strideq  ], m6
%if %2 == 8
    add           dst8q, mstrideq
    movhps [dst8q+mstrideq*2], m1
    movhps [dst8q+mstrideq  ], m2
    movhps     [dst8q], m3
    add          dst8q, strideq
    movhps     [dst8q], m4
    movhps [dst8q+ strideq  ], m5
    movhps [dst8q+ strideq*2], m6
%endif
%else
    inc          dst1q
    inc          dst2q

    TRANSPOSE4x4B    1, 2, 3, 4, 0
    SBUTTERFLY      bw, 5, 6, 0

    lea          dst8q, [dst8q+mstrideq+1]
    WRITE_4x4D       1, 2, 3, 4, dst1q, dst2q, dst8q, mstrideq, strideq, %2
    lea          dst1q, [dst2q+mstrideq+4]
    lea          dst8q, [dst8q+mstrideq+4]
    WRITE_8W        m5, dst2q, dst1q,  mstrideq, strideq
    WRITE_8W        m6, dst2q, dst8q, mstrideq, strideq
%endif

    RET
%endmacro

INIT_XMM sse2
MBEDGE_LOOPFILTER v, 16
MBEDGE_LOOPFILTER h, 16
MBEDGE_LOOPFILTER v,  8
MBEDGE_LOOPFILTER h,  8

INIT_XMM ssse3
MBEDGE_LOOPFILTER v, 16
MBEDGE_LOOPFILTER h, 16
MBEDGE_LOOPFILTER v,  8
MBEDGE_LOOPFILTER h,  8
