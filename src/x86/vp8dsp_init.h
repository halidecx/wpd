#ifndef WPD_X86_VP8DSP_INIT_H
#define WPD_X86_VP8DSP_INIT_H

#include "src/cpu.h"
#include "src/vp8dsp.h"
#include "src/wpd_codec.h"

void ff_vp8_idct_dc_add_sse2(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add_sse4(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_sse2(uint8_t *dst, int16_t block[4][16],
                               ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_sse2(uint8_t *dst, int16_t block[4][16],
                                ptrdiff_t stride);
void ff_vp8_luma_dc_wht_sse2(int16_t block[4][4][16], int16_t dc[16]);
void ff_vp8_luma_dc_wht_sse4(int16_t block[4][4][16], int16_t dc[16]);
void ff_vp8_idct_add_sse2(uint8_t *dst, int16_t block[16], ptrdiff_t stride);

#define DECLARE_LOOP_FILTER(NAME)                                          \
    void ff_vp8_v_loop_filter_simple_##NAME(                               \
        uint8_t *dst, ptrdiff_t stride, int flim);                         \
    void ff_vp8_h_loop_filter_simple_##NAME(                               \
        uint8_t *dst, ptrdiff_t stride, int flim);                         \
    void ff_vp8_v_loop_filter16y_inner_##NAME(                             \
        uint8_t *dst, ptrdiff_t stride, int e, int i, int hvt);            \
    void ff_vp8_h_loop_filter16y_inner_##NAME(                             \
        uint8_t *dst, ptrdiff_t stride, int e, int i, int hvt);            \
    void ff_vp8_v_loop_filter8uv_inner_##NAME(                             \
        uint8_t *dstU, uint8_t *dstV, ptrdiff_t s, int e, int i, int hvt); \
    void ff_vp8_h_loop_filter8uv_inner_##NAME(                             \
        uint8_t *dstU, uint8_t *dstV, ptrdiff_t s, int e, int i, int hvt); \
    void ff_vp8_v_loop_filter16y_mbedge_##NAME(                            \
        uint8_t *dst, ptrdiff_t stride, int e, int i, int hvt);            \
    void ff_vp8_h_loop_filter16y_mbedge_##NAME(                            \
        uint8_t *dst, ptrdiff_t stride, int e, int i, int hvt);            \
    void ff_vp8_v_loop_filter8uv_mbedge_##NAME(                            \
        uint8_t *dstU, uint8_t *dstV, ptrdiff_t s, int e, int i, int hvt); \
    void ff_vp8_h_loop_filter8uv_mbedge_##NAME(                            \
        uint8_t *dstU, uint8_t *dstV, ptrdiff_t s, int e, int i, int hvt);

DECLARE_LOOP_FILTER(sse2)
DECLARE_LOOP_FILTER(ssse3)

void ff_vp8_v_loop_filter8uv_inner_avx2(uint8_t *dstU, uint8_t *dstV,
                                        ptrdiff_t s, int e, int i, int hvt);

void ff_vp8_h_loop_filter16y_mbedge_sse4(uint8_t *dst, ptrdiff_t stride, int e,
                                         int i, int hvt);
void ff_vp8_h_loop_filter8uv_mbedge_sse4(uint8_t *dstU, uint8_t *dstV,
                                         ptrdiff_t s, int e, int i, int hvt);

VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_sse2,
                            ff_vp8_h_loop_filter_simple_sse2)
VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_ssse3,
                            ff_vp8_h_loop_filter_simple_ssse3)

VP8_V_LOOP_FILTER_SIMPLE_MB(vp8_v_loop_filter_simple_mb_sse2,
                            ff_vp8_v_loop_filter_simple_sse2)
VP8_V_LOOP_FILTER_SIMPLE_MB(vp8_v_loop_filter_simple_mb_ssse3,
                            ff_vp8_v_loop_filter_simple_ssse3)

VP8_H_LOOP_FILTER16Y_MB(vp8_h_loop_filter16y_mb_sse2,
                        ff_vp8_h_loop_filter16y_mbedge_sse2,
                        ff_vp8_h_loop_filter16y_inner_sse2)
VP8_H_LOOP_FILTER16Y_MB(vp8_h_loop_filter16y_mb_ssse3,
                        ff_vp8_h_loop_filter16y_mbedge_ssse3,
                        ff_vp8_h_loop_filter16y_inner_ssse3)
VP8_H_LOOP_FILTER8UV_MB(vp8_h_loop_filter8uv_mb_sse2,
                        ff_vp8_h_loop_filter8uv_mbedge_sse2,
                        ff_vp8_h_loop_filter8uv_inner_sse2)
VP8_H_LOOP_FILTER8UV_MB(vp8_h_loop_filter8uv_mb_ssse3,
                        ff_vp8_h_loop_filter8uv_mbedge_ssse3,
                        ff_vp8_h_loop_filter8uv_inner_ssse3)
VP8_V_LOOP_FILTER16Y_MB(vp8_v_loop_filter16y_mb_sse2,
                        ff_vp8_v_loop_filter16y_mbedge_sse2,
                        ff_vp8_v_loop_filter16y_inner_sse2)
VP8_V_LOOP_FILTER16Y_MB(vp8_v_loop_filter16y_mb_ssse3,
                        ff_vp8_v_loop_filter16y_mbedge_ssse3,
                        ff_vp8_v_loop_filter16y_inner_ssse3)
VP8_V_LOOP_FILTER8UV_MB(vp8_v_loop_filter8uv_mb_sse2,
                        ff_vp8_v_loop_filter8uv_mbedge_sse2,
                        ff_vp8_v_loop_filter8uv_inner_sse2)
VP8_V_LOOP_FILTER8UV_MB(vp8_v_loop_filter8uv_mb_ssse3,
                        ff_vp8_v_loop_filter8uv_mbedge_ssse3,
                        ff_vp8_v_loop_filter8uv_inner_ssse3)

void ff_vp8_v_loop_filter_simple_mb_avx2(uint8_t *dst, ptrdiff_t stride,
                                         int mbedge_lim, int bedge_lim);
void ff_vp8_h_loop_filter_simple_mb_avx2(uint8_t *dst, ptrdiff_t stride,
                                         int mbedge_lim, int bedge_lim);
void ff_vp8_h_loop_filter16y_mb_transpose_avx2(uint8_t *dst, ptrdiff_t stride,
                                               uint8_t *tmp);
void ff_vp8_h_loop_filter16y_mb_itranspose_avx2(uint8_t *dst, ptrdiff_t stride,
                                                const uint8_t *tmp);
void ff_vp8_h_loop_filter8uv_mb_transpose_avx2(uint8_t *dstU, uint8_t *dstV,
                                               ptrdiff_t stride, uint8_t *tmp);
void ff_vp8_h_loop_filter8uv_mb_itranspose_avx2(uint8_t *dstU, uint8_t *dstV,
                                                ptrdiff_t      stride,
                                                const uint8_t *tmp);

static void ff_vp8_h_loop_filter16y_mb_avx2(uint8_t *dst, ptrdiff_t stride,
                                            int mbedge_E, int bedge_E,
                                            int flim_I, int hev_thresh) {
    WPD_DECLARE_ALIGNED(32, uint8_t, tmp[16 * 16]);

    ff_vp8_h_loop_filter16y_mb_transpose_avx2(dst, stride, tmp);
    ff_vp8_v_loop_filter16y_mbedge_ssse3(
        tmp + 4 * 16, 16, mbedge_E, flim_I, hev_thresh);
    ff_vp8_v_loop_filter16y_inner_ssse3(
        tmp + 8 * 16, 16, bedge_E, flim_I, hev_thresh);
    ff_vp8_v_loop_filter16y_inner_ssse3(
        tmp + 12 * 16, 16, bedge_E, flim_I, hev_thresh);
    ff_vp8_h_loop_filter16y_mb_itranspose_avx2(dst, stride, tmp);
    ff_vp8_h_loop_filter16y_inner_ssse3(
        dst + 12, stride, bedge_E, flim_I, hev_thresh);
}

static void ff_vp8_h_loop_filter8uv_mb_avx2(uint8_t *dstU, uint8_t *dstV,
                                            ptrdiff_t stride, int mbedge_E,
                                            int bedge_E, int flim_I,
                                            int hev_thresh) {
    WPD_DECLARE_ALIGNED(32, uint8_t, tmp[16 * 16]);

    ff_vp8_h_loop_filter8uv_mb_transpose_avx2(dstU, dstV, stride, tmp);
    ff_vp8_v_loop_filter16y_mbedge_ssse3(
        tmp + 4 * 16, 16, mbedge_E, flim_I, hev_thresh);
    ff_vp8_v_loop_filter16y_inner_ssse3(
        tmp + 8 * 16, 16, bedge_E, flim_I, hev_thresh);
    ff_vp8_h_loop_filter8uv_mb_itranspose_avx2(dstU, dstV, stride, tmp);
}

static wpd_always_inline void ff_vp8dsp_init_x86(VP8DSPContext *c) {
    const unsigned flags = wpd_get_cpu_flags();

    if (flags & WPD_X86_CPU_FLAG_SSE2) {
        c->vp8_idct_add       = ff_vp8_idct_add_sse2;
        c->vp8_luma_dc_wht    = ff_vp8_luma_dc_wht_sse2;
        c->vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_sse2;

        c->vp8_v_loop_filter_simple    = ff_vp8_v_loop_filter_simple_sse2;
        c->vp8_v_loop_filter_simple_mb = vp8_v_loop_filter_simple_mb_sse2;

        c->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16y_inner_sse2;
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_sse2;

        c->vp8_v_loop_filter16y = ff_vp8_v_loop_filter16y_mbedge_sse2;
        c->vp8_v_loop_filter8uv = ff_vp8_v_loop_filter8uv_mbedge_sse2;

        c->vp8_idct_dc_add   = ff_vp8_idct_dc_add_sse2;
        c->vp8_idct_dc_add4y = ff_vp8_idct_dc_add4y_sse2;

        c->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter_simple_sse2;
        c->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_sse2;

        c->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16y_inner_sse2;
        c->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_sse2;

        c->vp8_h_loop_filter16y    = ff_vp8_h_loop_filter16y_mbedge_sse2;
        c->vp8_h_loop_filter8uv    = ff_vp8_h_loop_filter8uv_mbedge_sse2;
        c->vp8_h_loop_filter16y_mb = vp8_h_loop_filter16y_mb_sse2;
        c->vp8_h_loop_filter8uv_mb = vp8_h_loop_filter8uv_mb_sse2;
        c->vp8_v_loop_filter16y_mb = vp8_v_loop_filter16y_mb_sse2;
        c->vp8_v_loop_filter8uv_mb = vp8_v_loop_filter8uv_mb_sse2;
    }

    if (flags & WPD_X86_CPU_FLAG_SSSE3) {
        c->vp8_v_loop_filter_simple    = ff_vp8_v_loop_filter_simple_ssse3;
        c->vp8_v_loop_filter_simple_mb = vp8_v_loop_filter_simple_mb_ssse3;
        c->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter_simple_ssse3;
        c->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_ssse3;

        c->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16y_inner_ssse3;
        c->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16y_inner_ssse3;
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_ssse3;
        c->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_ssse3;

        c->vp8_v_loop_filter16y    = ff_vp8_v_loop_filter16y_mbedge_ssse3;
        c->vp8_h_loop_filter16y    = ff_vp8_h_loop_filter16y_mbedge_ssse3;
        c->vp8_v_loop_filter8uv    = ff_vp8_v_loop_filter8uv_mbedge_ssse3;
        c->vp8_h_loop_filter8uv    = ff_vp8_h_loop_filter8uv_mbedge_ssse3;
        c->vp8_h_loop_filter16y_mb = vp8_h_loop_filter16y_mb_ssse3;
        c->vp8_h_loop_filter8uv_mb = vp8_h_loop_filter8uv_mb_ssse3;
        c->vp8_v_loop_filter16y_mb = vp8_v_loop_filter16y_mb_ssse3;
        c->vp8_v_loop_filter8uv_mb = vp8_v_loop_filter8uv_mb_ssse3;
    }

    if (flags & WPD_X86_CPU_FLAG_SSE41) {
        c->vp8_idct_dc_add = ff_vp8_idct_dc_add_sse4;
        c->vp8_luma_dc_wht = ff_vp8_luma_dc_wht_sse4;
    }

    if (flags & WPD_X86_CPU_FLAG_AVX2) {
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_avx2;

        c->vp8_v_loop_filter_simple_mb = ff_vp8_v_loop_filter_simple_mb_avx2;
        c->vp8_h_loop_filter_simple_mb = ff_vp8_h_loop_filter_simple_mb_avx2;
        c->vp8_h_loop_filter16y_mb     = ff_vp8_h_loop_filter16y_mb_avx2;
        c->vp8_h_loop_filter8uv_mb     = ff_vp8_h_loop_filter8uv_mb_avx2;
    }
}

#endif
