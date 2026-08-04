/*
 * Standalone checkasm runner for WPD.
 * SPDX-License-Identifier: LGPL-2.1-or-later
 */
#include "checkasm.h"

void checkasm_check_vp8dsp(void);
void checkasm_check_vp8pred(void);

static const CheckasmCpuInfo cpu_flags[] = {
#if WPD_ARCH_X86
    { "MMX",    "mmx",    WPD_CPU_MMX },
    { "MMXEXT", "mmxext", WPD_CPU_MMX2 },
    { "SSE",    "sse",    WPD_CPU_SSE },
    { "SSE2",   "sse2",   WPD_CPU_SSE2 },
    { "SSSE3",  "ssse3",  WPD_CPU_SSSE3 },
    { "SSE4.1", "sse4",   WPD_CPU_SSE4 },
    { "AVX2",   "avx2",   WPD_CPU_AVX2 },
#elif WPD_ARCH_ARM
    { "ARMv6",  "armv6",  WPD_CPU_ARMV6 },
    { "NEON",   "neon",   WPD_CPU_NEON },
#elif WPD_ARCH_AARCH64
    { "NEON",   "neon",   WPD_CPU_NEON },
#endif
    { 0 }
};

static const CheckasmTest tests[] = {
    { "vp8dsp",   checkasm_check_vp8dsp },
    { "vp8pred",  checkasm_check_vp8pred },
    { 0 }
};

static void set_cpu_flags(CheckasmCpu flags)
{
    wpd_set_cpu_flags_for_test((int) flags);
}

int main(int argc, const char *argv[])
{
    CheckasmConfig config = {
        .cpu_flags     = cpu_flags,
        .tests         = tests,
        .cpu           = (CheckasmCpu) wpd_get_cpu_flags(),
        .set_cpu_flags = set_cpu_flags,
    };
    return checkasm_main(&config, argc, argv);
}
