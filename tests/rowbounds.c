
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L

#include "cpu.h"
#include "rescaler.h"
#include "yuvdsp.h"

#include <errno.h>
#include <setjmp.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
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

/* A guard page only reports an overrun if the fault reaches a handler. Some
 * hosts never deliver a synchronous access fault: the faulting instruction
 * retries forever, so an overrun would hang the test instead of failing it
 * and a clean run would prove nothing. Fault a page in a child under an
 * alarm and find out before any probe runs. */
#define FAULT_DELIVERED 42

static void on_child_fault(int sig) {
    (void)sig;
    _exit(FAULT_DELIVERED);
}

static int fault_delivery_works(void) {
    const pid_t pid = fork();
    int         status;

    if (pid < 0) {
        fprintf(stderr, "fork failed\n");
        return 0;
    }
    if (pid == 0) {
        const size_t page = (size_t)sysconf(_SC_PAGESIZE);
        uint8_t     *p    = mmap(NULL,
                                 page,
                                 PROT_READ | PROT_WRITE,
                                 MAP_PRIVATE | MAP_ANONYMOUS,
                                 -1,
                                 0);

        if (p == MAP_FAILED || mprotect(p, page, PROT_NONE) < 0)
            _exit(1);
        signal(SIGSEGV, on_child_fault);
        signal(SIGBUS, on_child_fault);
        /* SIGALRM's default action ends the child, so a store that spins
         * instead of faulting still lets the parent make progress. */
        alarm(5);
        *(volatile uint8_t *)p = 1;
        _exit(1); /* the store never faulted at all */
    }
    while (waitpid(pid, &status, 0) < 0)
        if (errno != EINTR)
            return 0;
    return WIFEXITED(status) && WEXITSTATUS(status) == FAULT_DELIVERED;
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
 * one into an address. checkasm clobbers those halves from a mask it builds
 * out of the declared parameter types, which covers every family whose C
 * table hands out the asm symbols themselves. The rescaler is the exception:
 * its kernels take their counts in elements and only apply below certain
 * ratios, so the dispatch stays in Rust and the table hands out wrappers
 * that rebuild the arguments. The junk never survives that trip, so these
 * call the asm directly.
 *
 * The overrun this exposes is read-only and its bytes are discarded, so both
 * implementations agree and no value comparison can see it. Ending each row
 * flush against a guard page turns it into a fault. */
#if WPD_HAVE_ASM && (WPD_ARCH_X86 || WPD_ARCH_AARCH64) && \
    UINTPTR_MAX == 0xffffffffffffffffULL

#define DIRTY(v) (0xdeadbeef00000000ULL | (uint32_t)(v))

typedef void (*dirty4)(uint64_t, uint64_t, uint64_t, uint64_t);
typedef void (*dirty6)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t,
                       uint64_t);
typedef void (*dirty7)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t,
                       uint64_t, uint64_t);

#define DECL_IMPORT_EXPAND(sym) \
    void sym(                   \
        uint32_t *frow, const uint8_t *src, int n, int a, int b, int c, int d)
#define DECL_IMPORT_SHRINK(sym)    \
    void sym(uint32_t      *frow,  \
             const uint8_t *src,   \
             int            n,     \
             int            x_add, \
             int            x_sub, \
             uint32_t       fx)
#define DECL_EXPORT4(sym) \
    void sym(uint8_t *dst, const uint32_t *a, int n, uint32_t b)
#define DECL_EXPORT6(sym)          \
    void sym(uint8_t        *dst,  \
             uint32_t       *irow, \
             const uint32_t *frow, \
             int             n,    \
             uint32_t        a,    \
             uint32_t        b)
#define DECL_EXPORT7(sym)          \
    void sym(uint8_t        *dst,  \
             const uint32_t *irow, \
             const uint32_t *frow, \
             int             n,    \
             uint32_t        a,    \
             uint32_t        b,    \
             uint32_t        c)

#if WPD_ARCH_X86
DECL_IMPORT_EXPAND(ff_rescale_import_expand_sse2);
DECL_IMPORT_SHRINK(ff_rescale_import_shrink_sse2);
DECL_EXPORT4(ff_rescale_export_direct_sse2);
DECL_EXPORT4(ff_rescale_export_direct_avx2);
DECL_EXPORT4(ff_rescale_export_shrink0_sse2);
DECL_EXPORT4(ff_rescale_export_shrink0_avx2);
DECL_EXPORT6(ff_rescale_export_shrink_sse2);
DECL_EXPORT6(ff_rescale_export_shrink_avx2);
DECL_EXPORT7(ff_rescale_export_blend_sse2);
DECL_EXPORT7(ff_rescale_export_blend_avx2);
#else
DECL_IMPORT_EXPAND(ff_rescale_import_expand_neon);
DECL_IMPORT_SHRINK(ff_rescale_import_shrink_neon);
DECL_EXPORT4(ff_rescale_export_direct_neon);
DECL_EXPORT4(ff_rescale_export_shrink0_neon);
DECL_EXPORT6(ff_rescale_export_shrink_neon);
DECL_EXPORT7(ff_rescale_export_blend_neon);
#endif

static int faulted(const char *name, int n) {
    if (!trapped)
        return 0;
    printf("FAIL %s: n=%d stepped outside its rows\n", name, n);
    return 1;
}

#define GUARD_BEGIN            \
    trapped = 0;               \
    signal(SIGSEGV, on_fault); \
    signal(SIGBUS, on_fault);  \
    if (sigsetjmp(escape, 1) == 0)
#define GUARD_END             \
    signal(SIGSEGV, SIG_DFL); \
    signal(SIGBUS, SIG_DFL)

/* Imports count in elements, so a four-channel row hands the kernel four
 * times the width it was given. Expansion walks the source to its last
 * pixel; shrinking walks it once per accumulated step. */
static int probe_import(const char *name, const void *func, int expand, int ch,
                        int src_w) {
    const int dst_w    = expand ? 2 * src_w : src_w / 2;
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

    GUARD_BEGIN {
        if (expand)
            ((dirty7)func)((uint64_t)(uintptr_t)frow,
                           (uint64_t)(uintptr_t)src,
                           DIRTY(dst_w * ch),
                           DIRTY(src_w * ch),
                           DIRTY(ch),
                           DIRTY(dst_w - 1),
                           DIRTY(src_w - 1));
        else
            ((dirty6)func)((uint64_t)(uintptr_t)frow,
                           (uint64_t)(uintptr_t)src,
                           DIRTY(dst_w * ch),
                           DIRTY(src_w),
                           DIRTY(dst_w),
                           DIRTY(0x10000 / dst_w));
    }
    GUARD_END;
    failed = faulted(name, src_w);
done:
    for (size_t i = 0; i < 2; i++)
        if (maps[i])
            munmap(maps[i], sizes[i]);
    return failed;
}

/* Exports write n bytes out of rows of n accumulators each. The weights do
 * not steer any address, so only the count has to be right. */
static int probe_export(const char *name, const void *func, int arity, int n) {
    uint8_t *maps[3]  = {NULL, NULL, NULL};
    size_t   sizes[3] = {0, 0, 0};
    uint8_t *dst      = guarded(&maps[0], &sizes[0], (size_t)n);
    uint8_t *irow = guarded(&maps[1], &sizes[1], (size_t)n * sizeof(uint32_t));
    uint8_t *frow = guarded(&maps[2], &sizes[2], (size_t)n * sizeof(uint32_t));
    int      failed = 0;

    if (!dst || !irow || !frow) {
        fprintf(stderr, "mmap failed\n");
        failed = 1;
        goto done;
    }

    GUARD_BEGIN {
        switch (arity) {
        case 4:
            /* export_direct reads frow; export_shrink0 rewrites the row it
             * is handed. Both take one row, so one guarded row does. */
            ((dirty4)func)((uint64_t)(uintptr_t)dst,
                           (uint64_t)(uintptr_t)frow,
                           DIRTY(n),
                           DIRTY(1 << 29));
            break;
        case 6:
            ((dirty6)func)((uint64_t)(uintptr_t)dst,
                           (uint64_t)(uintptr_t)irow,
                           (uint64_t)(uintptr_t)frow,
                           DIRTY(n),
                           DIRTY(1 << 15),
                           DIRTY(1 << 29));
            break;
        default:
            ((dirty7)func)((uint64_t)(uintptr_t)dst,
                           (uint64_t)(uintptr_t)irow,
                           (uint64_t)(uintptr_t)frow,
                           DIRTY(n),
                           DIRTY(1 << 29),
                           DIRTY(3),
                           DIRTY(5));
            break;
        }
    }
    GUARD_END;
    failed = faulted(name, n);
done:
    for (size_t i = 0; i < 3; i++)
        if (maps[i])
            munmap(maps[i], sizes[i]);
    return failed;
}

/* One instruction set's worth of kernels. A level that only rewrites some
 * of the six leaves the rest null and they are probed under the level that
 * did write them. `luma` marks an expand kernel that also has a
 * single-channel path; the rest are four-channel only, and handing one a
 * row of luma would have it read four times the source it was given. */
typedef struct {
    const char *suffix;
    int         luma;
    const void *import_expand;
    const void *import_shrink;
    const void *export_direct;
    const void *export_shrink0;
    const void *export_shrink;
    const void *export_blend;
} RawLevel;

static int probe_level(const RawLevel *lvl) {
    char name[64];
    int  failed = 0;

    for (int src_w = 8; src_w <= 40; src_w++) {
        if (lvl->import_expand) {
            for (int ch = 1; ch <= 4; ch += 3) {
                if (ch == 1 && !lvl->luma)
                    continue;
                snprintf(name,
                         sizeof name,
                         "rescale_import_expand_%s ch=%d",
                         lvl->suffix,
                         ch);
                failed |= probe_import(name, lvl->import_expand, 1, ch, src_w);
            }
        }
        if (lvl->import_shrink) {
            snprintf(
                name, sizeof name, "rescale_import_shrink_%s", lvl->suffix);
            failed |= probe_import(name, lvl->import_shrink, 0, 4, src_w);
        }
    }

    for (int n = 1; n <= 64; n++) {
        static const struct {
            size_t      slot;
            const char *stem;
            int         arity;
        } exports[] = {
            {offsetof(RawLevel, export_direct), "direct", 4},
            {offsetof(RawLevel, export_shrink0), "shrink0", 4},
            {offsetof(RawLevel, export_shrink), "shrink", 6},
            {offsetof(RawLevel, export_blend), "blend", 7},
        };

        for (size_t i = 0; i < sizeof(exports) / sizeof(*exports); i++) {
            const void *func = *(const void *const *)((const char *)lvl +
                                                      exports[i].slot);

            if (!func)
                continue;
            snprintf(name,
                     sizeof name,
                     "rescale_export_%s_%s",
                     exports[i].stem,
                     lvl->suffix);
            failed |= probe_export(name, func, exports[i].arity, n);
        }
    }
    return failed;
}

/* WPDRESCALERAWDSP is written out twice, once in Rust and once in the
 * header, and two of its six entries take the same argument list -- swapping
 * those two would compile, run, and quietly test the wrong kernel twice.
 * Pin every entry to the symbol it is supposed to be. */
#define PIN(field, want)                                             \
    do {                                                             \
        if ((void *)raw.field != (void *)(want)) {                   \
            printf("FAIL raw table: %s is not %s\n", #field, #want); \
            failed = 1;                                              \
        }                                                            \
    } while (0)

#if WPD_ARCH_X86
static int check_raw_bindings(void) {
    const unsigned   have = wpd_get_cpu_flags();
    const int        avx2 = (have & WPD_X86_CPU_FLAG_AVX2) != 0;
    WPDRESCALERAWDSP raw;
    int              failed = 0;

    wpd_rescale_raw_dsp_init(&raw);
    PIN(import_expand, ff_rescale_import_expand_sse2);
    PIN(import_shrink, ff_rescale_import_shrink_sse2);
    PIN(export_direct,
        avx2 ? ff_rescale_export_direct_avx2 : ff_rescale_export_direct_sse2);
    PIN(export_blend,
        avx2 ? ff_rescale_export_blend_avx2 : ff_rescale_export_blend_sse2);
    PIN(export_shrink,
        avx2 ? ff_rescale_export_shrink_avx2 : ff_rescale_export_shrink_sse2);
    PIN(export_shrink0,
        avx2 ? ff_rescale_export_shrink0_avx2 : ff_rescale_export_shrink0_sse2);
    return failed;
}

/* AVX2 rewrites only the exports, so the imports stay SSE2's and are probed
 * there. */
static int probe_dirty_args(int guards) {
    const unsigned have = wpd_get_cpu_flags();
    const RawLevel sse2 = {
        "sse2",
        1,
        ff_rescale_import_expand_sse2,
        ff_rescale_import_shrink_sse2,
        ff_rescale_export_direct_sse2,
        ff_rescale_export_shrink0_sse2,
        ff_rescale_export_shrink_sse2,
        ff_rescale_export_blend_sse2,
    };
    const RawLevel avx2 = {
        "avx2",
        0,
        NULL,
        NULL,
        ff_rescale_export_direct_avx2,
        ff_rescale_export_shrink0_avx2,
        ff_rescale_export_shrink_avx2,
        ff_rescale_export_blend_avx2,
    };
    int failed = 0;

    if (!(have & WPD_X86_CPU_FLAG_SSE2))
        return 0;
    failed |= check_raw_bindings();
    if (!guards)
        return failed;
    failed |= probe_level(&sse2);
    if (have & WPD_X86_CPU_FLAG_AVX2)
        failed |= probe_level(&avx2);
    return failed;
}
#else
static int check_raw_bindings(void) {
    WPDRESCALERAWDSP raw;
    int              failed = 0;

    wpd_rescale_raw_dsp_init(&raw);
    PIN(import_expand, ff_rescale_import_expand_neon);
    PIN(import_shrink, ff_rescale_import_shrink_neon);
    PIN(export_direct, ff_rescale_export_direct_neon);
    PIN(export_blend, ff_rescale_export_blend_neon);
    PIN(export_shrink, ff_rescale_export_shrink_neon);
    PIN(export_shrink0, ff_rescale_export_shrink0_neon);
    return failed;
}

/* AAPCS64 leaves the upper half of a 32-bit argument undefined the same way
 * the x86-64 ABI does, and all seven arguments arrive in registers there, so
 * the junk reaches the kernel without a stack slot to launder it. */
static int probe_dirty_args(int guards) {
    const RawLevel neon = {
        "neon",
        0,
        ff_rescale_import_expand_neon,
        ff_rescale_import_shrink_neon,
        ff_rescale_export_direct_neon,
        ff_rescale_export_shrink0_neon,
        ff_rescale_export_shrink_neon,
        ff_rescale_export_blend_neon,
    };
    int failed = 0;

    if (!(wpd_get_cpu_flags() & WPD_ARM_CPU_FLAG_NEON))
        return 0;
    failed |= check_raw_bindings();
    if (guards)
        failed |= probe_level(&neon);
    return failed;
}
#endif
#else
static int probe_dirty_args(int guards) {
    (void)guards;
    return 0;
}
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
    int      guards, failed = 0;

    wpd_init_cpu();
    have   = wpd_get_cpu_flags();
    guards = fault_delivery_works();
    if (!guards)
        printf(
            "rowbounds: SKIP guard pages: this host does not deliver "
            "synchronous fault signals\n");

    for (size_t i = 0; guards && i < sizeof(levels) / sizeof(*levels); i++) {
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
    failed |= probe_dirty_args(guards);
    printf("rowbounds: %s\n",
           failed       ? "FAILED"
               : guards ? "all rows stayed in bounds"
                        : "table bindings checked, rows unprobed");
    return failed;
}
