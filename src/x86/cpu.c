#include <stdint.h>

#include "src/cpu.h"
#include "src/x86/cpu.h"

#if defined(_MSC_VER)
#include <immintrin.h>
#include <intrin.h>
#else
#include <cpuid.h>
#endif

typedef struct CpuidRegisters {
    uint32_t eax, ebx, ecx, edx;
} CpuidRegisters;

static wpd_cold int cpuid(CpuidRegisters *const r, const unsigned leaf,
                          const unsigned subleaf) {
#if defined(_MSC_VER)
    int regs[4];
    __cpuidex(regs, (int)leaf, (int)subleaf);
    r->eax = (uint32_t)regs[0];
    r->ebx = (uint32_t)regs[1];
    r->ecx = (uint32_t)regs[2];
    r->edx = (uint32_t)regs[3];
    return 1;
#else
    return __get_cpuid_count(leaf, subleaf, &r->eax, &r->ebx, &r->ecx, &r->edx);
#endif
}

static wpd_cold uint64_t xgetbv0(void) {
#if defined(_MSC_VER)
    return _xgetbv(0);
#else
    uint32_t eax, edx;
    __asm__("xgetbv" : "=a"(eax), "=d"(edx) : "c"(0));
    return ((uint64_t)edx << 32) | eax;
#endif
}

#define X(reg, mask) (((reg) & (mask)) == (mask))

wpd_cold unsigned wpd_get_cpu_flags_x86(void) {
    unsigned       flags = wpd_get_default_cpu_flags();
    CpuidRegisters r;

    if (!cpuid(&r, 1, 0))
        return flags;

    if (X(r.edx, 0x00800000)) /* MMX */ {
        flags |= WPD_X86_CPU_FLAG_MMX;
        if (X(r.edx, 0x02000000)) /* SSE, implies the MMX extensions */ {
            flags |= WPD_X86_CPU_FLAG_MMXEXT | WPD_X86_CPU_FLAG_SSE;
            if (X(r.edx, 0x04008000)) /* CMOV/SSE2 */ {
                flags |= WPD_X86_CPU_FLAG_SSE2;
                if (X(r.ecx, 0x00000201)) /* SSE3/SSSE3 */ {
                    flags |= WPD_X86_CPU_FLAG_SSSE3;
                    if (X(r.ecx, 0x00080000)) /* SSE4.1 */
                        flags |= WPD_X86_CPU_FLAG_SSE41;
                }
            }
        }
    }

    if (X(r.ecx, 0x18000000)) /* OSXSAVE/AVX */ {
        const uint64_t xcr0 = xgetbv0();
        if (X(xcr0, 0x00000006)) /* XMM/YMM state saved by the OS */ {
            CpuidRegisters r7;
            if (cpuid(&r7, 7, 0) && X(r7.ebx, 0x00000128)) /* BMI1/BMI2/AVX2 */
                flags |= WPD_X86_CPU_FLAG_AVX2;
        }
    }

    return flags;
}
