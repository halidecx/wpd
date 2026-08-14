#ifndef WPD_AARCH64_YUVDSP_INIT_H
#define WPD_AARCH64_YUVDSP_INIT_H

#include "config.h"

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

void ff_dispatch_alpha_first_neon(uint8_t *dst, const uint8_t *src,
                                  int num_pixels);
void ff_dispatch_alpha_last_neon(uint8_t *dst, const uint8_t *src,
                                 int num_pixels);

#define PACK_ROW(name) \
    void ff_pack_##name##_neon(uint8_t *dst, const uint8_t *src, int n);

PACK_ROW(rgba)
PACK_ROW(bgra)
PACK_ROW(rgb)
PACK_ROW(bgr)
PACK_ROW(rgb565)
PACK_ROW(rgba4444)
PACK_ROW(bgr565)
PACK_ROW(bgra4444)
#undef PACK_ROW

void ff_premultiply_row_neon(uint8_t *rgba, int alpha_first, int num_pixels);
void ff_premultiply_row_4444_neon(uint8_t *rgba4444, int num_pixels);
void ff_premultiply_row_4444_swap_neon(uint8_t *bgra4444, int num_pixels);
void ff_argb_to_y_neon(uint8_t *y, const uint8_t *argb, int num_pixels);
void ff_argb_to_yuv444_neon(uint8_t *y, uint8_t *u, uint8_t *v,
                            const uint8_t *argb, int num_pixels);
void ff_argb_to_uv_neon(uint8_t *u, uint8_t *v, const uint8_t *argb,
                        ptrdiff_t argb_stride, int num_pixels,
                        int weight_alpha);

#if HAVE_DOTPROD
void ff_argb_to_yuv444_neon_dotprod(uint8_t *y, uint8_t *u, uint8_t *v,
                                    const uint8_t *argb, int num_pixels);
#if HAVE_I8MM
void ff_argb_to_yuv444_neon_i8mm(uint8_t *y, uint8_t *u, uint8_t *v,
                                 const uint8_t *argb, int num_pixels);
#endif
#endif

static wpd_always_inline void wpd_yuv_dsp_init_aarch64(WPDYUVDSP *dsp) {
    const unsigned flags = wpd_get_cpu_flags();

    if (!(flags & WPD_ARM_CPU_FLAG_NEON))
        return;
    dsp->upsample_block[WPD_LAYOUT_ARGB] = ff_upsample_block_argb_neon;
    dsp->upsample_block[WPD_LAYOUT_RGBA] = ff_upsample_block_rgba_neon;
    dsp->upsample_block[WPD_LAYOUT_BGRA] = ff_upsample_block_bgra_neon;
    dsp->upsample_block[WPD_LAYOUT_RGB]  = ff_upsample_block_rgb_neon;
    dsp->upsample_block[WPD_LAYOUT_BGR]  = ff_upsample_block_bgr_neon;
    dsp->dispatch_alpha_first            = ff_dispatch_alpha_first_neon;
    dsp->dispatch_alpha_last             = ff_dispatch_alpha_last_neon;
    dsp->pack_rgba                       = ff_pack_rgba_neon;
    dsp->pack_bgra                       = ff_pack_bgra_neon;
    dsp->pack_rgb                        = ff_pack_rgb_neon;
    dsp->pack_bgr                        = ff_pack_bgr_neon;
    dsp->pack_rgb565                     = ff_pack_rgb565_neon;
    dsp->pack_rgba4444                   = ff_pack_rgba4444_neon;
    dsp->pack_bgr565                     = ff_pack_bgr565_neon;
    dsp->pack_bgra4444                   = ff_pack_bgra4444_neon;
    dsp->premultiply_row                 = ff_premultiply_row_neon;
    dsp->premultiply_row_4444            = ff_premultiply_row_4444_neon;
    dsp->premultiply_row_4444_swap       = ff_premultiply_row_4444_swap_neon;
    dsp->argb_to_y                       = ff_argb_to_y_neon;
    dsp->argb_to_yuv444                  = ff_argb_to_yuv444_neon;
    dsp->argb_to_uv                      = ff_argb_to_uv_neon;

#if HAVE_DOTPROD
    if (flags & WPD_ARM_CPU_FLAG_DOTPROD)
        dsp->argb_to_yuv444 = ff_argb_to_yuv444_neon_dotprod;
#if HAVE_I8MM
    if (flags & WPD_ARM_CPU_FLAG_I8MM)
        dsp->argb_to_yuv444 = ff_argb_to_yuv444_neon_i8mm;
#endif
#endif
}

#endif
