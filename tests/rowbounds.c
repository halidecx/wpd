
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L

#include "cpu.h"
#include "rescaler.h"
#include "yuvdsp.h"

#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

typedef void (*row_func)(uint8_t *dst, const uint8_t *src, int num_pixels);
typedef void (*inplace_func)(uint8_t *row, int num_pixels);
typedef void (*alpha_first_func)(uint8_t *row, int alpha_first, int num_pixels);
typedef void (*yuv444_func)(uint8_t *y, uint8_t *u, uint8_t *v,
                            const uint8_t *argb, int num_pixels);

enum { KIND_ROW, KIND_INPLACE, KIND_ALPHA_FIRST };

typedef struct {
    const char *name;
    int         kind;
    int         dst_bpp;
    int         src_bpp;
    int         alpha_first;
    const void *func;
} RowTest;

static sigjmp_buf   escape;
static volatile int trapped;

static void on_fault(int sig) {
    (void)sig;
    trapped = 1;
    siglongjmp(escape, 1);
}

static uint8_t *guarded(uint8_t **map, size_t *map_size, size_t bytes) {
    const size_t page = (size_t)sysconf(_SC_PAGESIZE);
    const size_t body = (bytes + page - 1) / page * page;

    *map_size = body + page;
    *map      = mmap(NULL,
                     *map_size,
                     PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS,
                     -1,
                     0);
    if (*map == MAP_FAILED)
        return NULL;
    if (mprotect(*map + body, page, PROT_NONE) < 0) {
        munmap(*map, *map_size);
        return NULL;
    }
    memset(*map, 0x5a, body);
    return *map + body - bytes;
}

static void run(const RowTest *t, uint8_t *dst, const uint8_t *src, int n) {
    switch (t->kind) {
    case KIND_ROW: ((row_func)t->func)(dst, src, n); break;
    case KIND_INPLACE: ((inplace_func)t->func)(dst, n); break;
    default: ((alpha_first_func)t->func)(dst, t->alpha_first, n); break;
    }
}

static int probe(const RowTest *t, int n) {
    uint8_t *dst_map = NULL, *src_map = NULL;
    size_t   dst_size = 0, src_size = 0;
    uint8_t *dst, *src;
    int      failed = 0;

    dst = guarded(&dst_map, &dst_size, (size_t)n * t->dst_bpp);
    src = t->src_bpp ? guarded(&src_map, &src_size, (size_t)n * t->src_bpp)
                     : NULL;
    if (!dst || (t->src_bpp && !src)) {
        fprintf(stderr, "mmap failed\n");
        return 1;
    }

    trapped = 0;
    signal(SIGSEGV, on_fault);
    signal(SIGBUS, on_fault);
    if (sigsetjmp(escape, 1) == 0)
        run(t, dst, src, n);
    signal(SIGSEGV, SIG_DFL);
    signal(SIGBUS, SIG_DFL);

    if (trapped) {
        printf("FAIL %s: n=%d ran past the end of a row\n", t->name, n);
        failed = 1;
    }
    munmap(dst_map, dst_size);
    if (src_map)
        munmap(src_map, src_size);
    return failed;
}

static int probe_yuv444(yuv444_func func, int n) {
    uint8_t *y_map = NULL, *u_map = NULL, *v_map = NULL, *src_map = NULL;
    size_t   y_size = 0, u_size = 0, v_size = 0, src_size = 0;
    uint8_t *y, *u, *v, *src;
    int      failed = 0;

    y   = guarded(&y_map, &y_size, (size_t)n);
    u   = guarded(&u_map, &u_size, (size_t)n);
    v   = guarded(&v_map, &v_size, (size_t)n);
    src = guarded(&src_map, &src_size, (size_t)n * 4);
    if (!y || !u || !v || !src) {
        fprintf(stderr, "mmap failed\n");
        failed = 1;
        goto done;
    }

    trapped = 0;
    signal(SIGSEGV, on_fault);
    signal(SIGBUS, on_fault);
    if (sigsetjmp(escape, 1) == 0)
        func(y, u, v, src, n);
    signal(SIGSEGV, SIG_DFL);
    signal(SIGBUS, SIG_DFL);
    if (trapped) {
        printf("FAIL argb_to_yuv444: n=%d ran past the end of a row\n", n);
        failed = 1;
    }
done:
    if (y_map)
        munmap(y_map, y_size);
    if (u_map)
        munmap(u_map, u_size);
    if (v_map)
        munmap(v_map, v_size);
    if (src_map)
        munmap(src_map, src_size);
    return failed;
}

/* The upper half of a 32-bit argument is undefined under the 64-bit ABIs.
 * Compilers leave it zeroed in practice, which hides a kernel that widens
 * one into an address. checkasm already clobbers those halves, but every
 * kernel here is reached through a Rust trampoline that rebuilds its
 * arguments, so the junk never survives to the asm; these call the asm
 * symbols directly instead.
 *
 * The overrun they expose is read-only and the bytes are discarded, so the
 * output is identical either way and no value comparison can see it. Ending
 * the source row flush against a guard page turns it into a fault. */
#if WPD_HAVE_ASM && WPD_ARCH_X86 && UINTPTR_MAX == 0xffffffffffffffffULL
void ff_rescale_import_expand_sse2(uint32_t *frow, const uint8_t *src, int n,
                                   int src_width, int channels, int x_add,
                                   int x_sub);

#define DIRTY(v) (0xdeadbeef00000000ULL | (uint32_t)(v))
typedef void (*dirty_import_func)(uint64_t, uint64_t, uint64_t, uint64_t,
                                  uint64_t, uint64_t, uint64_t);

/* The kernel counts in elements, not pixels, so a four-channel row hands it
 * four times the width it was given. */
static int probe_import_tail(int ch, int src_w) {
    const int dst_w    = 2 * src_w;
    uint8_t  *maps[2]  = {NULL, NULL};
    size_t    sizes[2] = {0, 0};
    uint8_t  *src      = guarded(&maps[0], &sizes[0], (size_t)src_w * ch);
    uint8_t  *frow     = guarded(
        &maps[1], &sizes[1], (size_t)dst_w * ch * sizeof(uint32_t));
    int failed = 0;

    if (!src || !frow) {
        fprintf(stderr, "mmap failed\n");
        failed = 1;
        goto done;
    }

    trapped = 0;
    signal(SIGSEGV, on_fault);
    signal(SIGBUS, on_fault);
    if (sigsetjmp(escape, 1) == 0)
        ((dirty_import_func)(void *)ff_rescale_import_expand_sse2)(
            (uint64_t)(uintptr_t)frow,
            (uint64_t)(uintptr_t)src,
            DIRTY(dst_w * ch),
            DIRTY(src_w * ch),
            DIRTY(ch),
            DIRTY(dst_w - 1),
            DIRTY(src_w - 1));
    signal(SIGSEGV, SIG_DFL);
    signal(SIGBUS, SIG_DFL);

    if (trapped) {
        printf(
            "FAIL rescale_import_expand_sse2: ch=%d src_width=%d read past "
            "the end of the source row\n",
            ch,
            src_w);
        failed = 1;
    }
done:
    for (size_t i = 0; i < 2; i++)
        if (maps[i])
            munmap(maps[i], sizes[i]);
    return failed;
}

static int probe_dirty_args(void) {
    static const int channels[] = {1, 4};
    int              failed     = 0;

    if (!(wpd_get_cpu_flags() & WPD_X86_CPU_FLAG_SSE2))
        return 0;
    for (size_t c = 0; c < sizeof(channels) / sizeof(*channels); c++)
        for (int src_w = 8; src_w <= 40; src_w++)
            failed |= probe_import_tail(channels[c], src_w);
    return failed;
}
#else
static int probe_dirty_args(void) { return 0; }
#endif

/* A zero-length row starts on the guard page itself, so a rescaler kernel
 * that stores before it tests its count faults on the first write. The
 * source row is wide enough to reach the vector paths. */
#define RESCALE_SRC 64

static int probe_rescale(const WPDRESCALEDSP *dsp) {
    uint8_t *maps[4]  = {NULL, NULL, NULL, NULL};
    size_t   sizes[4] = {0, 0, 0, 0};
    uint8_t *dst      = guarded(&maps[0], &sizes[0], 0);
    uint8_t *src      = guarded(&maps[1], &sizes[1], 4 * RESCALE_SRC);
    uint8_t *frow     = guarded(&maps[2], &sizes[2], 0);
    uint8_t *irow     = guarded(&maps[3], &sizes[3], 0);
    int      failed   = 0;

    if (!dst || !src || !frow || !irow) {
        fprintf(stderr, "mmap failed\n");
        failed = 1;
        goto done;
    }

    trapped = 0;
    signal(SIGSEGV, on_fault);
    signal(SIGBUS, on_fault);
    if (sigsetjmp(escape, 1) == 0) {
        dsp->import_row_expand(
            (uint32_t *)frow, src, 0, RESCALE_SRC, 4, 1, 1, 0);
        dsp->import_row_shrink(
            (uint32_t *)frow, src, 0, RESCALE_SRC, 4, 1, 1, 0);
        /* Each export slot picks between two kernels, so both a y_accum
         * that lands on a source row and one that does not. */
        dsp->export_row_expand(
            dst, (const uint32_t *)irow, (const uint32_t *)frow, 0, 0, 1, 0);
        dsp->export_row_expand(
            dst, (const uint32_t *)irow, (const uint32_t *)frow, 0, -3, 7, 0);
        dsp->export_row_shrink(
            dst, (uint32_t *)irow, (const uint32_t *)frow, 0, 0, 0, 0);
        dsp->export_row_shrink(
            dst, (uint32_t *)irow, (const uint32_t *)frow, 0, -3, 1 << 29, 0);
    }
    signal(SIGSEGV, SIG_DFL);
    signal(SIGBUS, SIG_DFL);
    if (trapped) {
        printf("FAIL rescale: an empty row was still written\n");
        failed = 1;
    }
done:
    for (size_t i = 0; i < 4; i++)
        if (maps[i])
            munmap(maps[i], sizes[i]);
    return failed;
}

#define ROW(fn, dst, src) {#fn, KIND_ROW, dst, src, 0, dsp->fn}
#define INPLACE(fn, dst) {#fn, KIND_INPLACE, dst, 0, 0, dsp->fn}
#define PREMUL(pos, first)   \
    {"premultiply_row_" pos, \
     KIND_ALPHA_FIRST,       \
     4,                      \
     0,                      \
     first,                  \
     dsp->premultiply_row}

static int check(const WPDYUVDSP *dsp) {
    const RowTest tests[] = {
        ROW(dispatch_alpha_first, 4, 1),
        ROW(dispatch_alpha_last, 4, 1),
        ROW(pack_rgba, 4, 4),
        ROW(pack_bgra, 4, 4),
        ROW(pack_rgb, 3, 4),
        ROW(pack_bgr, 3, 4),
        ROW(pack_rgb565, 2, 4),
        ROW(pack_rgba4444, 2, 4),
        ROW(pack_bgr565, 2, 4),
        ROW(pack_bgra4444, 2, 4),
        ROW(argb_to_y, 1, 4),
        INPLACE(premultiply_row_4444, 2),
        INPLACE(premultiply_row_4444_swap, 2),
        PREMUL("alpha_first", 1),
        PREMUL("alpha_last", 0),
    };
    int failed = 0;

    for (size_t i = 0; i < sizeof(tests) / sizeof(*tests); i++)
        for (int n = 1; n <= 96; n++) failed |= probe(&tests[i], n);
    for (int n = 1; n <= 96; n++)
        failed |= probe_yuv444(dsp->argb_to_yuv444, n);
    return failed;
}

int main(void) {
    static const unsigned levels[] = {
#if WPD_ARCH_X86
        0,
        WPD_X86_CPU_FLAG_SSE,
        WPD_X86_CPU_FLAG_SSE2,
        WPD_X86_CPU_FLAG_SSSE3,
        WPD_X86_CPU_FLAG_SSE41,
        WPD_X86_CPU_FLAG_AVX2,
#elif WPD_ARCH_ARM || WPD_ARCH_AARCH64
        0,
        WPD_ARM_CPU_FLAG_ARMV6,
        WPD_ARM_CPU_FLAG_NEON,
        WPD_ARM_CPU_FLAG_DOTPROD,
        WPD_ARM_CPU_FLAG_I8MM,
#else
        0,
#endif
    };
    unsigned have;
    int      failed = 0;

    wpd_init_cpu();
    have = wpd_get_cpu_flags();

    for (size_t i = 0; i < sizeof(levels) / sizeof(*levels); i++) {
        WPDYUVDSP     dsp;
        WPDRESCALEDSP rdsp;

        if (levels[i] & ~have)
            continue;
        wpd_set_cpu_flags_mask(levels[i] ? levels[i] | (levels[i] - 1) : 0);
        wpd_yuv_dsp_init(&dsp);
        failed |= check(&dsp);
        wpd_rescale_dsp_init(&rdsp);
        failed |= probe_rescale(&rdsp);
    }
    wpd_set_cpu_flags_mask(~0u);
    failed |= probe_dirty_args();
    printf("rowbounds: %s\n", failed ? "FAILED" : "all rows stayed in bounds");
    return failed;
}
