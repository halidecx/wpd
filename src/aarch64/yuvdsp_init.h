#ifndef WPD_AARCH64_YUVDSP_INIT_H
#define WPD_AARCH64_YUVDSP_INIT_H

#include "src/cpu.h"
#include "src/yuvdsp.h"

#define UPSAMPLE_BLOCK(name)                                        \
    void ff_upsample_block_##name##_neon(const uint8_t *top_y,      \
                                         const uint8_t *bottom_y,   \
                                         const uint8_t *top_u,      \
                                         const uint8_t *top_v,      \
                                         const uint8_t *cur_u,      \
                                         const uint8_t *cur_v,      \
                                         uint8_t       *top_dst,    \
                                         uint8_t       *bottom_dst, \
                                         int            blocks);

UPSAMPLE_BLOCK(argb)
UPSAMPLE_BLOCK(rgba)
UPSAMPLE_BLOCK(bgra)
UPSAMPLE_BLOCK(rgb)
UPSAMPLE_BLOCK(bgr)
#undef UPSAMPLE_BLOCK

void ff_dispatch_alpha_neon(uint8_t *dst, const uint8_t *src, int num_pixels);

#define PACK_ROW(name) \
    void ff_pack_##name##_neon(uint8_t *dst, const uint8_t *src, int n);

PACK_ROW(rgba)
PACK_ROW(bgra)
PACK_ROW(rgb)
PACK_ROW(bgr)
#undef PACK_ROW

void ff_premultiply_row_neon(uint8_t *rgba, int alpha_first, int num_pixels);

static wpd_always_inline void wpd_yuv_dsp_init_aarch64(WPDYUVDSP *dsp) {
    if (!(wpd_get_cpu_flags() & WPD_ARM_CPU_FLAG_NEON))
        return;
    dsp->upsample_block[WPD_LAYOUT_ARGB] = ff_upsample_block_argb_neon;
    dsp->upsample_block[WPD_LAYOUT_RGBA] = ff_upsample_block_rgba_neon;
    dsp->upsample_block[WPD_LAYOUT_BGRA] = ff_upsample_block_bgra_neon;
    dsp->upsample_block[WPD_LAYOUT_RGB]  = ff_upsample_block_rgb_neon;
    dsp->upsample_block[WPD_LAYOUT_BGR]  = ff_upsample_block_bgr_neon;
    dsp->dispatch_alpha                  = ff_dispatch_alpha_neon;
    dsp->pack_rgba                       = ff_pack_rgba_neon;
    dsp->pack_bgra                       = ff_pack_bgra_neon;
    dsp->pack_rgb                        = ff_pack_rgb_neon;
    dsp->pack_bgr                        = ff_pack_bgr_neon;
    dsp->premultiply_row                 = ff_premultiply_row_neon;
}

#endif
