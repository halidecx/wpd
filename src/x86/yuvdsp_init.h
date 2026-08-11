#ifndef WPD_X86_YUVDSP_INIT_H
#define WPD_X86_YUVDSP_INIT_H

#include "src/cpu.h"
#include "src/yuvdsp.h"

#define UPSAMPLE_ARGB_BLOCK(cpu)                            \
    void ff_upsample_block_##cpu(const uint8_t *top_y,      \
                                 const uint8_t *bottom_y,   \
                                 const uint8_t *top_u,      \
                                 const uint8_t *top_v,      \
                                 const uint8_t *cur_u,      \
                                 const uint8_t *cur_v,      \
                                 uint8_t       *top_dst,    \
                                 uint8_t       *bottom_dst, \
                                 int            num_blocks);

#if WPD_ARCH_X86_64
UPSAMPLE_ARGB_BLOCK(argb_sse2)
UPSAMPLE_ARGB_BLOCK(rgba_sse2)
UPSAMPLE_ARGB_BLOCK(bgra_sse2)
UPSAMPLE_ARGB_BLOCK(rgb_sse2)
UPSAMPLE_ARGB_BLOCK(bgr_sse2)
UPSAMPLE_ARGB_BLOCK(rgb_ssse3)
UPSAMPLE_ARGB_BLOCK(bgr_ssse3)
UPSAMPLE_ARGB_BLOCK(argb_avx2)
UPSAMPLE_ARGB_BLOCK(rgba_avx2)
UPSAMPLE_ARGB_BLOCK(bgra_avx2)
UPSAMPLE_ARGB_BLOCK(rgb_avx2)
UPSAMPLE_ARGB_BLOCK(bgr_avx2)
#endif
#undef UPSAMPLE_ARGB_BLOCK

void ff_dispatch_alpha_sse2(uint8_t *dst, const uint8_t *src, int num_pixels);
void ff_dispatch_alpha_avx2(uint8_t *dst, const uint8_t *src, int num_pixels);

#define PACK_ROW(name, cpu) \
    void ff_pack_##name##_##cpu(uint8_t *dst, const uint8_t *src, int n);

PACK_ROW(rgba, ssse3)
PACK_ROW(bgra, ssse3)
PACK_ROW(rgb, ssse3)
PACK_ROW(bgr, ssse3)
PACK_ROW(rgb565, ssse3)
PACK_ROW(rgba4444, ssse3)
PACK_ROW(rgba, avx2)
PACK_ROW(bgra, avx2)
PACK_ROW(rgb, avx2)
PACK_ROW(bgr, avx2)
PACK_ROW(rgb565, avx2)
PACK_ROW(rgba4444, avx2)
#undef PACK_ROW

void ff_premultiply_row_ssse3(uint8_t *rgba, int alpha_first, int num_pixels);
void ff_premultiply_row_avx2(uint8_t *rgba, int alpha_first, int num_pixels);
void ff_premultiply_row_4444_ssse3(uint8_t *rgba4444, int num_pixels);
void ff_premultiply_row_4444_avx2(uint8_t *rgba4444, int num_pixels);
void ff_argb_to_y_ssse3(uint8_t *y, const uint8_t *argb, int num_pixels);
void ff_argb_to_y_avx2(uint8_t *y, const uint8_t *argb, int num_pixels);

static wpd_always_inline void wpd_yuv_dsp_init_x86(WPDYUVDSP *dsp) {
    const unsigned flags = wpd_get_cpu_flags();

    if (flags & WPD_X86_CPU_FLAG_SSE2) {
#if WPD_ARCH_X86_64
        dsp->upsample_block[WPD_LAYOUT_ARGB] = ff_upsample_block_argb_sse2;
        dsp->upsample_block[WPD_LAYOUT_RGBA] = ff_upsample_block_rgba_sse2;
        dsp->upsample_block[WPD_LAYOUT_BGRA] = ff_upsample_block_bgra_sse2;
        dsp->upsample_block[WPD_LAYOUT_RGB]  = ff_upsample_block_rgb_sse2;
        dsp->upsample_block[WPD_LAYOUT_BGR]  = ff_upsample_block_bgr_sse2;
#endif
        dsp->dispatch_alpha = ff_dispatch_alpha_sse2;
    }
    if (flags & WPD_X86_CPU_FLAG_SSSE3) {
#if WPD_ARCH_X86_64
        dsp->upsample_block[WPD_LAYOUT_RGB] = ff_upsample_block_rgb_ssse3;
        dsp->upsample_block[WPD_LAYOUT_BGR] = ff_upsample_block_bgr_ssse3;
#endif
        dsp->pack_rgba            = ff_pack_rgba_ssse3;
        dsp->pack_bgra            = ff_pack_bgra_ssse3;
        dsp->pack_rgb             = ff_pack_rgb_ssse3;
        dsp->pack_bgr             = ff_pack_bgr_ssse3;
        dsp->pack_rgb565          = ff_pack_rgb565_ssse3;
        dsp->pack_rgba4444        = ff_pack_rgba4444_ssse3;
        dsp->premultiply_row      = ff_premultiply_row_ssse3;
        dsp->premultiply_row_4444 = ff_premultiply_row_4444_ssse3;
        dsp->argb_to_y            = ff_argb_to_y_ssse3;
    }
    if (flags & WPD_X86_CPU_FLAG_AVX2) {
#if WPD_ARCH_X86_64
        dsp->upsample_block[WPD_LAYOUT_ARGB] = ff_upsample_block_argb_avx2;
        dsp->upsample_block[WPD_LAYOUT_RGBA] = ff_upsample_block_rgba_avx2;
        dsp->upsample_block[WPD_LAYOUT_BGRA] = ff_upsample_block_bgra_avx2;
        dsp->upsample_block[WPD_LAYOUT_RGB]  = ff_upsample_block_rgb_avx2;
        dsp->upsample_block[WPD_LAYOUT_BGR]  = ff_upsample_block_bgr_avx2;
#endif
        dsp->dispatch_alpha       = ff_dispatch_alpha_avx2;
        dsp->pack_rgba            = ff_pack_rgba_avx2;
        dsp->pack_bgra            = ff_pack_bgra_avx2;
        dsp->pack_rgb             = ff_pack_rgb_avx2;
        dsp->pack_bgr             = ff_pack_bgr_avx2;
        dsp->pack_rgb565          = ff_pack_rgb565_avx2;
        dsp->pack_rgba4444        = ff_pack_rgba4444_avx2;
        dsp->premultiply_row      = ff_premultiply_row_avx2;
        dsp->premultiply_row_4444 = ff_premultiply_row_4444_avx2;
        dsp->argb_to_y            = ff_argb_to_y_avx2;
    }
}

#endif
