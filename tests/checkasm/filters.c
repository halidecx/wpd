#include <string.h>

#include "checkasm.h"
#include "filtersdsp.h"

#define MAX_WIDTH 512
#define GUARD 16

/* Besides the usual boundaries, 161 lands a row end exactly where a kernel
 * that falls back to a fixed-length serial burst finishes one. */
static const int widths[] = {1,
                             2,
                             3,
                             7,
                             8,
                             9,
                             15,
                             16,
                             17,
                             24,
                             31,
                             63,
                             127,
                             128,
                             161,
                             255,
                             256,
                             509,
                             MAX_WIDTH};

static void check_unfilter(unfilter_func func, const char *name) {
    LOCAL_ALIGNED_16(uint8_t, prev, [MAX_WIDTH]);
    LOCAL_ALIGNED_16(uint8_t, row0, [MAX_WIDTH + GUARD]);
    LOCAL_ALIGNED_16(uint8_t, row1, [MAX_WIDTH + GUARD]);
    declare_func(void, const uint8_t *, uint8_t *, int);

    if (check_func(func, "%s", name)) {
        for (size_t i = 0; i < sizeof(widths) / sizeof(*widths); i++)
            for (int with_prev = 0; with_prev < 2; with_prev++)
                for (int mode = 0; mode < 3; mode++) {
                    const int w = widths[i];

                    for (int x = 0; x < MAX_WIDTH; x++)
                        prev[x] = (uint8_t)rnd();
                    for (int x = 0; x < MAX_WIDTH + GUARD; x++)
                        row0[x] = row1[x] = (uint8_t)rnd();
                    /* Smooth rows keep the gradient fast path honest; the
                     * random ones drive it through clip and wrap. */
                    if (mode == 1) {
                        for (int x = 0; x < MAX_WIDTH; x++) prev[x] = 128;
                        for (int x = 0; x < MAX_WIDTH + GUARD; x++)
                            row0[x] = row1[x] = (uint8_t)(rnd() & 3);
                    } else if (mode == 2) {
                        for (int x = 0; x < MAX_WIDTH; x++)
                            prev[x] = (x & 1) ? 255 : 0;
                    }

                    call_ref(with_prev ? prev : NULL, row0, w);
                    call_new(with_prev ? prev : NULL, row1, w);
                    if (memcmp(row0, row1, MAX_WIDTH + GUARD))
                        fail();
                }
        for (int x = 0; x < MAX_WIDTH; x++) {
            prev[x] = (uint8_t)rnd();
            row1[x] = (uint8_t)(rnd() & 3);
        }
        bench_new(prev, row1, MAX_WIDTH);
    }
}

void checkasm_check_filters(void) {
    WPDFILTERSDSP dsp;

    wpd_filters_dsp_init(&dsp);
    check_unfilter(dsp.horizontal_unfilter, "horizontal_unfilter");
    check_unfilter(dsp.vertical_unfilter, "vertical_unfilter");
    check_unfilter(dsp.gradient_unfilter, "gradient_unfilter");
    report("unfilter");
}
