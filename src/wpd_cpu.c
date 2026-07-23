#include "wpd_cpu.h"

#if defined(__arm__) && defined(__linux__)
#include <sys/auxv.h>
#ifndef HWCAP_NEON
#define HWCAP_NEON (1UL << 12)
#endif
#endif

static int cpu_flags_for_test = -1;

void wpd_set_cpu_flags_for_test(int flags)
{
    cpu_flags_for_test = flags;
}

int wpd_get_cpu_flags(void)
{
    int flags = 0;
#if defined(__i386__) || defined(__x86_64__)
    __builtin_cpu_init();
    if (__builtin_cpu_supports("mmx")) flags |= WPD_CPU_MMX;
    if (__builtin_cpu_supports("sse")) flags |= WPD_CPU_MMX2 | WPD_CPU_SSE;
    if (__builtin_cpu_supports("sse2")) flags |= WPD_CPU_SSE2;
    if (__builtin_cpu_supports("ssse3")) flags |= WPD_CPU_SSSE3;
    if (__builtin_cpu_supports("sse4.1")) flags |= WPD_CPU_SSE4;
    if (__builtin_cpu_supports("avx2")) flags |= WPD_CPU_AVX2;
#endif
#if defined(__arm__)
    flags |= WPD_CPU_ARMV6;
#if defined(__linux__)
    if (getauxval(AT_HWCAP) & HWCAP_NEON) flags |= WPD_CPU_NEON;
#elif defined(__ARM_NEON)
    flags |= WPD_CPU_NEON;
#endif
#endif
#if defined(__aarch64__)
    flags |= WPD_CPU_NEON;
#endif
    return cpu_flags_for_test < 0 ? flags : flags & cpu_flags_for_test;
}
