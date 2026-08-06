#ifndef WPD_ARM_ASM_CONFIG_H
#define WPD_ARM_ASM_CONFIG_H

#define CONFIG_PIC 1
#define CONFIG_THUMB 0
#define HAVE_ARMV5TE 1
#define HAVE_ARMV6 1
#define HAVE_ARMVFP 0
#define HAVE_VFP_ARGS 0
#if defined(WPD_ARM_NEON_ASM)
#define HAVE_NEON 1
#else
#define HAVE_NEON 0
#endif
#if defined(WPD_ARM_ARMV6T2_ASM)
#define HAVE_ARMV6T2 1
#else
#define HAVE_ARMV6T2 0
#endif
#if defined(WPD_ARM_ARMV6T2_EXTERNAL_ASM)
#define HAVE_ARMV6T2_EXTERNAL 1
#else
#define HAVE_ARMV6T2_EXTERNAL 0
#endif
#define EXTERN_ASM

#endif
