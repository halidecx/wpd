#ifndef WPD_CPU_H
#define WPD_CPU_H

#include "wpd_compat.h"

#ifndef WPD_HAVE_ASM
#define WPD_HAVE_ASM 0
#endif

#if defined(__aarch64__)
#define WPD_ARCH_AARCH64 1
#else
#define WPD_ARCH_AARCH64 0
#endif
#if defined(__arm__)
#define WPD_ARCH_ARM 1
#else
#define WPD_ARCH_ARM 0
#endif
#if defined(__i386__) || defined(__x86_64__)
#define WPD_ARCH_X86 1
#else
#define WPD_ARCH_X86 0
#endif

#define WPD_CPU_MMX (1 << 0)
#define WPD_CPU_MMX2 (1 << 1)
#define WPD_CPU_SSE (1 << 2)
#define WPD_CPU_SSE2 (1 << 3)
#define WPD_CPU_SSE2SLOW (1 << 4)
#define WPD_CPU_SSSE3 (1 << 5)
#define WPD_CPU_SSE4 (1 << 6)
#define WPD_CPU_NEON (1 << 7)
#define WPD_CPU_AVX2 (1 << 8)
#define WPD_CPU_ARMV6 (1 << 9)

#define WPD_CPU_HAS_MMX(f) ((f) & WPD_CPU_MMX)
#define WPD_CPU_HAS_MMX2(f) ((f) & WPD_CPU_MMX2)
#define WPD_CPU_HAS_SSE(f) ((f) & WPD_CPU_SSE)
#define WPD_CPU_HAS_SSE2(f) ((f) & WPD_CPU_SSE2)
#define WPD_CPU_HAS_SSE2_SLOW(f) ((f) & (WPD_CPU_SSE2 | WPD_CPU_SSE2SLOW))
#define WPD_CPU_HAS_SSSE3(f) ((f) & WPD_CPU_SSSE3)
#define WPD_CPU_HAS_SSE4(f) ((f) & WPD_CPU_SSE4)
#define WPD_CPU_HAS_AVX2(f) ((f) & WPD_CPU_AVX2)

#if WPD_ARCH_ARM || WPD_ARCH_AARCH64
static wpd_always_inline int wpd_have_armv6(int flags) {
    return !!(flags & WPD_CPU_ARMV6);
}
static wpd_always_inline int wpd_have_neon(int flags) {
    return !!(flags & WPD_CPU_NEON);
}
#endif

int  wpd_get_cpu_flags(void);
void wpd_set_cpu_flags_for_test(int flags);

#endif
