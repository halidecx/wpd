%include "asm/x86/x86util.asm"

SECTION_RODATA 32

pb_1:       times 32 db 1
pw_255:     times 16 dw 255
pw_19077:   times 16 dw 19077
pw_26149:   times 16 dw 26149
pw_14234:   times 16 dw 14234
pw_6419:    times 16 dw 6419
pw_13320:   times 16 dw 13320
pw_8708:    times 16 dw 8708
pw_33050:   times 16 dw 33050
pw_17685:   times 16 dw 17685
pd_rgbmask: times  8 dd 0xffffff00
pw_1:       times 16 dw 1
pd_alpha:   times  8 dd 0x000000ff
pd_alpha3:  times  8 dd 0xff000000

; ARGB -> RGBA, BGRA, RGB and BGR byte selectors.
shuf_rgba:  db 1, 2, 3, 0, 5, 6, 7, 4, 9, 10, 11, 8, 13, 14, 15, 12
            db 1, 2, 3, 0, 5, 6, 7, 4, 9, 10, 11, 8, 13, 14, 15, 12
shuf_bgra:  db 3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12
            db 3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12
shuf_rgb:   db 1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15, -1, -1, -1, -1
shuf_bgr:   db 3, 2, 1, 7, 6, 5, 11, 10, 9, 15, 14, 13, -1, -1, -1, -1
; Broadcasts each pixel's alpha over its four bytes.
shuf_bcasta: db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12
             db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12
shuf_bcasta3: db 3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15
              db 3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15

SECTION .text

%if ARCH_X86_64

; Reconstructs 32 chroma samples for each of the two output rows out of 17
; input samples, using the byte-domain identities from libwebp's SSE2
; upsampler: with s = (a+d+1)/2, t = (b+c+1)/2 and k = (a+b+c+d)/4,
; (9a+3b+3c+d+8)/16 collapses to avg(a, m) for m = (a+3b+3c+d)/8.
%macro RECON_UV 4 ; top, cur, top_scratch, bottom_scratch
    movu      xmm0, [%1]
    movu      xmm1, [%1 + 1]
    movu      xmm2, [%2]
    movu      xmm3, [%2 + 1]
    mova      xmm7, [pb_1]
    pavgb     xmm4, xmm0, xmm3          ; s
    pavgb     xmm5, xmm1, xmm2          ; t
    pxor      xmm6, xmm4, xmm5          ; s^t
    pxor      xmm8, xmm0, xmm3          ; a^d
    pxor      xmm9, xmm1, xmm2          ; b^c
    por       xmm10, xmm8, xmm9
    por       xmm10, xmm6
    pand      xmm10, xmm7
    pavgb     xmm11, xmm4, xmm5
    psubb     xmm11, xmm10              ; k
    pavgb     xmm12, xmm11, xmm5
    pand      xmm13, xmm9, xmm6
    pxor      xmm14, xmm11, xmm5
    por       xmm13, xmm14
    pand      xmm13, xmm7
    psubb     xmm12, xmm13              ; (a + 3b + 3c + d) / 8
    pavgb     xmm13, xmm11, xmm4
    pand      xmm14, xmm8, xmm6
    pxor      xmm15, xmm11, xmm4
    por       xmm14, xmm15
    pand      xmm14, xmm7
    psubb     xmm13, xmm14              ; (3a + b + c + 3d) / 8
    pavgb     xmm0, xmm12
    pavgb     xmm1, xmm13
    pavgb     xmm2, xmm13
    pavgb     xmm3, xmm12
    punpcklbw xmm4, xmm0, xmm1
    punpckhbw xmm5, xmm0, xmm1
    mova      [rsp + %3 +  0], xmm4
    mova      [rsp + %3 + 16], xmm5
    punpcklbw xmm4, xmm2, xmm3
    punpckhbw xmm5, xmm2, xmm3
    mova      [rsp + %4 +  0], xmm4
    mova      [rsp + %4 + 16], xmm5
%endmacro

; Interleaves the two packed channel pairs into whole pixels and stores them.
%macro STORE_PIXELS 4 ; first_pair, second_pair, dst, offset
    punpcklbw m0, %1, %2
    punpckhbw %1, %2
    punpcklwd m1, m0, %1
    punpckhwd m0, %1
%if cpuflag(avx2)
    vperm2i128 m7, m1, m0, 0x20
    vperm2i128 m0, m1, m0, 0x31
    movu      [%3 + 4 * %4 +  0], m7
    movu      [%3 + 4 * %4 + 32], m0
%else
    movu      [%3 + 4 * %4 +  0], m1
    movu      [%3 + 4 * %4 + 16], m0
%endif
%endmacro

; R = (19077 . y             + 26149 . v - 14234) >> 6
; G = (19077 . y -  6419 . u - 13320 . v +  8708) >> 6
; B = (19077 . y + 33050 . u             - 17685) >> 6
; where a . b is mulhi_epu16(a << 8, b), i.e. (a * b) >> 8 for a byte a.
%macro CONVERT_GROUP 6 ; y, dst, u_scratch, v_scratch, offset, layout
%if cpuflag(avx2)
    pmovzxbw  m3, [%1 + %5]
    pmovzxbw  m4, [rsp + %3 + %5]
    pmovzxbw  m5, [rsp + %4 + %5]
    psllw     m3, 8
    psllw     m4, 8
    psllw     m5, 8
%else
    movq      m0, [%1 + %5]
    movq      m1, [rsp + %3 + %5]
    movq      m2, [rsp + %4 + %5]
    pxor      m7, m7
    mova      m3, m7
    punpcklbw m3, m0
    mova      m4, m7
    punpcklbw m4, m1
    mova      m5, m7
    punpcklbw m5, m2
%endif
    pmulhuw   m3, [pw_19077]            ; 19077 . y
    pmulhuw   m6, m5, [pw_26149]
    paddw     m6, m3
    psubw     m6, [pw_14234]
    psraw     m6, 6                     ; R
    pmulhuw   m0, m4, [pw_6419]
    pmulhuw   m1, m5, [pw_13320]
    paddw     m0, m1
    paddw     m1, m3, [pw_8708]
    psubw     m1, m0
    psraw     m1, 6                     ; G
    ; 33050 does not fit in a signed word; keep the saturation unsigned.
    pmulhuw   m4, [pw_33050]
    paddusw   m4, m3
    psubusw   m4, [pw_17685]
    psrlw     m4, 6                     ; B
    mova      m2, [pw_255]              ; A
%ifidn %6, argb
    packuswb  m2, m1
    packuswb  m6, m4
    STORE_PIXELS m2, m6, %2, %5
%elifidn %6, rgba
    packuswb  m6, m4
    packuswb  m5, m1, m2
    STORE_PIXELS m6, m5, %2, %5
%else
    packuswb  m4, m6
    packuswb  m5, m1, m2
    STORE_PIXELS m4, m5, %2, %5
%endif
%endmacro

%macro CONVERT32 5 ; y, dst, u_scratch, v_scratch, layout
    %assign %%i 0
    %rep 64 / mmsize
    CONVERT_GROUP %1, %2, %3, %4, %%i, %5
    %assign %%i %%i + mmsize / 2
    %endrep
%endmacro

%macro UPSAMPLE_ARGB_BLOCK 1 ; layout
cglobal upsample_block_%1, 9, 9, 16, 128, top_y, bottom_y, top_u, top_v, \
                                            cur_u, cur_v, top_dst, bottom_dst, \
                                            nblocks
.loop:
    RECON_UV  top_uq, cur_uq,  0, 32
    RECON_UV  top_vq, cur_vq, 64, 96
    CONVERT32 top_yq, top_dstq, 0, 64, %1
    test      bottom_yq, bottom_yq
    jz        .no_bottom
    CONVERT32 bottom_yq, bottom_dstq, 32, 96, %1
    add       bottom_yq, 32
    add       bottom_dstq, 128
.no_bottom:
    add       top_yq, 32
    add       top_dstq, 128
    add       top_uq, 16
    add       top_vq, 16
    add       cur_uq, 16
    add       cur_vq, 16
    dec       nblocksd
    jg        .loop
    RET
%endmacro

INIT_XMM sse2
UPSAMPLE_ARGB_BLOCK argb
UPSAMPLE_ARGB_BLOCK rgba
UPSAMPLE_ARGB_BLOCK bgra
INIT_YMM avx2
UPSAMPLE_ARGB_BLOCK argb
UPSAMPLE_ARGB_BLOCK rgba
UPSAMPLE_ARGB_BLOCK bgra

%endif ; ARCH_X86_64

%macro PACK32 1
cglobal pack_%1, 3, 3, 2, dst, src, n
    mova      m1, [shuf_%1]
    sub       nd, mmsize / 4
    jl        .tail
.loop:
    movu      m0, [srcq]
    pshufb    m0, m1
    movu      [dstq], m0
    add       srcq, mmsize
    add       dstq, mmsize
    sub       nd, mmsize / 4
    jge       .loop
.tail:
    add       nd, mmsize / 4
    jz        .end
.tail_loop:
    movd      xmm0, [srcq]
    pshufb    xmm0, xmm1
    movd      [dstq], xmm0
    add       srcq, 4
    add       dstq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

; Four pixels at a time, storing 16 bytes for the 12 that matter. The vector
; loop stops eight pixels short so the overhang always lands inside the row.
%macro PACK24 1
cglobal pack_%1, 3, 4, 2, dst, src, n
    mova      m1, [shuf_%1]
    cmp       nd, 8
    jl        .tail
.loop4:
    movu      m0, [srcq]
    pshufb    m0, m1
    movu      [dstq], m0
    add       srcq, 16
    add       dstq, 12
    sub       nd, 4
    cmp       nd, 8
    jge       .loop4
.tail:
    test      nd, nd
    jz        .end
.tail_loop:
    movd      m0, [srcq]
    pshufb    m0, m1
    movd      r3d, m0
    mov       [dstq], r3w
    shr       r3d, 16
    mov       [dstq + 2], r3b
    add       srcq, 4
    add       dstq, 3
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

; c * a / 255 truncated, as (p + (p >> 8) + 1) >> 8. Giving the alpha lane a
; multiplier of 255 leaves it untouched, so no channel needs masking out.
%macro PREMULTIPLY_ROW 0
cglobal premultiply_row, 3, 6, 8, argb, alpha_first, n
    test      alpha_firstd, alpha_firstd
    jz        .alpha_last
    mova      m4, [shuf_bcasta]
    mova      m5, [pd_alpha]
    xor       r4d, r4d
    mov       r5d, 1
    jmp       .start
.alpha_last:
    mova      m4, [shuf_bcasta3]
    mova      m5, [pd_alpha3]
    mov       r4d, 3
    xor       r5d, r5d
.start:
    mova      m6, [pw_1]
    pxor      m7, m7
    sub       nd, mmsize / 4
    jl        .tail
.loop:
    movu      m0, [argbq]
    pshufb    m1, m0, m4
    por       m1, m5
    punpcklbw m2, m0, m7
    punpckhbw m3, m0, m7
    punpcklbw m0, m1, m7
    punpckhbw m1, m7
    pmullw    m2, m0
    pmullw    m3, m1
    psrlw     m0, m2, 8
    paddw     m2, m0
    paddw     m2, m6
    psrlw     m2, 8
    psrlw     m1, m3, 8
    paddw     m3, m1
    paddw     m3, m6
    psrlw     m3, 8
    packuswb  m2, m3
    movu      [argbq], m2
    add       argbq, mmsize
    sub       nd, mmsize / 4
    jge       .loop
.tail:
    add       nd, mmsize / 4
    jz        .end
.tail_loop:
    movzx     r3d, byte [argbq + r4q]
    imul      r3d, r3d, 32897
%assign %%i 0
%rep 3
    movzx     alpha_firstd, byte [argbq + r5q + %%i]
    imul      alpha_firstd, r3d
    shr       alpha_firstd, 23
    mov       [argbq + r5q + %%i], alpha_firstb
%assign %%i %%i + 1
%endrep
    add       argbq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

INIT_XMM ssse3
PACK32 rgba
PACK32 bgra
PACK24 rgb
PACK24 bgr
PREMULTIPLY_ROW
INIT_YMM avx2
PACK32 rgba
PACK32 bgra
PREMULTIPLY_ROW

INIT_XMM sse2
cglobal dispatch_alpha, 3, 4, 7, dst, src, n
    pxor      m5, m5
    mova      m4, [pd_rgbmask]
    sub       nd, 16
    jl        .tail
.loop16:
    movu      m0, [srcq]
    punpckhbw m1, m0, m5
    punpcklbw m0, m5
    punpckhwd m2, m0, m5
    punpcklwd m0, m5
    punpckhwd m3, m1, m5
    punpcklwd m1, m5
    movu      m6, [dstq +  0]
    pand      m6, m4
    por       m6, m0
    movu      [dstq +  0], m6
    movu      m6, [dstq + 16]
    pand      m6, m4
    por       m6, m2
    movu      [dstq + 16], m6
    movu      m6, [dstq + 32]
    pand      m6, m4
    por       m6, m1
    movu      [dstq + 32], m6
    movu      m6, [dstq + 48]
    pand      m6, m4
    por       m6, m3
    movu      [dstq + 48], m6
    add       srcq, 16
    add       dstq, 64
    sub       nd, 16
    jge       .loop16
.tail:
    add       nd, 16
    jz        .end
.tail_loop:
    movzx     r3d, byte [srcq]
    mov       [dstq], r3b
    inc       srcq
    add       dstq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET

INIT_YMM avx2
cglobal dispatch_alpha, 3, 4, 4, dst, src, n
    mova      m3, [pd_rgbmask]
    sub       nd, 16
    jl        .tail
.loop16:
    pmovzxbd  m0, [srcq]
    pmovzxbd  m1, [srcq + 8]
    pand      m2, m3, [dstq +  0]
    por       m2, m0
    movu      [dstq +  0], m2
    pand      m2, m3, [dstq + 32]
    por       m2, m1
    movu      [dstq + 32], m2
    add       srcq, 16
    add       dstq, 64
    sub       nd, 16
    jge       .loop16
.tail:
    add       nd, 16
    jz        .end
.tail_loop:
    movzx     r3d, byte [srcq]
    mov       [dstq], r3b
    inc       srcq
    add       dstq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET
