
#include <string.h>

#include "checkasm.h"
#include "vp8.h"

#include "arm/vp8.h"

#define RAC_BUF_SIZE 4096

typedef int (*decode_block_coeffs_fn)(VP56RangeCoder *c, WpdDctElem block[16],
                                      uint8_t probs[16][3][NUM_DCT_TOKENS - 1],
                                      int i, uint8_t *token_prob,
                                      int16_t qmul[2]);

static decode_block_coeffs_fn get_decode_block_coeffs(void) {
#if WPD_ARM_ARMV6_ASM
    if (wpd_get_cpu_flags() & WPD_ARM_CPU_FLAG_ARMV6)
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
        for (int i = 0; i <= 1; i++) {
            for (int zero_nhood = 0; zero_nhood < 3; zero_nhood++) {
                int res0, res1;

                for (int n = 0; n < RAC_BUF_SIZE; n++) buf[n] = rnd();

                for (int a = 0; a < 16; a++)
                    for (int b = 0; b < 3; b++)
                        for (int t = 0; t < NUM_DCT_TOKENS - 1; t++)
                            probs[a][b][t] = 1 + rnd() % 254;

                qmul[0] = 4 + rnd() % 154;
                qmul[1] = 4 + rnd() % 154;

                memset(&rac0, 0, sizeof(rac0));
                wpd_vp56_init_range_decoder(&rac0, buf, RAC_BUF_SIZE);
                memcpy(&rac1, &rac0, sizeof(rac0));

                memset(block0, 0, sizeof(block0));
                memset(block1, 0, sizeof(block1));

                res0 = call_ref(
                    &rac0, block0, probs, i, probs[i][zero_nhood], qmul);
                res1 = call_new(
                    &rac1, block1, probs, i, probs[i][zero_nhood], qmul);

                if (res0 != res1 || memcmp(block0, block1, sizeof(block0)) ||
                    memcmp(&rac0, &rac1, sizeof(rac0)))
                    fail();
            }
        }
    }
}

void checkasm_check_vp8coeffs(void) {
    check_decode_block_coeffs();
    report("decode_block_coeffs");
}
