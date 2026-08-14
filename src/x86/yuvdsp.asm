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
pd_rgbmask3: times 8 dd 0x00ffffff
pw_1:       times 16 dw 1
pd_alpha:   times  8 dd 0x000000ff
pd_alpha3:  times  8 dd 0xff000000

; ARGB -> RGBA, BGRA, RGB and BGR byte selectors.
shuf_rgba:  db 1, 2, 3, 0, 5, 6, 7, 4, 9, 10, 11, 8, 13, 14, 15, 12
            db 1, 2, 3, 0, 5, 6, 7, 4, 9, 10, 11, 8, 13, 14, 15, 12
shuf_bgra:  db 3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12
            db 3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12
shuf_rgb:   db 1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15, -1, -1, -1, -1
            db 1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15, -1, -1, -1, -1
shuf_bgr:   db 3, 2, 1, 7, 6, 5, 11, 10, 9, 15, 14, 13, -1, -1, -1, -1
            db 3, 2, 1, 7, 6, 5, 11, 10, 9, 15, 14, 13, -1, -1, -1, -1
; Drops the trailing alpha byte of four RGBA or BGRA pixels.
shuf_drop_a: db 0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14, -1, -1, -1, -1
             db 0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14, -1, -1, -1, -1
; The same, without pshufb: keep the low three bytes of each half, then merge
; in the other three that a byte-granular right shift has moved into place.
pq_lo3: times 2 db 255, 255, 255,   0,   0,   0, 0, 0
pq_hi3: times 2 db   0,   0,   0, 255, 255, 255, 0, 0
pb_lo6: times 6 db 255
        times 10 db 0
pb_hi6: times 6 db 0
        times 6 db 255
        times 4 db 0
; Gathers the six dwords each 256-bit shuffle leaves valid into 24 contiguous
; bytes; the _hi variant also parks its first two dwords in the upper lane so a
; blend can append them to the preceding group.
permd_rgb:    dd 0, 1, 2, 4, 5, 6, 0, 0
permd_rgb_hi: dd 2, 4, 5, 6, 0, 0, 0, 1
; Broadcasts each pixel's alpha over its four bytes.
shuf_bcasta: db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12
             db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12
shuf_bcasta3: db 3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15
              db 3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15

; The 4444 packer wants RGBA order, where the two nibble pairs it merges are
; already neighbours; pmaddubsw then folds each pair into one word.
%define shuf_rgba4444 shuf_rgba
mask_rgba4444: times 32 db 0xf0
mul_rgba4444:  times 16 db 16, 1
; A swapped layout lays the very same two bytes down the other way round, so it
; only exchanges the two field pairs the shuffle gathers; the mask and the
; multiplier are per-pair and ride along unchanged.
shuf_bgra4444: db 3, 0, 1, 2, 7, 4, 5, 6, 11, 8, 9, 10, 15, 12, 13, 14
               db 3, 0, 1, 2, 7, 4, 5, 6, 11, 8, 9, 10, 15, 12, 13, 14
%define mask_bgra4444 mask_rgba4444
%define mul_bgra4444  mul_rgba4444
; 565 needs the green byte twice, once per output byte. Weighting the masked
; fields by 32/1 and 64/1 lines them up for a right shift of 5 and 3, which
; pmulhuw does in one instruction with 2^11 and 2^13.
shuf_rgb565: db 1, 2, 2, 3, 5, 6, 6, 7, 9, 10, 10, 11, 13, 14, 14, 15
             db 1, 2, 2, 3, 5, 6, 6, 7, 9, 10, 10, 11, 13, 14, 14, 15
mask_rgb565: times 8 db 0xf8, 0xe0, 0x1c, 0xf8
mul_rgb565:  times 8 db 32, 1, 64, 1
pw_565scale: times 8 dw 2048, 8192
; The two 565 pairs carry a shift of their own, so exchanging them exchanges
; the mask, the multiplier and the scale with them.
shuf_bgr565: db 2, 3, 1, 2, 6, 7, 5, 6, 10, 11, 9, 10, 14, 15, 13, 14
             db 2, 3, 1, 2, 6, 7, 5, 6, 10, 11, 9, 10, 14, 15, 13, 14
mask_bgr565: times 8 db 0x1c, 0xf8, 0xf8, 0xe0
mul_bgr565:  times 8 db 64, 1, 32, 1
pw_bgr565scale: times 8 dw 8192, 2048

pw_15:   times 16 dw 15
pw_17:   times 16 dw 17
pw_240:  times 16 dw 240
pw_4369: times 16 dw 4369

; (R, G) and (G, B) pairs for pmaddwd. 33059 overflows a signed word, so it is
; split as 16675 on the first pair and 16384 on the second.
shuf_y_rg: db 1, -1, 2, -1, 5, -1, 6, -1, 9, -1, 10, -1, 13, -1, 14, -1
           db 1, -1, 2, -1, 5, -1, 6, -1, 9, -1, 10, -1, 13, -1, 14, -1
shuf_y_gb: db 2, -1, 3, -1, 6, -1, 7, -1, 10, -1, 11, -1, 14, -1, 15, -1
           db 2, -1, 3, -1, 6, -1, 7, -1, 10, -1, 11, -1, 14, -1, 15, -1
shuf_uv_b: db 3, -1, -1, -1, 7, -1, -1, -1
           db 11, -1, -1, -1, 15, -1, -1, -1
           db 3, -1, -1, -1, 7, -1, -1, -1
           db 11, -1, -1, -1, 15, -1, -1, -1
pw_y_rg:  times 8 dw 16839, 16675
pw_y_gb:  times 8 dw 16384, 6420
pd_y_rnd: times 8 dd 1081344
; Undoes the lane interleave the two packssdw and the packuswb leave behind.
permd_y:  dd 0, 4, 1, 5, 2, 6, 3, 7

pd_255:   times 8 dd 255
pd_511:   times 8 dd 511
pd_512:   times 8 dd 512
pd_64:    times 8 dd 64
pd_1020:  times 8 dd 1020
pd_65535: times 8 dd 65535
pd_1:     times 8 dd 1
ps_1_19:  times 8 dd 524288.0
; U = (-9719 R - 19081 G + 28800 B) and V = (28800 R - 24116 G - 4684 B), each
; rounded by half an output step plus the 128 offset. R and G ride in one word
; pair; B keeps a lane of its own, its second word being the zero the sums
; never carry into. The 4:2:0 path feeds sums of a 2x2 block and folds the
; average into a shift of 18; the 4:4:4 path feeds one pixel and shifts by 16.
pw_u_rg:      times 8 dw -9719, -19081
pw_u_b:       times 8 dw 28800, 0
pw_v_rg:      times 8 dw 28800, -24116
pw_v_b:       times 8 dw -4684, 0
pd_uv_rnd:    times 8 dd 33685504
pd_uv444_rnd: times 8 dd 8421376

cextern_naked wpd_gamma_to_linear_tab
cextern_naked wpd_linear_to_gamma_tab

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

%macro DROP_ALPHA 1 ; four pixels, alpha last
    psrlq     m7, %1, 8
    pand      %1, [pq_lo3]
    pand      m7, [pq_hi3]
    por       %1, m7
    psrldq    m7, %1, 2
    pand      %1, [pb_lo6]
    pand      m7, [pb_hi6]
    por       %1, m7
%endmacro

; Drops the alpha byte of each pixel, leaving three quarters of the register
; live, and squeezes the halves back together so every store is full width.
%macro STORE_RGB24 2 ; dst, offset
%if cpuflag(avx2)
    pshufb    m7, [shuf_drop_a]
    pshufb    m0, [shuf_drop_a]
    mova      m1, [permd_rgb]
    vpermd    m7, m1, m7
    mova      m1, [permd_rgb_hi]
    vpermd    m0, m1, m0
    vpblendd  m7, m7, m0, 0xc0
    movu      [%1 + 3 * %2 +  0], m7
    movu      [%1 + 3 * %2 + 32], xm0
%else
%if cpuflag(ssse3)
    pshufb    m1, [shuf_drop_a]
    pshufb    m0, [shuf_drop_a]
%else
    DROP_ALPHA m1
    DROP_ALPHA m0
%endif
    pslldq    m7, m0, 12
    por       m1, m7
    movu      [%1 + 3 * %2 +  0], m1
    psrldq    m0, 4
    movq      [%1 + 3 * %2 + 16], m0
%endif
%endmacro

; Interleaves the two packed channel pairs into whole pixels and stores them.
%macro STORE_PIXELS 5 ; first_pair, second_pair, dst, offset, bpp
    punpcklbw m0, %1, %2
    punpckhbw %1, %2
    punpcklwd m1, m0, %1
    punpckhwd m0, %1
%if cpuflag(avx2)
    vperm2i128 m7, m1, m0, 0x20
    vperm2i128 m0, m1, m0, 0x31
%endif
%if %5 == 3
    STORE_RGB24 %3, %4
%elif cpuflag(avx2)
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
%macro CONVERT_GROUP 7 ; y, dst, u_scratch, v_scratch, offset, layout, bpp
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
    STORE_PIXELS m2, m6, %2, %5, %7
%elifidn %6, bgra
    packuswb  m4, m6
    packuswb  m5, m1, m2
    STORE_PIXELS m4, m5, %2, %5, %7
%elifidn %6, bgr
    packuswb  m4, m6
    packuswb  m5, m1, m2
    STORE_PIXELS m4, m5, %2, %5, %7
%else
    packuswb  m6, m4
    packuswb  m5, m1, m2
    STORE_PIXELS m6, m5, %2, %5, %7
%endif
%endmacro

%macro CONVERT32 6 ; y, dst, u_scratch, v_scratch, layout, bpp
    %assign %%i 0
    %rep 64 / mmsize
    CONVERT_GROUP %1, %2, %3, %4, %%i, %5, %6
    %assign %%i %%i + mmsize / 2
    %endrep
%endmacro

%macro UPSAMPLE_ARGB_BLOCK 2 ; layout, bpp
cglobal upsample_block_%1, 9, 9, 16, 128, top_y, bottom_y, top_u, top_v, \
                                            cur_u, cur_v, top_dst, bottom_dst, \
                                            nblocks
.loop:
    RECON_UV  top_uq, cur_uq,  0, 32
    RECON_UV  top_vq, cur_vq, 64, 96
    CONVERT32 top_yq, top_dstq, 0, 64, %1, %2
    test      bottom_yq, bottom_yq
    jz        .no_bottom
    CONVERT32 bottom_yq, bottom_dstq, 32, 96, %1, %2
    add       bottom_yq, 32
    add       bottom_dstq, 32 * %2
.no_bottom:
    add       top_yq, 32
    add       top_dstq, 32 * %2
    add       top_uq, 16
    add       top_vq, 16
    add       cur_uq, 16
    add       cur_vq, 16
    dec       nblocksd
    jg        .loop
    RET
%endmacro

INIT_XMM sse2
UPSAMPLE_ARGB_BLOCK argb, 4
UPSAMPLE_ARGB_BLOCK rgba, 4
UPSAMPLE_ARGB_BLOCK bgra, 4
UPSAMPLE_ARGB_BLOCK rgb, 3
UPSAMPLE_ARGB_BLOCK bgr, 3
INIT_XMM ssse3
UPSAMPLE_ARGB_BLOCK rgb, 3
UPSAMPLE_ARGB_BLOCK bgr, 3
INIT_YMM avx2
UPSAMPLE_ARGB_BLOCK argb, 4
UPSAMPLE_ARGB_BLOCK rgba, 4
UPSAMPLE_ARGB_BLOCK bgra, 4
UPSAMPLE_ARGB_BLOCK rgb, 3
UPSAMPLE_ARGB_BLOCK bgr, 3

; Splits sixteen ARGB columns of both rows into the four samples every output
; pixel averages: even and odd column of the top row, then of the bottom one.
%macro ARGB_TO_UV_COLUMNS 2 ; src, stride
    movu      m0, [%1]
    movu      m1, [%1 + 32]
    shufps    m2, m0, m1, q2020
    shufps    m3, m0, m1, q3131
    vpermq    m2, m2, q3120
    vpermq    m3, m3, q3120
    mova      [rsp +  0], m2
    mova      [rsp + 32], m3
    movu      m0, [%1 + %2]
    movu      m1, [%1 + %2 + 32]
    shufps    m2, m0, m1, q2020
    shufps    m3, m0, m1, q3131
    vpermq    m2, m2, q3120
    vpermq    m3, m3, q3120
    mova      [rsp + 64], m2
    mova      [rsp + 96], m3
%endmacro

%macro ARGB_TO_UV_IDX 1 ; channel shift
%if %1
    psrld     m0, [rsp +  0], %1
    psrld     m1, [rsp + 32], %1
    psrld     m2, [rsp + 64], %1
    psrld     m3, [rsp + 96], %1
%else
    mova      m0, [rsp +  0]
    mova      m1, [rsp + 32]
    mova      m2, [rsp + 64]
    mova      m3, [rsp + 96]
%endif
%if %1 != 24
    pand      m0, [pd_255]
    pand      m1, [pd_255]
    pand      m2, [pd_255]
    pand      m3, [pd_255]
%endif
%endmacro

; One channel of eight output pixels. A gather with a scale of two lands the
; wanted entry in the low word of each dword and its neighbour in the high one,
; which the four-sample sum can ignore: the sum of four 12-bit entries never
; carries into bit 16. The same overlap is what makes the interpolation of the
; inverse table a single pmaddwd, its two entries arriving in one dword.
%macro ARGB_TO_UV_CHANNEL 2 ; channel shift, dst
    ARGB_TO_UV_IDX %1
    pcmpeqd    m7, m7, m7
    vpgatherdd m4, [r6 + m0 * 2], m7
    pcmpeqd    m7, m7, m7
    vpgatherdd m5, [r6 + m1 * 2], m7
    pcmpeqd    m7, m7, m7
    vpgatherdd m6, [r6 + m2 * 2], m7
    pcmpeqd    m7, m7, m7
    vpgatherdd m0, [r6 + m3 * 2], m7
    paddd      m1, m4, m5
    paddd      m2, m6, m0
    paddd      m1, m2
    pand       m1, [pd_65535]
    test       r5d, r5d
    jz         %%unweighted
    pand       m4, [pd_65535]
    pand       m6, [pd_65535]
    pslld      m5, 16
    pslld      m0, 16
    por        m4, m5
    por        m6, m0
    pmaddwd    m4, m13
    pmaddwd    m6, m12
    paddd      m4, m6
    pmulld     m4, m9
    psrld      m4, 17
    vpblendvb  m1, m1, m4, m10
%%unweighted:
    psrld      m2, m1, 9
    pand       m1, [pd_511]
    pcmpeqd    m7, m7, m7
    vpgatherdd m3, [r7 + m2 * 2], m7
    pslld      m2, m1, 16
    mova       m0, [pd_512]
    psubd      m0, m1
    por        m2, m0
    pmaddwd    m3, m2
    paddd      m3, [pd_64]
    psrad      %2, m3, 7
%endmacro

%macro ARGB_TO_UV_BODY 2 ; src, stride
    ARGB_TO_UV_COLUMNS %1, %2
    ARGB_TO_UV_IDX 0
    paddd     m5, m0, m1
    paddd     m6, m2, m3
    paddd     m5, m6
    pslld     m1, 16
    pslld     m3, 16
    por       m13, m0, m1
    por       m12, m2, m3
    pxor      m6, m6
    pcmpeqd   m4, m5, m6
    pcmpeqd   m7, m5, [pd_1020]
    por       m4, m7
    ; A block that is opaque, fully transparent, or whose alpha the caller does
    ; not want kept is averaged unweighted; when none of the eight need the
    ; weighted average, which is every block of an opaque image, skip it.
    pandn     m10, m4, m11
    pmovmskb  r5d, m10
    pmaxsd    m5, [pd_1]
    cvtdq2ps  m5, m5
    mova      m9, [ps_1_19]
    divps     m9, m5
    ; 2^19 / total_a is never within a float rounding error of an integer from
    ; below, so truncating agrees with the integer divide.
    cvttps2dq m9, m9
    ARGB_TO_UV_CHANNEL  8, m8
    ARGB_TO_UV_CHANNEL 16, m14
    ARGB_TO_UV_CHANNEL 24, m15
    pslld     m0, m14, 16
    por       m0, m8
    pmaddwd   m1, m0, [pw_u_rg]
    pmaddwd   m2, m15, [pw_u_b]
    paddd     m1, m2
    paddd     m1, [pd_uv_rnd]
    psrad     m1, 18
    pmaddwd   m2, m0, [pw_v_rg]
    pmaddwd   m3, m15, [pw_v_b]
    paddd     m2, m3
    paddd     m2, [pd_uv_rnd]
    psrad     m2, 18
    packssdw  m1, m2
    packuswb  m1, m1
    vextracti128 xm2, m1, 1
    punpckldq xm1, xm1, xm2
%endmacro

INIT_YMM avx2
cglobal argb_to_uv, 6, 10, 16, 288, u, v, argb, argb_stride, n, weight_alpha
    lea       r6, [wpd_gamma_to_linear_tab]
    lea       r7, [wpd_linear_to_gamma_tab]
    pxor      m11, m11
    test      weight_alphad, weight_alphad
    jz        .no_weight
    pcmpeqd   m11, m11
.no_weight:
    mov       r8d, nd
    sar       r8d, 4
    jz        .tail
.loop:
    ARGB_TO_UV_BODY argbq, argb_strideq
    movq      [uq], xm1
    movhps    [vq], xm1
    add       argbq, 64
    add       uq, 8
    add       vq, 8
    dec       r8d
    jg        .loop
.tail:
    and       nd, 15
    jz        .end
    ; Fewer than sixteen columns left: gather them into a zeroed block, with
    ; the last column doubled up when the width is odd, which is exactly how
    ; the scalar path folds a lone column onto itself.
    pxor      m0, m0
    mova      [rsp + 128], m0
    mova      [rsp + 160], m0
    mova      [rsp + 192], m0
    mova      [rsp + 224], m0
    mov       r8d, nd
    lea       r9, [rsp + 128]
.copy:
    mov       r5d, [argbq]
    mov       [r9], r5d
    mov       r5d, [argbq + argb_strideq]
    mov       [r9 + 64], r5d
    add       argbq, 4
    add       r9, 4
    dec       r8d
    jg        .copy
    test      nd, 1
    jz        .no_dup
    mov       r5d, [r9 - 4]
    mov       [r9], r5d
    mov       r5d, [r9 + 60]
    mov       [r9 + 64], r5d
.no_dup:
    lea       r9, [rsp + 128]
    ARGB_TO_UV_BODY r9, 64
    movq      [rsp + 256], xm1
    movhps    [rsp + 264], xm1
    add       nd, 1
    shr       nd, 1
    xor       r9d, r9d
.store:
    mov       r5b, [rsp + 256 + r9]
    mov       [uq + r9], r5b
    mov       r5b, [rsp + 264 + r9]
    mov       [vq + r9], r5b
    inc       r9
    dec       nd
    jg        .store
.end:
    RET

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
; The 256-bit pass gathers the six live dwords of a shuffle into 24 contiguous
; bytes and stops sixteen pixels short, for the same reason.
%macro PACK24 1
cglobal pack_%1, 3, 4, 3, dst, src, n
    mova      m1, [shuf_%1]
%if mmsize == 32
    mova      m2, [permd_rgb]
    cmp       nd, 16
    jl        .loop4_start
.loop8:
    movu      m0, [srcq]
    pshufb    m0, m1
    vpermd    m0, m2, m0
    movu      [dstq], m0
    add       srcq, 32
    add       dstq, 24
    sub       nd, 8
    cmp       nd, 16
    jge       .loop8
.loop4_start:
%endif
    cmp       nd, 8
    jl        .tail
.loop4:
    movu      xm0, [srcq]
    pshufb    xm0, xm1
    movu      [dstq], xm0
    add       srcq, 16
    add       dstq, 12
    sub       nd, 4
    cmp       nd, 8
    jge       .loop4
.tail:
    test      nd, nd
    jz        .end
.tail_loop:
    movd      xm0, [srcq]
    pshufb    xm0, xm1
    movd      r3d, xm0
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

%macro PACK16_SCALE 2 ; layout, reg
%ifidn %1, rgb565
    pmulhuw   %2, [pw_565scale]
%elifidn %1, bgr565
    pmulhuw   %2, [pw_bgr565scale]
%else
    psrlw     %2, 4
%endif
%endmacro

; Two bytes out per pixel. The shuffle puts the fields of each output byte side
; by side, the mask clears the bits that would carry into the neighbouring
; field, and pmaddubsw merges the pair into one word that the scale shifts back
; down; packuswb then lays the words out as the little-endian byte pairs.
%macro PACK16 1
cglobal pack_%1, 3, 4, 5, dst, src, n
    mova      m2, [shuf_%1]
    mova      m3, [mask_%1]
    mova      m4, [mul_%1]
    sub       nd, mmsize / 2
    jl        .tail
.loop:
    movu      m0, [srcq]
    movu      m1, [srcq + mmsize]
    pshufb    m0, m2
    pshufb    m1, m2
    pand      m0, m3
    pand      m1, m3
    pmaddubsw m0, m4
    pmaddubsw m1, m4
    PACK16_SCALE %1, m0
    PACK16_SCALE %1, m1
    packuswb  m0, m1
%if mmsize == 32
    vpermq    m0, m0, q3120
%endif
    movu      [dstq], m0
    add       srcq, 2 * mmsize
    add       dstq, mmsize
    sub       nd, mmsize / 2
    jge       .loop
.tail:
    add       nd, mmsize / 2
    jz        .end
.tail_loop:
    movd      xm0, [srcq]
    pshufb    xm0, xm2
    pand      xm0, xm3
    pmaddubsw xm0, xm4
    PACK16_SCALE %1, xm0
    packuswb  xm0, xm0
    movd      r3d, xm0
    mov       [dstq], r3w
    add       srcq, 4
    add       dstq, 2
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

; Y = (16839 R + 33059 G + 6420 B + 32768 + (16 << 16)) >> 16, never leaving
; [16, 235], so neither pack saturates.
; The load is a parameter so the one-pixel tail can take its four bytes with a
; movd rather than reading a whole group past the end of the row.
%macro ARGB_TO_Y_GROUP 4-5 movu ; dst, scratch, src, offset, load
    %5        %2, [%3 + %4]
    pshufb    %1, %2, [shuf_y_rg]
    pshufb    %2, [shuf_y_gb]
    pmaddwd   %1, [pw_y_rg]
    pmaddwd   %2, [pw_y_gb]
    paddd     %1, %2
    paddd     %1, [pd_y_rnd]
    psrad     %1, 16
%endmacro

%macro ARGB_TO_Y 0
cglobal argb_to_y, 3, 4, 6, y, argb, n
    sub       nd, mmsize
    jl        .loop4
.loop:
    ARGB_TO_Y_GROUP m2, m0, argbq, 0 * mmsize
    ARGB_TO_Y_GROUP m3, m0, argbq, 1 * mmsize
    ARGB_TO_Y_GROUP m4, m0, argbq, 2 * mmsize
    ARGB_TO_Y_GROUP m5, m0, argbq, 3 * mmsize
    packssdw  m2, m3
    packssdw  m4, m5
    packuswb  m2, m4
%if mmsize == 32
    mova      m3, [permd_y]
    vpermd    m2, m3, m2
%endif
    movu      [yq], m2
    add       argbq, 4 * mmsize
    add       yq, mmsize
    sub       nd, mmsize
    jge       .loop
.loop4:
    add       nd, mmsize
.loop4_test:
    cmp       nd, 4
    jl        .tail
    ARGB_TO_Y_GROUP xm2, xm0, argbq, 0
    packssdw  xm2, xm2
    packuswb  xm2, xm2
    movd      [yq], xm2
    add       argbq, 16
    add       yq, 4
    sub       nd, 4
    jmp       .loop4_test
.tail:
    test      nd, nd
    jz        .end
.tail_loop:
    ARGB_TO_Y_GROUP xm2, xm0, argbq, 0, movd
    movd      r3d, xm2
    mov       [yq], r3b
    add       argbq, 4
    inc       yq
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

%macro ARGB_TO_YUV444_GROUP 0
    mova      m1, m0
    mova      m2, m0
    pshufb    m1, [shuf_y_rg]
    pshufb    m2, [shuf_y_gb]
    pmaddwd   m1, [pw_y_rg]
    pmaddwd   m2, [pw_y_gb]
    paddd     m1, m2
    paddd     m1, [pd_y_rnd]
    psrad     m1, 16
    mova      m2, m0
    mova      m3, m0
    pshufb    m2, [shuf_y_rg]
    pshufb    m3, [shuf_uv_b]
    mova      m4, m2
    mova      m5, m3
    pmaddwd   m2, [pw_u_rg]
    pmaddwd   m3, [pw_u_b]
    paddd     m2, m3
    paddd     m2, [pd_uv444_rnd]
    psrad     m2, 16
    pmaddwd   m4, [pw_v_rg]
    pmaddwd   m5, [pw_v_b]
    paddd     m4, m5
    paddd     m4, [pd_uv444_rnd]
    psrad     m4, 16
    packssdw  m1, m1
    packuswb  m1, m1
    packssdw  m2, m2
    packuswb  m2, m2
    packssdw  m4, m4
    packuswb  m4, m4
%if mmsize == 32
    vextracti128 xm0, m1, 1
    punpckldq xm1, xm0
    vextracti128 xm0, m2, 1
    punpckldq xm2, xm0
    vextracti128 xm0, m4, 1
    punpckldq xm4, xm0
%endif
%endmacro

%macro ARGB_TO_YUV444 0
cglobal argb_to_yuv444, 5, 6, 6, y, u, v, argb, n, tmp
    sub       nd, mmsize / 4
    jl        .tail
.loop:
    movu      m0, [argbq]
    ARGB_TO_YUV444_GROUP
%if mmsize == 32
    movq      [yq], xm1
    movq      [uq], xm2
    movq      [vq], xm4
%else
    movd      [yq], xm1
    movd      [uq], xm2
    movd      [vq], xm4
%endif
    add       argbq, mmsize
    add       yq, mmsize / 4
    add       uq, mmsize / 4
    add       vq, mmsize / 4
    sub       nd, mmsize / 4
    jge       .loop
.tail:
    add       nd, mmsize / 4
    jz        .end
.tail_loop:
    movd      xm0, [argbq]
    ARGB_TO_YUV444_GROUP
    movd      tmpd, xm1
    mov       [yq], tmpb
    movd      tmpd, xm2
    mov       [uq], tmpb
    movd      tmpd, xm4
    mov       [vq], tmpb
    add       argbq, 4
    inc       yq
    inc       uq
    inc       vq
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

; Every channel is a nibble expanded to eight bits by a multiply by 17, and
; the alpha multiplier is the same expansion doubled up, so the truncating
; divide by 255 the C does with a 32-bit product is exactly pmulhuw here.
; Alpha keeps its nibble untouched and blue shares alpha's byte in either
; layout, so a swap only moves which of the two bytes that pair lands in, and
; with it the four nibble positions the extraction reads.
%macro PREMULTIPLY_4444 7 ; word, scratch, a, b, r, g, swap
%if %7
    pand      %3, %1, [pw_15]
    psrlw     %4, %1, 4
    pand      %4, [pw_15]
    psrlw     %6, %1, 8
    pand      %6, [pw_15]
    psrlw     %5, %1, 12
%else
    psrlw     %2, %1, 4
    pand      %5, %2, [pw_15]
    pand      %6, %1, [pw_15]
    psrlw     %3, %1, 8
    pand      %3, [pw_15]
    psrlw     %4, %1, 12
%endif
    pmullw    %2, %3, [pw_4369]
    pmullw    %4, [pw_17]
    pmullw    %5, [pw_17]
    pmullw    %6, [pw_17]
    pmulhuw   %4, %2
    pmulhuw   %5, %2
    pmulhuw   %6, %2
    pand      %4, [pw_240]
    por       %4, %3
    pand      %5, [pw_240]
    psrlw     %6, 4
    por       %5, %6
%if %7
    psllw     %5, 8
    por       %1, %4, %5
%else
    psllw     %4, 8
    por       %1, %5, %4
%endif
%endmacro

%macro PREMULTIPLY_ROW_4444 2 ; name, swap
cglobal %1, 2, 3, 6, rgba, n
    sub       nd, mmsize / 2
    jl        .tail
.loop:
    movu      m0, [rgbaq]
    PREMULTIPLY_4444 m0, m1, m2, m3, m4, m5, %2
    movu      [rgbaq], m0
    add       rgbaq, mmsize
    sub       nd, mmsize / 2
    jge       .loop
.tail:
    add       nd, mmsize / 2
    jz        .end
.tail_loop:
    movzx     r2d, word [rgbaq]
    movd      xm0, r2d
    PREMULTIPLY_4444 xm0, xm1, xm2, xm3, xm4, xm5, %2
    movd      r2d, xm0
    mov       [rgbaq], r2w
    add       rgbaq, 2
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
PACK16 rgb565
PACK16 rgba4444
PACK16 bgr565
PACK16 bgra4444
PREMULTIPLY_ROW
PREMULTIPLY_ROW_4444 premultiply_row_4444, 0
PREMULTIPLY_ROW_4444 premultiply_row_4444_swap, 1
ARGB_TO_Y
%if ARCH_X86_64
ARGB_TO_YUV444
%endif
INIT_YMM avx2
PACK32 rgba
PACK32 bgra
PACK24 rgb
PACK24 bgr
PACK16 rgb565
PACK16 rgba4444
PACK16 bgr565
PACK16 bgra4444
PREMULTIPLY_ROW
PREMULTIPLY_ROW_4444 premultiply_row_4444, 0
PREMULTIPLY_ROW_4444 premultiply_row_4444_swap, 1
ARGB_TO_Y
%if ARCH_X86_64
ARGB_TO_YUV444
%endif

; Rewrites one byte of every pixel, reading back the other three and storing
; them again unchanged. 'dst' is the start of the pixel, never the alpha byte
; inside it: sixteen pixels are a whole 64-byte read-modify-write, so a 'dst'
; biased by the alpha offset would make the last group of a row carry that
; bias past the end of the row and into the row below.
%macro DISPATCH_ALPHA 2 ; name, alpha offset within the pixel
%if cpuflag(avx2)
cglobal %1, 3, 4, 4, dst, src, n
%if %2
    mova      m3, [pd_rgbmask3]
%else
    mova      m3, [pd_rgbmask]
%endif
%else
cglobal %1, 3, 4, 7, dst, src, n
    pxor      m5, m5
%if %2
    mova      m4, [pd_rgbmask3]
%else
    mova      m4, [pd_rgbmask]
%endif
%endif
    sub       nd, 16
    jl        .tail
.loop16:
%if cpuflag(avx2)
    pmovzxbd  m0, [srcq]
    pmovzxbd  m1, [srcq + 8]
%if %2
    pslld     m0, 8 * %2
    pslld     m1, 8 * %2
%endif
    pand      m2, m3, [dstq +  0]
    por       m2, m0
    movu      [dstq +  0], m2
    pand      m2, m3, [dstq + 32]
    por       m2, m1
    movu      [dstq + 32], m2
%else
    movu      m0, [srcq]
    punpckhbw m1, m0, m5
    punpcklbw m0, m5
    punpckhwd m2, m0, m5
    punpcklwd m0, m5
    punpckhwd m3, m1, m5
    punpcklwd m1, m5
%if %2
    pslld     m0, 8 * %2
    pslld     m1, 8 * %2
    pslld     m2, 8 * %2
    pslld     m3, 8 * %2
%endif
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
%endif
    add       srcq, 16
    add       dstq, 64
    sub       nd, 16
    jge       .loop16
.tail:
    add       nd, 16
    jz        .end
.tail_loop:
    movzx     r3d, byte [srcq]
    mov       [dstq + %2], r3b
    inc       srcq
    add       dstq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

INIT_XMM sse2
DISPATCH_ALPHA dispatch_alpha_first, 0
DISPATCH_ALPHA dispatch_alpha_last,  3

INIT_YMM avx2
DISPATCH_ALPHA dispatch_alpha_first, 0
DISPATCH_ALPHA dispatch_alpha_last,  3
