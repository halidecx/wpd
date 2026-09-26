
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

/* Two rows, the second over the first. Values narrowed to a few steps make
 * the select predictor's distances tie, and a flat regime lets a pixel's
 * pick carry from one block to the next. */
static uint32_t pair_sample(int regime) {
    switch (regime) {
    case 1: return (uint32_t)rnd() & 0x03030303u;
    case 2: return (rnd() & 15) ? 0 : (uint32_t)rnd();
    default: return (uint32_t)rnd();
    }
}

static void check_pred_pair(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, upper, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, a0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, a1, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, b0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, b1, [BUF_PIXELS]);
    declare_func(void,
                 const uint32_t *,
                 const uint32_t *,
                 int,
                 uint32_t *,
                 const uint32_t *,
                 uint32_t *);

    for (int mode = 0; mode < WPD_PRED_COUNT; mode++) {
        if (check_func(dsp->pred_add_pair[mode], "pred_add_pair_%d", mode)) {
            for (int regime = 0; regime < 3; regime++) {
                for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths);
                     i++) {
                    const int n = lengths[i];

                    for (int x = 0; x < BUF_PIXELS; x++) {
                        upper[x] = pair_sample(regime);
                        a0[x] = a1[x] = pair_sample(regime);
                        b0[x] = b1[x] = pair_sample(regime);
                    }
                    call_ref(a0 + 1, upper + 1, n, a0 + 1, b0 + 1, b0 + 1);
                    call_new(a1 + 1, upper + 1, n, a1 + 1, b1 + 1, b1 + 1);
                    if (memcmp(a0, a1, sizeof(a0)) ||
                        memcmp(b0, b1, sizeof(b0)))
                        fail();
                }
            }
            for (int x = 0; x < BUF_PIXELS; x++) {
                upper[x] = (uint32_t)rnd();
                a1[x]    = (uint32_t)rnd();
                b1[x]    = (uint32_t)rnd();
            }
            bench_new(a1 + 1, upper + 1, MAX_PIXELS, a1 + 1, b1 + 1, b1 + 1);
        }
    }
}

#define GREEN_PIXELS 1024

static const int green_lengths[] = {
    1, 2, 3, 5, 8, 15, 16, 17, 31, 33, 63, 64, 65, 255, 257, GREEN_PIXELS};

/* The green of an alpha image's pixel x in one of a few regimes. Narrow
 * values make the select predictor's two distances tie often. Flat rows
 * with sparse changes keep whole blocks taking T, as alpha does, and throw
 * each lane of a block off in turn. Banded rows alternate a stretch of
 * noise with a flat one, so a kernel that gives up on a noisy stretch has
 * a flat one to come back for. Last, a flat row above residuals that are
 * never 0 (their flat is 0): from a left off the flat, no pixel of a block
 * takes T. */
static uint8_t green_sample(int regime, int x, uint8_t flat) {
    switch (regime) {
    case 1: return (uint8_t)(rnd() & 3);
    case 2: return (rnd() & 63) ? flat : (uint8_t)rnd();
    case 3: return (rnd() & 7) ? flat : (uint8_t)rnd();
    case 4: return (x / 80) % 3 == 0 ? (uint8_t)rnd() : flat;
    case 5: return flat ? flat : (uint8_t)(rnd() | 1);
    default: return (uint8_t)rnd();
    }
}

static void check_pred_green(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, res, [GREEN_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, upper, [GREEN_PIXELS + GUARD_PIXELS + 2]);
    LOCAL_ALIGNED_16(uint8_t, row0, [GREEN_PIXELS + GUARD_PIXELS + 1]);
    LOCAL_ALIGNED_16(uint8_t, row1, [GREEN_PIXELS + GUARD_PIXELS + 1]);
    declare_func(void, const uint32_t *, const uint8_t *, int, uint8_t *);

    for (int mode = 0; mode < WPD_PRED_COUNT; mode++) {
        if (check_func(dsp->pred_green[mode], "pred_green_%d", mode)) {
            for (int regime = 0; regime < 6; regime++) {
                for (size_t i = 0;
                     i < sizeof(green_lengths) / sizeof(*green_lengths);
                     i++) {
                    const int     n    = green_lengths[i];
                    const uint8_t flat = (uint8_t)rnd();

                    for (int x = 0; x < GREEN_PIXELS + GUARD_PIXELS; x++) {
                        const uint32_t g = green_sample(regime, x, 0);

                        res[x] = ((uint32_t)rnd() & 0xFF00FFFFu) | g << 16;
                    }
                    for (int x = 0; x < GREEN_PIXELS + GUARD_PIXELS + 2; x++)
                        upper[x] = green_sample(regime, x, flat);
                    for (int x = 0; x < GREEN_PIXELS + GUARD_PIXELS + 1; x++)
                        row0[x] = row1[x] = green_sample(regime, x, flat);
                    /* A left off the flat takes L all along a flat row. */
                    if (i & 1)
                        row0[0] = row1[0] = (uint8_t)rnd();

                    call_ref(res, upper + 1, n, row0 + 1);
                    call_new(res, upper + 1, n, row1 + 1);
                    if (memcmp(row0, row1, sizeof(row0)))
                        fail();
                }
            }
            for (int x = 0; x < GREEN_PIXELS + GUARD_PIXELS; x++)
                res[x] = (uint32_t)rnd();
            for (int x = 0; x < GREEN_PIXELS + GUARD_PIXELS + 2; x++)
                upper[x] = (uint8_t)rnd();
            bench_new(res, upper + 1, MAX_PIXELS, row1 + 1);
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

#define MAP_PIXELS 1024
#define MAP_BUF (MAP_PIXELS + GUARD_PIXELS)

static const int map_lengths[] = {
    1, 7, 8, 9, 16, 17, 33, 159, 160, 161, 175, 177, 400, 1023, MAP_PIXELS};

/* An index image's pixel x in one of a few regimes. Runs, some long enough
 * for a kernel to follow and some not; one long run with a rare odd pixel,
 * which lands in each lane of a block in turn; a dither's two pixels in
 * turn, whose pairs are all alike though the pixels are not; and noise. */
static uint32_t map_sample(int regime, int x, uint32_t *run, int *left) {
    switch (regime) {
    case 1:
        if (--*left <= 0) {
            *run  = (uint32_t)rnd();
            *left = 1 + (int)(rnd() % 400);
        }
        return *run;
    case 2: return (rnd() & 127) ? *run : (uint32_t)rnd();
    case 3: return (x & 1) ? ~*run : *run;
    default: return (uint32_t)rnd();
    }
}

static void check_map_color32(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, palette, [256]);
    LOCAL_ALIGNED_16(uint8_t, src, [4 * MAP_BUF]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * MAP_BUF]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * MAP_BUF]);
    declare_func(void, uint8_t *, const uint8_t *, const uint32_t *, int);

    if (check_func(dsp->map_color32, "map_color32")) {
        for (int regime = 0; regime < 4; regime++) {
            for (size_t i = 0; i < sizeof(map_lengths) / sizeof(*map_lengths);
                 i++) {
                const int n    = map_lengths[i];
                uint32_t  run  = (uint32_t)rnd();
                int       left = 0;

                for (int x = 0; x < 256; x++) palette[x] = (uint32_t)rnd();
                for (int x = 0; x < 4 * MAP_BUF; x += 4) {
                    WPD_WN32A(src + x, map_sample(regime, x / 4, &run, &left));
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
        }
        bench_new(dst1, src, palette, MAP_PIXELS);
    }
}

#define NIBBLE_BLOCKS 64

static const int nibble_blocks[] = {
    0, 1, 2, 3, 4, 5, 7, 8, 9, 31, NIBBLE_BLOCKS};

static void check_expand_alpha_nibbles(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, palette, [16]);
    LOCAL_ALIGNED_16(uint8_t, src, [8 * NIBBLE_BLOCKS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [16 * NIBBLE_BLOCKS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [16 * NIBBLE_BLOCKS + GUARD_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, const uint8_t *, int);

    if (check_func(dsp->expand_alpha_nibbles, "expand_alpha_nibbles")) {
        for (size_t i = 0; i < sizeof(nibble_blocks) / sizeof(*nibble_blocks);
             i++) {
            const int n = nibble_blocks[i];

            for (int x = 0; x < 16; x++) palette[x] = (uint8_t)rnd();
            for (int x = 0; x < 8 * NIBBLE_BLOCKS + GUARD_PIXELS; x++)
                src[x] = (uint8_t)rnd();
            for (int x = 0; x < 16 * NIBBLE_BLOCKS + GUARD_PIXELS; x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, palette, n);
            call_new(dst1, src, palette, n);
            if (memcmp(dst0, dst1, 16 * NIBBLE_BLOCKS + GUARD_PIXELS))
                fail();
        }
        bench_new(dst1, src, palette, NIBBLE_BLOCKS);
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

static void check_add_green(WPDLosslessDSP *dsp) {
    LOCAL_ALIGNED_16(uint32_t, src, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, dst0, [BUF_PIXELS]);
    LOCAL_ALIGNED_16(uint32_t, dst1, [BUF_PIXELS]);
    declare_func(void, uint32_t *, const uint32_t *, int);

    if (check_func(dsp->add_green, "add_green")) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int n = lengths[i];

            for (int x = 0; x < BUF_PIXELS; x++) {
                src[x]  = (uint32_t)rnd();
                dst0[x] = dst1[x] = (uint32_t)rnd();
            }

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();

            memcpy(dst0, src, sizeof(dst0));
            memcpy(dst1, src, sizeof(dst1));
            call_ref(dst0, dst0, n);
            call_new(dst1, dst1, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
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
    check_pred_pair(&dsp);
    report("pred_add_pair");
    check_pred_green(&dsp);
    report("pred_green");
    check_extract_green(&dsp);
    report("extract_green");
    check_add_green(&dsp);
    report("add_green");
    check_map_color32(&dsp);
    report("map_color32");
    check_expand_alpha_nibbles(&dsp);
    report("expand_alpha_nibbles");
    check_blend_row_argb(&dsp);
    report("blend_row_argb");
    check_blend_row_argb_premult(&dsp);
    report("blend_row_argb_premult");
    check_color_row(&dsp);
    report("color_row");
}
