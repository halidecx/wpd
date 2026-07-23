#ifndef WPD_COMPAT_H
#define WPD_COMPAT_H

#include <stddef.h>
#include <stdint.h>

#if defined(__GNUC__) || defined(__clang__)
#define wpd_always_inline inline __attribute__((always_inline))
#define wpd_cold __attribute__((cold))
#define wpd_noinline __attribute__((noinline))
#define wpd_unused __attribute__((unused))
#define wpd_const __attribute__((const))
#define WPD_DECLARE_ALIGNED(n, t, v) t __attribute__((aligned(n))) v
#else
#define wpd_always_inline inline
#define wpd_cold
#define wpd_noinline
#define wpd_unused
#define wpd_const
#define WPD_DECLARE_ALIGNED(n, t, v) t v
#endif

#define WPD_LOCAL_ALIGNED(a, t, n, s) WPD_DECLARE_ALIGNED(a, t, n) s

#endif
