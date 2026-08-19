#ifndef WPD_COMPAT_H
#define WPD_COMPAT_H

#include <stddef.h>
#include <stdint.h>

#if defined(__GNUC__) || defined(__clang__)
#define wpd_always_inline inline __attribute__((always_inline))
#else
#define wpd_always_inline inline
#endif

#endif
