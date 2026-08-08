
#include "config.h"

#ifdef __ELF__
#   define ELF
#else
#   define ELF #
#endif

#if HAVE_AS_FUNC
#   define FUNC
#else
#   define FUNC #
#endif

#ifndef __has_feature
#   define __has_feature(x) 0
#endif

#if HAVE_AS_ARCH_DIRECTIVE
        .arch           AS_ARCH_LEVEL
#endif

#if HAVE_AS_ARCHEXT_CRC_DIRECTIVE
#define ENABLE_ARM_CRC  .arch_extension crc
#define DISABLE_ARM_CRC .arch_extension nocrc
#else
#define ENABLE_ARM_CRC
#define DISABLE_ARM_CRC
#endif

#if HAVE_AS_ARCHEXT_DOTPROD_DIRECTIVE
#define ENABLE_DOTPROD  .arch_extension dotprod
#define DISABLE_DOTPROD .arch_extension nodotprod
#else
#define ENABLE_DOTPROD
#define DISABLE_DOTPROD
#endif

#if HAVE_AS_ARCHEXT_I8MM_DIRECTIVE
#define ENABLE_I8MM  .arch_extension i8mm
#define DISABLE_I8MM .arch_extension noi8mm
#else
#define ENABLE_I8MM
#define DISABLE_I8MM
#endif

#if HAVE_AS_ARCHEXT_AES_DIRECTIVE
#define ENABLE_PMULL  .arch_extension aes
#define DISABLE_PMULL .arch_extension noaes
#else
#define ENABLE_PMULL
#define DISABLE_PMULL
#endif

#if HAVE_AS_ARCHEXT_SHA3_DIRECTIVE
#define ENABLE_EOR3  .arch_extension sha3
#define DISABLE_EOR3 .arch_extension nosha3
#else
#define ENABLE_EOR3
#define DISABLE_EOR3
#endif

#if HAVE_AS_ARCHEXT_SVE_DIRECTIVE
#define ENABLE_SVE  .arch_extension sve
#define DISABLE_SVE .arch_extension nosve
#else
#define ENABLE_SVE
#define DISABLE_SVE
#endif

#if HAVE_AS_ARCHEXT_SVE2_DIRECTIVE
#define ENABLE_SVE2  .arch_extension sve2
#define DISABLE_SVE2 .arch_extension nosve2
#else
#define ENABLE_SVE2
#define DISABLE_SVE2
#endif

#if HAVE_AS_ARCHEXT_SME_DIRECTIVE
#define ENABLE_SME   .arch_extension sme
#define DISABLE_SME  .arch_extension nosme
#else
#define ENABLE_SME
#define DISABLE_SME
#endif

#if HAVE_AS_ARCHEXT_SME_I16I64_DIRECTIVE
#define ENABLE_SME_I16I64  .arch_extension sme-i16i64
#define DISABLE_SME_I16I64 .arch_extension nosme-i16i64
#else
#define ENABLE_SME_I16I64
#define DISABLE_SME_I16I64
#endif

#if HAVE_AS_ARCHEXT_SME2_DIRECTIVE
#define ENABLE_SME2   .arch_extension sme2
#define DISABLE_SME2  .arch_extension nosme2
#else
#define ENABLE_SME2
#define DISABLE_SME2
#endif

DISABLE_ARM_CRC
DISABLE_PMULL
DISABLE_EOR3
DISABLE_DOTPROD
DISABLE_I8MM
DISABLE_SVE
DISABLE_SVE2
DISABLE_SME
DISABLE_SME_I16I64
DISABLE_SME2


#if defined(__ARM_FEATURE_BTI_DEFAULT) && (__ARM_FEATURE_BTI_DEFAULT == 1)
#   define GNU_PROPERTY_AARCH64_BTI (1 << 0)
#   define AARCH64_VALID_JUMP_CALL_TARGET hint #38
#   define AARCH64_VALID_CALL_TARGET      hint #34
#   define AARCH64_VALID_JUMP_TARGET      hint #38
#else
#   define GNU_PROPERTY_AARCH64_BTI 0
#   define AARCH64_VALID_JUMP_CALL_TARGET
#   define AARCH64_VALID_CALL_TARGET
#   define AARCH64_VALID_JUMP_TARGET
#endif

#if defined(__ARM_FEATURE_PAC_DEFAULT)
#   if ((__ARM_FEATURE_PAC_DEFAULT & (1 << 0)) != 0)
#       define AARCH64_SIGN_LINK_REGISTER      paciasp
#       define AARCH64_VALIDATE_LINK_REGISTER  autiasp
#   elif ((__ARM_FEATURE_PAC_DEFAULT & (1 << 1)) != 0)
#       define AARCH64_SIGN_LINK_REGISTER      pacibsp
#       define AARCH64_VALIDATE_LINK_REGISTER  autibsp
#   else
#       error Pointer authentication defines no valid key!
#   endif
#   if ((__ARM_FEATURE_PAC_DEFAULT & (1 << 2)) != 0)
#       error Authentication of leaf functions is enabled but not supported in FFmpeg!
#   endif
#   define GNU_PROPERTY_AARCH64_PAC (1 << 1)
#else
#   define GNU_PROPERTY_AARCH64_PAC 0
#   define AARCH64_SIGN_LINK_REGISTER
#   define AARCH64_VALIDATE_LINK_REGISTER
#endif

#if defined(__ARM_FEATURE_GCS_DEFAULT) && __ARM_FEATURE_GCS_DEFAULT == 1
#define GNU_PROPERTY_AARCH64_GCS (1 << 2)
#else
#define GNU_PROPERTY_AARCH64_GCS 0
#endif


#if (GNU_PROPERTY_AARCH64_BTI != 0 || GNU_PROPERTY_AARCH64_PAC != 0 || GNU_PROPERTY_AARCH64_GCS != 0) && defined(__ELF__)
        .pushsection .note.gnu.property, "a"
        .balign 8
        .long 4
        .long 0x10
        .long 0x5
        .asciz "GNU"
        .long 0xc0000000
        .long 4
        .long (GNU_PROPERTY_AARCH64_BTI | GNU_PROPERTY_AARCH64_PAC | GNU_PROPERTY_AARCH64_GCS)
        .long 0
        .popsection
#endif

.macro  function name, export=0, jumpable=0, align=4
    .macro endfunc
ELF     .size   \name, . - \name
FUNC    .endfunc
        .purgem endfunc
    .endm
        .text
        .align          \align
    .if \export
        .global EXTERN_ASM\name
ELF     .type   EXTERN_ASM\name, %function
FUNC    .func   EXTERN_ASM\name
EXTERN_ASM\name:
    .if \jumpable
        AARCH64_VALID_JUMP_TARGET
    .else
        AARCH64_VALID_CALL_TARGET
    .endif
    .else
ELF     .type   \name, %function
FUNC    .func   \name
\name:
    .endif
.endm

.macro  const   name, align=2
    .macro endconst
ELF     .size   \name, . - \name
        .purgem endconst
    .endm
#if defined(__MACH__)
        .const_data
#elif defined(_WIN32)
        .section        .rdata
#else
        .section        .rodata
#endif
        .align          \align
\name:
.endm

.macro  movrel rd, val
#if defined(__MACH__)
        adrp            \rd, \val@PAGE
        add             \rd, \rd, \val@PAGEOFF
#else
        adrp            \rd, \val
        add             \rd, \rd, :lo12:\val
#endif
.endm

#define GLUE(a, b) a ## b
#define JOIN(a, b) GLUE(a, b)
#define X(s) JOIN(EXTERN_ASM, s)

#define x18 do_not_use_x18
#define w18 do_not_use_w18

#if HAVE_SME
#endif
