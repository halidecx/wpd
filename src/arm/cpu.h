#ifndef WPD_ARM_CPU_H
#define WPD_ARM_CPU_H

enum WpdArmCpuFlags {
    WPD_ARM_CPU_FLAG_ARMV6   = 1 << 0,
    WPD_ARM_CPU_FLAG_NEON    = 1 << 1,
    WPD_ARM_CPU_FLAG_DOTPROD = 1 << 2,
    WPD_ARM_CPU_FLAG_I8MM    = 1 << 3,
};

#endif
