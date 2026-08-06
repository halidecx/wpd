
#include "config.h"

#ifdef __ELF__
#   define ELF
#else
#   define ELF @
#endif

#if CONFIG_THUMB
#   define A @
#   define T
#else
#   define A
#   define T @
#endif

#if   HAVE_NEON
        .arch           armv7-a
#elif HAVE_ARMV6T2
        .arch           armv6t2
#elif HAVE_ARMV6
        .arch           armv6
#elif HAVE_ARMV5TE
        .arch           armv5te
#endif

#if   HAVE_NEON
        .fpu            neon
#elif HAVE_ARMVFP
        .fpu            vfp
#endif

        .syntax unified
T       .thumb

.macro  function name, export=0
    .macro endfunc
ELF     .size   \name, . - \name
        .purgem endfunc
    .endm
        .text
        .align          2
    .if \export
        .global \name
    .endif
ELF     .type   \name, %function
\name:
.endm

.macro  const   name, align=2
    .macro endconst
ELF     .size   \name, . - \name
        .purgem endconst
    .endm
        .section        .rodata
        .align          \align
\name:
.endm

#if !HAVE_ARMV6T2
.macro  movw    rd, val
        mov     \rd, \val &  255
        orr     \rd, \val & ~255
.endm
#endif

.macro  mov32   rd, val
#if HAVE_ARMV6T2
        movw            \rd, #(\val) & 0xffff
    .if (\val) >> 16
        movt            \rd, #(\val) >> 16
    .endif
#else
        ldr             \rd, =\val
#endif
.endm

.macro  movrel rd, val
#if HAVE_ARMV6T2 && !CONFIG_PIC && !defined(__APPLE__)
        movw            \rd, #:lower16:\val
        movt            \rd, #:upper16:\val
#else
        ldr             \rd, =\val
#endif
.endm

.macro  ldr_dpre        rt,  rn,  rm:vararg
A       ldr             \rt, [\rn, -\rm]!
T       sub             \rn, \rn, \rm
T       ldr             \rt, [\rn]
.endm

.macro  ldr_nreg        rt,  rn,  rm:vararg
A       ldr             \rt, [\rn, -\rm]
T       sub             \rt, \rn, \rm
T       ldr             \rt, [\rt]
.endm

.macro  ldr_post        rt,  rn,  rm:vararg
A       ldr             \rt, [\rn], \rm
T       ldr             \rt, [\rn]
T       add             \rn, \rn, \rm
.endm

.macro  ldrb_post       rt,  rn,  rm
A       ldrb            \rt, [\rn], \rm
T       ldrb            \rt, [\rn]
T       add             \rn, \rn, \rm
.endm

.macro  str_post       rt,  rn,  rm:vararg
A       str             \rt, [\rn], \rm
T       str             \rt, [\rn]
T       add             \rn, \rn, \rm
.endm

.macro  strb_post       rt,  rn,  rm:vararg
A       strb            \rt, [\rn], \rm
T       strb            \rt, [\rn]
T       add             \rn, \rn, \rm
.endm

.macro  strh_post       rt,  rn,  rm
A       strh            \rt, [\rn], \rm
T       strh            \rt, [\rn]
T       add             \rn, \rn, \rm
.endm

#if HAVE_VFP_ARGS
        .eabi_attribute 28, 1
#endif

#define GLUE(a, b) a ## b
#define JOIN(a, b) GLUE(a, b)
#define X(s) JOIN(EXTERN_ASM, s)
