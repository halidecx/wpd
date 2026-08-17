
%define private_prefix ff
%define public_prefix  avpriv
%define cpuflags_mmxext cpuflags_mmx2

%if HAVE_X86_SSE2AVX
%define FORCE_VEX_ENCODING 1
%endif

%include "ext/x86/x86inc.asm"

%define PASS8ROWS(base, base3, stride, stride3) \
    [base],           [base  + stride],   [base  + 2*stride], [base3], \
    [base3 + stride], [base3 + 2*stride], [base3 + stride3],  [base3 + stride*4]

%macro SBUTTERFLY 4
%ifidn %1, dqqq
    vperm2i128  m%4, m%2, m%3, q0301
    vinserti128 m%2, m%2, xm%3, 1
%elif avx_enabled == 0
    mova      m%4, m%2
    punpckl%1 m%2, m%3
    punpckh%1 m%4, m%3
%else
    punpckh%1 m%4, m%2, m%3
    punpckl%1 m%2, m%3
%endif
    SWAP %3, %4
%endmacro

%macro SBUTTERFLY2 4
    punpckl%1 m%4, m%2, m%3
    punpckh%1 m%2, m%2, m%3
    SWAP %2, %4, %3
%endmacro

%macro SBUTTERFLYPS 3
    unpcklps m%3, m%1, m%2
    unpckhps m%1, m%1, m%2
    SWAP %1, %3, %2
%endmacro

%macro SBUTTERFLYPD 3
    movlhps m%3, m%1, m%2
    movhlps m%2, m%2, m%1
    SWAP %1, %3
%endmacro

%macro TRANSPOSE4x4B 5
    SBUTTERFLY bw, %1, %2, %5
    SBUTTERFLY bw, %3, %4, %5
    SBUTTERFLY wd, %1, %3, %5
    SBUTTERFLY wd, %2, %4, %5
    SWAP %2, %3
%endmacro

%macro TRANSPOSE4x4W 5
    SBUTTERFLY wd, %1, %2, %5
    SBUTTERFLY wd, %3, %4, %5
    SBUTTERFLY dq, %1, %3, %5
    SBUTTERFLY dq, %2, %4, %5
    SWAP %2, %3
%endmacro

%macro TRANSPOSE2x4x4B 5
    SBUTTERFLY bw,  %1, %2, %5
    SBUTTERFLY bw,  %3, %4, %5
    SBUTTERFLY wd,  %1, %3, %5
    SBUTTERFLY wd,  %2, %4, %5
    SBUTTERFLY dq,  %1, %2, %5
    SBUTTERFLY dq,  %3, %4, %5
%endmacro

%macro TRANSPOSE2x4x4W 5
    SBUTTERFLY wd,  %1, %2, %5
    SBUTTERFLY wd,  %3, %4, %5
    SBUTTERFLY dq,  %1, %3, %5
    SBUTTERFLY dq,  %2, %4, %5
    SBUTTERFLY qdq, %1, %2, %5
    SBUTTERFLY qdq, %3, %4, %5
%endmacro

%macro TRANSPOSE4x4D 5
    SBUTTERFLY dq,  %1, %2, %5
    SBUTTERFLY dq,  %3, %4, %5
    SBUTTERFLY qdq, %1, %3, %5
    SBUTTERFLY qdq, %2, %4, %5
    SWAP %2, %3
%endmacro

%macro TRANSPOSE4x4PS 5
    SBUTTERFLYPS %1, %2, %5
    SBUTTERFLYPS %3, %4, %5
    SBUTTERFLYPD %1, %3, %5
    SBUTTERFLYPD %2, %4, %5
    SWAP %2, %3
%endmacro

%macro TRANSPOSE8x4D 9-11
%if ARCH_X86_64
    SBUTTERFLY dq,  %1, %2, %9
    SBUTTERFLY dq,  %3, %4, %9
    SBUTTERFLY dq,  %5, %6, %9
    SBUTTERFLY dq,  %7, %8, %9
    SBUTTERFLY qdq, %1, %3, %9
    SBUTTERFLY qdq, %2, %4, %9
    SBUTTERFLY qdq, %5, %7, %9
    SBUTTERFLY qdq, %6, %8, %9
    SWAP %2, %5
    SWAP %4, %7
%else
    movdqa %9, m%7
    SBUTTERFLY dq,  %1, %2, %7
    movdqa %10, m%2
    movdqa m%7, %9
    SBUTTERFLY dq,  %3, %4, %2
    SBUTTERFLY dq,  %5, %6, %2
    SBUTTERFLY dq,  %7, %8, %2
    SBUTTERFLY qdq, %1, %3, %2
    movdqa %9, m%3
    movdqa m%2, %10
    SBUTTERFLY qdq, %2, %4, %3
    SBUTTERFLY qdq, %5, %7, %3
    SBUTTERFLY qdq, %6, %8, %3
    SWAP %2, %5
    SWAP %4, %7
%if %0<11
    movdqa m%3, %9
%endif
%endif
%endmacro

%macro TRANSPOSE8x8W 9-11
%if ARCH_X86_64
    SBUTTERFLY wd,  %1, %2, %9
    SBUTTERFLY wd,  %3, %4, %9
    SBUTTERFLY wd,  %5, %6, %9
    SBUTTERFLY wd,  %7, %8, %9
    SBUTTERFLY dq,  %1, %3, %9
    SBUTTERFLY dq,  %2, %4, %9
    SBUTTERFLY dq,  %5, %7, %9
    SBUTTERFLY dq,  %6, %8, %9
    SBUTTERFLY qdq, %1, %5, %9
    SBUTTERFLY qdq, %2, %6, %9
    SBUTTERFLY qdq, %3, %7, %9
    SBUTTERFLY qdq, %4, %8, %9
    SWAP %2, %5
    SWAP %4, %7
%else
%if %0<11
    movdqa %9, m%7
%endif
    SBUTTERFLY wd,  %1, %2, %7
    movdqa %10, m%2
    movdqa m%7, %9
    SBUTTERFLY wd,  %3, %4, %2
    SBUTTERFLY wd,  %5, %6, %2
    SBUTTERFLY wd,  %7, %8, %2
    SBUTTERFLY dq,  %1, %3, %2
    movdqa %9, m%3
    movdqa m%2, %10
    SBUTTERFLY dq,  %2, %4, %3
    SBUTTERFLY dq,  %5, %7, %3
    SBUTTERFLY dq,  %6, %8, %3
    SBUTTERFLY qdq, %1, %5, %3
    SBUTTERFLY qdq, %2, %6, %3
    movdqa %10, m%2
    movdqa m%3, %9
    SBUTTERFLY qdq, %3, %7, %2
    SBUTTERFLY qdq, %4, %8, %2
    SWAP %2, %5
    SWAP %4, %7
%if %0<11
    movdqa m%5, %10
%endif
%endif
%endmacro

%macro TRANSPOSE16x16W 18-19
%if %0 < 19
    mova       %17, m%7
%endif

    SBUTTERFLY dqqq, %1,  %9, %7
    SBUTTERFLY dqqq, %2, %10, %7
    SBUTTERFLY dqqq, %3, %11, %7
    SBUTTERFLY dqqq, %4, %12, %7
    SBUTTERFLY dqqq, %5, %13, %7
    SBUTTERFLY dqqq, %6, %14, %7
    mova       %18, m%14
    mova       m%7, %17
    SBUTTERFLY dqqq, %7, %15, %14
    SBUTTERFLY dqqq, %8, %16, %14

    SBUTTERFLY  wd,  %1,  %2, %14
    SBUTTERFLY  wd,  %3,  %4, %14
    SBUTTERFLY  wd,  %5,  %6, %14
    SBUTTERFLY  wd,  %7,  %8, %14
    SBUTTERFLY  wd,  %9, %10, %14
    SBUTTERFLY  wd, %11, %12, %14
    mova       %17, m%12
    mova      m%14, %18
    SBUTTERFLY  wd, %13, %14, %12
    SBUTTERFLY  wd, %15, %16, %12

    SBUTTERFLY  dq,  %1,  %3, %12
    SBUTTERFLY  dq,  %2,  %4, %12
    SBUTTERFLY  dq,  %5,  %7, %12
    SBUTTERFLY  dq,  %6,  %8, %12
    SBUTTERFLY  dq,  %9, %11, %12
    mova       %18, m%11
    mova      m%12, %17
    SBUTTERFLY  dq, %10, %12, %11
    SBUTTERFLY  dq, %13, %15, %11
    SBUTTERFLY  dq, %14, %16, %11

    SBUTTERFLY qdq,  %1,  %5, %11
    SBUTTERFLY qdq,  %2,  %6, %11
    SBUTTERFLY qdq,  %3,  %7, %11
    SBUTTERFLY qdq,  %4,  %8, %11

    SWAP        %2, %5
    SWAP        %4, %7

    SBUTTERFLY qdq,  %9, %13, %11
    SBUTTERFLY qdq, %10, %14, %11
    mova      m%11, %18
    mova       %18, m%5
    SBUTTERFLY qdq, %11, %15, %5
    SBUTTERFLY qdq, %12, %16, %5

%if %0 < 19
    mova       m%5, %18
%endif

    SWAP       %10, %13
    SWAP       %12, %15
%endmacro

%macro TRANSPOSE_8X8B 8
    %if mmsize == 8
        %error "This macro does not support mmsize == 8"
    %endif
    punpcklbw m%1, m%2
    punpcklbw m%3, m%4
    punpcklbw m%5, m%6
    punpcklbw m%7, m%8
    TRANSPOSE4x4W %1, %3, %5, %7, %2
    MOVHL m%2, m%1
    MOVHL m%4, m%3
    MOVHL m%6, m%5
    MOVHL m%8, m%7
%endmacro

%macro PABSW 2
%if cpuflag(ssse3)
    pabsw      %1, %2
%elif cpuflag(mmxext)
    pxor    %1, %1
    psubw   %1, %2
    pmaxsw  %1, %2
%else
    pxor       %1, %1
    pcmpgtw    %1, %2
    pxor       %2, %1
    psubw      %2, %1
    SWAP       %1, %2
%endif
%endmacro

%macro PSIGNW 2
%if cpuflag(ssse3)
    psignw     %1, %2
%else
    pxor       %1, %2
    psubw      %1, %2
%endif
%endmacro

%macro ABS1 2
%if cpuflag(ssse3)
    pabsw   %1, %1
%elif cpuflag(mmxext)
    pxor    %2, %2
    psubw   %2, %1
    pmaxsw  %1, %2
%else
    pxor       %2, %2
    pcmpgtw    %2, %1
    pxor       %1, %2
    psubw      %1, %2
%endif
%endmacro

%macro ABS2 4
%if cpuflag(ssse3)
    pabsw   %1, %1
    pabsw   %2, %2
%elif cpuflag(mmxext)
    pxor    %3, %3
    pxor    %4, %4
    psubw   %3, %1
    psubw   %4, %2
    pmaxsw  %1, %3
    pmaxsw  %2, %4
%else
    pxor       %3, %3
    pxor       %4, %4
    pcmpgtw    %3, %1
    pcmpgtw    %4, %2
    pxor       %1, %3
    pxor       %2, %4
    psubw      %1, %3
    psubw      %2, %4
%endif
%endmacro

%macro ABSB 2
%if cpuflag(ssse3)
    pabsb   %1, %1
%else
    pxor    %2, %2
    psubb   %2, %1
    pminub  %1, %2
%endif
%endmacro

%macro ABSB2 4
%if cpuflag(ssse3)
    pabsb   %1, %1
    pabsb   %2, %2
%else
    pxor    %3, %3
    pxor    %4, %4
    psubb   %3, %1
    psubb   %4, %2
    pminub  %1, %3
    pminub  %2, %4
%endif
%endmacro

%macro ABSD2 4
    pxor    %3, %3
    pxor    %4, %4
    pcmpgtd %3, %1
    pcmpgtd %4, %2
    pxor    %1, %3
    pxor    %2, %4
    psubd   %1, %3
    psubd   %2, %4
%endmacro

%macro ABS4 6
    ABS2 %1, %2, %5, %6
    ABS2 %3, %4, %5, %6
%endmacro

%macro SPLATB_LOAD 3
%if cpuflag(ssse3)
    movd      %1, [%2-3]
    pshufb    %1, %3
%else
    movd      %1, [%2-3]
    punpcklbw %1, %1
    SPLATW    %1, %1, 3
%endif
%endmacro

%macro SPLATB_REG 3
%if cpuflag(ssse3)
    movd      %1, %2d
    pshufb    %1, %3
%else
    movd      %1, %2d
    punpcklbw %1, %1
    SPLATW    %1, %1, 0
%endif
%endmacro

%macro HADDD 2
%if sizeof%1 == 32
%define %2 xmm%2
    vextracti128 %2, %1, 1
%define %1 xmm%1
    paddd   %1, %2
%endif
%if mmsize >= 16
%if cpuflag(xop) && sizeof%1 == 16
    vphadddq %1, %1
%endif
    movhlps %2, %1
    paddd   %1, %2
%endif
%if notcpuflag(xop) || sizeof%1 != 16
%if cpuflag(mmxext)
    PSHUFLW %2, %1, q0032
%else
    mova    %2, %1
    psrlq   %2, 32
%endif
    paddd   %1, %2
%endif
%undef %1
%undef %2
%endmacro

%macro HADDW 2
%if cpuflag(xop) && sizeof%1 == 16
    vphaddwq  %1, %1
    movhlps   %2, %1
    paddd     %1, %2
%else
    pmaddwd %1, [pw_1]
    HADDD   %1, %2
%endif
%endmacro

%macro HADDPS 3
%if cpuflag(sse3)
    haddps  %1, %1, %2
%else
    movaps  %3, %1
    shufps  %1, %2, q2020
    shufps  %3, %2, q3131
    addps   %1, %3
%endif
%endmacro

%macro PALIGNR 4-5
%if cpuflag(ssse3)
%if %0==5
    palignr %1, %2, %3, %4
%else
    palignr %1, %2, %3
%endif
%else
    %define %%dst %1
%if %0==5
%ifnidn %1, %2
    mova    %%dst, %2
%endif
    %rotate 1
%endif
%ifnidn %4, %2
    mova    %4, %2
%endif
%if mmsize==8
    psllq   %%dst, (8-%3)*8
    psrlq   %4, %3*8
%else
    pslldq  %%dst, 16-%3
    psrldq  %4, %3
%endif
    por     %%dst, %4
%endif
%endmacro

%macro PAVGB 2-4
%if cpuflag(mmxext)
    pavgb   %1, %2
%elif cpuflag(3dnow)
    pavgusb %1, %2
%elif cpuflag(mmx)
    movu   %3, %2
    por    %3, %1
    pxor   %1, %2
    pand   %1, %4
    psrlq  %1, 1
    psubb  %3, %1
    SWAP   %1, %3
%endif
%endmacro

%macro PSHUFLW 1+
    %if mmsize == 8
        pshufw %1
    %else
        pshuflw %1
    %endif
%endmacro

%macro PSWAPD 2
%if cpuflag(mmxext)
    pshufw    %1, %2, q1032
%elif cpuflag(3dnowext)
    pswapd    %1, %2
%elif cpuflag(3dnow)
    movq      %1, %2
    psrlq     %1, 32
    punpckldq %1, %2
%endif
%endmacro

%macro DEINTB 5
%ifnum %5
    pand   m%3, m%5, m%4
    pand   m%1, m%5, m%2
%else
    mova   m%1, %5
    pand   m%3, m%1, m%4
    pand   m%1, m%1, m%2
%endif
    psrlw  m%2, 8
    psrlw  m%4, 8
%endmacro

%macro SUMSUB_BA 3-4
%if %0==3
    padd%1  m%2, m%3
    padd%1  m%3, m%3
    psub%1  m%3, m%2
%else
%if avx_enabled == 0
    mova    m%4, m%2
    padd%1  m%2, m%3
    psub%1  m%3, m%4
%else
    padd%1  m%4, m%2, m%3
    psub%1  m%3, m%2
    SWAP    %2, %4
%endif
%endif
%endmacro

%macro SUMSUB_BADC 5-6
%if %0==6
    SUMSUB_BA %1, %2, %3, %6
    SUMSUB_BA %1, %4, %5, %6
%else
    padd%1  m%2, m%3
    padd%1  m%4, m%5
    padd%1  m%3, m%3
    padd%1  m%5, m%5
    psub%1  m%3, m%2
    psub%1  m%5, m%4
%endif
%endmacro

%macro SUMSUB2_AB 4
%ifnum %3
    psub%1  m%4, m%2, m%3
    psub%1  m%4, m%3
    padd%1  m%2, m%2
    padd%1  m%2, m%3
%else
    mova    m%4, m%2
    padd%1  m%2, m%2
    padd%1  m%2, %3
    psub%1  m%4, %3
    psub%1  m%4, %3
%endif
%endmacro

%macro SUMSUB2_BA 4
%if avx_enabled == 0
    mova    m%4, m%2
    padd%1  m%2, m%3
    padd%1  m%2, m%3
    psub%1  m%3, m%4
    psub%1  m%3, m%4
%else
    padd%1  m%4, m%2, m%3
    padd%1  m%4, m%3
    psub%1  m%3, m%2
    psub%1  m%3, m%2
    SWAP     %2,  %4
%endif
%endmacro

%macro SUMSUBD2_AB 5
%ifnum %4
    psra%1  m%5, m%2, 1
    psra%1  m%4, m%3, 1
    padd%1  m%4, m%2
    psub%1  m%5, m%3
    SWAP     %2, %5
    SWAP     %3, %4
%else
    mova    %5, m%2
    mova    %4, m%3
    psra%1  m%3, 1
    psra%1  m%2, 1
    padd%1  m%3, %5
    psub%1  m%2, %4
%endif
%endmacro

%macro DCT4_1D 5
%ifnum %5
    SUMSUB_BADC w, %4, %1, %3, %2, %5
    SUMSUB_BA   w, %3, %4, %5
    SUMSUB2_AB  w, %1, %2, %5
    SWAP %1, %3, %4, %5, %2
%else
    SUMSUB_BADC w, %4, %1, %3, %2
    SUMSUB_BA   w, %3, %4
    mova     [%5], m%2
    SUMSUB2_AB  w, %1, [%5], %2
    SWAP %1, %3, %4, %2
%endif
%endmacro

%macro IDCT4_1D 6-7
%ifnum %6
    SUMSUBD2_AB %1, %3, %5, %7, %6
    SUMSUB_BA   %1, %4, %2, %7
    SUMSUB_BADC %1, %5, %4, %3, %2, %7
%else
%ifidn %1, w
    SUMSUBD2_AB %1, %3, %5, [%6], [%6+16]
%else
    SUMSUBD2_AB %1, %3, %5, [%6], [%6+32]
%endif
    SUMSUB_BA   %1, %4, %2
    SUMSUB_BADC %1, %5, %4, %3, %2
%endif
    SWAP %2, %5, %4
%endmacro


%macro LOAD_DIFF 5
%ifidn %3, none
    movh       %1, %4
    movh       %2, %5
    punpcklbw  %1, %2
    punpcklbw  %2, %2
    psubw      %1, %2
%else
    movh       %1, %4
    punpcklbw  %1, %3
    movh       %2, %5
    punpcklbw  %2, %3
    psubw      %1, %2
%endif
%endmacro

%macro STORE_DCT 6
    movq   [%5+%6+ 0], m%1
    movq   [%5+%6+ 8], m%2
    movq   [%5+%6+16], m%3
    movq   [%5+%6+24], m%4
    movhps [%5+%6+32], m%1
    movhps [%5+%6+40], m%2
    movhps [%5+%6+48], m%3
    movhps [%5+%6+56], m%4
%endmacro

%macro LOAD_DIFF_8x4P 7-10 r0,r2,0
    LOAD_DIFF m%1, m%5, m%7, [%8],      [%9]
    LOAD_DIFF m%2, m%6, m%7, [%8+r1],   [%9+r3]
    LOAD_DIFF m%3, m%5, m%7, [%8+2*r1], [%9+2*r3]
    LOAD_DIFF m%4, m%6, m%7, [%8+r4],   [%9+r5]
%if %10
    lea %8, [%8+4*r1]
    lea %9, [%9+4*r3]
%endif
%endmacro

%macro DIFFx2 6-7
    movh       %3, %5
    punpcklbw  %3, %4
    psraw      %1, 6
    paddsw     %1, %3
    movh       %3, %6
    punpcklbw  %3, %4
    psraw      %2, 6
    paddsw     %2, %3
    packuswb   %2, %1
%endmacro

%macro STORE_DIFF 4
    movh       %2, %4
    punpcklbw  %2, %3
    psraw      %1, 6
    paddsw     %1, %2
    packuswb   %1, %1
    movh       %4, %1
%endmacro

%macro STORE_DIFFx2 8
    movh       %3, [%7]
    movh       %4, [%7+%8]
    psraw      %1, %6
    psraw      %2, %6
    punpcklbw  %3, %5
    punpcklbw  %4, %5
    paddw      %3, %1
    paddw      %4, %2
    packuswb   %3, %5
    packuswb   %4, %5
    movh     [%7], %3
    movh  [%7+%8], %4
%endmacro

%macro PMINUB 3
%if cpuflag(mmxext)
    pminub   %1, %2
%else
    mova     %3, %1
    psubusb  %3, %2
    psubb    %1, %3
%endif
%endmacro

%macro SPLATW 2-3 0
%if cpuflag(avx2) && %3 == 0
    vpbroadcastw %1, %2
%elif mmsize == 16
    pshuflw    %1, %2, (%3)*0x55
    punpcklqdq %1, %1
%elif cpuflag(mmxext)
    pshufw     %1, %2, (%3)*0x55
%else
    %ifnidn %1, %2
        mova       %1, %2
    %endif
    %if %3 & 2
        punpckhwd  %1, %1
    %else
        punpcklwd  %1, %1
    %endif
    %if %3 & 1
        punpckhwd  %1, %1
    %else
        punpcklwd  %1, %1
    %endif
%endif
%endmacro

%macro SPLATD 1
%if mmsize == 8
    punpckldq  %1, %1
%elif cpuflag(sse2)
    pshufd  %1, %1, 0
%elif cpuflag(sse)
    shufps  %1, %1, 0
%endif
%endmacro

%macro CLIPUB 3
    pmaxub %1, %2
    pminub %1, %3
%endmacro

%macro CLIPW 3
    pmaxsw %1, %2
    pminsw %1, %3
%endmacro

%macro PMINSD 3
%if cpuflag(sse4)
    pminsd    %1, %2
%else
    mova      %3, %2
    pcmpgtd   %3, %1
    pxor      %1, %2
    pand      %1, %3
    pxor      %1, %2
%endif
%endmacro

%macro PMAXSD 3
%if cpuflag(sse4)
    pmaxsd    %1, %2
%else
    mova      %3, %1
    pcmpgtd   %3, %2
    pand      %1, %3
    pandn     %3, %2
    por       %1, %3
%endif
%endmacro

%macro CLIPD 3-4
%if cpuflag(sse4);  src/dst, min, max, unused
    pminsd  %1, %3
    pmaxsd  %1, %2
%elif cpuflag(sse2)
    cvtdq2ps  %1, %1
    minps     %1, %3
    maxps     %1, %2
    cvtps2dq  %1, %1
%else
    PMINSD    %1, %3, %4
    PMAXSD    %1, %2, %4
%endif
%endmacro

%macro VBROADCASTSS 2
%if cpuflag(avx2)
    vbroadcastss  %1, %2
%elif cpuflag(avx)
    %ifnum sizeof%2
        shufps  xmm%1, xmm%2, xmm%2, q0000
        %if sizeof%1 >= 32
            vinsertf128  %1, %1, xmm%1, 1
        %endif
    %else
        vbroadcastss  %1, %2
    %endif
%else
    %ifnum sizeof%2
        shufps  %1, %2, %2, q0000
    %else
        movss   %1, %2
        shufps  %1, %1, 0
    %endif
%endif
%endmacro

%macro VBROADCASTSD 2
%if cpuflag(avx) && mmsize == 32
    vbroadcastsd %1, %2
%elif cpuflag(sse3)
    movddup      %1, %2
%else
    movsd        %1, %2
    movlhps      %1, %1
%endif
%endmacro

%macro VPBROADCASTD 2
%if cpuflag(avx2)
    vpbroadcastd  %1, %2
%elif cpuflag(avx) && sizeof%1 >= 32
    %error vpbroadcastd not possible with ymm on avx1. try vbroadcastss
%else
    %ifnum sizeof%2
        pshufd  %1, %2, q0000
    %else
        movd    %1, %2
        pshufd  %1, %1, 0
    %endif
%endif
%endmacro

%macro VBROADCASTI128 2
%if mmsize > 16
    vbroadcasti128 %1, %2
%else
    mova           %1, %2
%endif
%endmacro

%macro SHUFFLE_MASK_W 8
    %rep 8
        %if %1>=0x80
            db %1, %1
        %else
            db %1*2
            db %1*2+1
        %endif
        %rotate 1
    %endrep
%endmacro

%macro PMOVSXWD 2; dst, src
%if cpuflag(sse4)
    pmovsxwd     %1, %2
%else
    %ifnidn %1, %2
    mova         %1, %2
    %endif
    punpcklwd    %1, %1
    psrad        %1, 16
%endif
%endmacro

%macro FMULADD_PS 5
    %if cpuflag(fma3) || cpuflag(fma4)
        fmaddps %1, %2, %3, %4
    %elifidn %1, %4
        mulps   %5, %2, %3
        addps   %1, %4, %5
    %else
        mulps   %1, %2, %3
        addps   %1, %4
    %endif
%endmacro

%macro LSHIFT 2
%if mmsize > 8
    pslldq  %1, %2
%else
    psllq   %1, 8*(%2)
%endif
%endmacro

%macro RSHIFT 2
%if mmsize > 8
    psrldq  %1, %2
%else
    psrlq   %1, 8*(%2)
%endif
%endmacro

%macro MOVHL 2
%ifidn %1, %2
    punpckhqdq %1, %2
%elif cpuflag(avx)
    punpckhqdq %1, %2, %2
%elif cpuflag(sse4)
    pshufd     %1, %2, q3232
%else
    movhlps    %1, %2
%endif
%endmacro

%macro HSUMPS 2
%if cpuflag(avx)
    %if sizeof%1>=32
        vperm2f128  %2, %1, %1, (0)*16+(1)
        addps       %1, %2
    %endif
    shufps      %2, %1, %1, q1032
    addps       %1, %2
    shufps      %2, %1, %1, q0321
    addps       %1, %2
%else
    movaps      %2, %1
    shufps      %1, %1, q1032
    addps       %1, %2
    movaps      %2, %1
    shufps      %1, %1, q0321
    addps       %1, %2
%endif
%endmacro

%macro BLENDVPS 3
%if cpuflag(avx)
    blendvps  %1, %1, %2, %3
%elif cpuflag(sse4)
    %ifnidn %3,xmm0
        %error sse41 blendvps uses xmm0 as default 3d operand, you used %3
    %endif
    blendvps  %1, %2, %3
%else
    xorps  %2, %1
    andps  %2, %3
    xorps  %1, %2
%endif
%endmacro

%macro PBLENDVB 3
%if cpuflag(avx)
    %if cpuflag(avx) && notcpuflag(avx2) && sizeof%1 >= 32
        %error pblendb not possible with ymm on avx1, try blendvps.
    %endif
    pblendvb  %1, %1, %2, %3
%elif cpuflag(sse4)
    %ifnidn %3,xmm0
        %error sse41 pblendvd uses xmm0 as default 3d operand, you used %3
    %endif
    pblendvb  %1, %2, %3
%else
    pxor  %2, %1
    pand  %2, %3
    pxor  %1, %2
%endif
%endmacro

%ifidn __OUTPUT_FORMAT__,win64
    section .x86util info
        db 0
%elifidn __OUTPUT_FORMAT__,win32
    section .x86util info
        db 0
%endif
