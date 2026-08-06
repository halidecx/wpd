
#include <stdint.h>
#include "vp8dsp.h"

void ff_vp8_luma_dc_wht_neon(WpdDctElem block[4][4][16], WpdDctElem dc[16]);

void ff_vp8_idct_add_neon(uint8_t *dst, WpdDctElem block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add_neon(uint8_t *dst, WpdDctElem block[16],
                             ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_neon(uint8_t *dst, WpdDctElem block[4][16],
                               ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_neon(uint8_t *dst, WpdDctElem block[4][16],
                                ptrdiff_t stride);

VP8_LF(neon);

wpd_cold void ff_vp8dsp_init_neon(VP8DSPContext *dsp) {
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
