/*
 * Standalone checkasm runner for ffvp8.
 * SPDX-License-Identifier: LGPL-2.1-or-later
 */
#include "checkasm.h"

void checkasm_check_vp8dsp(void);
void checkasm_check_h264pred(void);
void checkasm_check_videodsp(void);

static const CheckasmCpuInfo cpu_flags[] = {
#if ARCH_X86
    { "MMX",    "mmx",    AV_CPU_FLAG_MMX },
    { "MMXEXT", "mmxext", AV_CPU_FLAG_MMX2 },
    { "SSE",    "sse",    AV_CPU_FLAG_SSE },
    { "SSE2",   "sse2",   AV_CPU_FLAG_SSE2 },
    { "SSSE3",  "ssse3",  AV_CPU_FLAG_SSSE3 },
    { "SSE4.1", "sse4",   AV_CPU_FLAG_SSE4 },
    { "AVX2",   "avx2",   AV_CPU_FLAG_AVX2 },
#elif ARCH_ARM
    { "ARMv6",  "armv6",  AV_CPU_FLAG_ARMV6 },
    { "NEON",   "neon",   AV_CPU_FLAG_NEON },
#elif ARCH_AARCH64
    { "NEON",   "neon",   AV_CPU_FLAG_NEON },
#endif
    { 0 }
};

static const CheckasmTest tests[] = {
    { "vp8dsp",   checkasm_check_vp8dsp,   NULL, NULL },
    { "h264pred", checkasm_check_h264pred, NULL, NULL },
    { "videodsp", checkasm_check_videodsp, NULL, NULL },
    { 0 }
};

static void set_cpu_flags(CheckasmCpu flags)
{
    ffvp8_set_cpu_flags_for_test((int) flags);
}

int main(int argc, const char *argv[])
{
    CheckasmConfig config = {
        .cpu_flags     = cpu_flags,
        .tests         = tests,
        .cpu           = (CheckasmCpu) av_get_cpu_flags(),
        .set_cpu_flags = set_cpu_flags,
    };
    return checkasm_main(&config, argc, argv);
}
