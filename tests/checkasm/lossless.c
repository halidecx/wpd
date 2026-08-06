
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

void checkasm_check_lossless(void) {
    WPDLosslessDSP dsp;

    wpd_vp8l_dsp_init(&dsp);
    check_pred_add(&dsp);
    report("pred_add");
    check_extract_green(&dsp);
    report("extract_green");
}
