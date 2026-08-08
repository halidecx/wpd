#ifndef WPD_ARM_VP8DSP_INIT_H
#define WPD_ARM_VP8DSP_INIT_H

#include "src/arm/vp8dsp.h"
#include "src/cpu.h"
#include "src/vp8dsp.h"
#include "src/wpd_codec.h"

#if WPD_ARM_ARMV6_ASM
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

VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_armv6,
                            ff_vp8_h_loop_filter16_simple_armv6)
VP8_V_LOOP_FILTER_SIMPLE_MB(vp8_v_loop_filter_simple_mb_armv6,
                            ff_vp8_v_loop_filter16_simple_armv6)

static wpd_always_inline void ff_vp8dsp_init_armv6(VP8DSPContext *dsp) {
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

    dsp->vp8_v_loop_filter_simple    = ff_vp8_v_loop_filter16_simple_armv6;
    dsp->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter16_simple_armv6;
    dsp->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_armv6;
    dsp->vp8_v_loop_filter_simple_mb = vp8_v_loop_filter_simple_mb_armv6;
}
#endif

void ff_vp8_luma_dc_wht_neon(WpdDctElem block[4][4][16], WpdDctElem dc[16]);

void ff_vp8_idct_add_neon(uint8_t *dst, WpdDctElem block[16], ptrdiff_t stride);
void ff_vp8_idct_dc_add_neon(uint8_t *dst, WpdDctElem block[16],
                             ptrdiff_t stride);
void ff_vp8_idct_dc_add4y_neon(uint8_t *dst, WpdDctElem block[4][16],
                               ptrdiff_t stride);
void ff_vp8_idct_dc_add4uv_neon(uint8_t *dst, WpdDctElem block[4][16],
                                ptrdiff_t stride);

VP8_LF(neon);

VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_neon,
                            ff_vp8_h_loop_filter16_simple_neon)
VP8_V_LOOP_FILTER_SIMPLE_MB(vp8_v_loop_filter_simple_mb_neon,
                            ff_vp8_v_loop_filter16_simple_neon)

static wpd_always_inline void ff_vp8dsp_init_neon(VP8DSPContext *dsp) {
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

    dsp->vp8_v_loop_filter_simple    = ff_vp8_v_loop_filter16_simple_neon;
    dsp->vp8_h_loop_filter_simple    = ff_vp8_h_loop_filter16_simple_neon;
    dsp->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_neon;
    dsp->vp8_v_loop_filter_simple_mb = vp8_v_loop_filter_simple_mb_neon;
}

static wpd_always_inline void ff_vp8dsp_init_arm(VP8DSPContext *dsp) {
    const unsigned flags = wpd_get_cpu_flags();

#if WPD_ARM_ARMV6_ASM
    if (flags & WPD_ARM_CPU_FLAG_ARMV6)
        ff_vp8dsp_init_armv6(dsp);
#endif
    if (flags & WPD_ARM_CPU_FLAG_NEON)
        ff_vp8dsp_init_neon(dsp);
}

#endif
