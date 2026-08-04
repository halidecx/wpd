;******************************************************************************
;* VP8 ASM optimizations
;* Copyright (c) 2010 Ronald S. Bultje <rsbultje@gmail.com>
;* Copyright (c) 2010 Fiona Glaser <fiona@x264.com>
;*
;* This file is part of FFmpeg.
;*
;* FFmpeg is free software; you can redistribute it and/or
;* modify it under the terms of the GNU Lesser General Public
;* License as published by the Free Software Foundation; either
;* version 2.1 of the License, or (at your option) any later version.
;*
;* FFmpeg is distributed in the hope that it will be useful,
;* but WITHOUT ANY WARRANTY; without even the implied warranty of
;* MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
;* Lesser General Public License for more details.
;*
;* You should have received a copy of the GNU Lesser General Public
;* License along with FFmpeg; if not, write to the Free Software
;* Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA
;******************************************************************************

%include "asm/x86/x86util.asm"

SECTION_RODATA

pw_20091: times 4 dw 20091
pw_17734: times 4 dw 17734

cextern_naked wpd_pw_3
cextern_naked wpd_pw_4

SECTION .text

;-----------------------------------------------------------------------------
; void ff_vp8_idct_dc_add_<opt>(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
;-----------------------------------------------------------------------------

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
    ; load data
    movd       m0, [blockq]
    pxor       m1, m1

    ; calculate DC
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

;-----------------------------------------------------------------------------
; void ff_vp8_idct_dc_add4y_<opt>(uint8_t *dst, int16_t block[4][16], ptrdiff_t stride);
;-----------------------------------------------------------------------------

INIT_XMM sse2
cglobal vp8_idct_dc_add4y, 3, 3, 6, dst, block, stride
    ; load data
    movd      m0, [blockq+32*0] ; A
    movd      m1, [blockq+32*2] ; C
    punpcklwd m0, [blockq+32*1] ; A B
    punpcklwd m1, [blockq+32*3] ; C D
    punpckldq m0, m1        ; A B C D
    pxor      m1, m1

    ; calculate DC
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

    ; add DC
    DEFINE_ARGS dst1, dst2, stride
    lea    dst2q, [dst1q+strideq*2]
    ADD_DC    m0, m1, 0, mova
    RET

;-----------------------------------------------------------------------------
; void ff_vp8_idct_dc_add4uv_<opt>(uint8_t *dst, int16_t block[4][16], ptrdiff_t stride);
;-----------------------------------------------------------------------------

INIT_MMX mmx
cglobal vp8_idct_dc_add4uv, 3, 3, 0, dst, block, stride
    ; load data
    movd      m0, [blockq+32*0] ; A
    movd      m1, [blockq+32*2] ; C
    punpcklwd m0, [blockq+32*1] ; A B
    punpcklwd m1, [blockq+32*3] ; C D
    punpckldq m0, m1        ; A B C D
    pxor      m6, m6

    ; calculate DC
    paddw     m0, [wpd_pw_4]
    movd [blockq+32*0], m6
    movd [blockq+32*1], m6
    movd [blockq+32*2], m6
    movd [blockq+32*3], m6
    psraw     m0, 3
    psubw     m6, m0
    packuswb  m0, m0
    packuswb  m6, m6
    punpcklbw m0, m0 ; AABBCCDD
    punpcklbw m6, m6 ; AABBCCDD
    movq      m1, m0
    movq      m7, m6
    punpcklbw m0, m0 ; AAAABBBB
    punpckhbw m1, m1 ; CCCCDDDD
    punpcklbw m6, m6 ; AAAABBBB
    punpckhbw m7, m7 ; CCCCDDDD

    ; add DC
    DEFINE_ARGS dst1, dst2, stride
    lea    dst2q, [dst1q+strideq*2]
    ADD_DC    m0, m6, 0, mova
    lea    dst1q, [dst1q+strideq*4]
    lea    dst2q, [dst2q+strideq*4]
    ADD_DC    m1, m7, 0, mova
    RET

;-----------------------------------------------------------------------------
; void ff_vp8_idct_add_<opt>(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
;-----------------------------------------------------------------------------

; calculate %1=mul_35468(%1)-mul_20091(%2); %2=mul_20091(%1)+mul_35468(%2)
;           this macro assumes that m6/m7 have words for 20091/17734 loaded
%macro VP8_MULTIPLY_SUMSUB 4
    mova      %3, %1
    mova      %4, %2
    pmulhw    %3, m6 ;20091(1)
    pmulhw    %4, m6 ;20091(2)
    paddw     %3, %1
    paddw     %4, %2
    paddw     %1, %1
    paddw     %2, %2
    pmulhw    %1, m7 ;35468(1)
    pmulhw    %2, m7 ;35468(2)
    psubw     %1, %4
    paddw     %2, %3
%endmacro

; calculate x0=%1+%3; x1=%1-%3
;           x2=mul_35468(%2)-mul_20091(%4); x3=mul_20091(%2)+mul_35468(%4)
;           %1=x0+x3 (tmp0); %2=x1+x2 (tmp1); %3=x1-x2 (tmp2); %4=x0-x3 (tmp3)
;           %5/%6 are temporary registers
;           we assume m6/m7 have constant words 20091/17734 loaded in them
%macro VP8_IDCT_TRANSFORM4x4_1D 6
    SUMSUB_BA         w, %3,  %1,  %5     ;t0, t1
    VP8_MULTIPLY_SUMSUB m%2, m%4, m%5,m%6 ;t2, t3
    SUMSUB_BA         w, %4,  %3,  %5     ;tmp0, tmp3
    SUMSUB_BA         w, %2,  %1,  %5     ;tmp1, tmp2
    SWAP                 %4,  %1
    SWAP                 %4,  %3
%endmacro

INIT_MMX sse
cglobal vp8_idct_add, 3, 3, 0, dst, block, stride
    ; load block data
    movq         m0, [blockq+ 0]
    movq         m1, [blockq+ 8]
    movq         m2, [blockq+16]
    movq         m3, [blockq+24]
    movq         m6, [pw_20091]
    movq         m7, [pw_17734]
    xorps      xmm0, xmm0
    movaps [blockq+ 0], xmm0
    movaps [blockq+16], xmm0

    ; actual IDCT
    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5
    TRANSPOSE4x4W            0, 1, 2, 3, 4
    paddw        m0, [wpd_pw_4]
    VP8_IDCT_TRANSFORM4x4_1D 0, 1, 2, 3, 4, 5
    TRANSPOSE4x4W            0, 1, 2, 3, 4

    ; store
    pxor         m4, m4
    DEFINE_ARGS dst1, dst2, stride
    lea       dst2q, [dst1q+2*strideq]
    STORE_DIFFx2 m0, m1, m6, m7, m4, 3, dst1q, strideq
    STORE_DIFFx2 m2, m3, m6, m7, m4, 3, dst2q, strideq

    RET

;-----------------------------------------------------------------------------
; void ff_vp8_luma_dc_wht(int16_t block[4][4][16], int16_t dc[16])
;-----------------------------------------------------------------------------

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
