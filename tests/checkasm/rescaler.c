#include <string.h>

#include "checkasm.h"
#include "rescaler.h"

#define MAX_OUT 512
#define MAX_SRC 512
#define GUARD 8

/* Realistic magnitudes: rows accumulate at most 255 * x_add per lane, and
 * both implementations only agree while the fixed-point products stay
 * inside 63 bits. */
#define ROW_MASK 0x00ffffff

static const struct {
    int src_w, dst_w;
} expand_sizes[] = {{1, 3},
                    {2, 5},
                    {3, 4},
                    {7, 9},
                    {8, 9},
                    {8, 64},
                    {9, 17},
                    {16, 80},
                    {31, 32},
                    {33, 100},
                    {64, 65},
                    {100, 128}};

static const struct {
    int src_w, dst_w;
} shrink_sizes[] = {{3, 1},
                    {5, 2},
                    {8, 3},
                    {9, 8},
                    {17, 9},
                    {64, 8},
                    {65, 64},
                    {100, 33},
                    {128, 1},
                    {257, 2},
                    {512, 3},
                    {129, 1}};

static uint32_t frac32(uint32_t x, uint32_t y) {
    return (uint32_t)((((uint64_t)x) << 32) / y);
}

static void check_import(rescale_import_row_func func, const char *name,
                         int expand) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * MAX_SRC]);
    LOCAL_ALIGNED_16(uint32_t, frow0, [4 * MAX_OUT + GUARD]);
    LOCAL_ALIGNED_16(uint32_t, frow1, [4 * MAX_OUT + GUARD]);
    declare_func(void,
                 uint32_t *,
                 const uint8_t *,
                 int,
                 int,
                 int,
                 uint32_t,
                 uint32_t,
                 uint32_t);

    if (check_func(func, "%s", name)) {
        const size_t cases = expand
            ? sizeof(expand_sizes) / sizeof(*expand_sizes)
            : sizeof(shrink_sizes) / sizeof(*shrink_sizes);

        for (size_t i = 0; i < cases; i++)
            for (int pass = 0; pass < 3; pass++) {
                static const int chans[] = {1, 4, 2};
                const int        ch      = chans[pass];
                const int        sw      = expand ? expand_sizes[i].src_w
                                                  : shrink_sizes[i].src_w;
                const int        dw      = expand ? expand_sizes[i].dst_w
                                                  : shrink_sizes[i].dst_w;
                const uint32_t   x_add   = expand ? (uint32_t)(dw - 1)
                                                  : (uint32_t)sw;
                const uint32_t   x_sub   = expand ? (uint32_t)(sw - 1)
                                                  : (uint32_t)dw;
                const uint32_t   fx      = expand ? 0 : frac32(1, x_sub);

                for (int x = 0; x < 4 * MAX_SRC; x++) src[x] = (uint8_t)rnd();
                for (int x = 0; x < 4 * MAX_OUT + GUARD; x++)
                    frow0[x] = frow1[x] = rnd();

                call_ref(frow0, src, dw, sw, ch, x_add, x_sub, fx);
                call_new(frow1, src, dw, sw, ch, x_add, x_sub, fx);
                if (memcmp(
                        frow0, frow1, (4 * MAX_OUT + GUARD) * sizeof(*frow0)))
                    fail();
            }
        {
            const int      sw    = expand ? 100 : 512;
            const int      dw    = expand ? 128 : 3;
            const uint32_t x_add = expand ? (uint32_t)(dw - 1) : (uint32_t)sw;
            const uint32_t x_sub = expand ? (uint32_t)(sw - 1) : (uint32_t)dw;

            bench_new(frow1,
                      src,
                      dw,
                      sw,
                      4,
                      x_add,
                      x_sub,
                      expand ? 0 : frac32(1, x_sub));
        }
    }
}

static void check_export_expand(WPDRESCALEDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, irow, [MAX_OUT]);
    LOCAL_ALIGNED_16(uint32_t, frow, [MAX_OUT]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [MAX_OUT + GUARD]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [MAX_OUT + GUARD]);
    static const int widths[] = {1, 2, 7, 8, 9, 16, 63, 255, MAX_OUT};
    declare_func(void,
                 uint8_t *,
                 const uint32_t *,
                 const uint32_t *,
                 int,
                 int,
                 uint32_t,
                 uint32_t);

    if (check_func(dsp->export_row_expand, "rescale_export_expand")) {
        for (size_t i = 0; i < sizeof(widths) / sizeof(*widths); i++)
            for (int blend = 0; blend < 2; blend++) {
                const int      n       = widths[i];
                const uint32_t y_sub   = 1 + (rnd() & 0xffff);
                const int      y_accum = blend ? -(int)(1 + rnd() % y_sub) : 0;
                const uint32_t fy      = rnd();

                for (int x = 0; x < MAX_OUT; x++) {
                    irow[x] = rnd() & ROW_MASK;
                    frow[x] = rnd() & ROW_MASK;
                }
                for (int x = 0; x < MAX_OUT + GUARD; x++)
                    dst0[x] = dst1[x] = (uint8_t)rnd();

                call_ref(dst0, irow, frow, n, y_accum, y_sub, fy);
                call_new(dst1, irow, frow, n, y_accum, y_sub, fy);
                if (memcmp(dst0, dst1, MAX_OUT + GUARD))
                    fail();
            }
        bench_new(dst1, irow, frow, MAX_OUT, -3, 7, frac32(1, 7));
    }
}

static void check_export_shrink(WPDRESCALEDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, irow0, [MAX_OUT + GUARD]);
    LOCAL_ALIGNED_16(uint32_t, irow1, [MAX_OUT + GUARD]);
    LOCAL_ALIGNED_16(uint32_t, frow, [MAX_OUT]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [MAX_OUT + GUARD]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [MAX_OUT + GUARD]);
    static const int widths[] = {1, 2, 7, 8, 9, 16, 63, 255, MAX_OUT};
    declare_func(void,
                 uint8_t *,
                 uint32_t *,
                 const uint32_t *,
                 int,
                 int,
                 uint32_t,
                 uint32_t);

    if (check_func(dsp->export_row_shrink, "rescale_export_shrink")) {
        for (size_t i = 0; i < sizeof(widths) / sizeof(*widths); i++)
            for (int carry = 0; carry < 2; carry++) {
                const int      n       = widths[i];
                const int      y_accum = carry ? -(int)(1 + (rnd() & 0xff)) : 0;
                const uint32_t fy      = carry ? 1 + (rnd() & 0xffffff) : rnd();
                const uint32_t fxy     = rnd();

                for (int x = 0; x < MAX_OUT; x++) {
                    /* irow accumulates frow, so it dominates the fraction
                     * carried out of it. */
                    frow[x] = rnd() & ROW_MASK;
                }
                for (int x = 0; x < MAX_OUT + GUARD; x++)
                    irow0[x] = irow1[x] = (x < MAX_OUT ? frow[x] : 0) +
                        (rnd() & ROW_MASK);
                for (int x = 0; x < MAX_OUT + GUARD; x++)
                    dst0[x] = dst1[x] = (uint8_t)rnd();

                call_ref(dst0, irow0, frow, n, y_accum, fy, fxy);
                call_new(dst1, irow1, frow, n, y_accum, fy, fxy);
                if (memcmp(dst0, dst1, MAX_OUT + GUARD) ||
                    memcmp(irow0, irow1, (MAX_OUT + GUARD) * sizeof(*irow0)))
                    fail();
            }
        bench_new(dst1, irow1, frow, MAX_OUT, -3, frac32(1, 9), frac32(1, 63));
    }
}

void checkasm_check_rescaler(void) {
    WPDRESCALEDSP dsp;

    wpd_rescale_dsp_init(&dsp);
    check_import(dsp.import_row_expand, "rescale_import_expand", 1);
    check_import(dsp.import_row_shrink, "rescale_import_shrink", 0);
    report("import_row");
    check_export_expand(&dsp);
    check_export_shrink(&dsp);
    report("export_row");
}
