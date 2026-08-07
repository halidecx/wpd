#ifndef WPD_AARCH64_VP8DSP_INIT_H
#define WPD_AARCH64_VP8DSP_INIT_H

#include "src/aarch64/vp8dsp.h"
#include "src/cpu.h"
#include "src/vp8dsp.h"
#include "src/wpd_codec.h"

void ff_vp8_luma_dc_wht_neon(int16_t block[4][4][16], int16_t dc[16]);

void ff_vp8_idct_add_neon(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add_neon(uint8_t *dst, int16_t block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_neon(uint8_t *dst, int16_t block[4][16],
                               ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_neon(uint8_t *dst, int16_t block[4][16],
                                ptrdiff_t stride);

VP8_LF(neon);

static wpd_always_inline void ff_vp8dsp_init_aarch64(VP8DSPContext *dsp) {
    if (!(wpd_get_cpu_flags() & WPD_ARM_CPU_FLAG_NEON))
        return;
    dsp->vp8_luma_dc_wht = ff_vp8_luma_dc_wht_neon;

    dsp->vp8_idct_add       = ff_vp8_idct_add_neon;
    dsp->vp8_idct_dc_add    = ff_vp8_idct_dc_add_neon;
    dsp->vp8_idct_dc_add4y  = ff_vp8_idct_dc_add4y_neon;
    dsp->vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_neon;

    dsp->vp8_v_loop_filter16y = ff_vp8_v_loop_filter16_neon;
    dsp->vp8_h_loop_filter16y = ff_vp8_h_loop_filter16_neon;
    dsp->vp8_v_loop_filter8uv = ff_vp8_v_loop_filter8uv_neon;
    dsp->vp8_h_loop_filter8uv = ff_vp8_h_loop_filter8uv_neon;

    dsp->vp8_v_loop_filter16y_inner = ff_vp8_v_loop_filter16_inner_neon;
    dsp->vp8_h_loop_filter16y_inner = ff_vp8_h_loop_filter16_inner_neon;
    dsp->vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_neon;
    dsp->vp8_h_loop_filter8uv_inner = ff_vp8_h_loop_filter8uv_inner_neon;

    dsp->vp8_v_loop_filter_simple = ff_vp8_v_loop_filter16_simple_neon;
    dsp->vp8_h_loop_filter_simple = ff_vp8_h_loop_filter16_simple_neon;
}

#endif
