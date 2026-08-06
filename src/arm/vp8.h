
#ifndef WPD_ARM_VP8_H
#define WPD_ARM_VP8_H

#include <stddef.h>

#include "wpd_codec.h"

#if defined(__arm__)
#define decode_block_coeffs_internal ff_decode_block_coeffs_armv6
int ff_decode_block_coeffs_armv6(VP56RangeCoder *rc, WpdDctElem block[16],
                                 uint8_t probs[16][3][NUM_DCT_TOKENS - 1],
                                 int i, uint8_t *token_prob, int16_t qmul[2]);

_Static_assert(offsetof(VP56RangeCoder, high) == 0,
               "ff_decode_block_coeffs_armv6 expects high at offset 0");
_Static_assert(offsetof(VP56RangeCoder, bits) == 4,
               "ff_decode_block_coeffs_armv6 expects bits at offset 4");
_Static_assert(offsetof(VP56RangeCoder, buffer) == 8,
               "ff_decode_block_coeffs_armv6 expects buffer at offset 8");
_Static_assert(offsetof(VP56RangeCoder, end) == 12,
               "ff_decode_block_coeffs_armv6 expects end at offset 12");
_Static_assert(offsetof(VP56RangeCoder, code_word) == 16,
               "ff_decode_block_coeffs_armv6 expects code_word at offset 16");
#endif

#endif
