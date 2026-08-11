#include "src/cpu.h"

#include <errno.h>

#if WPD_HAVE_GETAUXVAL || WPD_HAVE_ELF_AUX_INFO
#include <sys/auxv.h>
#endif

#ifndef __has_feature
#define __has_feature(x) 0
#endif

atomic_uint wpd_cpu_flags      = 0U;
atomic_uint wpd_cpu_flags_mask = ~0U;

wpd_cold void wpd_init_cpu(void) {
#if WPD_HAVE_ASM && !__has_feature(memory_sanitizer)
/* memory sanitizer is inherently incompatible with asm */
#if WPD_ARCH_AARCH64 || WPD_ARCH_ARM
    atomic_store_explicit(
        &wpd_cpu_flags, wpd_get_cpu_flags_arm(), memory_order_release);
#elif WPD_ARCH_X86
    atomic_store_explicit(
        &wpd_cpu_flags, wpd_get_cpu_flags_x86(), memory_order_release);
#endif
#endif
}

wpd_cold void wpd_set_cpu_flags_mask(const unsigned mask) {
    atomic_store_explicit(&wpd_cpu_flags_mask, mask, memory_order_relaxed);
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
