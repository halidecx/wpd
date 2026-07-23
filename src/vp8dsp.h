#ifndef WPD_VP8DSP_H
#define WPD_VP8DSP_H

#include "wpd_codec.h"

typedef void (*vp8_mc_func)(uint8_t *dst/*align 8*/, ptrdiff_t dstStride,
                            const uint8_t *src/*align 1*/, ptrdiff_t srcStride,
                            int h, int x, int y);

typedef struct VP8DSPContext {
    void (*vp8_luma_dc_wht)(WpdDctElem block[4][4][16], WpdDctElem dc[16]);
    void (*vp8_luma_dc_wht_dc)(WpdDctElem block[4][4][16], WpdDctElem dc[16]);
    void (*vp8_idct_add)(uint8_t *dst, WpdDctElem block[16], ptrdiff_t stride);
    void (*vp8_idct_dc_add)(uint8_t *dst, WpdDctElem block[16], ptrdiff_t stride);
    void (*vp8_idct_dc_add4y)(uint8_t *dst, WpdDctElem block[4][16],
                              ptrdiff_t stride);
    void (*vp8_idct_dc_add4uv)(uint8_t *dst, WpdDctElem block[4][16],
                               ptrdiff_t stride);

    // loop filter applied to edges between macroblocks
    void (*vp8_v_loop_filter16y)(uint8_t *dst, ptrdiff_t stride,
                                 int flim_E, int flim_I, int hev_thresh);
    void (*vp8_h_loop_filter16y)(uint8_t *dst, ptrdiff_t stride,
                                 int flim_E, int flim_I, int hev_thresh);
    void (*vp8_v_loop_filter8uv)(uint8_t *dstU, uint8_t *dstV, ptrdiff_t stride,
                                 int flim_E, int flim_I, int hev_thresh);
    void (*vp8_h_loop_filter8uv)(uint8_t *dstU, uint8_t *dstV, ptrdiff_t stride,
                                 int flim_E, int flim_I, int hev_thresh);

    // loop filter applied to inner macroblock edges
    void (*vp8_v_loop_filter16y_inner)(uint8_t *dst, ptrdiff_t stride,
                                       int flim_E, int flim_I, int hev_thresh);
    void (*vp8_h_loop_filter16y_inner)(uint8_t *dst, ptrdiff_t stride,
                                       int flim_E, int flim_I, int hev_thresh);
    void (*vp8_v_loop_filter8uv_inner)(uint8_t *dstU, uint8_t *dstV,
                                       ptrdiff_t stride,
                                       int flim_E, int flim_I, int hev_thresh);
    void (*vp8_h_loop_filter8uv_inner)(uint8_t *dstU, uint8_t *dstV,
                                       ptrdiff_t stride,
                                       int flim_E, int flim_I, int hev_thresh);

    void (*vp8_v_loop_filter_simple)(uint8_t *dst, ptrdiff_t stride, int flim);
    void (*vp8_h_loop_filter_simple)(uint8_t *dst, ptrdiff_t stride, int flim);

    /**
     * first dimension: width>>3, height is assumed equal to width
     * second dimension: 0 if no vertical interpolation is needed;
     *                   1 4-tap vertical interpolation filter (my & 1)
     *                   2 6-tap vertical interpolation filter (!(my & 1))
     * third dimension: same as second dimension, for horizontal interpolation
     * so something like put_vp8_epel_pixels_tab[width>>3][2*!!my-(my&1)][2*!!mx-(mx&1)](..., mx, my)
     */
    vp8_mc_func put_vp8_epel_pixels_tab[3][3][3];
    vp8_mc_func put_vp8_bilinear_pixels_tab[3][3][3];
} VP8DSPContext;

void ff_put_vp8_pixels16_c(uint8_t *dst, uint8_t *src, ptrdiff_t stride,
                           int h, int x, int y);
void ff_put_vp8_pixels8_c(uint8_t *dst, uint8_t *src, ptrdiff_t stride,
                          int h, int x, int y);
void ff_put_vp8_pixels4_c(uint8_t *dst, uint8_t *src, ptrdiff_t stride,
                          int h, int x, int y);

void ff_vp8dsp_init(VP8DSPContext *c);
void ff_vp78dsp_init_x86(VP8DSPContext *c);
void ff_vp8dsp_init_x86(VP8DSPContext *c);
void ff_vp8dsp_init_altivec(VP8DSPContext *c);
void ff_vp8dsp_init_arm(VP8DSPContext *c);
void ff_vp8dsp_init_aarch64(VP8DSPContext *c);

#endif /* WPD_VP8DSP_H */
