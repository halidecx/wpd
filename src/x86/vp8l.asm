;******************************************************************************
;* AVX2 inverse predictors for the VP8L predictor transform, ported from
;* src/vp8l_dsp.c and mirroring src/aarch64/vp8l_neon.S.
;*
;* Every operation here is per byte, so the decoder's [A, R, G, B] pixel
;* order needs no special care; only PRED_MODE_BLACK names a channel, and
;* it builds its constant from bytes.
;*
;* Predictors 5 to 7 and 10 to 13 depend on the pixel to their left, so
;* those run one pixel at a time in the low dword with the left pixel held
;* in a register. Predictors 0 to 4, 8 and 9 have no such dependency and
;* run sixteen pixels at a time.
;*
;* All of these take (const uint32_t *in, const uint32_t *upper,
;* int num_pixels, uint32_t *out). in and out alias, which is why every
;* block loads its residual before storing the reconstruction. out[-1]
;* holds the pixel to the left, and upper[-1] through upper[num_pixels] may
;* be read, nothing beyond. Neither pointer is better than 4-byte aligned,
;* so every access is unaligned.
;******************************************************************************

%include "asm/x86/x86util.asm"

SECTION_RODATA 32

; The in-lane packs in extract_green interleave the two halves; this puts
; the eight result dwords back into pixel order.
eg_perm: dd 0, 4, 1, 5, 2, 6, 3, 7

SECTION .text

;-----------------------------------------------------------------------------
; pavgb rounds up, but VP8L wants the floor average. Since (a + b) & 1 is
; (a ^ b) & 1, subtracting that off the rounded result gives (a + b) >> 1.
; The correction is computed first so the destination may alias a source.
;-----------------------------------------------------------------------------
%macro AVG2 5 ; dst, a, b, tmp, ones
    pxor       %4, %2, %3
    pand       %4, %4, %5
    pavgb      %1, %2, %3
    psubb      %1, %1, %4
%endmacro

%macro SET_ONES 1 ; dst = 0x01 bytes
    pcmpeqb    %1, %1
    pabsb      %1, %1
%endmacro

;-----------------------------------------------------------------------------
; void ff_pred_add_0(const uint32_t *in, const uint32_t *upper,
;                    int num_pixels, uint32_t *out)
; PRED_MODE_BLACK
;-----------------------------------------------------------------------------

INIT_YMM avx2
cglobal pred_add_0, 4, 4, 3, src, upper, n, dst
    pcmpeqd    m0, m0
    psrld      m0, 24                  ; FF 00 00 00 per pixel
    cmp        nd, 16
    jl .tail4
.loop16:
    movu       m1, [srcq]
    movu       m2, [srcq+32]
    paddb      m1, m1, m0
    paddb      m2, m2, m0
    movu       [dstq], m1
    movu       [dstq+32], m2
    add        srcq, 64
    add        dstq, 64
    sub        nd, 16
    cmp        nd, 16
    jge .loop16
.tail4:
    cmp        nd, 4
    jl .tail1
.loop4:
    movu       xm1, [srcq]
    paddb      xm1, xm1, xm0
    movu       [dstq], xm1
    add        srcq, 16
    add        dstq, 16
    sub        nd, 4
    cmp        nd, 4
    jge .loop4
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movd       xm1, [srcq]
    paddb      xm1, xm1, xm0
    movd       [dstq], xm1
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_1(const uint32_t *in, const uint32_t *upper,
;                    int num_pixels, uint32_t *out)
; PRED_MODE_L, a per-byte prefix sum. The left pixel stays in a register
; rather than being reloaded from out[-1], which keeps a store-to-load
; forwarding hop off the loop-carried chain. upper is never touched, so it
; may be NULL.
;
; A ymm version would need a cross-lane carry (pslldq shifts within each
; 128-bit lane), and its paddb + vpermd recurrence costs four cycles per
; eight pixels -- exactly what paddb + pshufd costs per four here.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_1, 4, 4, 3, src, upper, n, dst
    vpbroadcastd m0, [dstq-4]
    cmp        nd, 4
    jl .tail1
.loop4:
    movu       m1, [srcq]
    pslldq     m2, m1, 4
    paddb      m1, m1, m2
    pslldq     m2, m1, 8
    paddb      m1, m1, m2               ; prefix sum within the quad
    paddb      m1, m1, m0               ; carry in the pixel to the left
    movu       [dstq], m1
    pshufd     m0, m1, 0xff             ; new left, already broadcast
    add        srcq, 16
    add        dstq, 16
    sub        nd, 4
    cmp        nd, 4
    jge .loop4
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movd       m1, [srcq]
    paddb      m0, m1, m0               ; the result is the next left pixel
    movd       [dstq], m0
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_{2,3,4}(const uint32_t *in, const uint32_t *upper,
;                          int num_pixels, uint32_t *out)
; T, TR and TL: one pixel from the row above, at a fixed offset.
;-----------------------------------------------------------------------------

%macro PRED_TOP 2 ; index, byte offset into upper
INIT_YMM avx2
cglobal pred_add_%1, 4, 4, 4, src, upper, n, dst
    cmp        nd, 16
    jl .tail4
.loop16:
    movu       m0, [upperq+%2]
    movu       m1, [upperq+%2+32]
    movu       m2, [srcq]
    movu       m3, [srcq+32]
    paddb      m0, m0, m2
    paddb      m1, m1, m3
    movu       [dstq], m0
    movu       [dstq+32], m1
    add        srcq, 64
    add        upperq, 64
    add        dstq, 64
    sub        nd, 16
    cmp        nd, 16
    jge .loop16
.tail4:
    cmp        nd, 4
    jl .tail1
.loop4:
    movu       xm0, [upperq+%2]
    movu       xm2, [srcq]
    paddb      xm0, xm0, xm2
    movu       [dstq], xm0
    add        srcq, 16
    add        upperq, 16
    add        dstq, 16
    sub        nd, 4
    cmp        nd, 4
    jge .loop4
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movd       xm0, [upperq+%2]
    movd       xm2, [srcq]
    paddb      xm0, xm0, xm2
    movd       [dstq], xm0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET
%endmacro

PRED_TOP 2, 0                            ; T
PRED_TOP 3, 4                            ; TR
PRED_TOP 4, -4                           ; TL

;-----------------------------------------------------------------------------
; void ff_pred_add_{8,9}(const uint32_t *in, const uint32_t *upper,
;                        int num_pixels, uint32_t *out)
; The average of two adjacent pixels in the row above.
;-----------------------------------------------------------------------------

%macro PRED_AVGTOP 3 ; index, offset a, offset b
INIT_YMM avx2
cglobal pred_add_%1, 4, 4, 6, src, upper, n, dst
    SET_ONES   m5
    cmp        nd, 16
    jl .tail4
.loop16:
    movu       m0, [upperq+%2]
    movu       m1, [upperq+%3]
    movu       m3, [srcq]
    AVG2       m2, m0, m1, m4, m5
    paddb      m2, m2, m3
    movu       [dstq], m2
    movu       m0, [upperq+%2+32]
    movu       m1, [upperq+%3+32]
    movu       m3, [srcq+32]
    AVG2       m2, m0, m1, m4, m5
    paddb      m2, m2, m3
    movu       [dstq+32], m2
    add        srcq, 64
    add        upperq, 64
    add        dstq, 64
    sub        nd, 16
    cmp        nd, 16
    jge .loop16
.tail4:
    cmp        nd, 4
    jl .tail1
.loop4:
    movu       xm0, [upperq+%2]
    movu       xm1, [upperq+%3]
    movu       xm3, [srcq]
    AVG2       xm2, xm0, xm1, xm4, xm5
    paddb      xm2, xm2, xm3
    movu       [dstq], xm2
    add        srcq, 16
    add        upperq, 16
    add        dstq, 16
    sub        nd, 4
    cmp        nd, 4
    jge .loop4
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movd       xm0, [upperq+%2]
    movd       xm1, [upperq+%3]
    movd       xm3, [srcq]
    AVG2       xm2, xm0, xm1, xm4, xm5
    paddb      xm2, xm2, xm3
    movd       [dstq], xm2
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET
%endmacro

PRED_AVGTOP 8, -4, 0                     ; avg2(TL, T)
PRED_AVGTOP 9, 0, 4                      ; avg2(T, TR)

;-----------------------------------------------------------------------------
; void ff_pred_add_{6,7}(const uint32_t *in, const uint32_t *upper,
;                        int num_pixels, uint32_t *out)
; The average of the left pixel and one from the row above. The recurrence
; runs through m0, so these go a pixel at a time.
;-----------------------------------------------------------------------------

%macro PRED_AVGLEFT 2 ; index, byte offset into upper
INIT_XMM avx2
cglobal pred_add_%1, 4, 4, 5, src, upper, n, dst
    test       nd, nd
    jz .ret
    SET_ONES   m4
    movd       m0, [dstq-4]
.loop:
    movd       m2, [upperq+%2]
    AVG2       m0, m0, m2, m3, m4
    movd       m1, [srcq]
    paddb      m0, m0, m1
    movd       [dstq], m0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET
%endmacro

PRED_AVGLEFT 6, -4                       ; avg2(L, TL)
PRED_AVGLEFT 7, 0                        ; avg2(L, T)

;-----------------------------------------------------------------------------
; void ff_pred_add_5(const uint32_t *in, const uint32_t *upper,
;                    int num_pixels, uint32_t *out)
; avg3(L, T, TR) is avg2(avg2(L, TR), T) -- note the inner pair.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_5, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    SET_ONES   m4
    movd       m0, [dstq-4]
.loop:
    movd       m2, [upperq]
    movd       m5, [upperq+4]
    AVG2       m0, m0, m5, m3, m4
    AVG2       m0, m0, m2, m3, m4
    movd       m1, [srcq]
    paddb      m0, m0, m1
    movd       [dstq], m0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_10(const uint32_t *in, const uint32_t *upper,
;                     int num_pixels, uint32_t *out)
; avg4(L, TL, T, TR) is avg2(avg2(L, TL), avg2(T, TR)). The residual load is
; held back so the dead TR register can serve as scratch, which keeps this
; inside six vector registers.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_10, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    SET_ONES   m4
    movd       m0, [dstq-4]
.loop:
    movd       m2, [upperq-4]
    movd       m3, [upperq]
    movd       m5, [upperq+4]
    AVG2       m3, m3, m5, m1, m4        ; avg2(T, TR), m1 free until now
    AVG2       m0, m0, m2, m5, m4        ; avg2(L, TL), TR now dead
    AVG2       m0, m0, m3, m5, m4
    movd       m1, [srcq]
    paddb      m0, m0, m1
    movd       [dstq], m0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_11(const uint32_t *in, const uint32_t *upper,
;                     int num_pixels, uint32_t *out)
; PRED_MODE_SELECT: whichever of T and L is closer to TL, summed over the
; four channels. psadbw does the whole sum in one instruction. The C picks
; T when sum|L-TL| - sum|T-TL| <= 0, so L wins only on a strict greater-than
; and ties go to T. Both psadbw operands must be zero above the low dword,
; which holds by induction: m0 is only ever a blend of two movd results.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_11, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    movd       m0, [dstq-4]
.loop:
    movd       m2, [upperq]
    movd       m3, [upperq-4]
    movd       m1, [srcq]
    psadbw     m4, m0, m3                ; sum |L - TL|
    psadbw     m5, m2, m3                ; sum |T - TL|
    paddb      m2, m2, m1
    paddb      m0, m0, m1
    pcmpgtd    m4, m4, m5                ; set where L wins
    vpblendvb  m0, m2, m0, m4
    movd       [dstq], m0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_12(const uint32_t *in, const uint32_t *upper,
;                     int num_pixels, uint32_t *out)
; CLAMPED_ADD_SUBTRACT_FULL: clip_uint8(L + T - TL) per channel. The left
; pixel is kept widened to words so the signed intermediate survives until
; packuswb clamps it; L + T - TL lands in [-255, 510], well inside a word.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_12, 4, 4, 4, src, upper, n, dst
    test       nd, nd
    jz .ret
    movd       m0, [dstq-4]
    pmovzxbw   m0, m0
.loop:
    movd       m2, [upperq]
    pmovzxbw   m2, m2
    movd       m3, [upperq-4]
    pmovzxbw   m3, m3
    psubw      m2, m2, m3
    paddw      m2, m2, m0
    packuswb   m2, m2, m2                ; clip_uint8
    movd       m1, [srcq]
    paddb      m2, m2, m1
    movd       [dstq], m2
    pmovzxbw   m0, m2
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_pred_add_13(const uint32_t *in, const uint32_t *upper,
;                     int num_pixels, uint32_t *out)
; CLAMPED_ADD_SUBTRACT_HALF: clip_uint8(ave + (ave - TL) / 2) per channel,
; where ave is the floor average of L and T. Working in words makes the
; average exact and lets the halving match C, whose division truncates
; toward zero: biasing the difference by its sign bit before an arithmetic
; shift rounds negatives the same way.
;-----------------------------------------------------------------------------

INIT_XMM avx2
cglobal pred_add_13, 4, 4, 5, src, upper, n, dst
    test       nd, nd
    jz .ret
    movd       m0, [dstq-4]
    pmovzxbw   m0, m0
.loop:
    movd       m2, [upperq]
    pmovzxbw   m2, m2
    movd       m3, [upperq-4]
    pmovzxbw   m3, m3
    paddw      m2, m2, m0
    psrlw      m2, 1                     ; ave = (L + T) >> 1
    psubw      m3, m2, m3                ; ave - TL, in [-255, 255]
    psrlw      m4, m3, 15
    paddw      m3, m3, m4
    psraw      m3, 1                     ; halved toward zero
    paddw      m2, m2, m3
    packuswb   m2, m2, m2                ; clip_uint8
    movd       m1, [srcq]
    paddb      m2, m2, m1
    movd       [dstq], m2
    pmovzxbw   m0, m2
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET

;-----------------------------------------------------------------------------
; void ff_extract_green(uint8_t *dst, const uint8_t *src, int num_pixels)
; Note the argument order differs from the predictors: dst comes first.
;
; The shift leaves alpha in the next byte up, so the mask is not optional.
; The tail stays narrow because dst is an alpha plane row and must not be
; written past num_pixels, however much padding the source has.
;-----------------------------------------------------------------------------

INIT_YMM avx2
cglobal extract_green, 3, 4, 6, dst, src, n
    pcmpeqd    m4, m4
    psrld      m4, 24                    ; 00 00 00 FF per pixel
    mova       m5, [eg_perm]
    cmp        nd, 32
    jl .tail8
.loop32:
    movu       m0, [srcq]
    movu       m1, [srcq+32]
    movu       m2, [srcq+64]
    movu       m3, [srcq+96]
    psrld      m0, 16
    psrld      m1, 16
    psrld      m2, 16
    psrld      m3, 16
    pand       m0, m0, m4
    pand       m1, m1, m4
    pand       m2, m2, m4
    pand       m3, m3, m4
    packusdw   m0, m0, m1
    packusdw   m2, m2, m3
    packuswb   m0, m0, m2
    vpermd     m0, m5, m0
    movu       [dstq], m0
    add        srcq, 128
    add        dstq, 32
    sub        nd, 32
    cmp        nd, 32
    jge .loop32
.tail8:
    cmp        nd, 8
    jl .tail1
.loop8:
    movu       xm0, [srcq]
    movu       xm1, [srcq+16]
    psrld      xm0, 16
    psrld      xm1, 16
    pand       xm0, xm0, xm4
    pand       xm1, xm1, xm4
    packusdw   xm0, xm0, xm1
    packuswb   xm0, xm0, xm0
    movq       [dstq], xm0
    add        srcq, 32
    add        dstq, 8
    sub        nd, 8
    cmp        nd, 8
    jge .loop8
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movzx      r3d, byte [srcq+2]
    mov        [dstq], r3b
    add        srcq, 4
    inc        dstq
    dec        nd
    jg .loop1
.ret:
    RET
