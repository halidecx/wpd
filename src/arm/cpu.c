#include "src/cpu.h"
#include "src/arm/cpu.h"

#if WPD_HAVE_GETAUXVAL || WPD_HAVE_ELF_AUX_INFO
#include <sys/auxv.h>

#ifndef HWCAP_ARM_NEON
#define HWCAP_ARM_NEON (1 << 12)
#endif

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    unsigned flags = wpd_get_default_cpu_flags();
#if WPD_ARCH_ARM
    /* AArch64 always has NEON, so it is already covered by the defaults. */
    if (wpd_getauxval(AT_HWCAP) & HWCAP_ARM_NEON)
        flags |= WPD_ARM_CPU_FLAG_NEON;
#endif
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
    return flags;
}

#else /* Unsupported OS: rely on what the compiler was told to target */

wpd_cold unsigned wpd_get_cpu_flags_arm(void) {
    return wpd_get_default_cpu_flags();
}

#endif
