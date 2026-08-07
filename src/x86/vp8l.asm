
%include "asm/x86/x86util.asm"

SECTION_RODATA 32

eg_perm: dd 0, 4, 1, 5, 2, 6, 3, 7

; blend_scale[a] = (1 << 24) / a; entry 0 is never selected.
blend_scale:
    dd 0
%assign bsi 1
%rep 255
    dd (1 << 24) / bsi
    %assign bsi bsi + 1
%endrep

SECTION .text

; pavgb rounds up; subtracting (a ^ b) & 1 yields VP8L's floor average.
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


INIT_YMM avx2
cglobal pred_add_0, 4, 4, 3, src, upper, n, dst
    ; Load residuals before storing because in and out may alias.
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
    ; Keeping left in a register avoids a loop-carried store-to-load hop.
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

PRED_AVGLEFT 6, -4
PRED_AVGLEFT 7, 0


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
    AVG2       m3, m3, m5, m1, m4
    AVG2       m0, m0, m2, m5, m4
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


INIT_XMM avx2
cglobal pred_add_11, 4, 4, 6, src, upper, n, dst
    test       nd, nd
    jz .ret
    movd       m0, [dstq-4]
.loop:
    movd       m2, [upperq]
    movd       m3, [upperq-4]
    movd       m1, [srcq]
    psadbw     m4, m0, m3
    psadbw     m5, m2, m3
    paddb      m2, m2, m1
    paddb      m0, m0, m1
    pcmpgtd    m4, m4, m5
    vpblendvb  m0, m2, m0, m4
    movd       [dstq], m0
    add        srcq, 4
    add        upperq, 4
    add        dstq, 4
    dec        nd
    jg .loop
.ret:
    RET


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
    packuswb   m2, m2, m2
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


INIT_XMM avx2
cglobal pred_add_13, 4, 4, 5, src, upper, n, dst
    ; Bias negative differences before shifting to match C's truncation toward zero.
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
    psrlw      m2, 1
    psubw      m3, m2, m3
    psrlw      m4, m3, 15
    paddw      m3, m3, m4
    psraw      m3, 1
    paddw      m2, m2, m3
    packuswb   m2, m2, m2
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


INIT_YMM avx2
cglobal extract_green, 3, 4, 6, dst, src, n
    ; Keep the tail narrow: alpha rows cannot be written past num_pixels.
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
    ; vpgatherdd clears its mask operand, so rebuild it every iteration.
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


; Blend one 32-bit channel of m0 over m1 and OR the result into %1.
; m2 = src alpha, m4 = dst factor, m6 = 2^24/blend_alpha, m7 = 0x000000ff.
%macro BLEND_CH 2 ; acc, shift
    psrld      m9, m0, %2
    psrld      m10, m1, %2
    pand       m9, m9, m7
    pand       m10, m10, m7
    pmulld     m9, m9, m2
    pmulld     m10, m10, m4
    paddd      m9, m9, m10
    pmulld     m9, m9, m6
    psrld      m9, m9, 24
    pslld      m9, m9, %2
    por        %1, %1, m9
%endmacro

INIT_YMM avx2
cglobal blend_row_argb, 3, 5, 15, dst, src, n
    pcmpeqd    m7, m7
    psrld      m7, 24                  ; 0x000000ff, doubles as the opaque value
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
    pmulld     m4, m4, m3
    psrld      m4, m4, 8               ; (dst_a * (256 - src_a)) >> 8
    paddd      m5, m2, m4              ; blend alpha
    pcmpeqd    m12, m12                ; vpgatherdd consumes its mask
    vpgatherdd m6, [r4q+m5*4], m12
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
