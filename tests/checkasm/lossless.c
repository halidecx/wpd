
#include <string.h>

#include "checkasm.h"
#include "vp8l_dsp.h"

#define MAX_PIXELS 256
#define GUARD_PIXELS 8
#define BUF_PIXELS (1 + MAX_PIXELS + GUARD_PIXELS)

static const int lengths[] = {
    1, 2, 3, 4, 5, 7, 8, 15, 16, 17, 19, 31, 63, 64, 255, MAX_PIXELS};

#define randomize_pixels(buf0, buf1)                 \
    do {                                             \
        for (int i = 0; i < BUF_PIXELS; i++)         \
            (buf0)[i] = (buf1)[i] = (uint32_t)rnd(); \
    } while (0)

static void check_pred_add(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, upper0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, upper1, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, row0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, row1, [BUF_PIXELS]);
    declare_func(void, const uint32_t *, const uint32_t *, int, uint32_t *);

    for (int mode = 0; mode < WPD_PRED_COUNT; mode++) {
        if (check_func(dsp->pred_add[mode], "pred_add_%d", mode)) {
            for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
                const int n = lengths[i];

                randomize_pixels(upper0, upper1);
                randomize_pixels(row0, row1);
                call_ref(row0 + 1, upper0 + 1, n, row0 + 1);
                call_new(row1 + 1, upper1 + 1, n, row1 + 1);
                if (memcmp(row0, row1, sizeof(row0)) ||
                    memcmp(upper0, upper1, sizeof(upper0)))
                    fail();
            }
            randomize_pixels(upper0, upper1);
            randomize_pixels(row0, row1);
            bench_new(row1 + 1, upper1 + 1, MAX_PIXELS, row1 + 1);
        }
    }
}

static void check_extract_green(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [MAX_PIXELS + GUARD_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->extract_green, "extract_green")) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int n = lengths[i];

            for (int x = 0; x < 4 * MAX_PIXELS; x += 4)
                WPD_WN32A(src + x, rnd());
            for (int x = 0; x < MAX_PIXELS + GUARD_PIXELS; x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static void check_map_color32(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, palette, [256]);
    LOCAL_ALIGNED_16(uint8_t, src, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * BUF_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, const uint32_t *, int);

    if (check_func(dsp->map_color32, "map_color32")) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int n = lengths[i];

            for (int x = 0; x < 256; x++) palette[x] = (uint32_t)rnd();
            for (int x = 0; x < 4 * BUF_PIXELS; x += 4) {
                WPD_WN32A(src + x, rnd());
                WPD_WN32A(dst0 + x, rnd());
                memcpy(dst1 + x, dst0 + x, 4);
            }

            call_ref(dst0, src, palette, n);
            call_new(dst1, src, palette, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();

            memcpy(dst0, src, sizeof(dst0));
            memcpy(dst1, src, sizeof(dst1));
            call_ref(dst0, dst0, palette, n);
            call_new(dst1, dst1, palette, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(dst1, src, palette, MAX_PIXELS);
    }
}

static void check_blend_row_argb(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * BUF_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->blend_row_argb, "blend_row_argb")) {
        for (int mode = 0; mode < 3; mode++) {
            for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
                const int n = lengths[i];

                for (int x = 0; x < 4 * BUF_PIXELS; x += 4) {
                    WPD_WN32A(src + x, rnd());
                    WPD_WN32A(dst0 + x, rnd());
                    if (mode == 1 && (rnd() & 7))
                        src[x] = 255;
                    else if (mode == 2 && (rnd() & 7))
                        src[x] = 0;
                    memcpy(dst1 + x, dst0 + x, 4);
                }

                call_ref(dst0, src, n);
                call_new(dst1, src, n);
                if (memcmp(dst0, dst1, sizeof(dst0)))
                    fail();
            }
        }

        for (int x = 0; x < 4 * BUF_PIXELS; x += 4) {
            WPD_WN32A(src + x, rnd());
            WPD_WN32A(dst1 + x, rnd());
            src[x] = (x & 0x3F) < 8 ? (uint8_t)rnd() : (x & 0x80) ? 255 : 0;
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static void check_blend_row_argb_premult(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * BUF_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->blend_row_argb_premult, "blend_row_argb_premult")) {
        for (int mode = 0; mode < 3; mode++) {
            for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
                const int n = lengths[i];

                for (int x = 0; x < 4 * BUF_PIXELS; x += 4) {
                    WPD_WN32A(src + x, rnd());
                    WPD_WN32A(dst0 + x, rnd());
                    if (mode == 1 && (rnd() & 7))
                        src[x] = 255;
                    else if (mode == 2 && (rnd() & 7))
                        src[x] = 0;
                    memcpy(dst1 + x, dst0 + x, 4);
                }

                call_ref(dst0, src, n);
                call_new(dst1, src, n);
                if (memcmp(dst0, dst1, sizeof(dst0)))
                    fail();
            }
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static void check_color_row(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, src, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, dst0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, dst1, [BUF_PIXELS]);
    declare_func(void, uint32_t *, const uint32_t *, int, uint32_t);

    if (check_func(dsp->color_row, "color_row")) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int      n    = lengths[i];
            const uint32_t mult = (uint32_t)rnd();

            for (int x = 0; x < BUF_PIXELS; x++) {
                src[x]  = (uint32_t)rnd();
                dst0[x] = dst1[x] = (uint32_t)rnd();
            }

            call_ref(dst0, src, n, mult);
            call_new(dst1, src, n, mult);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();

            memcpy(dst0, src, sizeof(dst0));
            memcpy(dst1, src, sizeof(dst1));
            call_ref(dst0, dst0, n, mult);
            call_new(dst1, dst1, n, mult);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS, 0x00204060u);
    }
}

void checkasm_check_lossless(void) {
    WPDLosslessDSP dsp;

    wpd_vp8l_dsp_init(&dsp);
    check_pred_add(&dsp);
    report("pred_add");
    check_extract_green(&dsp);
    report("extract_green");
    check_map_color32(&dsp);
    report("map_color32");
    check_blend_row_argb(&dsp);
    report("blend_row_argb");
    check_blend_row_argb_premult(&dsp);
    report("blend_row_argb_premult");
    check_color_row(&dsp);
    report("color_row");
}
