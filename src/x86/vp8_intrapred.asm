;******************************************************************************
;* VP8 intra prediction asm optimizations
;* Copyright (c) 2010 Fiona Glaser
;* Copyright (c) 2010 Holger Lubitz
;* Copyright (c) 2010 Loren Merritt
;* Copyright (c) 2010 Ronald S. Bultje
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

tm_shuf: times 8 db 0x03, 0x80

; pred4x4_vertical_left, over A (bytes 0-7) then B (bytes 8-15):
;   row0 = A0 A1 A2 A3   row1 = B0 B1 B2 B3
;   row2 = A1 A2 A3 B4   row3 = B1 B2 B3 B5
vl_shuf: db 0, 1, 2, 3, 8, 9, 10, 11, 1, 2, 3, 12, 9, 10, 11, 13

SECTION .text

cextern_naked wpd_pb_1
cextern_naked wpd_pb_3

; On unrolling: a taken branch per iteration costs roughly a fixed amount, so it
; hurts in proportion to how little work the iteration does. Measured on a
; 13700K, replacing the row loop with %rep is worth -53% on pred8x8_horizontal
; and -38% on pred8x8_dc (bodies of two stores), but nothing at all on the
; pred16x16/pred8x8 TrueMotion loops or pred16x16_vertical, whose bodies are
; long enough to hide the branch. Those keep their loops rather than pay four to
; sixteen times the code size for no gain.

;-----------------------------------------------------------------------------
; void ff_pred16x16_vertical_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_XMM sse
cglobal pred16x16_vertical_8, 2,3
    sub   r0, r1
    mov   r2, 4
    movaps xmm0, [r0]
.loop:
    movaps [r0+r1*1], xmm0
    movaps [r0+r1*2], xmm0
    lea   r0, [r0+r1*2]
    movaps [r0+r1*1], xmm0
    movaps [r0+r1*2], xmm0
    lea   r0, [r0+r1*2]
    dec   r2
    jg .loop
    RET

;-----------------------------------------------------------------------------
; void ff_pred16x16_horizontal_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

%macro PRED16x16_H 0
cglobal pred16x16_horizontal_8, 2,3
%if cpuflag(ssse3) && notcpuflag(avx2)
    mova      m2, [wpd_pb_3]
%endif
%rep 8
%if cpuflag(avx2)
    vpbroadcastb m0, [r0+r1*0-1]
    vpbroadcastb m1, [r0+r1*1-1]
%else
    movd      m0, [r0+r1*0-4]
    movd      m1, [r0+r1*1-4]

%if cpuflag(ssse3)
    pshufb    m0, m2
    pshufb    m1, m2
%else
    punpcklbw m0, m0
    punpcklbw m1, m1
    SPLATW    m0, m0, 3
    SPLATW    m1, m1, 3
%endif
%endif

    mova [r0+r1*0], m0
    mova [r0+r1*1], m1
    lea       r0, [r0+r1*2]
%endrep
    RET
%endmacro

INIT_XMM sse2
PRED16x16_H
INIT_XMM ssse3
PRED16x16_H
INIT_XMM avx2
PRED16x16_H

;-----------------------------------------------------------------------------
; void ff_pred16x16_dc_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

; Fill %2 rows of mmsize (or 8, for movq) bytes at r2, stride r1, with m0.
; Four rows per step so the pointer only advances %2/4 times; r3 is dead by the
; time any caller gets here.
%macro DC_FILL 2 ; storeop, rows
    lea       r3, [r1+r1*2]
%rep %2/4
    %1 [r2+r1*0], m0
    %1 [r2+r1*1], m0
    %1 [r2+r1*2], m0
    %1 [r2+r3*1], m0
    lea       r2, [r2+r1*4]
%endrep
%endmacro

%macro PRED16x16_DC 0
cglobal pred16x16_dc_8, 2,7
    mov       r4, r0
    sub       r0, r1
    pxor      mm0, mm0
    pxor      mm1, mm1
    psadbw    mm0, [r0+0]
    psadbw    mm1, [r0+8]
    dec        r0
    movzx     r5d, byte [r0+r1*1]
    paddw     mm0, mm1
    movd      r6d, mm0
    lea        r0, [r0+r1*2]
%rep 7
    movzx     r2d, byte [r0+r1*0]
    movzx     r3d, byte [r0+r1*1]
    add       r5d, r2d
    add       r6d, r3d
    lea        r0, [r0+r1*2]
%endrep
    movzx     r2d, byte [r0+r1*0]
    add       r5d, r6d
    lea       r2d, [r2+r5+16]
    shr       r2d, 5
%if cpuflag(ssse3)
    pxor       m1, m1
%endif
    SPLATB_REG m0, r2, m1

%rep 4
    mova [r4+r1*0], m0
    mova [r4+r1*1], m0
    lea   r4, [r4+r1*2]
    mova [r4+r1*0], m0
    mova [r4+r1*1], m0
    lea   r4, [r4+r1*2]
%endrep
    RET
%endmacro

INIT_XMM sse2
PRED16x16_DC
INIT_XMM ssse3
PRED16x16_DC

;-----------------------------------------------------------------------------
; void ff_pred16x16_top_dc_8(uint8_t *src, ptrdiff_t stride)
; void ff_pred16x16_left_dc_8(uint8_t *src, ptrdiff_t stride)
; void ff_pred16x16_dc_128_8(uint8_t *src, ptrdiff_t stride)
;
; The edge-clamped DC modes, reached only on the frame's first row and column
; (check_intra_pred8x8_mode). Each averages one edge instead of two, so the
; rounding bias halves: (sum + 8) >> 4 rather than (sum + 16) >> 5.
;-----------------------------------------------------------------------------

%macro PRED16x16_TOP_DC 0
cglobal pred16x16_top_dc_8, 2,4
    mov       r2, r0
    sub       r0, r1
    pxor      m1, m1
    mova      m0, [r0]
    psadbw    m0, m1              ; halves land in words 0 and 4
    movhlps   m2, m0
    paddd     m0, m2
    movd     r3d, m0
    add      r3d, 8
    shr      r3d, 4
    SPLATB_REG m0, r3, m1
    DC_FILL mova, 16
    RET
%endmacro

INIT_XMM sse2
PRED16x16_TOP_DC
INIT_XMM ssse3
PRED16x16_TOP_DC

%macro PRED16x16_LEFT_DC 0
cglobal pred16x16_left_dc_8, 2,7
    mov       r2, r0
    dec       r0
    movzx    r4d, byte [r0+r1*0]
    movzx    r5d, byte [r0+r1*1]
    lea       r0, [r0+r1*2]
%rep 7
    movzx    r3d, byte [r0+r1*0]
    movzx    r6d, byte [r0+r1*1]
    add      r4d, r3d
    add      r5d, r6d
    lea       r0, [r0+r1*2]
%endrep
    add      r4d, r5d
    add      r4d, 8
    shr      r4d, 4
    pxor      m1, m1
    SPLATB_REG m0, r4, m1
    DC_FILL mova, 16
    RET
%endmacro

INIT_XMM sse2
PRED16x16_LEFT_DC
INIT_XMM ssse3
PRED16x16_LEFT_DC

; No dc_128 here: it is a plain memset of a compile-time constant, which the C
; compiler already lowers to the same store stream. Measured identical.

;-----------------------------------------------------------------------------
; void ff_pred16x16_tm_vp8_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_XMM sse2
cglobal pred16x16_tm_vp8_8, 2,6,6
    sub          r0, r1
    pxor       xmm2, xmm2
    movdqa     xmm0, [r0]
    movdqa     xmm1, xmm0
    punpcklbw  xmm0, xmm2
    punpckhbw  xmm1, xmm2
    movzx       r4d, byte [r0-1]
    mov         r5d, 8
.loop:
    movzx       r2d, byte [r0+r1*1-1]
    movzx       r3d, byte [r0+r1*2-1]
    sub         r2d, r4d
    sub         r3d, r4d
    movd       xmm2, r2d
    movd       xmm4, r3d
    pshuflw    xmm2, xmm2, 0
    pshuflw    xmm4, xmm4, 0
    punpcklqdq xmm2, xmm2
    punpcklqdq xmm4, xmm4
    movdqa     xmm3, xmm2
    movdqa     xmm5, xmm4
    paddw      xmm2, xmm0
    paddw      xmm3, xmm1
    paddw      xmm4, xmm0
    paddw      xmm5, xmm1
    packuswb   xmm2, xmm3
    packuswb   xmm4, xmm5
    movdqa [r0+r1*1], xmm2
    movdqa [r0+r1*2], xmm4
    lea          r0, [r0+r1*2]
    dec         r5d
    jg .loop
    RET

%if HAVE_AVX2_EXTERNAL
INIT_YMM avx2
cglobal pred16x16_tm_vp8_8, 2, 4, 5, dst, stride, stride3, iteration
    sub                       dstq, strideq
    pmovzxbw                    m0, [dstq]
    vpbroadcastb               xm1, [r0-1]
    pmovzxbw                    m1, xm1
    psubw                       m0, m1
    mov                 iterationd, 4
    lea                   stride3q, [strideq*3]
.loop:
    vpbroadcastb               xm1, [dstq+strideq*1-1]
    vpbroadcastb               xm2, [dstq+strideq*2-1]
    vpbroadcastb               xm3, [dstq+stride3q-1]
    vpbroadcastb               xm4, [dstq+strideq*4-1]
    pmovzxbw                    m1, xm1
    pmovzxbw                    m2, xm2
    pmovzxbw                    m3, xm3
    pmovzxbw                    m4, xm4
    paddw                       m1, m0
    paddw                       m2, m0
    paddw                       m3, m0
    paddw                       m4, m0
    vpackuswb                   m1, m1, m2
    vpackuswb                   m3, m3, m4
    vpermq                      m1, m1, q3120
    vpermq                      m3, m3, q3120
    movdqa        [dstq+strideq*1], xm1
    vextracti128  [dstq+strideq*2], m1, 1
    movdqa       [dstq+stride3q*1], xm3
    vextracti128  [dstq+strideq*4], m3, 1
    lea                       dstq, [dstq+strideq*4]
    dec                 iterationd
    jg .loop
    RET
%endif
INIT_XMM sse2
cglobal pred8x8_vertical_8, 2,2
    sub    r0, r1
    movq   m0, [r0]
%rep 3
    movq [r0+r1*1], m0
    movq [r0+r1*2], m0
    lea    r0, [r0+r1*2]
%endrep
    movq [r0+r1*1], m0
    movq [r0+r1*2], m0
    RET

;-----------------------------------------------------------------------------
; void ff_pred8x8_horizontal_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

%macro PRED8x8_H 0
cglobal pred8x8_horizontal_8, 2,3,3
%if cpuflag(ssse3) && notcpuflag(avx2)
    mova      m2, [wpd_pb_3]
%endif
%rep 4
%if cpuflag(avx2)
    vpbroadcastb m0, [r0+r1*0-1]
    vpbroadcastb m1, [r0+r1*1-1]
%else
    SPLATB_LOAD m0, r0+r1*0-1, m2
    SPLATB_LOAD m1, r0+r1*1-1, m2
%endif
    movq [r0+r1*0], m0
    movq [r0+r1*1], m1
    lea       r0, [r0+r1*2]
%endrep
    RET
%endmacro

INIT_XMM sse2
PRED8x8_H
INIT_XMM ssse3
PRED8x8_H
INIT_XMM avx2
PRED8x8_H

;-----------------------------------------------------------------------------
; void ff_pred8x8_dc_vp8_8_mmxext(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------
INIT_MMX mmxext
cglobal pred8x8_dc_vp8_8, 2,7
    mov       r4, r0
    sub       r0, r1
    pxor      mm0, mm0
    psadbw    mm0, [r0]
    dec        r0
    movzx     r5d, byte [r0+r1*1]
    movd      r6d, mm0
    lea        r0, [r0+r1*2]
%rep 3
    movzx     r2d, byte [r0+r1*0]
    movzx     r3d, byte [r0+r1*1]
    add       r5d, r2d
    add       r6d, r3d
    lea        r0, [r0+r1*2]
%endrep
    movzx     r2d, byte [r0+r1*0]
    add       r5d, r6d
    lea       r2d, [r2+r5+8]
    shr       r2d, 4
    movd      mm0, r2d
    punpcklbw mm0, mm0
    pshufw    mm0, mm0, 0
%rep 4
    movq [r4+r1*0], mm0
    movq [r4+r1*1], mm0
    lea   r4, [r4+r1*2]
%endrep
    RET

;-----------------------------------------------------------------------------
; void ff_pred8x8_top_dc_8(uint8_t *src, ptrdiff_t stride)
; void ff_pred8x8_left_dc_8(uint8_t *src, ptrdiff_t stride)
; void ff_pred8x8_dc_128_8(uint8_t *src, ptrdiff_t stride)
;
; As above at 8x8: one edge, (sum + 4) >> 3.
;-----------------------------------------------------------------------------

%macro PRED8x8_TOP_DC 0
cglobal pred8x8_top_dc_8, 2,4
    mov       r2, r0
    sub       r0, r1
    pxor      m1, m1
    movq      m0, [r0]
    psadbw    m0, m1
    movd     r3d, m0
    add      r3d, 4
    shr      r3d, 3
    SPLATB_REG m0, r3, m1
    DC_FILL movq, 8
    RET
%endmacro

INIT_XMM sse2
PRED8x8_TOP_DC
INIT_XMM ssse3
PRED8x8_TOP_DC

%macro PRED8x8_LEFT_DC 0
cglobal pred8x8_left_dc_8, 2,7
    mov       r2, r0
    dec       r0
    movzx    r4d, byte [r0+r1*0]
    movzx    r5d, byte [r0+r1*1]
    lea       r0, [r0+r1*2]
%rep 3
    movzx    r3d, byte [r0+r1*0]
    movzx    r6d, byte [r0+r1*1]
    add      r4d, r3d
    add      r5d, r6d
    lea       r0, [r0+r1*2]
%endrep
    add      r4d, r5d
    add      r4d, 4
    shr      r4d, 3
    pxor      m1, m1
    SPLATB_REG m0, r4, m1
    DC_FILL movq, 8
    RET
%endmacro

INIT_XMM sse2
PRED8x8_LEFT_DC
INIT_XMM ssse3
PRED8x8_LEFT_DC

;-----------------------------------------------------------------------------
; void ff_pred8x8_tm_vp8_8(uint8_t *src, ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_XMM sse2
cglobal pred8x8_tm_vp8_8, 2,6,4
    sub          r0, r1
    pxor       xmm1, xmm1
    movq       xmm0, [r0]
    punpcklbw  xmm0, xmm1
    movzx       r4d, byte [r0-1]
    mov         r5d, 4
.loop:
    movzx       r2d, byte [r0+r1*1-1]
    movzx       r3d, byte [r0+r1*2-1]
    sub         r2d, r4d
    sub         r3d, r4d
    movd       xmm2, r2d
    movd       xmm3, r3d
    pshuflw    xmm2, xmm2, 0
    pshuflw    xmm3, xmm3, 0
    punpcklqdq xmm2, xmm2
    punpcklqdq xmm3, xmm3
    paddw      xmm2, xmm0
    paddw      xmm3, xmm0
    packuswb   xmm2, xmm3
    movq   [r0+r1*1], xmm2
    movhps [r0+r1*2], xmm2
    lea          r0, [r0+r1*2]
    dec         r5d
    jg .loop
    RET

INIT_XMM ssse3
cglobal pred8x8_tm_vp8_8, 2,3,6
    sub          r0, r1
    movdqa     xmm4, [tm_shuf]
    pxor       xmm1, xmm1
    movq       xmm0, [r0]
    punpcklbw  xmm0, xmm1
    movd       xmm5, [r0-4]
    pshufb     xmm5, xmm4
%rep 4
    movd       xmm2, [r0+r1*1-4]
    movd       xmm3, [r0+r1*2-4]
    pshufb     xmm2, xmm4
    pshufb     xmm3, xmm4
    psubw      xmm2, xmm5
    psubw      xmm3, xmm5
    paddw      xmm2, xmm0
    paddw      xmm3, xmm0
    packuswb   xmm2, xmm3
    movq   [r0+r1*1], xmm2
    movhps [r0+r1*2], xmm2
    lea          r0, [r0+r1*2]
%endrep
    RET

; dest, left, right, src, tmp
; output: %1 = (t[n-1] + t[n]*2 + t[n+1] + 2) >> 2
%macro PRED4x4_LOWPASS 5
    mova    %5, %2
    pavgb   %2, %3
    pxor    %3, %5
%ifnidn %1, %4
    mova    %1, %4
%endif
    pand    %3, [wpd_pb_1]
    psubusb %2, %3
    pavgb   %1, %2
%endmacro

INIT_MMX mmxext
cglobal pred4x4_dc_8, 3,5
    pxor   mm7, mm7
    mov     r4, r0
    sub     r0, r2
    movd   mm0, [r0]
    psadbw mm0, mm7
    movzx  r1d, byte [r0+r2*1-1]
    movd   r3d, mm0
    add    r3d, r1d
    movzx  r1d, byte [r0+r2*2-1]
    lea     r0, [r0+r2*2]
    add    r3d, r1d
    movzx  r1d, byte [r0+r2*1-1]
    add    r3d, r1d
    movzx  r1d, byte [r0+r2*2-1]
    add    r3d, r1d
    add    r3d, 4
    shr    r3d, 3
    imul   r3d, 0x01010101
    mov   [r4+r2*0], r3d
    mov   [r0+r2*0], r3d
    mov   [r0+r2*1], r3d
    mov   [r0+r2*2], r3d
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_tm_vp8_8_mmxext(uint8_t *src, const uint8_t *topright,
;                                 ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_tm_vp8_8, 3,6
    sub        r0, r2
    pxor      mm7, mm7
    movd      mm0, [r0]
    punpcklbw mm0, mm7
    movzx     r4d, byte [r0-1]
    mov       r5d, 2
.loop:
    movzx     r1d, byte [r0+r2*1-1]
    movzx     r3d, byte [r0+r2*2-1]
    sub       r1d, r4d
    sub       r3d, r4d
    movd      mm2, r1d
    movd      mm4, r3d
    pshufw    mm2, mm2, 0
    pshufw    mm4, mm4, 0
    paddw     mm2, mm0
    paddw     mm4, mm0
    packuswb  mm2, mm2
    packuswb  mm4, mm4
    movd [r0+r2*1], mm2
    movd [r0+r2*2], mm4
    lea        r0, [r0+r2*2]
    dec       r5d
    jg .loop
    RET

INIT_XMM ssse3
cglobal pred4x4_tm_vp8_8, 3,3
    sub         r0, r2
    movq       mm6, [tm_shuf]
    pxor       mm1, mm1
    movd       mm0, [r0]
    punpcklbw  mm0, mm1
    movd       mm7, [r0-4]
    pshufb     mm7, mm6
    lea         r1, [r0+r2*2]
    movd       mm2, [r0+r2*1-4]
    movd       mm3, [r0+r2*2-4]
    movd       mm4, [r1+r2*1-4]
    movd       mm5, [r1+r2*2-4]
    pshufb     mm2, mm6
    pshufb     mm3, mm6
    pshufb     mm4, mm6
    pshufb     mm5, mm6
    psubw      mm0, mm7
    paddw      mm2, mm0
    paddw      mm3, mm0
    paddw      mm4, mm0
    paddw      mm5, mm0
    packuswb   mm2, mm2
    packuswb   mm3, mm3
    packuswb   mm4, mm4
    packuswb   mm5, mm5
    movd [r0+r2*1], mm2
    movd [r0+r2*2], mm3
    movd [r1+r2*1], mm4
    movd [r1+r2*2], mm5
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_vertical_vp8_8_mmxext(uint8_t *src, const uint8_t *topright,
;                                       ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_vertical_vp8_8, 3,3
    sub       r0, r2
    movd      m1, [r0-1]
    movd      m0, [r0]
    mova      m2, m0   ;t0 t1 t2 t3
    punpckldq m0, [r1] ;t0 t1 t2 t3 t4 t5 t6 t7
    lea       r1, [r0+r2*2]
    psrlq     m0, 8    ;t1 t2 t3 t4
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    movd [r0+r2*1], m2
    movd [r0+r2*2], m2
    movd [r1+r2*1], m2
    movd [r1+r2*2], m2
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_horizontal_vp8_8_sse2(uint8_t *src, const uint8_t *topright,
;                                       ptrdiff_t stride)
;
; No vector registers: the left edge is five strided single-byte loads, and
; gathering those into a register costs more than the whole transform. Every
; output row is one repeated byte, so an imul splats it across the dword store.
; The last tap is (l2 + 3*l3 + 2) >> 2, i.e. avg3(l2, l3, l3) -- l3 is its own
; right neighbour. The sse2 tag is only the dispatch tier (x86-64 baseline).
;-----------------------------------------------------------------------------

INIT_XMM sse2
cglobal pred4x4_horizontal_vp8_8, 3,7
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movzx    r3d, byte [r0-1]           ; lt
    movzx    r4d, byte [r0+r2*1-1]      ; l0
    movzx    r5d, byte [r0+r2*2-1]      ; l1
    movzx    r6d, byte [r1+r2*1-1]      ; l2

    lea      r3d, [r3+r5+2]             ; lt + l1 + 2
    lea      r3d, [r3+r4*2]             ; + 2*l0
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r0+r2*1], r3d

    lea      r3d, [r4+r6+2]             ; l0 + l2 + 2
    lea      r3d, [r3+r5*2]             ; + 2*l1
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r0+r2*2], r3d

    movzx    r4d, byte [r1+r2*2-1]      ; l3
    lea      r3d, [r5+r4+2]             ; l1 + l3 + 2
    lea      r3d, [r3+r6*2]             ; + 2*l2
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r1+r2*1], r3d

    lea      r3d, [r6+r4+2]             ; l2 + l3 + 2
    lea      r3d, [r3+r4*2]             ; + 2*l3
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r1+r2*2], r3d
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_vertical_left_vp8_8_ssse3(uint8_t *src,
;                                           const uint8_t *topright,
;                                           ptrdiff_t stride)
;
; From seq = t0..t7 build A[i] = avg2(t[i], t[i+1]) and B[i] = avg3(t[i],
; t[i+1], t[i+2]) once. Rows 0 and 1 are A[0..3] and B[0..3]; rows 2 and 3 are
; those shifted by one, except that each ends one step further along B. That
; irregularity is what makes pshufb worth it: concatenating A and B puts every
; output byte in one register, so a single shuffle lays out all four rows.
;-----------------------------------------------------------------------------

INIT_XMM ssse3
cglobal pred4x4_vertical_left_vp8_8, 3,3
    sub        r0, r2
    movd       m0, [r0]        ; t0 t1 t2 t3
    movd       m1, [r1]
    punpckldq  m0, m1          ; t0 t1 t2 t3 t4 t5 t6 t7
    mova       m1, m0
    psrldq     m1, 1           ; t1 ..
    mova       m2, m0
    psrldq     m2, 2           ; t2 ..
    mova       m3, m0
    pavgb      m3, m1          ; A
    PRED4x4_LOWPASS m4, m0, m2, m1, m5  ; B
    punpcklqdq m3, m4          ; A in bytes 0-7, B in bytes 8-15
    pshufb     m3, [vl_shuf]   ; the four rows, packed
    lea        r1, [r0+r2*2]
    movd [r0+r2*1], m3
    psrldq     m3, 4
    movd [r0+r2*2], m3
    psrldq     m3, 4
    movd [r1+r2*1], m3
    psrldq     m3, 4
    movd [r1+r2*2], m3
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_down_left_8_mmxext(uint8_t *src, const uint8_t *topright,
;                                    ptrdiff_t stride)
;-----------------------------------------------------------------------------
INIT_MMX mmxext
cglobal pred4x4_down_left_8, 3,3
    sub       r0, r2
    movq      m1, [r0]
    punpckldq m1, [r1]
    movq      m2, m1
    movq      m0, m1
    psllq     m1, 8
    pxor      m2, m1
    psrlq     m2, 8
    pxor      m2, m0
    PRED4x4_LOWPASS m0, m1, m2, m0, m3
    lea       r1, [r0+r2*2]
    psrlq     m0, 8
    movd      [r0+r2*1], m0
    psrlq     m0, 8
    movd      [r0+r2*2], m0
    psrlq     m0, 8
    movd      [r1+r2*1], m0
    psrlq     m0, 8
    movd      [r1+r2*2], m0
    RET

;------------------------------------------------------------------------------
; void ff_pred4x4_horizontal_up_8_mmxext(uint8_t *src, const uint8_t *topright,
;                                        ptrdiff_t stride)
;------------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_horizontal_up_8, 3,3
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movd      m0, [r0+r2*1-4]
    punpcklbw m0, [r0+r2*2-4]
    movd      m1, [r1+r2*1-4]
    punpcklbw m1, [r1+r2*2-4]
    punpckhwd m0, m1
    movq      m1, m0
    punpckhbw m1, m1
    pshufw    m1, m1, 0xFF
    punpckhdq m0, m1
    movq      m2, m0
    movq      m3, m0
    movq      m7, m0
    psrlq     m2, 16
    psrlq     m3, 8
    pavgb     m7, m3
    PRED4x4_LOWPASS m3, m0, m2, m3, m5
    punpcklbw m7, m3
    movd    [r0+r2*1], m7
    psrlq    m7, 16
    movd    [r0+r2*2], m7
    psrlq    m7, 16
    movd    [r1+r2*1], m7
    movd    [r1+r2*2], m1
    RET

;------------------------------------------------------------------------------
; void ff_pred4x4_horizontal_down_8_mmxext(uint8_t *src,
;                                          const uint8_t *topright,
;                                          ptrdiff_t stride)
;------------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_horizontal_down_8, 3,3
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movh      m0, [r0-4]      ; lt ..
    punpckldq m0, [r0]        ; t3 t2 t1 t0 lt .. .. ..
    psllq     m0, 8           ; t2 t1 t0 lt .. .. .. ..
    movd      m1, [r1+r2*2-4] ; l3
    punpcklbw m1, [r1+r2*1-4] ; l2 l3
    movd      m2, [r0+r2*2-4] ; l1
    punpcklbw m2, [r0+r2*1-4] ; l0 l1
    punpckhwd m1, m2          ; l0 l1 l2 l3
    punpckhdq m1, m0          ; t2 t1 t0 lt l0 l1 l2 l3
    movq      m0, m1
    movq      m2, m1
    movq      m5, m1
    psrlq     m0, 16          ; .. .. t2 t1 t0 lt l0 l1
    psrlq     m2, 8           ; .. t2 t1 t0 lt l0 l1 l2
    pavgb     m5, m2
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    punpcklbw m5, m2
    psrlq     m2, 32
    PALIGNR   m2, m5, 6, m4
    movh      [r1+r2*2], m5
    psrlq     m5, 16
    movh      [r1+r2*1], m5
    psrlq     m5, 16
    movh      [r0+r2*2], m5
    movh      [r0+r2*1], m2
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_vertical_right_8_mmxext(uint8_t *src,
;                                         const uint8_t *topright,
;                                         ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_vertical_right_8, 3,3
    sub     r0, r2
    lea     r1, [r0+r2*2]
    movh    m0, [r0]                    ; ........t3t2t1t0
    movq    m5, m0
    PALIGNR m0, [r0-8], 7, m1           ; ......t3t2t1t0lt
    pavgb   m5, m0
    PALIGNR m0, [r0+r2*1-8], 7, m1      ; ....t3t2t1t0ltl0
    movq    m1, m0
    PALIGNR m0, [r0+r2*2-8], 7, m2      ; ..t3t2t1t0ltl0l1
    movq    m2, m0
    PALIGNR m0, [r1+r2*1-8], 7, m3      ; t3t2t1t0ltl0l1l2
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    movq    m1, m2
    psrlq   m2, 16
    psllq   m1, 48
    movh    [r0+r2*1], m5
    movh    [r0+r2*2], m2
    PALIGNR m5, m1, 7, m3
    psllq   m1, 8
    movh    [r1+r2*1], m5
    PALIGNR m2, m1, 7, m1
    movh    [r1+r2*2], m2
    RET

;-----------------------------------------------------------------------------
; void ff_pred4x4_down_right_8_mmxext(uint8_t *src, const uint8_t *topright,
;                                     ptrdiff_t stride)
;-----------------------------------------------------------------------------

INIT_MMX mmxext
cglobal pred4x4_down_right_8, 3,3
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movq      m1, [r1-8]
    movq      m2, [r0+r2*1-8]
    punpckhbw m2, [r0-8]
    movh      m3, [r0]
    punpckhwd m1, m2
    PALIGNR   m3, m1, 5, m1
    movq      m1, m3
    PALIGNR   m3, [r1+r2*1-8], 7, m4
    movq      m0, m3
    PALIGNR   m3, [r1+r2*2-8], 7, m4
    PRED4x4_LOWPASS m0, m3, m1, m0, m4
    movh      [r1+r2*2], m0
    psrlq     m0, 8
    movh      [r1+r2*1], m0
    psrlq     m0, 8
    movh      [r0+r2*2], m0
    psrlq     m0, 8
    movh      [r0+r2*1], m0
    RET
