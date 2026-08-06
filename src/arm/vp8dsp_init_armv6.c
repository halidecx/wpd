/*
 * This file is part of Libav.
 *
 * Libav is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * Libav is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with Libav; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA
 */

#include <stdint.h>
#include "vp8dsp.h"

void ff_vp8_luma_dc_wht_armv6(WpdDctElem block[4][4][16], WpdDctElem dc[16]);
void ff_vp8_luma_dc_wht_dc_armv6(WpdDctElem block[4][4][16], WpdDctElem dc[16]);

void ff_vp8_idct_add_armv6(uint8_t *dst, WpdDctElem block[16],
                           ptrdiff_t stride);
void ff_vp8_idct_dc_add_armv6(uint8_t *dst, WpdDctElem block[16],
                              ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_armv6(uint8_t *dst, WpdDctElem block[4][16],
                                ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_armv6(uint8_t *dst, WpdDctElem block[4][16],
                                 ptrdiff_t stride);

VP8_LF(armv6);

wpd_cold void ff_vp8dsp_init_armv6(VP8DSPContext *dsp) {
    dsp->vp8_luma_dc_wht    = ff_vp8_luma_dc_wht_armv6;
    dsp->vp8_luma_dc_wht_dc = ff_vp8_luma_dc_wht_dc_armv6;

    dsp->vp8_idct_add       = ff_vp8_idct_add_armv6;
    dsp->vp8_idct_dc_add    = ff_vp8_idct_dc_add_armv6;
    dsp->vp8_idct_dc_add4y  = ff_vp8_idct_dc_add4y_armv6;
    dsp->vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_armv6;

    dsp->vp8_v_loop_filter16y = ff_vp8_v_loop_filter16_armv6;
    dsp->vp8_h_loop_filter16y = ff_vp8_h_loop_filter16_armv6;
    dsp->vp8_v_loop_filter8uv = ff_vp8_v_loop_filter8uv_armv6;
    dsp->vp8_h_loop_filter8uv = ff_vp8_h_loop_filter8uv_armv6;

    dsp->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16_inner_armv6;
    dsp->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16_inner_armv6;
    dsp->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_armv6;
    dsp->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_armv6;

    dsp->vp8_v_loop_filter_simple = ff_vp8_v_loop_filter16_simple_armv6;
    dsp->vp8_h_loop_filter_simple = ff_vp8_h_loop_filter16_simple_armv6;
}
