#include "src/cpu.h"

#include <errno.h>

#if WPD_HAVE_GETAUXVAL || WPD_HAVE_ELF_AUX_INFO
#include <sys/auxv.h>
#endif

#ifndef __has_feature
#define __has_feature(x) 0
#endif

unsigned wpd_cpu_flags      = 0U;
unsigned wpd_cpu_flags_mask = ~0U;

wpd_cold void wpd_init_cpu(void) {
#if WPD_HAVE_ASM && !__has_feature(memory_sanitizer)
/* memory sanitizer is inherently incompatible with asm */
#if WPD_ARCH_AARCH64 || WPD_ARCH_ARM
    wpd_cpu_flags = wpd_get_cpu_flags_arm();
#elif WPD_ARCH_X86
    wpd_cpu_flags = wpd_get_cpu_flags_x86();
#endif
#endif
}

wpd_cold void wpd_set_cpu_flags_mask(const unsigned mask) {
    wpd_cpu_flags_mask = mask;
}

wpd_cold unsigned long wpd_getauxval(unsigned long type) {
#if WPD_HAVE_GETAUXVAL
    return getauxval(type);
#elif WPD_HAVE_ELF_AUX_INFO
    unsigned long aux = 0;
    int           ret = elf_aux_info((int)type, &aux, sizeof(aux));
    if (ret != 0)
        errno = ret;
    return aux;
#else
    (void)type;
    errno = ENOSYS;
    return 0;
#endif
}
