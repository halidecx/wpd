/*
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * FFmpeg is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with FFmpeg; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA
 */

#ifndef WPD_ARM_VP8_H
#define WPD_ARM_VP8_H

#include <stddef.h>

#include "wpd_codec.h"

#if defined(__arm__)
#define decode_block_coeffs_internal ff_decode_block_coeffs_armv6
int ff_decode_block_coeffs_armv6(VP56RangeCoder *rc, WpdDctElem block[16],
                                 uint8_t probs[16][3][NUM_DCT_TOKENS - 1],
                                 int i, uint8_t *token_prob, int16_t qmul[2]);

/*
 * The asm reads the coder state directly: `ldm r0, {r5-r7}` pulls high, bits
 * and buffer as three consecutive words, and code_word comes from a hardcoded
 * [r0, #16]. Reordering VP56RangeCoder would silently corrupt decoding on
 * arm32 only, so pin the layout here rather than in a comment.
 */
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

#endif /* WPD_ARM_VP8_H */
