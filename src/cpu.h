#ifndef WPD_CPU_H
#define WPD_CPU_H

#include "wpd_compat.h"

#ifndef WPD_HAVE_ASM
#define WPD_HAVE_ASM 0
#endif

#if defined(__aarch64__) || defined(_M_ARM64)
#define WPD_ARCH_AARCH64 1
#else
#define WPD_ARCH_AARCH64 0
#endif
#if defined(__arm__) || defined(_M_ARM)
#define WPD_ARCH_ARM 1
#else
#define WPD_ARCH_ARM 0
#endif
#if defined(__x86_64__) || defined(_M_X64)
#define WPD_ARCH_X86_64 1
#else
#define WPD_ARCH_X86_64 0
#endif
#if defined(__i386__) || defined(_M_IX86)
#define WPD_ARCH_X86_32 1
#else
#define WPD_ARCH_X86_32 0
#endif
#define WPD_ARCH_X86 (WPD_ARCH_X86_64 || WPD_ARCH_X86_32)

#if WPD_ARCH_AARCH64 || WPD_ARCH_ARM
#include "src/arm/cpu.h"
#elif WPD_ARCH_X86
#include "src/x86/cpu.h"
#endif

extern unsigned wpd_cpu_flags;
extern unsigned wpd_cpu_flags_mask;

void          wpd_init_cpu(void);
void          wpd_set_cpu_flags_mask(unsigned mask);
unsigned long wpd_getauxval(unsigned long type);

/* Feature set the compiler was told to target. Detection starts from these,
 * so a build with e.g. -march=native can constant-fold the dispatch away. */
static wpd_always_inline unsigned wpd_get_default_cpu_flags(void) {
    unsigned flags = 0;

#if WPD_ARCH_AARCH64 || WPD_ARCH_ARM
#if defined(__ARM_NEON) || defined(__APPLE__) || defined(_WIN32) || \
    WPD_ARCH_AARCH64
    flags |= WPD_ARM_CPU_FLAG_NEON;
#endif
    /* The ARMv6 asm is only assembled when the target baseline supports it,
     * so this is a compile-time property rather than a runtime one. */
#ifdef WPD_ARM_ARMV6_ASM
    flags |= WPD_ARM_CPU_FLAG_ARMV6;
#endif
#elif WPD_ARCH_X86
#if defined(__AVX2__)
    flags |= WPD_X86_CPU_FLAG_AVX2 | WPD_X86_CPU_FLAG_SSE41 |
        WPD_X86_CPU_FLAG_SSSE3 | WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE |
        WPD_X86_CPU_FLAG_MMXEXT | WPD_X86_CPU_FLAG_MMX;
#elif defined(__SSE4_1__) || defined(__AVX__)
    flags |= WPD_X86_CPU_FLAG_SSE41 | WPD_X86_CPU_FLAG_SSSE3 |
        WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE | WPD_X86_CPU_FLAG_MMXEXT |
        WPD_X86_CPU_FLAG_MMX;
#elif defined(__SSSE3__)
    flags |= WPD_X86_CPU_FLAG_SSSE3 | WPD_X86_CPU_FLAG_SSE2 |
        WPD_X86_CPU_FLAG_SSE | WPD_X86_CPU_FLAG_MMXEXT | WPD_X86_CPU_FLAG_MMX;
#elif WPD_ARCH_X86_64 || defined(__SSE2__) || \
    (defined(_M_IX86_FP) && _M_IX86_FP >= 2)
    flags |= WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE |
        WPD_X86_CPU_FLAG_MMXEXT | WPD_X86_CPU_FLAG_MMX;
#elif defined(__SSE__) || (defined(_M_IX86_FP) && _M_IX86_FP >= 1)
    flags |= WPD_X86_CPU_FLAG_SSE | WPD_X86_CPU_FLAG_MMXEXT |
        WPD_X86_CPU_FLAG_MMX;
#elif defined(__MMX__)
    flags |= WPD_X86_CPU_FLAG_MMX;
#endif
#endif

    return flags;
}

static wpd_always_inline unsigned wpd_get_cpu_flags(void) {
    unsigned flags = wpd_cpu_flags & wpd_cpu_flags_mask;

#if WPD_TRIM_DSP_FUNCTIONS
    /* Since this function is inlined into the DSP init functions, which are in
     * turn inlined into their caller, unconditionally setting the compile-time
     * flags here lets the compiler drop every fallback the build target can
     * never reach. A binary cannot run on a CPU below its own target anyway. */
    flags |= wpd_get_default_cpu_flags();
#endif

    return flags;
}

#endif
