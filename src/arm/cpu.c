#include "src/cpu.h"
#include "src/arm/cpu.h"

#if WPD_HAVE_GETAUXVAL || WPD_HAVE_ELF_AUX_INFO
#include <sys/auxv.h>

#ifndef HWCAP_ARM_NEON
#define HWCAP_ARM_NEON (1 << 12)
#endif
#define HWCAP_AARCH64_ASIMDDP (1 << 20)
#define HWCAP2_AARCH64_I8MM (1 << 13)
#define HWCAP_ARM_ASIMDDP (1 << 24)
#define HWCAP_ARM_I8MM (1 << 27)

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    const unsigned long hw_cap = wpd_getauxval(AT_HWCAP);
    unsigned            flags  = wpd_get_default_cpu_flags();

#if WPD_ARCH_ARM
    /* AArch64 always has NEON, so it is already covered by the defaults. */
    if (hw_cap & HWCAP_ARM_NEON)
        flags |= WPD_ARM_CPU_FLAG_NEON;
    if (hw_cap & HWCAP_ARM_ASIMDDP)
        flags |= WPD_ARM_CPU_FLAG_DOTPROD;
    if (hw_cap & HWCAP_ARM_I8MM)
        flags |= WPD_ARM_CPU_FLAG_I8MM;
#else
    if (hw_cap & HWCAP_AARCH64_ASIMDDP)
        flags |= WPD_ARM_CPU_FLAG_DOTPROD;
    if (wpd_getauxval(AT_HWCAP2) & HWCAP2_AARCH64_I8MM)
        flags |= WPD_ARM_CPU_FLAG_I8MM;
#endif
    return flags;
}

#elif defined(__APPLE__)
#include <sys/sysctl.h>

static wpd_cold int have_feature(const char *feature) {
    int    supported = 0;
    size_t size      = sizeof(supported);

    if (sysctlbyname(feature, &supported, &size, NULL, 0) != 0)
        return 0;
    return supported;
}

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    unsigned flags = wpd_get_default_cpu_flags();

    if (have_feature("hw.optional.arm.FEAT_DotProd"))
        flags |= WPD_ARM_CPU_FLAG_DOTPROD;
    if (have_feature("hw.optional.arm.FEAT_I8MM"))
        flags |= WPD_ARM_CPU_FLAG_I8MM;
    return flags;
}

#elif defined(__ANDROID__) || defined(__linux__)
#include <ctype.h>
#include <stdio.h>
#include <string.h>

static wpd_cold int parse_proc_cpuinfo(const char *const flag) {
    FILE *file = fopen("/proc/cpuinfo", "r");
    if (!file)
        return 0;

    char         line_buffer[120];
    const char  *line;
    const size_t flaglen = strlen(flag);

    while ((line = fgets(line_buffer, sizeof(line_buffer), file))) {
        /* check all occurrences as whole words */
        const char *found = line;
        while ((found = strstr(found, flag))) {
            if ((found == line_buffer || !isgraph((unsigned char)found[-1])) &&
                (isspace((unsigned char)found[flaglen]) || feof(file))) {
                fclose(file);
                return 1;
            }
            found += flaglen;
        }
        /* if the line is incomplete, seek back so the search string cannot be
         * split across two buffers */
        if (!strchr(line, '\n') && strlen(line) > flaglen) {
            if (fseek(file, -(long)flaglen, SEEK_CUR))
                break;
        }
    }

    fclose(file);
    return 0;
}

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    unsigned flags = wpd_get_default_cpu_flags();
    if (parse_proc_cpuinfo("neon") || parse_proc_cpuinfo("asimd"))
        flags |= WPD_ARM_CPU_FLAG_NEON;
    if (parse_proc_cpuinfo("asimddp"))
        flags |= WPD_ARM_CPU_FLAG_DOTPROD;
    if (parse_proc_cpuinfo("i8mm"))
        flags |= WPD_ARM_CPU_FLAG_I8MM;
    return flags;
}

#elif defined(_WIN32)
#include <windows.h>

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    unsigned flags = wpd_get_default_cpu_flags();
#ifdef PF_ARM_NEON_INSTRUCTIONS_AVAILABLE
    if (IsProcessorFeaturePresent(PF_ARM_NEON_INSTRUCTIONS_AVAILABLE))
        flags |= WPD_ARM_CPU_FLAG_NEON;
#endif
#ifdef PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE
    if (IsProcessorFeaturePresent(PF_ARM_V82_DP_INSTRUCTIONS_AVAILABLE))
        flags |= WPD_ARM_CPU_FLAG_DOTPROD;
#endif
    return flags;
}

#else /* Unsupported OS: rely on what the compiler was told to target */

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    return wpd_get_default_cpu_flags();
}

#endif
