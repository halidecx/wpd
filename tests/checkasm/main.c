#include "checkasm.h"

static const CheckasmCpuInfo cpu_flags[] = {
#if WPD_ARCH_X86
    {"SSE", "sse", WPD_X86_CPU_FLAG_SSE},
    {"SSE2", "sse2", WPD_X86_CPU_FLAG_SSE2},
    {"SSSE3", "ssse3", WPD_X86_CPU_FLAG_SSSE3},
    {"SSE4.1", "sse4", WPD_X86_CPU_FLAG_SSE41},
    {"AVX2", "avx2", WPD_X86_CPU_FLAG_AVX2},
#elif WPD_ARCH_ARM
    {"ARMv6", "armv6", WPD_ARM_CPU_FLAG_ARMV6},
    {"NEON", "neon", WPD_ARM_CPU_FLAG_NEON},
#elif WPD_ARCH_AARCH64
    {"NEON", "neon", WPD_ARM_CPU_FLAG_NEON},
    {"DOTPROD", "dotprod", WPD_ARM_CPU_FLAG_DOTPROD},
    {"I8MM", "i8mm", WPD_ARM_CPU_FLAG_I8MM},
#endif
    {0}};

static const CheckasmTest tests[] = {
    {.name = "filters", .func = checkasm_check_filters},
    {.name = "lossless", .func = checkasm_check_lossless},
    {.name = "rescaler", .func = checkasm_check_rescaler},
    {.name = "vp8dsp", .func = checkasm_check_vp8dsp},
    {.name = "vp8pred", .func = checkasm_check_vp8pred},
    {.name = "yuvdsp", .func = checkasm_check_yuvdsp},
    {0},
};

static void set_cpu_flags(CheckasmCpu flags) {
    wpd_set_cpu_flags_mask((unsigned)flags);
}

int main(int argc, const char *argv[]) {
    CheckasmConfig config;

    wpd_init_cpu();
    config = (CheckasmConfig){
        .cpu_flags     = cpu_flags,
        .tests         = tests,
        .cpu           = (CheckasmCpu)wpd_get_cpu_flags(),
        .set_cpu_flags = set_cpu_flags,
    };
    return checkasm_main(&config, argc, argv);
}
