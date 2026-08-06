/*
 * Coefficient decoder tests for WPD.
 * SPDX-License-Identifier: LGPL-2.1-or-later
 */

#include <string.h>

#include "checkasm.h"
#include "vp8.h"

#include "arm/vp8.h"

// A block reads at most 16 coefficients, each a handful of bool decodes, so
// this is far more bitstream than one call can consume. Staying clear of the
// end keeps both implementations on their normal paths instead of the
// out-of-data fallback.
#define RAC_BUF_SIZE 4096

typedef int (*decode_block_coeffs_fn)(VP56RangeCoder *c, WpdDctElem block[16],
                                      uint8_t probs[16][3][NUM_DCT_TOKENS - 1],
                                      int i, uint8_t *token_prob,
                                      int16_t qmul[2]);

/*
 * decode_block_coeffs_internal is substituted at compile time rather than
 * dispatched through a DSP struct, so there is no context to init. Pick the
 * implementation the way the decoder's #define would, based on the CPU flags
 * checkasm is currently exercising.
 */
static decode_block_coeffs_fn get_decode_block_coeffs(void) {
#if defined(__arm__)
    if (wpd_have_armv6(wpd_get_cpu_flags()))
        return ff_decode_block_coeffs_armv6;
#endif
    return wpd_decode_block_coeffs_c;
}

static void check_decode_block_coeffs(void) {
    LOCAL_ALIGNED_16(uint8_t, buf, [RAC_BUF_SIZE]);
    uint8_t        probs[16][3][NUM_DCT_TOKENS - 1];
    WpdDctElem     block0[16], block1[16];
    VP56RangeCoder rac0, rac1;
    int16_t        qmul[2];

    declare_func(int,
                 VP56RangeCoder *,
                 WpdDctElem *,
                 uint8_t (*)[3][NUM_DCT_TOKENS - 1],
                 int,
                 uint8_t *,
                 int16_t *);

    if (check_func(get_decode_block_coeffs(), "decode_block_coeffs")) {
        // i is 0 for blocks carrying their own DC and 1 when the DC came from
        // a separate WHT block; zero_nhood is the left/top all-zero context.
        for (int i = 0; i <= 1; i++) {
            for (int zero_nhood = 0; zero_nhood < 3; zero_nhood++) {
                int res0, res1;

                for (int n = 0; n < RAC_BUF_SIZE; n++) buf[n] = rnd();

                // Probabilities are 8-bit and never 0 or 255 in a real stream.
                for (int a = 0; a < 16; a++)
                    for (int b = 0; b < 3; b++)
                        for (int t = 0; t < NUM_DCT_TOKENS - 1; t++)
                            probs[a][b][t] = 1 + rnd() % 254;

                // Roughly the range of the VP8 dequant tables.
                qmul[0] = 4 + rnd() % 154;
                qmul[1] = 4 + rnd() % 154;

                // Zero the whole struct, padding included, so the states can
                // be compared with memcmp afterwards.
                memset(&rac0, 0, sizeof(rac0));
                wpd_vp56_init_range_decoder(&rac0, buf, RAC_BUF_SIZE);
                memcpy(&rac1, &rac0, sizeof(rac0));

                memset(block0, 0, sizeof(block0));
                memset(block1, 0, sizeof(block1));

                res0 = call_ref(
                    &rac0, block0, probs, i, probs[i][zero_nhood], qmul);
                res1 = call_new(
                    &rac1, block1, probs, i, probs[i][zero_nhood], qmul);

                // The coder state matters as much as the coefficients: the
                // caller keeps decoding from it, so leaving it even one bit
                // off desynchronizes the rest of the partition.
                if (res0 != res1 || memcmp(block0, block1, sizeof(block0)) ||
                    memcmp(&rac0, &rac1, sizeof(rac0)))
                    fail();
            }
        }

        // No bench_new: each call advances the coder, so a benchmark loop
        // would drain the buffer and end up timing the out-of-data path.
        // Use scripts/bench.sh for coefficient decoding throughput.
    }
}

void checkasm_check_vp8coeffs(void) {
    check_decode_block_coeffs();
    report("decode_block_coeffs");
}
