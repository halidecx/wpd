#ifndef WPD_COMPAT_H
#define WPD_COMPAT_H

#include <stddef.h>
#include <stdint.h>

#if defined(__GNUC__) || defined(__clang__)
#define wpd_always_inline inline __attribute__((always_inline))
#define wpd_cold __attribute__((cold))
#if defined(__clang__)
#define wpd_noclone __attribute__((noinline))
#else
#define wpd_noclone __attribute__((noinline, noclone))
#endif
#define wpd_unused __attribute__((unused))
#define WPD_DECLARE_ALIGNED(n, t, v) t __attribute__((aligned(n))) v
#else
#define wpd_always_inline inline
#define wpd_cold
#define wpd_noclone
#define wpd_unused
#define WPD_DECLARE_ALIGNED(n, t, v) t v
#endif

#endif
