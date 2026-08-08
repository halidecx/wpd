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

void ff_vp8_h_loop_filter_simple_sse4(uint8_t *dst, ptrdiff_t stride, int flim);
void ff_vp8_h_loop_filter16y_mbedge_sse4(uint8_t *dst, ptrdiff_t stride, int e,
                                         int i, int hvt);
void ff_vp8_h_loop_filter8uv_mbedge_sse4(uint8_t *dstU, uint8_t *dstV,
                                         ptrdiff_t s, int e, int i, int hvt);

VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_sse2,
                            ff_vp8_h_loop_filter_simple_sse2)
VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_ssse3,
                            ff_vp8_h_loop_filter_simple_ssse3)
VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_sse4,
                            ff_vp8_h_loop_filter_simple_sse4)

static wpd_always_inline void ff_vp8dsp_init_x86(VP8DSPContext *c) {
    const unsigned flags = wpd_get_cpu_flags();

    if (flags & WPD_X86_CPU_FLAG_SSE2) {
        c->vp8_idct_add       = ff_vp8_idct_add_sse2;
        c->vp8_luma_dc_wht    = ff_vp8_luma_dc_wht_sse2;
        c->vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_sse2;

        c->vp8_v_loop_filter_simple = ff_vp8_v_loop_filter_simple_sse2;

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

        c->vp8_h_loop_filter16y = ff_vp8_h_loop_filter16y_mbedge_sse2;
        c->vp8_h_loop_filter8uv = ff_vp8_h_loop_filter8uv_mbedge_sse2;
    }

    if (flags & WPD_X86_CPU_FLAG_SSSE3) {
        c->vp8_v_loop_filter_simple    = ff_vp8_v_loop_filter_simple_ssse3;
        c->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter_simple_ssse3;
        c->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_ssse3;

        c->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16y_inner_ssse3;
        c->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16y_inner_ssse3;
        c->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_ssse3;
        c->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_ssse3;

        c->vp8_v_loop_filter16y = ff_vp8_v_loop_filter16y_mbedge_ssse3;
        c->vp8_h_loop_filter16y = ff_vp8_h_loop_filter16y_mbedge_ssse3;
        c->vp8_v_loop_filter8uv = ff_vp8_v_loop_filter8uv_mbedge_ssse3;
        c->vp8_h_loop_filter8uv = ff_vp8_h_loop_filter8uv_mbedge_ssse3;
    }

    if (flags & WPD_X86_CPU_FLAG_SSE41) {
        c->vp8_idct_dc_add = ff_vp8_idct_dc_add_sse4;
        c->vp8_luma_dc_wht = ff_vp8_luma_dc_wht_sse4;

        c->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter_simple_sse4;
        c->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_sse4;
        c->vp8_h_loop_filter16y        = ff_vp8_h_loop_filter16y_mbedge_sse4;
        c->vp8_h_loop_filter8uv        = ff_vp8_h_loop_filter8uv_mbedge_sse4;
    }
}

#endif
