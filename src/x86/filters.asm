;******************************************************************************
;* Alpha-plane unfilters
;* Copyright (c) 2026 Halide Compression, LLC
;*
;* This file is part of wpd.
;******************************************************************************

%include "ext/x86/x86util.asm"

SECTION_RODATA 32

pw_255f:    times 16 dw 255
grad_w7:    times 16 db 6, 7
grad_lane1: times 16 db 0
            times 16 db 0xff
grad_mask:  db 0xff
            times 15 db 0

SECTION .text

; All three take (const uint8_t *prev, uint8_t *row, int width) and
; reconstruct row in place. A null prev marks the top row, which is
; left-predicted whatever the mode.

;------------------------------------------------------------------------------
; The running total makes every byte depend on the one before it; adding the
; vector to itself shifted by 1, 2 then 4 turns eight loads into three vector
; adds on that critical path.
INIT_XMM sse2
cglobal horizontal_unfilter, 3, 4, 3, prev, row, w
    test      wd, wd
    jle       .end
    movzx     r3d, byte [rowq]
    test      prevq, prevq
    jz        .seeded
    movzx     prevd, byte [prevq]
    add       r3d, prevd
    mov       [rowq], r3b
    movzx     r3d, r3b
.seeded:
    sub       wd, 1
    jz        .end
    add       rowq, 1
    movd      m0, r3d
    sub       wd, 8
    jl        .tail
.loop:
    movq      m1, [rowq]
    paddb     m1, m0
    mova      m2, m1
    pslldq    m2, 1
    paddb     m1, m2
    mova      m2, m1
    pslldq    m2, 2
    paddb     m1, m2
    mova      m2, m1
    pslldq    m2, 4
    paddb     m1, m2
    movq      [rowq], m1
    mova      m0, m1
    psrlq     m0, 56
    add       rowq, 8
    sub       wd, 8
    jge       .loop
.tail:
    add       wd, 8
    jz        .end
    movd      r3d, m0
.tail_loop:
    add       r3b, [rowq]
    mov       [rowq], r3b
    add       rowq, 1
    sub       wd, 1
    jnz       .tail_loop
.end:
    RET

;------------------------------------------------------------------------------
%macro VERTICAL_UNFILTER 0
cglobal vertical_unfilter, 3, 4, 4, prev, row, w
    test      prevq, prevq
    jz        .top
    sub       wd, 2 * mmsize
    jl        .tail
.loop:
    movu      m0, [prevq]
    movu      m1, [prevq + mmsize]
    movu      m2, [rowq]
    movu      m3, [rowq + mmsize]
    paddb     m0, m2
    paddb     m1, m3
    movu      [rowq], m0
    movu      [rowq + mmsize], m1
    add       prevq, 2 * mmsize
    add       rowq, 2 * mmsize
    sub       wd, 2 * mmsize
    jge       .loop
.tail:
    add       wd, 2 * mmsize
    jz        .end
    cmp       wd, mmsize
    jl        .bytes
    movu      m0, [prevq]
    movu      m2, [rowq]
    paddb     m0, m2
    movu      [rowq], m0
    add       prevq, mmsize
    add       rowq, mmsize
    sub       wd, mmsize
    jz        .end
.bytes:
    movzx     r3d, byte [prevq]
    add       [rowq], r3b
    add       prevq, 1
    add       rowq, 1
    sub       wd, 1
    jnz       .bytes
.end:
    RET
.top:
    jmp       ff_horizontal_unfilter_sse2
%endmacro

INIT_XMM sse2
VERTICAL_UNFILTER
INIT_YMM avx2
VERTICAL_UNFILTER


;------------------------------------------------------------------------------
; One serially reconstructed pixel: left = in + clip(left + top - top_left),
; clamped with cmov so adversarial planes cannot feed the branch predictor.
%macro GRADIENT_PIXEL 0
    movzx     r4d, byte [prevq]
    add       r3d, r4d
    movzx     r4d, byte [prevq - 1]
    sub       r3d, r4d
    mov       r4d, 0
    cmovs     r3d, r4d
    cmp       r3d, 255
    mov       r4d, 255
    cmovg     r3d, r4d
    add       r3b, [rowq]
    mov       [rowq], r3b
    movzx     r3d, r3b
    add       rowq, 1
    add       prevq, 1
%endmacro

; Without the clip and the byte wrap the recurrence telescopes into a plain
; prefix sum, which vectorises. Run that closed form on eight-pixel blocks
; in 16-bit lanes, and keep it only when every intermediate stays inside
; 0..255, so neither the clip nor the wrap could have fired; the rare dirty
; block falls back to the serial loop above.
INIT_XMM sse2
cglobal gradient_unfilter, 3, 6, 8, prev, row, w
    test      prevq, prevq
    jz        .top
    test      wd, wd
    jle       .end
    movzx     r3d, byte [prevq]
    add       r3b, [rowq]
    mov       [rowq], r3b
    movzx     r3d, r3b
    sub       wd, 1
    jz        .end
    add       rowq, 1
    add       prevq, 1
    pxor      m7, m7
    mova      m6, [pw_255f]
    movd      m0, r3d
    pshuflw   m0, m0, 0
    punpcklqdq m0, m0                ; running left, in every word
    sub       wd, 8
    jl        .tail
.loop:
    movq      m1, [prevq]
    movq      m2, [prevq - 1]
    movq      m3, [rowq]
    punpcklbw m1, m7
    punpcklbw m2, m7
    punpcklbw m3, m3
    psraw     m3, 8                  ; residuals sign-extend: byte 255 is -1,
                                     ; and mod 256 the check makes them equal
    psubw     m1, m2
    paddw     m1, m3                 ; in + top - top_left
    mova      m2, m1
    pslldq    m2, 2
    paddw     m1, m2
    mova      m2, m1
    pslldq    m2, 4
    paddw     m1, m2
    mova      m2, m1
    pslldq    m2, 8
    paddw     m1, m2                 ; prefix sums
    paddw     m1, m0                 ; speculative out, unclipped
    mova      m2, m1
    psubw     m2, m3                 ; the predictor each pixel would clip
    mova      m4, m2
    pminsw    m4, m1
    mova      m5, m2
    pmaxsw    m5, m1
    pcmpgtw   m5, m6                 ; anything above 255
    mova      m2, m7
    pcmpgtw   m2, m4                 ; anything below 0
    por       m2, m5
    pmovmskb  r4d, m2
    test      r4d, r4d
    jnz       .slow
    packuswb  m1, m1
    movq      [rowq], m1
    psrldq    m1, 7
    pand      m1, [grad_mask]        ; out[7] seeds the next block
    pshuflw   m0, m1, 0
    punpcklqdq m0, m0
    add       rowq, 8
    add       prevq, 8
.next:
    sub       wd, 8
    jge       .loop
.tail:
    add       wd, 8
    jz        .end
    movd      r3d, m0
    movzx     r3d, r3w               ; every word holds the running left
.tail_loop:
    GRADIENT_PIXEL
    sub       wd, 1
    jnz       .tail_loop
.end:
    RET
.slow:
    movd      r3d, m0
    movzx     r3d, r3w
    mov       r5d, 8
.slow_loop:
    GRADIENT_PIXEL
    sub       r5d, 1
    jnz       .slow_loop
    movd      m0, r3d
    pshuflw   m0, m0, 0
    punpcklqdq m0, m0
    jmp       .next
.top:
    jmp       ff_horizontal_unfilter_sse2

; The same speculative closed form, sixteen pixels per block: the prefix
; sums run inside each 128-bit lane, then the low lane's total carries
; into the high one with one cross-lane permute.
INIT_YMM avx2
cglobal gradient_unfilter, 3, 6, 8, prev, row, w
    test      prevq, prevq
    jz        .top
    test      wd, wd
    jle       .end
    movzx     r3d, byte [prevq]
    add       r3b, [rowq]
    mov       [rowq], r3b
    movzx     r3d, r3b
    sub       wd, 1
    jz        .end
    add       rowq, 1
    add       prevq, 1
    pxor      m7, m7
    mova      m6, [pw_255f]
    movd      xm0, r3d
    vpbroadcastw m0, xm0             ; running left, in every word
    sub       wd, 16
    jl        .tail
.loop:
    pmovzxbw  m1, [prevq]
    pmovzxbw  m2, [prevq - 1]
    pmovsxbw  m3, [rowq]             ; the residuals, sign-extended
    psubw     m1, m2
    paddw     m1, m3                 ; in + top - top_left
    pslldq    m2, m1, 2
    paddw     m1, m2
    pslldq    m2, m1, 4
    paddw     m1, m2
    pslldq    m2, m1, 8
    paddw     m1, m2                 ; prefix sums, per lane
    vpermq    m2, m1, q1111
    pshufb    m2, [grad_w7]          ; the low lane's total, everywhere
    pand      m2, [grad_lane1]
    paddw     m1, m2                 ; carried into the high lane
    paddw     m1, m0                 ; speculative out, unclipped
    psubw     m2, m1, m3             ; the predictor each pixel would clip
    pminsw    m4, m2, m1
    pmaxsw    m5, m2, m1
    pcmpgtw   m5, m6                 ; anything above 255
    pcmpgtw   m4, m7, m4             ; anything below 0
    por       m4, m5
    pmovmskb  r4d, m4
    test      r4d, r4d
    jnz       .slow
    packuswb  m1, m1
    vpermq    m1, m1, q3120
    movu      [rowq], xm1
    psrldq    xm1, 15
    vpbroadcastw m0, xm1             ; out[15] seeds the next block
    add       rowq, 16
    add       prevq, 16
.next:
    sub       wd, 16
    jge       .loop
.tail:
    add       wd, 16
    jz        .end
    movd      r3d, xm0
    movzx     r3d, r3w               ; every word holds the running left
.tail_loop:
    GRADIENT_PIXEL
    sub       wd, 1
    jnz       .tail_loop
.end:
    RET
.slow:
    movd      r3d, xm0
    movzx     r3d, r3w
    mov       r5d, 16
.slow_loop:
    GRADIENT_PIXEL
    sub       r5d, 1
    jnz       .slow_loop
    movd      xm0, r3d
    vpbroadcastw m0, xm0
    jmp       .next
.top:
    jmp       ff_horizontal_unfilter_sse2
