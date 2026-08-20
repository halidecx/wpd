;******************************************************************************
;* Area-rescaler row kernels
;* Copyright (c) 2026 Halide Compression, LLC
;*
;* This file is part of wpd.
;******************************************************************************

%include "ext/x86/x86util.asm"

%if ARCH_X86_64

SECTION_RODATA 16

pq_rrounder: times 2 dq 0x80000000
pd_odd:      dd 0, -1, 0, -1

SECTION .text

; The fixed point is 32 bits, so (x * y + rounder) >> 32 leaves even lanes'
; results in low dwords and odd lanes' in high ones; masking and ORing
; reassembles the row without a cross-lane shuffle.

; Load eight dwords as even lanes in the first two registers and odd lanes
; in the qword tops of the last two.
%macro LOAD_PAIRS 5 ; addr, even0, even1, odd0, odd1
    movu      %2, [%1]
    movu      %3, [%1 + 16]
    mova      %4, %2
    psrlq     %4, 32
    mova      %5, %3
    psrlq     %5, 32
%endmacro

; Multiply-fix eight values by m6 with rounder m7, clip to bytes, store.
%macro PROCESS_ROW 5 ; even0, even1, odd0, odd1, dst addr
    pmuludq   %1, m6
    pmuludq   %2, m6
    pmuludq   %3, m6
    pmuludq   %4, m6
    paddq     %1, m7
    paddq     %2, m7
    paddq     %3, m7
    paddq     %4, m7
    psrlq     %1, 32
    psrlq     %2, 32
    pand      %3, [pd_odd]
    pand      %4, [pd_odd]
    por       %1, %3
    por       %2, %4
    packssdw  %1, %2
    packuswb  %1, %1
    movq      [%5], %1
%endmacro

; One value in m0's low dword, multiply-fixed by m6/m7 into a byte store.
%macro PROCESS_ONE 3 ; dst addr, scratch reg32, scratch reg8
    pmuludq   m0, m6
    paddq     m0, m7
    psrlq     m0, 32
    packssdw  m0, m0
    packuswb  m0, m0
    movd      %2, m0
    mov       [%1], %3
%endmacro

; void ff_rescale_export_direct_sse2(uint8_t *dst, const uint32_t *frow,
;                                    int n, uint32_t fy_scale)
INIT_XMM sse2
cglobal rescale_export_direct, 4, 5, 8, dst, frow, n, fy
    movd      m6, fyd
    pshufd    m6, m6, q3030
    mova      m7, [pq_rrounder]
    sub       nd, 8
    jl        .tail
.loop:
    LOAD_PAIRS frowq, m0, m1, m2, m3
    PROCESS_ROW m0, m1, m2, m3, dstq
    add       frowq, 32
    add       dstq, 8
    sub       nd, 8
    jge       .loop
.tail:
    add       nd, 8
    jz        .end
.tail_loop:
    movd      m0, [frowq]
    PROCESS_ONE dstq, r4d, r4b
    add       frowq, 4
    add       dstq, 1
    sub       nd, 1
    jnz       .tail_loop
.end:
    RET

; void ff_rescale_export_blend_sse2(uint8_t *dst, const uint32_t *irow,
;                                   const uint32_t *frow, int n,
;                                   uint32_t fy_scale, uint32_t wa,
;                                   uint32_t wb)
INIT_XMM sse2
cglobal rescale_export_blend, 7, 8, 14, dst, irow, frow, n, fy, wa, wb
    movd      m6, fyd
    pshufd    m6, m6, q3030
    mova      m7, [pq_rrounder]
    movd      m12, wad
    pshufd    m12, m12, q3030        ; the frow weight
    movd      m13, wbd
    pshufd    m13, m13, q3030        ; the irow weight
    sub       nd, 8
    jl        .tail
.loop:
    LOAD_PAIRS frowq, m0, m1, m2, m3
    LOAD_PAIRS irowq, m4, m5, m8, m9
    pmuludq   m0, m12
    pmuludq   m1, m12
    pmuludq   m2, m12
    pmuludq   m3, m12
    pmuludq   m4, m13
    pmuludq   m5, m13
    pmuludq   m8, m13
    pmuludq   m9, m13
    paddq     m0, m4
    paddq     m1, m5
    paddq     m2, m8
    paddq     m3, m9
    paddq     m0, m7
    paddq     m1, m7
    paddq     m2, m7
    paddq     m3, m7
    psrlq     m0, 32
    psrlq     m1, 32
    psrlq     m2, 32
    psrlq     m3, 32
    PROCESS_ROW m0, m1, m2, m3, dstq
    add       frowq, 32
    add       irowq, 32
    add       dstq, 8
    sub       nd, 8
    jge       .loop
.tail:
    add       nd, 8
    jz        .end
.tail_loop:
    movd      m0, [frowq]
    movd      m1, [irowq]
    pmuludq   m0, m12
    pmuludq   m1, m13
    paddq     m0, m1
    paddq     m0, m7
    psrlq     m0, 32
    PROCESS_ONE dstq, r7d, r7b
    add       frowq, 4
    add       irowq, 4
    add       dstq, 1
    sub       nd, 1
    jnz       .tail_loop
.end:
    RET

; void ff_rescale_export_shrink_sse2(uint8_t *dst, uint32_t *irow,
;                                    const uint32_t *frow, int n,
;                                    uint32_t yscale, uint32_t fxy_scale)
INIT_XMM sse2
cglobal rescale_export_shrink, 6, 7, 11, dst, irow, frow, n, yscale, fxy
    movd      m6, fxyd
    pshufd    m6, m6, q3030
    mova      m7, [pq_rrounder]
    movd      m10, yscaled
    pshufd    m10, m10, q3030
    sub       nd, 8
    jl        .tail
.loop:
    LOAD_PAIRS irowq, m0, m1, m2, m3
    LOAD_PAIRS frowq, m4, m5, m8, m9
    pmuludq   m4, m10
    pmuludq   m5, m10
    pmuludq   m8, m10
    pmuludq   m9, m10
    psrlq     m4, 32                 ; the carried fraction, floored
    psrlq     m5, 32
    psrlq     m8, 32
    psrlq     m9, 32
    psubq     m0, m4                 ; irow - frac
    psubq     m1, m5
    psubq     m2, m8
    psubq     m3, m9
    psllq     m8, 32
    por       m4, m8
    psllq     m9, 32
    por       m5, m9
    movu      [irowq], m4            ; the fraction starts the next row
    movu      [irowq + 16], m5
    PROCESS_ROW m0, m1, m2, m3, dstq
    add       frowq, 32
    add       irowq, 32
    add       dstq, 8
    sub       nd, 8
    jge       .loop
.tail:
    add       nd, 8
    jz        .end
.tail_loop:
    movd      m1, [frowq]
    pmuludq   m1, m10
    psrlq     m1, 32
    movd      m0, [irowq]
    psubq     m0, m1
    movd      [irowq], m1
    PROCESS_ONE dstq, r6d, r6b
    add       frowq, 4
    add       irowq, 4
    add       dstq, 1
    sub       nd, 1
    jnz       .tail_loop
.end:
    RET

; void ff_rescale_export_shrink0_sse2(uint8_t *dst, uint32_t *irow, int n,
;                                     uint32_t fxy_scale)
INIT_XMM sse2
cglobal rescale_export_shrink0, 4, 5, 8, dst, irow, n, fxy
    movd      m6, fxyd
    pshufd    m6, m6, q3030
    mova      m7, [pq_rrounder]
    pxor      m4, m4
    sub       nd, 8
    jl        .tail
.loop:
    LOAD_PAIRS irowq, m0, m1, m2, m3
    movu      [irowq], m4
    movu      [irowq + 16], m4
    PROCESS_ROW m0, m1, m2, m3, dstq
    add       irowq, 32
    add       dstq, 8
    sub       nd, 8
    jge       .loop
.tail:
    add       nd, 8
    jz        .end
.tail_loop:
    movd      m0, [irowq]
    mov       dword [irowq], 0
    PROCESS_ONE dstq, r4d, r4b
    add       irowq, 4
    add       dstq, 1
    sub       nd, 1
    jnz       .tail_loop
.end:
    RET

; Interleave a pair of pixels channel-wise so pmaddwd blends left and right
; with the accumulator weights in one step.
%macro LOAD_TWO_PIXELS 2 ; dst, addr
    movq      %1, [%2]
    punpcklbw %1, m7
    mova      m5, %1
    psrldq    m5, 8
    punpcklwd %1, m5
%endmacro

; void ff_rescale_import_expand_sse2(uint32_t *frow, const uint8_t *src,
;                                    int n, int src_width, int channels,
;                                    int x_add, int x_sub)
; The caller keeps src_width >= 8 and x_add < 1 << 15 so the weights fit
; pmaddwd's signed words.
INIT_XMM sse2
cglobal rescale_import_expand, 7, 9, 8, frow, src, n, srcw, ch, x_add, x_sub
    pxor      m7, m7
    mov       r7d, x_addd            ; accum
    cmp       chd, 4
    jne       .luma
    LOAD_TWO_PIXELS m0, srcq
.loop4:
    mov       r8d, x_addd
    sub       r8d, r7d
    shl       r8d, 16
    or        r8d, r7d
    movd      m1, r8d
    pshufd    m1, m1, q0000
    mova      m2, m0
    pmaddwd   m2, m1                 ; left * accum + right * (x_add - accum)
    movu      [frowq], m2
    add       frowq, 16
    sub       nd, 4
    jz        .end
    sub       r7d, x_subd
    jns       .loop4
    add       srcq, 4
    LOAD_TWO_PIXELS m0, srcq
    add       r7d, x_addd
    jmp       .loop4
.luma:
    lea       srcwq, [srcq + srcwq - 8] ; the last full eight-pixel load
    movq      m0, [srcq]
    punpcklbw m0, m7
    add       srcq, 7
    mov       chd, 7                 ; pixels left in the window
.loop1:
    mov       r8d, x_addd
    sub       r8d, r7d
    shl       r8d, 16
    or        r8d, r7d
    movd      m1, r8d
    mova      m2, m0
    pmaddwd   m2, m1
    movd      [frowq], m2
    add       frowq, 4
    sub       nd, 1
    jz        .end
    sub       r7d, x_subd
    jns       .loop1
    add       r7d, x_addd
    sub       chd, 1
    jz        .reload
    psrldq    m0, 2
    jmp       .loop1
.reload:
    cmp       srcq, srcwq
    ja        .straggle
    movq      m0, [srcq]
    punpcklbw m0, m7
    add       srcq, 7
    mov       chd, 7
    jmp       .loop1
.straggle:
    psrldq    m0, 2
    movzx     r8d, byte [srcq + 1]
    pinsrw    m0, r8d, 1
    add       srcq, 1
    mov       chd, 1
    jmp       .loop1
.end:
    RET

; void ff_rescale_import_shrink_sse2(uint32_t *frow, const uint8_t *src,
;                                    int n, int x_add, int x_sub,
;                                    uint32_t fx_scale)
; Four channels only, and the caller keeps x_add <= x_sub << 7 so
; sum * x_sub stays inside sixteen unsigned bits.
INIT_XMM sse2
cglobal rescale_import_shrink, 6, 8, 10, frow, src, n, x_add, x_sub, fx
    pxor      m0, m0
    movd      m1, x_subd
    pshuflw   m1, m1, q0000
    punpcklqdq m1, m1
    movd      m2, fxd
    pshufd    m2, m2, q3030
    mova      m3, [pq_rrounder]
    pxor      m4, m4                 ; carried sum
    xor       r6d, r6d               ; accum
.loop:
    add       r6d, x_addd
.inner:
    movd      m5, [srcq]
    punpcklbw m5, m0
    paddw     m4, m5                 ; sum += base
    add       srcq, 4
    sub       r6d, x_subd
    jg        .inner
    mov       r7d, r6d
    neg       r7d
    movd      m6, r7d
    pshuflw   m6, m6, q0000
    punpcklqdq m6, m6                ; -accum
    mova      m7, m5
    pmullw    m7, m6
    mova      m8, m5
    pmulhuw   m8, m6
    punpcklwd m7, m8                 ; frac = base * -accum, in dwords
    mova      m8, m4
    pmullw    m8, m1
    mova      m9, m4
    pmulhuw   m9, m1
    punpcklwd m8, m9                 ; sum * x_sub
    psubd     m8, m7
    movu      [frowq], m8
    mova      m8, m7
    psrlq     m8, 32
    pmuludq   m7, m2                 ; multiply-fix the fraction back down
    pmuludq   m8, m2
    paddq     m7, m3
    paddq     m8, m3
    pshufd    m7, m7, q0031
    pshufd    m8, m8, q0031
    punpckldq m7, m8
    packssdw  m7, m0
    mova      m4, m7                 ; it starts the next pixel's sum
    add       frowq, 16
    sub       nd, 4
    jg        .loop
    RET

%endif ; ARCH_X86_64
