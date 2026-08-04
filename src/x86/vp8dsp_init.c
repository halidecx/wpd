/*
 * VP8 DSP functions x86-optimized
 * Copyright (c) 2010 Ronald S. Bultje <rsbultje@gmail.com>
 * Copyright (c) 2010 Fiona Glaser <fiona@x264.com>
 *
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

#include "wpd_codec.h"
#include "vp8dsp.h"

void ff_vp8_idct_dc_add_sse2(uint8_t *dst, int16_t block[16],
                             ptrdiff_t stride);
void ff_vp8_idct_dc_add_sse4(uint8_t *dst, int16_t block[16],
                             ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_sse2(uint8_t *dst, int16_t block[4][16],
                               ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_mmx(uint8_t *dst, int16_t block[2][16],
                               ptrdiff_t stride);
void ff_vp8_luma_dc_wht_sse(int16_t block[4][4][16], int16_t dc[16]);
void ff_vp8_idct_add_sse(uint8_t *dst, int16_t block[16], ptrdiff_t stride);

#define DECLARE_LOOP_FILTER(NAME)                                       \
void ff_vp8_v_loop_filter_simple_ ## NAME(uint8_t *dst,                 \
                                          ptrdiff_t stride,             \
                                          int flim);                    \
void ff_vp8_h_loop_filter_simple_ ## NAME(uint8_t *dst,                 \
                                          ptrdiff_t stride,             \
                                          int flim);                    \
void ff_vp8_v_loop_filter16y_inner_ ## NAME (uint8_t *dst,              \
                                             ptrdiff_t stride,          \
                                             int e, int i, int hvt);    \
void ff_vp8_h_loop_filter16y_inner_ ## NAME (uint8_t *dst,              \
                                             ptrdiff_t stride,          \
                                             int e, int i, int hvt);    \
void ff_vp8_v_loop_filter8uv_inner_ ## NAME (uint8_t *dstU,             \
                                             uint8_t *dstV,             \
                                             ptrdiff_t s,               \
                                             int e, int i, int hvt);    \
void ff_vp8_h_loop_filter8uv_inner_ ## NAME (uint8_t *dstU,             \
                                             uint8_t *dstV,             \
                                             ptrdiff_t s,               \
                                             int e, int i, int hvt);    \
void ff_vp8_v_loop_filter16y_mbedge_ ## NAME(uint8_t *dst,              \
                                             ptrdiff_t stride,          \
                                             int e, int i, int hvt);    \
void ff_vp8_h_loop_filter16y_mbedge_ ## NAME(uint8_t *dst,              \
                                             ptrdiff_t stride,          \
                                             int e, int i, int hvt);    \
void ff_vp8_v_loop_filter8uv_mbedge_ ## NAME(uint8_t *dstU,             \
                                             uint8_t *dstV,             \
                                             ptrdiff_t s,               \
                                             int e, int i, int hvt);    \
void ff_vp8_h_loop_filter8uv_mbedge_ ## NAME(uint8_t *dstU,             \
                                             uint8_t *dstV,             \
                                             ptrdiff_t s,               \
                                             int e, int i, int hvt);

DECLARE_LOOP_FILTER(sse2)
DECLARE_LOOP_FILTER(ssse3)

/* Only the horizontal variants have an SSE4 implementation. */
void ff_vp8_h_loop_filter_simple_sse4(uint8_t *dst, ptrdiff_t stride, int flim);
void ff_vp8_h_loop_filter16y_mbedge_sse4(uint8_t *dst, ptrdiff_t stride,
                                         int e, int i, int hvt);
void ff_vp8_h_loop_filter8uv_mbedge_sse4(uint8_t *dstU, uint8_t *dstV,
                                         ptrdiff_t s, int e, int i, int hvt);

wpd_cold void ff_vp8dsp_init_x86(VP8DSPContext *c)
{
    int cpu_flags = wpd_get_cpu_flags();

    if (WPD_CPU_HAS_MMX(cpu_flags)) {
        c->vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_mmx;
    }

    if (WPD_CPU_HAS_SSE(cpu_flags)) {
        c->vp8_idct_add                         = ff_vp8_idct_add_sse;
        c->vp8_luma_dc_wht                      = ff_vp8_luma_dc_wht_sse;
    }

    if (WPD_CPU_HAS_SSE2_SLOW(cpu_flags)) {
        c->vp8_v_loop_filter_simple = ff_vp8_v_loop_filter_simple_sse2;

        c->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16y_inner_sse2;
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_sse2;

        c->vp8_v_loop_filter16y       = ff_vp8_v_loop_filter16y_mbedge_sse2;
        c->vp8_v_loop_filter8uv       = ff_vp8_v_loop_filter8uv_mbedge_sse2;
    }

    if (WPD_CPU_HAS_SSE2(cpu_flags)) {
        c->vp8_idct_dc_add            = ff_vp8_idct_dc_add_sse2;
        c->vp8_idct_dc_add4y          = ff_vp8_idct_dc_add4y_sse2;

        c->vp8_h_loop_filter_simple   = ff_vp8_h_loop_filter_simple_sse2;

        c->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16y_inner_sse2;
        c->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_sse2;

        c->vp8_h_loop_filter16y       = ff_vp8_h_loop_filter16y_mbedge_sse2;
        c->vp8_h_loop_filter8uv       = ff_vp8_h_loop_filter8uv_mbedge_sse2;
    }

    if (WPD_CPU_HAS_SSSE3(cpu_flags)) {
        c->vp8_v_loop_filter_simple = ff_vp8_v_loop_filter_simple_ssse3;
        c->vp8_h_loop_filter_simple = ff_vp8_h_loop_filter_simple_ssse3;

        c->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16y_inner_ssse3;
        c->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16y_inner_ssse3;
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_ssse3;
        c->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_ssse3;

        c->vp8_v_loop_filter16y       = ff_vp8_v_loop_filter16y_mbedge_ssse3;
        c->vp8_h_loop_filter16y       = ff_vp8_h_loop_filter16y_mbedge_ssse3;
        c->vp8_v_loop_filter8uv       = ff_vp8_v_loop_filter8uv_mbedge_ssse3;
        c->vp8_h_loop_filter8uv       = ff_vp8_h_loop_filter8uv_mbedge_ssse3;
    }

    if (WPD_CPU_HAS_SSE4(cpu_flags)) {
        c->vp8_idct_dc_add            = ff_vp8_idct_dc_add_sse4;

        c->vp8_h_loop_filter_simple   = ff_vp8_h_loop_filter_simple_sse4;
        c->vp8_h_loop_filter16y       = ff_vp8_h_loop_filter16y_mbedge_sse4;
        c->vp8_h_loop_filter8uv       = ff_vp8_h_loop_filter8uv_mbedge_sse4;
    }
}
