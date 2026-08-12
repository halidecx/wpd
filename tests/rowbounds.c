/* Every row function is handed a row and a pixel count, and must write only
   the bytes that row owns. Assembly that reads a group of pixels back, edits
   one byte of each and stores the group again writes bytes it did not change:
   comparing its output against the C reference cannot see that, because the
   bytes it puts back are the ones it found, and no sanitizer instruments hand
   written assembly either. What does see it is the store itself failing, so
   each destination row here ends against a page that is not mapped.

   Only the destination is bounded this way. Several of these functions read a
   whole group from the source however few pixels are left, which is safe on
   the padded buffers the decoder hands them. */

#define _POSIX_C_SOURCE 200809L

#include "cpu.h"
#include "yuvdsp.h"

#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

typedef void (*row_func)(uint8_t *dst, const uint8_t *src, int num_pixels);
typedef void (*inplace_func)(uint8_t *row, int num_pixels);
typedef void (*alpha_first_func)(uint8_t *row, int alpha_first, int num_pixels);

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

/* A buffer of 'bytes' whose last byte is the last one of a mapping. */
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
    src = t->src_bpp
        ? guarded(&src_map, &src_size, (size_t)(n + 64) * t->src_bpp)
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
        printf("FAIL %s: n=%d wrote past the end of the row\n", t->name, n);
        failed = 1;
    }
    munmap(dst_map, dst_size);
    if (src_map)
        munmap(src_map, src_size);
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
#else
        0,
#endif
    };
    unsigned have;
    int      failed = 0;

    wpd_init_cpu();
    have = wpd_get_cpu_flags();

    /* The flags are ordered by feature set, so everything below a level is
       what a machine with that level has. */
    for (size_t i = 0; i < sizeof(levels) / sizeof(*levels); i++) {
        WPDYUVDSP dsp;

        if (levels[i] & ~have)
            continue;
        wpd_set_cpu_flags_mask(levels[i] ? levels[i] | (levels[i] - 1) : 0);
        wpd_yuv_dsp_init(&dsp);
        failed |= check(&dsp);
    }
    wpd_set_cpu_flags_mask(~0u);
    printf("rowbounds: %s\n", failed ? "FAILED" : "all rows stayed in bounds");
    return failed;
}
