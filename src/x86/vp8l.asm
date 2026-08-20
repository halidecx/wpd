
%include "ext/x86/x86util.asm"

SECTION_RODATA 32

eg_perm: dd 0, 4, 1, 5, 2, 6, 3, 7

pw_256: times 16 dw 256
bcast_alpha: db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12
             db 0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12

ct_green: db -1, 2, -1, 2, -1, 6, -1, 6, -1, 10, -1, 10, -1, 14, -1, 14
          db -1, 2, -1, 2, -1, 6, -1, 6, -1, 10, -1, 10, -1, 14, -1, 14
ct_red:   db -1, -1, -1, 1, -1, -1, -1, 5, -1, -1, -1, 9, -1, -1, -1, 13
          db -1, -1, -1, 1, -1, -1, -1, 5, -1, -1, -1, 9, -1, -1, -1, 13

blend_scale:
    dd 0
%assign bsi 1
%rep 255
    dd (1 << 24) / bsi
    %assign bsi bsi + 1
%endrep

SECTION .text

%macro AVG2 5
    pxor       %4, %2, %3
    pand       %4, %4, %5
    pavgb      %1, %2, %3
    psubb      %1, %1, %4
%endmacro

%macro SET_ONES 1
    pcmpeqb    %1, %1
    pabsb      %1, %1
%endmacro

%macro ROW_INDEX 0
    movsxdifnidn nq, nd
    lea        srcq, [srcq+nq*4]
    lea        upperq, [upperq+nq*4]
    lea        dstq, [dstq+nq*4]
    neg        nq
%endmacro


INIT_YMM avx2
cglobal pred_add_0, 4, 4, 3, src, upper, n, dst
    pcmpeqd    m0, m0
    psrld      m0, 24
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
    paddb      m1, m1, m2
    paddb      m1, m1, m0
    movu       [dstq], m1
    pshufd     m0, m1, 0xff
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
    paddb      m0, m1, m0
    movd       [dstq], m0
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET


%macro PRED_TOP 2
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

PRED_TOP 2, 0
PRED_TOP 3, 4
PRED_TOP 4, -4


%macro PRED_AVGTOP 3
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

PRED_AVGTOP 8, -4, 0
PRED_AVGTOP 9, 0, 4


%macro PRED_AVGLEFT 2
cglobal pred_add_%1, 4, 4, 5, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    pcmpeqb    m4, m4
    movd       m0, [dstq+nq*4-4]
    pxor       m0, m0, m4
.loop:
    movd       m1, [upperq+nq*4+%2]
    pxor       m1, m1, m4
    pavgb      m0, m0, m1
    movd       m2, [srcq+nq*4]
    psubb      m0, m0, m2
    pxor       m3, m0, m4
    movd       [dstq+nq*4], m3
    inc        nq
    jl .loop
.ret:
    RET
%endmacro


%macro PRED_AVG3 0
cglobal pred_add_5, 4, 4, 5, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    pcmpeqb    m4, m4
    movd       m0, [dstq+nq*4-4]
    pxor       m0, m0, m4
.loop:
    movq       m1, [upperq+nq*4]      ; t | tr
    pxor       m1, m1, m4
    psrldq     m2, m1, 4
    pavgb      m0, m0, m2
    pavgb      m0, m0, m1
    movd       m3, [srcq+nq*4]
    psubb      m0, m0, m3
    pxor       m3, m0, m4
    movd       [dstq+nq*4], m3
    inc        nq
    jl .loop
.ret:
    RET
%endmacro


%macro PRED_AVG4 0
cglobal pred_add_10, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    pcmpeqb    m5, m5
    movd       m0, [dstq+nq*4-4]
    pxor       m0, m0, m5
.loop:
    movq       m1, [upperq+nq*4-4]    ; tl | t
    movd       m2, [upperq+nq*4+4]    ; tr
    pxor       m1, m1, m5
    pxor       m2, m2, m5
    psrldq     m3, m1, 4
    pavgb      m2, m2, m3             ; off the carried chain
    pavgb      m0, m0, m1
    pavgb      m0, m0, m2
    movd       m4, [srcq+nq*4]
    psubb      m0, m0, m4
    pxor       m4, m0, m5
    movd       [dstq+nq*4], m4
    inc        nq
    jl .loop
.ret:
    RET
%endmacro


INIT_XMM avx2
cglobal pred_add_11, 4, 4, 16, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    movd       m0, [dstq+nq*4-4]
    test       nq, 1
    jz .loop2
    movd       m1, [upperq+nq*4-4]
    movd       m2, [upperq+nq*4]
    movd       m4, [srcq+nq*4]
    psadbw     m11, m0, m1
    psadbw     m8, m2, m1
    paddb      m6, m2, m4
    paddb      m0, m0, m4
    pcmpgtd    m11, m11, m8
    vpblendvb  m0, m6, m0, m11
    movd       [dstq+nq*4], m0
    inc        nq
    jz .ret
.loop2:
    movd       m1, [upperq+nq*4-4]    ; tl
    movd       m2, [upperq+nq*4]      ; t, doubles as the next pixel's tl
    movd       m3, [upperq+nq*4+4]
    movd       m4, [srcq+nq*4]
    movd       m5, [srcq+nq*4+4]
    paddb      m6, m2, m4             ; top candidate for this pixel
    paddb      m7, m3, m5             ; top candidate for the next one
    psadbw     m8, m2, m1
    psadbw     m9, m3, m2
    psadbw     m12, m6, m2
    pcmpgtd    m12, m12, m9
    paddb      m14, m6, m5
    vpblendvb  m12, m7, m14, m12
    paddb      m10, m0, m4            ; left candidate for this pixel
    psadbw     m11, m0, m1
    psadbw     m13, m10, m2
    pcmpgtd    m11, m11, m8
    pcmpgtd    m13, m13, m9
    paddb      m15, m10, m5
    pand       m14, m7, m11
    pandn      m2, m11, m12
    por        m14, m14, m2
    vpblendvb  m6, m6, m10, m11
    movd       [dstq+nq*4], m6
    pand       m13, m13, m11          ; both pixels chained left
    pand       m15, m15, m13
    pandn      m13, m13, m14
    por        m0, m15, m13
    movd       [dstq+nq*4+4], m0
    add        nq, 2
    jl .loop2
.ret:
    RET


%macro PRED_CLAMP_FULL 0
cglobal pred_add_12, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    movd       m0, [dstq+nq*4-4]
.loop:
    movq       m1, [upperq+nq*4-4]    ; tl | t
    psrldq     m2, m1, 4
    psubusb    m3, m2, m1
    psubusb    m4, m1, m2
    paddusb    m0, m0, m3
    psubusb    m0, m0, m4
    movd       m5, [srcq+nq*4]
    paddb      m0, m0, m5
    movd       [dstq+nq*4], m0
    inc        nq
    jl .loop
.ret:
    RET
%endmacro


%macro PRED_CLAMP_HALF 0
cglobal pred_add_13, 4, 4, 7, src, upper, n, dst
    test       nd, nd
    jz .ret
    ROW_INDEX
    pcmpeqw    m6, m6
    psrlw      m6, 8                  ; complements a channel, keeps it in-word
    movd       m0, [dstq+nq*4-4]
    pmovzxbw   m0, m0
    pxor       m0, m0, m6
.loop:
    pmovzxbw   m1, [upperq+nq*4-4]    ; tl | t
    pxor       m1, m1, m6
    psrldq     m2, m1, 8
    pavgb      m0, m0, m2             ; ~ave
    psubusb    m3, m1, m0
    psubusb    m4, m0, m1
    psrlw      m3, m3, 1
    psrlw      m4, m4, 1
    psubusb    m0, m0, m3
    paddusb    m0, m0, m4
    movd       m5, [srcq+nq*4]
    pmovzxbw   m5, m5
    psubb      m0, m0, m5
    pxor       m5, m0, m6
    packuswb   m5, m5, m5
    movd       [dstq+nq*4], m5
    inc        nq
    jl .loop
.ret:
    RET
%endmacro


INIT_XMM sse2
PRED_AVGLEFT 6, -4
PRED_AVGLEFT 7, 0
PRED_AVG3
PRED_AVG4
PRED_CLAMP_FULL

INIT_XMM sse4
PRED_CLAMP_HALF


INIT_YMM avx2
cglobal extract_green, 3, 4, 6, dst, src, n
    pcmpeqd    m4, m4
    psrld      m4, 24
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


INIT_YMM avx2
cglobal map_color32, 4, 5, 5, dst, src, pal, n
    pcmpeqd    m3, m3
    psrld      m3, 24
    cmp        nd, 8
    jl .tail1
.loop8:
    movu       m0, [srcq]
    psrld      m0, 16
    pand       m0, m0, m3
    pcmpeqd    m4, m4
    vpgatherdd m1, [palq+m0*4], m4
    movu       [dstq], m1
    add        srcq, 32
    add        dstq, 32
    sub        nd, 8
    cmp        nd, 8
    jge .loop8
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movzx      r4d, byte [srcq+2]
    mov        r4d, [palq+r4*4]
    mov        [dstq], r4d
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET


%macro BLEND_CH 2 ; acc, shift
    psrld      m9, m0, %2
%if %2 != 24
    pand       m9, m9, m7
%endif
%if %2 == 8
    pslld      m10, m1, 8
    pand       m10, m10, m15
%elif %2 == 16
    pand       m10, m1, m15
%else
    psrld      m10, m1, 8
    pand       m10, m10, m15
%endif
    por        m9, m9, m10
    pmaddwd    m9, m9, m3
    pmulld     m9, m9, m6
    psrld      m9, m9, 24
    pslld      m9, m9, %2
    por        %1, %1, m9
%endmacro

INIT_YMM avx2
cglobal blend_row_argb, 3, 5, 16, dst, src, n
    pcmpeqd    m7, m7
    psrld      m7, 24                  ; 0x000000ff, doubles as the opaque value
    pslld      m15, m7, 16             ; 0x00ff0000
    pcmpeqd    m8, m8
    psrld      m8, 31
    pslld      m8, 8                   ; 256
    pxor       m11, m11
    lea        r4q, [blend_scale]
    cmp        nd, 8
    jl .tail1
.loop8:
    movu       m0, [srcq]
    pand       m2, m0, m7              ; src alpha
    pcmpeqd    m14, m2, m7
    movmskps   r3d, m14
    cmp        r3d, 0xff
    je .opaque                         ; whole vector opaque: copy src
    ptest      m2, m2
    jz .next8                          ; whole vector transparent: keep dst
    movu       m1, [dstq]
    pand       m3, m1, m7              ; dst alpha
    psubd      m4, m8, m2
    pmullw     m4, m4, m3              ; 256 * 255 still fits a word
    psrld      m4, m4, 8               ; (dst_a * (256 - src_a)) >> 8
    paddd      m5, m2, m4              ; blend alpha
    pcmpeqd    m12, m12                ; vpgatherdd consumes its mask
    vpgatherdd m6, [r4q+m5*4], m12
    pslld      m3, m4, 16
    por        m3, m3, m2
    mova       m13, m5
    BLEND_CH   m13, 8
    BLEND_CH   m13, 16
    BLEND_CH   m13, 24
    pblendvb   m13, m13, m0, m14       ; opaque lanes copy src verbatim
    pcmpeqd    m9, m2, m11
    pblendvb   m13, m13, m1, m9        ; transparent lanes keep dst
    movu       [dstq], m13
    jmp .next8
.opaque:
    movu       [dstq], m0
.next8:
    add        srcq, 32
    add        dstq, 32
    sub        nd, 8
    cmp        nd, 8
    jge .loop8
.tail1:
    test       nd, nd
    jz .ret
.loop1:
    movzx      r3d, byte [srcq]
    cmp        r3d, 0xff
    je .copy1
    test       r3d, r3d
    jz .skip1
    movd       xm0, [srcq]
    movd       xm1, [dstq]
    pmovzxbd   xm0, xm0
    pmovzxbd   xm1, xm1
    pshufd     xm2, xm0, 0
    pshufd     xm3, xm1, 0
    psubd      xm4, xm8, xm2
    pmulld     xm4, xm4, xm3
    psrld      xm4, xm4, 8
    paddd      xm5, xm2, xm4
    movd       r3d, xm5
    mov        r3d, [r4q+r3*4]
    movd       xm6, r3d
    pshufd     xm6, xm6, 0
    pmulld     xm0, xm0, xm2
    pmulld     xm1, xm1, xm4
    paddd      xm0, xm0, xm1
    pmulld     xm0, xm0, xm6
    psrld      xm0, xm0, 24
    packusdw   xm0, xm0, xm0
    packuswb   xm0, xm0, xm0
    movd       [dstq], xm0
    movd       r3d, xm5                ; lane 0 held alpha scratch, not a result
    mov        [dstq], r3b
    jmp .skip1
.copy1:
    mov        r3d, [srcq]
    mov        [dstq], r3d
.skip1:
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg .loop1
.ret:
    RET

%macro BLEND_ROW_ARGB_PREMULT 0
cglobal blend_row_argb_premult, 3, 3, 10, dst, src, n
    mova      m5, [bcast_alpha]
    mova      m6, [pw_256]
    pxor      m7, m7
    sub       nd, mmsize / 4
    jl        .tail
.loop:
    movu      m0, [srcq]
    movu      m1, [dstq]
    pshufb    m2, m0, m5
    punpcklbw m3, m1, m7
    punpckhbw m4, m1, m7
    punpcklbw m1, m2, m7
    punpckhbw m2, m7
    mova      m8, m6
    psubw     m8, m1
    mova      m9, m6
    psubw     m9, m2
    pmullw    m3, m8
    pmullw    m4, m9
    psrlw     m3, 8
    psrlw     m4, 8
    packuswb  m3, m4
    paddb     m3, m0
    movu      [dstq], m3
    add       srcq, mmsize
    add       dstq, mmsize
    sub       nd, mmsize / 4
    jge       .loop
.tail:
    add       nd, mmsize / 4
    jz        .end
.tail_loop:
    movd      xmm0, [srcq]
    movd      xmm1, [dstq]
    pshufb    xmm2, xmm0, xmm5
    punpcklbw xmm1, xmm7
    punpcklbw xmm2, xmm7
    mova      xmm8, xmm6
    psubw     xmm8, xmm2
    pmullw    xmm1, xmm8
    psrlw     xmm1, 8
    packuswb  xmm1, xmm1
    paddb     xmm1, xmm0
    movd      [dstq], xmm1
    add       srcq, 4
    add       dstq, 4
    dec       nd
    jg        .tail_loop
.end:
    RET
%endmacro

INIT_XMM ssse3
BLEND_ROW_ARGB_PREMULT
INIT_YMM avx2
BLEND_ROW_ARGB_PREMULT

%macro COLOR_ROW 0
cglobal color_row, 4, 6, 6, dst, src, n, mult
    mov        r4d, multd
    sar        r4d, 24                 ; green_to_red, sign-extended
    shl        r4d, 3
    and        r4d, 0xffff
    mov        r5d, multd
    shl        r5d, 8
    sar        r5d, 24                 ; green_to_blue
    shl        r5d, 3
    shl        r5d, 16
    or         r4d, r5d
    movd       xm3, r4d
    mov        r5d, multd
    shl        r5d, 16
    sar        r5d, 24                 ; red_to_blue
    shl        r5d, 3
    shl        r5d, 16                 ; only the lane that holds blue
    movd       xm4, r5d
%if mmsize == 32
    vpbroadcastd m3, xm3
    vpbroadcastd m4, xm4
%else
    pshufd     m3, m3, 0
    pshufd     m4, m4, 0
%endif
    mova       m1, [ct_green]
    mova       m2, [ct_red]
    sub        nd, mmsize / 4
    jl         .tail
.loop:
    movu       m0, [srcq]
    pshufb     m5, m0, m1
    pmulhw     m5, m5, m3
    psllw      m5, m5, 8
    paddb      m0, m0, m5
    pshufb     m5, m0, m2
    pmulhw     m5, m5, m4
    psllw      m5, m5, 8
    paddb      m0, m0, m5
    movu       [dstq], m0
    add        srcq, mmsize
    add        dstq, mmsize
    sub        nd, mmsize / 4
    jge        .loop
.tail:
    add        nd, mmsize / 4
    jz         .end
%if mmsize == 32
    cmp        nd, 4
    jl         .tail1
    movu       xm0, [srcq]
    pshufb     xm5, xm0, xm1
    pmulhw     xm5, xm5, xm3
    psllw      xm5, xm5, 8
    paddb      xm0, xm0, xm5
    pshufb     xm5, xm0, xm2
    pmulhw     xm5, xm5, xm4
    psllw      xm5, xm5, 8
    paddb      xm0, xm0, xm5
    movu       [dstq], xm0
    add        srcq, 16
    add        dstq, 16
    sub        nd, 4
    jz         .end
.tail1:
%endif
.tail_loop:
    movd       xm0, [srcq]
    pshufb     xm5, xm0, xm1
    pmulhw     xm5, xm5, xm3
    psllw      xm5, xm5, 8
    paddb      xm0, xm0, xm5
    pshufb     xm5, xm0, xm2
    pmulhw     xm5, xm5, xm4
    psllw      xm5, xm5, 8
    paddb      xm0, xm0, xm5
    movd       [dstq], xm0
    add        srcq, 4
    add        dstq, 4
    dec        nd
    jg         .tail_loop
.end:
    RET
%endmacro

INIT_XMM ssse3
COLOR_ROW
INIT_YMM avx2
COLOR_ROW
