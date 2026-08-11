#if WPD_HAVE_PTHREAD_GETAFFINITY_NP && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif

#include "src/cpu.h"

#include "wpd_thread.h"

#include <errno.h>

#if WPD_HAVE_GETAUXVAL || WPD_HAVE_ELF_AUX_INFO
#include <sys/auxv.h>
#endif

#if WPD_HAVE_THREADS
#if defined(_WIN32)
#include <windows.h>
#elif defined(__APPLE__)
#include <sys/sysctl.h>
#include <sys/types.h>
#else
#include <pthread.h>
#include <unistd.h>
#endif
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

wpd_cold int wpd_num_logical_processors(void) {
#if WPD_HAVE_THREADS
#if defined(_WIN32)
    DWORD count = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS);

    if (count)
        return (int)count;
#elif WPD_HAVE_PTHREAD_GETAFFINITY_NP && defined(CPU_COUNT)
    cpu_set_t affinity;

    /* Respects taskset and a container's cpuset, which the machine-wide counts
       below do not. */
    if (!pthread_getaffinity_np(pthread_self(), sizeof(affinity), &affinity)) {
        const int count = CPU_COUNT(&affinity);

        if (count > 0)
            return count;
    }
#elif defined(__APPLE__)
    int    count  = 0;
    size_t length = sizeof(count);

    if (!sysctlbyname("hw.logicalcpu", &count, &length, NULL, 0) && count > 0)
        return count;
#elif defined(_SC_NPROCESSORS_ONLN)
    const long count = sysconf(_SC_NPROCESSORS_ONLN);

    if (count > 0)
        return (int)count;
#endif
#endif
    return 1;
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
