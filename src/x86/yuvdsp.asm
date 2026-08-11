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
; 565 needs the green byte twice, once per output byte. Weighting the masked
; fields by 32/1 and 64/1 lines them up for a right shift of 5 and 3, which
; pmulhuw does in one instruction with 2^11 and 2^13.
shuf_rgb565: db 1, 2, 2, 3, 5, 6, 6, 7, 9, 10, 10, 11, 13, 14, 14, 15
             db 1, 2, 2, 3, 5, 6, 6, 7, 9, 10, 10, 11, 13, 14, 14, 15
mask_rgb565: times 8 db 0xf8, 0xe0, 0x1c, 0xf8
mul_rgb565:  times 8 db 32, 1, 64, 1
pw_565scale: times 8 dw 2048, 8192

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
pw_y_rg:  times 8 dw 16839, 16675
pw_y_gb:  times 8 dw 16384, 6420
pd_y_rnd: times 8 dd 1081344
; Undoes the lane interleave the two packssdw and the packuswb leave behind.
permd_y:  dd 0, 4, 1, 5, 2, 6, 3, 7

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
%macro ARGB_TO_Y_GROUP 4 ; dst, scratch, src, offset
    movu      %2, [%3 + %4]
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
    ARGB_TO_Y_GROUP xm2, xm0, argbq, 0
    movd      r3d, xm2
    mov       [yq], r3b
    add       argbq, 4
    inc       yq
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

; Every channel is a nibble expanded to eight bits by a multiply by 17, and
; the alpha multiplier is the same expansion doubled up, so the truncating
; divide by 255 the C does with a 32-bit product is exactly pmulhuw here.
%macro PREMULTIPLY_4444 6 ; word, scratch, r, g, b, a
    psrlw     %2, %1, 4
    pand      %3, %2, [pw_15]
    pand      %4, %1, [pw_15]
    psrlw     %6, %1, 8
    pand      %6, [pw_15]
    psrlw     %5, %1, 12
    pmullw    %2, %6, [pw_4369]
    pmullw    %3, [pw_17]
    pmullw    %4, [pw_17]
    pmullw    %5, [pw_17]
    pmulhuw   %3, %2
    pmulhuw   %4, %2
    pmulhuw   %5, %2
    pand      %3, [pw_240]
    psrlw     %4, 4
    por       %3, %4
    pand      %5, [pw_240]
    por       %5, %6
    psllw     %5, 8
    por       %1, %3, %5
%endmacro

%macro PREMULTIPLY_ROW_4444 0
cglobal premultiply_row_4444, 2, 3, 6, rgba, n
    sub       nd, mmsize / 2
    jl        .tail
.loop:
    movu      m0, [rgbaq]
    PREMULTIPLY_4444 m0, m1, m2, m3, m4, m5
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
    PREMULTIPLY_4444 xm0, xm1, xm2, xm3, xm4, xm5
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
PREMULTIPLY_ROW
PREMULTIPLY_ROW_4444
ARGB_TO_Y
INIT_YMM avx2
PACK32 rgba
PACK32 bgra
PACK24 rgb
PACK24 bgr
PACK16 rgb565
PACK16 rgba4444
PREMULTIPLY_ROW
PREMULTIPLY_ROW_4444
ARGB_TO_Y

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
