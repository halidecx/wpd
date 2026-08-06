
%include "asm/x86/x86util.asm"

SECTION_RODATA

pw_20091: times 4 dw 20091
pw_17734: times 4 dw 17734

cextern_naked wpd_pw_3
cextern_naked wpd_pw_4

SECTION .text


%macro ADD_DC 4
    %4        m2, [dst1q+%3]
    %4        m3, [dst1q+strideq+%3]
    %4        m4, [dst2q+%3]
    %4        m5, [dst2q+strideq+%3]
    paddusb   m2, %1
    paddusb   m3, %1
    paddusb   m4, %1
    paddusb   m5, %1
    psubusb   m2, %2
    psubusb   m3, %2
    psubusb   m4, %2
    psubusb   m5, %2
    %4 [dst1q+%3], m2
    %4 [dst1q+strideq+%3], m3
    %4 [dst2q+%3], m4
    %4 [dst2q+strideq+%3], m5
%endmacro

%macro VP8_IDCT_DC_ADD 0
cglobal vp8_idct_dc_add, 3, 3, 6, dst, block, stride
    movd       m0, [blockq]
    pxor       m1, m1

    paddw      m0, [wpd_pw_4]
    movd [blockq], m1
    DEFINE_ARGS dst1, dst2, stride
    lea     dst2q, [dst1q+strideq*2]
    movd       m2, [dst1q]
    movd       m3, [dst1q+strideq]
    movd       m4, [dst2q]
    movd       m5, [dst2q+strideq]
    psraw      m0, 3
    pshuflw    m0, m0, 0
    punpcklqdq m0, m0
    punpckldq  m2, m3
    punpckldq  m4, m5
    punpcklbw  m2, m1
    punpcklbw  m4, m1
    paddw      m2, m0
    paddw      m4, m0
    packuswb   m2, m4
    movd   [dst1q], m2
%if cpuflag(sse4)
    pextrd [dst1q+strideq], m2, 1
    pextrd [dst2q], m2, 2
    pextrd [dst2q+strideq], m2, 3
%else
    psrldq     m2, 4
    movd [dst1q+strideq], m2
    psrldq     m2, 4
    movd [dst2q], m2
    psrldq     m2, 4
    movd [dst2q+strideq], m2
%endif
    RET
%endmacro

INIT_XMM sse2
VP8_IDCT_DC_ADD
INIT_XMM sse4
VP8_IDCT_DC_ADD


INIT_XMM sse2
cglobal vp8_idct_dc_add4y, 3, 3, 6, dst, block, stride
    movd      m0, [blockq+32*0]
    movd      m1, [blockq+32*2]
    punpcklwd m0, [blockq+32*1]
    punpcklwd m1, [blockq+32*3]
    punpckldq m0, m1
    pxor      m1, m1

    paddw     m0, [wpd_pw_4]
    movd [blockq+32*0], m1
    movd [blockq+32*1], m1
    movd [blockq+32*2], m1
    movd [blockq+32*3], m1
    psraw     m0, 3
    psubw     m1, m0
    packuswb  m0, m0
    packuswb  m1, m1
    punpcklbw m0, m0
    punpcklbw m1, m1
    punpcklbw m0, m0
    punpcklbw m1, m1

    DEFINE_ARGS dst1, dst2, stride
    lea    dst2q, [dst1q+strideq*2]
    ADD_DC    m0, m1, 0, mova
    RET


INIT_MMX mmx
cglobal vp8_idct_dc_add4uv, 3, 3, 0, dst, block, stride
    movd      m0, [blockq+32*0]
    movd      m1, [blockq+32*2]
    punpcklwd m0, [blockq+32*1]
    punpcklwd m1, [blockq+32*3]
    punpckldq m0, m1
    pxor      m6, m6

    paddw     m0, [wpd_pw_4]
    movd [blockq+32*0], m6
    movd [blockq+32*1], m6
    movd [blockq+32*2], m6
    movd [blockq+32*3], m6
    psraw     m0, 3
    psubw     m6, m0
    packuswb  m0, m0
    packuswb  m6, m6
    punpcklbw m0, m0
    punpcklbw m6, m6
    movq      m1, m0
    movq      m7, m6
    punpcklbw m0, m0
    punpckhbw m1, m1
    punpcklbw m6, m6
    punpckhbw m7, m7

    DEFINE_ARGS dst1, dst2, stride
    lea    dst2q, [dst1q+strideq*2]
    ADD_DC    m0, m6, 0, mova
    lea    dst1q, [dst1q+strideq*4]
    lea    dst2q, [dst2q+strideq*4]
    ADD_DC    m1, m7, 0, mova
    RET


%macro VP8_MULTIPLY_SUMSUB 4
    mova      %3, %1
    mova      %4, %2
    pmulhw    %3, m6
    pmulhw    %4, m6
    paddw     %3, %1
    paddw     %4, %2
    paddw     %1, %1
    paddw     %2, %2
    pmulhw    %1, m7
    pmulhw    %2, m7
    psubw     %1, %4
    paddw     %2, %3
%endmacro

%macro VP8_IDCT_TRANSFORM4x4_1D 6
    SUMSUB_BA         w, %3,  %1,  %5
    VP8_MULTIPLY_SUMSUB m%2, m%4, m%5,m%6
    SUMSUB_BA         w, %4,  %3,  %5
    SUMSUB_BA         w, %2,  %1,  %5
    SWAP                 %4,  %1
    SWAP                 %4,  %3
%endmacro

INIT_MMX sse
cglobal vp8_idct_add, 3, 3, 0, dst, block, stride
    movq         m0, [blockq+ 0]
    movq         m1, [blockq+ 8]
    movq         m2, [blockq+16]
    movq         m3, [blockq+24]
    movq         m6, [pw_20091]
    movq         m7, [pw_17734]
    xorps      xmm0, xmm0
    movaps [blockq+ 0], xmm0
    movaps [blockq+16], xmm0

    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5
    TRANSPOSE4x4W            0, 1, 2, 3, 4
    paddw        m0, [wpd_pw_4]
    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5
    TRANSPOSE4x4W            0, 1, 2, 3, 4

    pxor         m4, m4
    DEFINE_ARGS dst1, dst2, stride
    lea       dst2q, [dst1q+2*strideq]
    STORE_DIFFx2 m0, m1, m6, m7, m4, 3, dst1q, strideq
    STORE_DIFFx2 m2, m3, m6, m7, m4, 3, dst2q, strideq

    RET


%macro SCATTER_WHT 3
    movd dc1d, m%1
    movd dc2d, m%2
    mov [blockq+2*16*(0+%3)], dc1w
    mov [blockq+2*16*(1+%3)], dc2w
    shr  dc1d, 16
    shr  dc2d, 16
    psrlq m%1, 32
    psrlq m%2, 32
    mov [blockq+2*16*(4+%3)], dc1w
    mov [blockq+2*16*(5+%3)], dc2w
    movd dc1d, m%1
    movd dc2d, m%2
    mov [blockq+2*16*(8+%3)], dc1w
    mov [blockq+2*16*(9+%3)], dc2w
    shr  dc1d, 16
    shr  dc2d, 16
    mov [blockq+2*16*(12+%3)], dc1w
    mov [blockq+2*16*(13+%3)], dc2w
%endmacro

%macro HADAMARD4_1D 4
    SUMSUB_BADC w, %2, %1, %4, %3
    SUMSUB_BADC w, %4, %2, %3, %1
    SWAP %1, %4, %3
%endmacro

INIT_MMX sse
cglobal vp8_luma_dc_wht, 2, 3, 0, block, dc1, dc2
    movq          m0, [dc1q]
    movq          m1, [dc1q+8]
    movq          m2, [dc1q+16]
    movq          m3, [dc1q+24]
    xorps      xmm0, xmm0
    movaps [dc1q+ 0], xmm0
    movaps [dc1q+16], xmm0
    HADAMARD4_1D  0, 1, 2, 3
    TRANSPOSE4x4W 0, 1, 2, 3, 4
    paddw         m0, [wpd_pw_3]
    HADAMARD4_1D  0, 1, 2, 3
    psraw         m0, 3
    psraw         m1, 3
    psraw         m2, 3
    psraw         m3, 3
    SCATTER_WHT   0, 1, 0
    SCATTER_WHT   2, 3, 2
    RET
