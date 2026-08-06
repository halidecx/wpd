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

#ifndef WPD_ARM_VP8DSP_H
#define WPD_ARM_VP8DSP_H

#include "../vp8dsp.h"

void ff_vp8dsp_init_armv6(VP8DSPContext *dsp);
void ff_vp8dsp_init_neon(VP8DSPContext *dsp);

#define VP8_LF_Y(hv, inner, opt)                                      \
    void ff_vp8_##hv##_loop_filter16##inner##_##opt(uint8_t  *dst,    \
                                                    ptrdiff_t stride, \
                                                    int       flim_E, \
                                                    int       flim_I, \
                                                    int       hev_thresh)

#define VP8_LF_UV(hv, inner, opt)                                      \
    void ff_vp8_##hv##_loop_filter8uv##inner##_##opt(uint8_t  *dstU,   \
                                                     uint8_t  *dstV,   \
                                                     ptrdiff_t stride, \
                                                     int       flim_E, \
                                                     int       flim_I, \
                                                     int       hev_thresh)

#define VP8_LF_SIMPLE(hv, opt)                     \
    void ff_vp8_##hv##_loop_filter16_simple_##opt( \
        uint8_t *dst, ptrdiff_t stride, int flim)

#define VP8_LF_HV(inner, opt) \
    VP8_LF_Y(h, inner, opt);  \
    VP8_LF_Y(v, inner, opt);  \
    VP8_LF_UV(h, inner, opt); \
    VP8_LF_UV(v, inner, opt)

#define VP8_LF(opt)         \
    VP8_LF_HV(, opt);       \
    VP8_LF_HV(_inner, opt); \
    VP8_LF_SIMPLE(h, opt);  \
    VP8_LF_SIMPLE(v, opt)

#endif /* WPD_ARM_VP8DSP_H */
