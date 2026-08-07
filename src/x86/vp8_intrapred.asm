
%include "asm/x86/x86util.asm"

SECTION_RODATA

tm_shuf: times 8 db 0x03, 0x80

tm_shuf2: times 4 db 0x03, 0x80
          times 4 db 0x07, 0x80

vl_shuf: db 0, 1, 2, 3, 8, 9, 10, 11, 1, 2, 3, 12, 9, 10, 11, 13

SECTION .text

cextern_naked wpd_pb_1
cextern_naked wpd_pb_3

%macro PALIGNR_Q 4
%ifnidn %4, %2
    mova    %4, %2
%endif
    psllq   %1, (8-%3)*8
    psrlq   %4, %3*8
    por     %1, %4
%endmacro

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


%macro PRED16x16_H 0
cglobal pred16x16_horizontal_8, 2,3
%if cpuflag(ssse3)
    mova      m2, [wpd_pb_3]
%endif
%rep 8
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


%macro DC_FILL 2
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
    pxor       m0, m0
    psadbw     m0, [r0]
    dec        r0
    movzx     r5d, byte [r0+r1*1]
    movhlps    m1, m0
    paddd      m0, m1
    movd      r6d, m0
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


%macro PRED16x16_TOP_DC 0
cglobal pred16x16_top_dc_8, 2,4
    mov       r2, r0
    sub       r0, r1
    pxor      m1, m1
    mova      m0, [r0]
    psadbw    m0, m1
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

; Broadcast the byte at [%2] to every lane of %1.
%macro TM_SPLATB 2
%if cpuflag(avx2)
    vpbroadcastb %1, [%2]
%else
    movd    %1, [%2]
    pshufb  %1, m4
%endif
%endmacro

; Bytewise TM: with a = top -us tl and b = tl -us top (one is always 0),
; clip(top + left - tl) == (left +us a) -us b exactly.
%macro PRED16x16_TM_BYTEWISE 0
cglobal pred16x16_tm_vp8_8, 2, 4, 5, dst, stride, stride3, iteration
    sub       dstq, strideq
    mova        m0, [dstq]
%if notcpuflag(avx2)
    pxor        m4, m4
%endif
    TM_SPLATB   m2, dstq-1
    mova        m1, m0
    psubusb     m1, m2
    psubusb     m2, m0
    lea   stride3q, [strideq*3]
    mov iterationd, 4
.loop:
    TM_SPLATB   m3, dstq+strideq*1-1
    paddusb     m3, m1
    psubusb     m3, m2
    mova [dstq+strideq*1], m3
    TM_SPLATB   m3, dstq+strideq*2-1
    paddusb     m3, m1
    psubusb     m3, m2
    mova [dstq+strideq*2], m3
    TM_SPLATB   m3, dstq+stride3q-1
    paddusb     m3, m1
    psubusb     m3, m2
    mova [dstq+stride3q], m3
    TM_SPLATB   m3, dstq+strideq*4-1
    paddusb     m3, m1
    psubusb     m3, m2
    mova [dstq+strideq*4], m3
    lea       dstq, [dstq+strideq*4]
    dec iterationd
    jg .loop
    RET
%endmacro

INIT_XMM ssse3
PRED16x16_TM_BYTEWISE
INIT_XMM avx2
PRED16x16_TM_BYTEWISE
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


%macro PRED8x8_H 0
cglobal pred8x8_horizontal_8, 2,3,3
%if cpuflag(ssse3)
    mova      m2, [wpd_pb_3]
%endif
%rep 4
    SPLATB_LOAD m0, r0+r1*0-1, m2
    SPLATB_LOAD m1, r0+r1*1-1, m2
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

INIT_XMM sse2
cglobal pred8x8_dc_vp8_8, 2,7
    mov       r4, r0
    sub       r0, r1
    pxor       m0, m0
    movq       m1, [r0]
    psadbw     m0, m1
    dec        r0
    movzx     r5d, byte [r0+r1*1]
    movd      r6d, m0
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
    movd       m0, r2d
    punpcklbw  m0, m0
    pshuflw    m0, m0, 0
%rep 4
    movq [r4+r1*0], m0
    movq [r4+r1*1], m0
    lea   r4, [r4+r1*2]
%endrep
    RET


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

INIT_XMM sse2
cglobal pred4x4_dc_8, 3,5
    pxor    m7, m7
    mov     r4, r0
    sub     r0, r2
    movd    m0, [r0]
    psadbw  m0, m7
    movzx  r1d, byte [r0+r2*1-1]
    movd   r3d, m0
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


; Two rows at a time: words 0-3 hold row y, words 4-7 hold row y+1.
%macro PRED4x4_TM_STORE 3
    packuswb   %1, %1
    movd      [%2], %1
    psrldq     %1, 4
    movd      [%3], %1
%endmacro

INIT_XMM sse2
cglobal pred4x4_tm_vp8_8, 3,6
    sub         r0, r2
    pxor        m4, m4
    movd        m0, [r0]
    punpcklbw   m0, m4
    punpcklqdq  m0, m0
    movzx      r4d, byte [r0-1]
    lea         r1, [r0+r2*2]
    movzx      r3d, byte [r0+r2*1-1]
    movzx      r5d, byte [r0+r2*2-1]
    sub        r3d, r4d
    sub        r5d, r4d
    movd        m1, r3d
    movd        m2, r5d
    punpcklqdq  m1, m2
    movzx      r3d, byte [r1+r2*1-1]
    movzx      r5d, byte [r1+r2*2-1]
    sub        r3d, r4d
    sub        r5d, r4d
    movd        m2, r3d
    movd        m3, r5d
    punpcklqdq  m2, m3
    pshuflw     m1, m1, 0
    pshufhw     m1, m1, 0
    pshuflw     m2, m2, 0
    pshufhw     m2, m2, 0
    paddw       m1, m0
    paddw       m2, m0
    PRED4x4_TM_STORE m1, r0+r2*1, r0+r2*2
    PRED4x4_TM_STORE m2, r1+r2*1, r1+r2*2
    RET

INIT_XMM ssse3
cglobal pred4x4_tm_vp8_8, 3,3,6
    sub         r0, r2
    mova        m5, [tm_shuf2]
    pxor        m4, m4
    movd        m0, [r0]
    punpcklbw   m0, m4
    punpcklqdq  m0, m0
    movd        m4, [r0-4]
    pshufb      m4, [tm_shuf]
    psubw       m0, m4
    lea         r1, [r0+r2*2]
    movd        m1, [r0+r2*1-4]
    movd        m3, [r0+r2*2-4]
    punpckldq   m1, m3
    movd        m2, [r1+r2*1-4]
    movd        m3, [r1+r2*2-4]
    punpckldq   m2, m3
    pshufb      m1, m5
    pshufb      m2, m5
    paddw       m1, m0
    paddw       m2, m0
    PRED4x4_TM_STORE m1, r0+r2*1, r0+r2*2
    PRED4x4_TM_STORE m2, r1+r2*1, r1+r2*2
    RET


INIT_XMM sse2
cglobal pred4x4_vertical_vp8_8, 3,3,5
    sub       r0, r2
    movd      m1, [r0-1]
    movd      m0, [r0]
    movd      m3, [r1]
    mova      m2, m0
    punpckldq m0, m3
    lea       r1, [r0+r2*2]
    psrlq     m0, 8
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    movd [r0+r2*1], m2
    movd [r0+r2*2], m2
    movd [r1+r2*1], m2
    movd [r1+r2*2], m2
    RET


INIT_XMM sse2
cglobal pred4x4_horizontal_vp8_8, 3,7
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movzx    r3d, byte [r0-1]
    movzx    r4d, byte [r0+r2*1-1]
    movzx    r5d, byte [r0+r2*2-1]
    movzx    r6d, byte [r1+r2*1-1]

    lea      r3d, [r3+r5+2]
    lea      r3d, [r3+r4*2]
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r0+r2*1], r3d

    lea      r3d, [r4+r6+2]
    lea      r3d, [r3+r5*2]
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r0+r2*2], r3d

    movzx    r4d, byte [r1+r2*2-1]
    lea      r3d, [r5+r4+2]
    lea      r3d, [r3+r6*2]
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r1+r2*1], r3d

    lea      r3d, [r6+r4+2]
    lea      r3d, [r3+r4*2]
    shr      r3d, 2
    imul     r3d, 0x01010101
    mov [r1+r2*2], r3d
    RET


INIT_XMM ssse3
cglobal pred4x4_vertical_left_vp8_8, 3,3
    sub        r0, r2
    movd       m0, [r0]
    movd       m1, [r1]
    punpckldq  m0, m1
    mova       m1, m0
    psrldq     m1, 1
    mova       m2, m0
    psrldq     m2, 2
    mova       m3, m0
    pavgb      m3, m1
    PRED4x4_LOWPASS m4, m0, m2, m1, m5
    punpcklqdq m3, m4
    pshufb     m3, [vl_shuf]
    lea        r1, [r0+r2*2]
    movd [r0+r2*1], m3
    psrldq     m3, 4
    movd [r0+r2*2], m3
    psrldq     m3, 4
    movd [r1+r2*1], m3
    psrldq     m3, 4
    movd [r1+r2*2], m3
    RET

INIT_XMM sse2
cglobal pred4x4_down_left_8, 3,3,4
    sub       r0, r2
    movd      m1, [r0]
    movd      m3, [r1]
    punpckldq m1, m3
    mova      m2, m1
    mova      m0, m1
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


INIT_XMM sse2
cglobal pred4x4_horizontal_up_8, 3,3,6
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movd      m0, [r0+r2*1-4]
    movd      m4, [r0+r2*2-4]
    punpcklbw m0, m4
    movd      m1, [r1+r2*1-4]
    movd      m4, [r1+r2*2-4]
    punpcklbw m1, m4
    psrldq    m0, 6
    psrldq    m1, 6
    punpcklwd m0, m1
    punpcklbw m1, m0, m0
    pshuflw   m1, m1, 0xFF
    punpckldq m0, m1
    mova      m2, m0
    mova      m3, m0
    mova      m4, m0
    psrlq     m2, 16
    psrlq     m3, 8
    pavgb     m4, m3
    PRED4x4_LOWPASS m3, m0, m2, m3, m5
    punpcklbw m4, m3
    movd    [r0+r2*1], m4
    psrlq    m4, 16
    movd    [r0+r2*2], m4
    psrlq    m4, 16
    movd    [r1+r2*1], m4
    movd    [r1+r2*2], m1
    RET


INIT_XMM sse2
cglobal pred4x4_horizontal_down_8, 3,3,6
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movd      m0, [r0-4]
    movd      m3, [r0]
    punpckldq m0, m3
    psllq     m0, 8
    psrldq    m0, 4
    movd      m1, [r1+r2*2-4]
    movd      m4, [r1+r2*1-4]
    punpcklbw m1, m4
    movd      m2, [r0+r2*2-4]
    movd      m4, [r0+r2*1-4]
    punpcklbw m2, m4
    psrldq    m1, 6
    psrldq    m2, 6
    punpcklwd m1, m2
    punpckldq m1, m0
    mova      m0, m1
    mova      m2, m1
    mova      m5, m1
    psrlq     m0, 16
    psrlq     m2, 8
    pavgb     m5, m2
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    punpcklbw m5, m2
    psrlq     m2, 32
    PALIGNR_Q m2, m5, 6, m4
    movd      [r1+r2*2], m5
    psrlq     m5, 16
    movd      [r1+r2*1], m5
    psrlq     m5, 16
    movd      [r0+r2*2], m5
    movd      [r0+r2*1], m2
    RET


INIT_XMM sse2
cglobal pred4x4_vertical_right_8, 3,3,6
    sub     r0, r2
    lea     r1, [r0+r2*2]
    movd    m0, [r0]
    mova    m5, m0
    movq    m1, [r0-8]
    PALIGNR_Q m0, m1, 7, m1
    pavgb   m5, m0
    movq    m1, [r0+r2*1-8]
    PALIGNR_Q m0, m1, 7, m1
    mova    m1, m0
    movq    m2, [r0+r2*2-8]
    PALIGNR_Q m0, m2, 7, m2
    mova    m2, m0
    movq    m3, [r1+r2*1-8]
    PALIGNR_Q m0, m3, 7, m3
    PRED4x4_LOWPASS m2, m1, m0, m2, m4
    mova    m1, m2
    psrlq   m2, 16
    psllq   m1, 48
    movd    [r0+r2*1], m5
    movd    [r0+r2*2], m2
    PALIGNR_Q m5, m1, 7, m3
    psllq   m1, 8
    movd    [r1+r2*1], m5
    PALIGNR_Q m2, m1, 7, m1
    movd    [r1+r2*2], m2
    RET


INIT_XMM sse2
cglobal pred4x4_down_right_8, 3,3,5
    sub       r0, r2
    lea       r1, [r0+r2*2]
    movd      m3, [r0-4]
    movd      m4, [r0]
    punpckldq m3, m4
    psrldq    m3, 3
    movd      m1, [r1-4]
    movd      m2, [r0+r2*1-4]
    punpcklbw m1, m2
    psrldq    m1, 6
    pslldq    m3, 2
    por       m3, m1
    mova      m1, m3
    movq      m4, [r1+r2*1-8]
    PALIGNR_Q m3, m4, 7, m4
    mova      m0, m3
    movq      m4, [r1+r2*2-8]
    PALIGNR_Q m3, m4, 7, m4
    PRED4x4_LOWPASS m0, m3, m1, m0, m4
    movd      [r1+r2*2], m0
    psrlq     m0, 8
    movd      [r1+r2*1], m0
    psrlq     m0, 8
    movd      [r0+r2*2], m0
    psrlq     m0, 8
    movd      [r0+r2*1], m0
    RET
