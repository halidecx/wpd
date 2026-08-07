#ifndef WPD_X86_CPU_H
#define WPD_X86_CPU_H

enum WpdX86CpuFlags {
    WPD_X86_CPU_FLAG_SSE   = 1 << 0,
    WPD_X86_CPU_FLAG_SSE2  = 1 << 1,
    WPD_X86_CPU_FLAG_SSSE3 = 1 << 2,
    WPD_X86_CPU_FLAG_SSE41 = 1 << 3,
    WPD_X86_CPU_FLAG_AVX2  = 1 << 4,
};

unsigned wpd_get_cpu_flags_x86(void);

#endif
