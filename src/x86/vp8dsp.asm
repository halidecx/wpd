%include "asm/x86/x86util.asm"

SECTION_RODATA

pw_20091: times 8 dw 20091
pw_17734: times 8 dw 17734
pw_3:     times 8 dw 3
pw_4:     times 8 dw 4

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

    paddw      m0, [pw_4]
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

    paddw     m0, [pw_4]
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


%macro ADD_DC_2ROWS 3
    movq      m6, [%3]
    movhps    m6, [%3+strideq]
    paddusb   m6, %1
    psubusb   m6, %2
    movq    [%3], m6
    movhps  [%3+strideq], m6
%endmacro

INIT_XMM sse2
cglobal vp8_idct_dc_add4uv, 3, 3, 7, dst, block, stride
    movd      m0, [blockq+32*0]
    movd      m1, [blockq+32*2]
    punpcklwd m0, [blockq+32*1]
    punpcklwd m1, [blockq+32*3]
    punpckldq m0, m1
    pxor      m1, m1

    paddw     m0, [pw_4]
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
    pshufd    m2, m0, q1010
    pshufd    m3, m0, q3232
    pshufd    m4, m1, q1010
    pshufd    m5, m1, q3232

    DEFINE_ARGS dst1, dst2, stride
    lea    dst2q, [dst1q+strideq*2]
    ADD_DC_2ROWS m2, m4, dst1q
    ADD_DC_2ROWS m2, m4, dst2q
    lea    dst1q, [dst1q+strideq*4]
    lea    dst2q, [dst2q+strideq*4]
    ADD_DC_2ROWS m3, m5, dst1q
    ADD_DC_2ROWS m3, m5, dst2q
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

%macro TRANSPOSE4x4W_LO 4
    punpcklwd %1, %2
    punpcklwd %3, %4
    mova      %2, %1
    punpckldq %1, %3
    punpckhdq %2, %3
    pshufd    %4, %2, q3232
    mova      %3, %2
    pshufd    %2, %1, q3232
%endmacro

%macro STORE_DIFF_2ROWS 5
    movd      %2, [%4]
    movd      %3, [%4+%5]
    punpckldq %2, %3
    punpcklbw %2, m7
    psraw     %1, 3
    paddw     %1, %2
    packuswb  %1, %1
    movd    [%4], %1
    psrldq    %1, 4
    movd [%4+%5], %1
%endmacro

INIT_XMM sse2
cglobal vp8_idct_add, 3, 3, 8, dst, block, stride
    mova         m0, [blockq+ 0]
    mova         m2, [blockq+16]
    pshufd       m1, m0, q3232
    pshufd       m3, m2, q3232
    mova         m6, [pw_20091]
    mova         m7, [pw_17734]
    pxor         m4, m4
    mova [blockq+ 0], m4
    mova [blockq+16], m4

    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5
    TRANSPOSE4x4W_LO         m0, m1, m2, m3
    paddw        m0, [pw_4]
    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5

    punpcklwd    m0, m1
    punpcklwd    m2, m3
    mova         m1, m0
    punpckldq    m0, m2
    punpckhdq    m1, m2

    pxor         m7, m7
    DEFINE_ARGS dst1, dst2, stride
    lea       dst2q, [dst1q+2*strideq]
    STORE_DIFF_2ROWS m0, m4, m5, dst1q, strideq
    STORE_DIFF_2ROWS m1, m4, m5, dst2q, strideq
    RET


%macro SCATTER_WHT 2
%assign %%i 0
%rep 4
%if cpuflag(sse4)
    pextrw [blockq+2*16*(%2+4*%%i)], %1, %%i
%else
    pextrw dc1d, %1, %%i
    mov [blockq+2*16*(%2+4*%%i)], dc1w
%endif
%assign %%i %%i+1
%endrep
%endmacro

%macro HADAMARD4_1D 4
    SUMSUB_BADC w, %2, %1, %4, %3
    SUMSUB_BADC w, %4, %2, %3, %1
    SWAP %1, %4, %3
%endmacro

%macro VP8_LUMA_DC_WHT 0
cglobal vp8_luma_dc_wht, 2, 2, 5, block, dc1
    mova          m0, [dc1q]
    mova          m2, [dc1q+16]
    pshufd        m1, m0, q3232
    pshufd        m3, m2, q3232
    pxor          m4, m4
    mova   [dc1q+ 0], m4
    mova   [dc1q+16], m4
    HADAMARD4_1D  0, 1, 2, 3
    TRANSPOSE4x4W_LO m0, m1, m2, m3
    paddw         m0, [pw_3]
    HADAMARD4_1D  0, 1, 2, 3
    psraw         m0, 3
    psraw         m1, 3
    psraw         m2, 3
    psraw         m3, 3
    SCATTER_WHT   m0, 0
    SCATTER_WHT   m1, 1
    SCATTER_WHT   m2, 2
    SCATTER_WHT   m3, 3
    RET
%endmacro

INIT_XMM sse2
VP8_LUMA_DC_WHT
INIT_XMM sse4
VP8_LUMA_DC_WHT
