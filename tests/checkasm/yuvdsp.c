#include <string.h>

#include "checkasm.h"
#include "yuvdsp.h"

#define MAX_BLOCKS 8
#define MAX_PIXELS (MAX_BLOCKS * WPD_UPSAMPLE_BLOCK)
#define GUARD_PIXELS 8
#define UV_PIXELS (MAX_PIXELS / 2 + 16)

static const int blocks[] = {1, 2, 3, 5, MAX_BLOCKS};

static void check_upsample_block(WPDYUVDSP *dsp, int layout, const char *name) {
    LOCAL_ALIGNED_16(uint8_t, top_y, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, bottom_y, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, top_u, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, top_v, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, cur_u, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, cur_v, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [8 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [8 * (MAX_PIXELS + GUARD_PIXELS)]);
    uint8_t *const bot0 = dst0 + 4 * (MAX_PIXELS + GUARD_PIXELS);
    uint8_t *const bot1 = dst1 + 4 * (MAX_PIXELS + GUARD_PIXELS);
    declare_func(void,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 uint8_t *,
                 uint8_t *,
                 int);

    if (check_func(dsp->upsample_block[layout], "upsample_block_%s", name)) {
        for (size_t i = 0; i < sizeof(blocks) / sizeof(*blocks); i++) {
            const int n = blocks[i];

            for (int x = 0; x < MAX_PIXELS; x++) {
                top_y[x]    = (uint8_t)rnd();
                bottom_y[x] = (uint8_t)rnd();
            }
            for (int x = 0; x < UV_PIXELS; x++) {
                top_u[x] = (uint8_t)rnd();
                top_v[x] = (uint8_t)rnd();
                cur_u[x] = (uint8_t)rnd();
                cur_v[x] = (uint8_t)rnd();
            }
            for (int x = 0; x < 8 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(
                top_y, bottom_y, top_u, top_v, cur_u, cur_v, dst0, bot0, n);
            call_new(
                top_y, bottom_y, top_u, top_v, cur_u, cur_v, dst1, bot1, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();

            for (int x = 0; x < 8 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();
            call_ref(top_y, NULL, top_u, top_v, cur_u, cur_v, dst0, NULL, n);
            call_new(top_y, NULL, top_u, top_v, cur_u, cur_v, dst1, NULL, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(top_y,
                  bottom_y,
                  top_u,
                  top_v,
                  cur_u,
                  cur_v,
                  dst1,
                  bot1,
                  MAX_BLOCKS);
    }
}

static void check_dispatch_alpha(WPDYUVDSP *dsp) {
    static const int lengths[] = {1, 3, 8, 15, 16, 17, 31, 63, 64, MAX_PIXELS};
    LOCAL_ALIGNED_16(uint8_t, src, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->dispatch_alpha, "dispatch_alpha")) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int n = lengths[i];

            for (int x = 0; x < MAX_PIXELS; x++) src[x] = (uint8_t)rnd();
            for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static const int row_lengths[] = {
    1, 2, 3, 4, 5, 7, 8, 15, 16, 17, 31, 63, 64, 255, MAX_PIXELS};

static void check_pack_row(pack_row_func func, const char *name, int bpp) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(func, "%s", name)) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int x = 0; x < 4 * MAX_PIXELS; x++) src[x] = (uint8_t)rnd();
            for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            /* The packers may overwrite up to a pixel past the row. */
            if (memcmp(dst0, dst1, (size_t)n * bpp) ||
                memcmp(dst0 + (size_t)n * bpp + 4,
                       dst1 + (size_t)n * bpp + 4,
                       4 * GUARD_PIXELS - 4))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static void check_premultiply_row(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, argb0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, argb1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, int, int);

    if (check_func(dsp->premultiply_row, "premultiply_row")) {
        for (int alpha_first = 0; alpha_first < 2; alpha_first++)
            for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
                 i++) {
                const int n   = row_lengths[i];
                const int off = alpha_first ? 0 : 3;

                for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                    argb0[x] = argb1[x] = (uint8_t)rnd();
                /* Opaque and fully transparent pixels take different paths. */
                argb0[off] = argb1[off] = 0xff;
                if (n > 1)
                    argb0[4 + off] = argb1[4 + off] = 0;

                call_ref(argb0, alpha_first, n);
                call_new(argb1, alpha_first, n);
                if (memcmp(argb0, argb1, sizeof(argb0)))
                    fail();
            }
        bench_new(argb1, 1, MAX_PIXELS);
    }
}

void checkasm_check_yuvdsp(void) {
    WPDYUVDSP dsp;

    wpd_yuv_dsp_init(&dsp);
    check_upsample_block(&dsp, WPD_LAYOUT_ARGB, "argb");
    check_upsample_block(&dsp, WPD_LAYOUT_RGBA, "rgba");
    check_upsample_block(&dsp, WPD_LAYOUT_BGRA, "bgra");
    report("upsample_block");
    check_dispatch_alpha(&dsp);
    report("dispatch_alpha");
    check_pack_row(dsp.pack_rgba, "pack_rgba", 4);
    check_pack_row(dsp.pack_bgra, "pack_bgra", 4);
    check_pack_row(dsp.pack_rgb, "pack_rgb", 3);
    check_pack_row(dsp.pack_bgr, "pack_bgr", 3);
    report("pack_row");
    check_premultiply_row(&dsp);
    report("premultiply_row");
}
