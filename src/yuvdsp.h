#ifndef WPD_YUVDSP_H
#define WPD_YUVDSP_H

#include "wpd_codec.h"

#include <stddef.h>
#include <stdint.h>

#define WPD_UPSAMPLE_BLOCK 32

/* Packed output layouts the upsampler can emit directly. The three-byte ones
   drop alpha, as libwebp's RGB and BGR colorspaces do. */
enum {
    WPD_LAYOUT_ARGB,
    WPD_LAYOUT_RGBA,
    WPD_LAYOUT_BGRA,
    WPD_LAYOUT_RGB,
    WPD_LAYOUT_BGR,
    WPD_LAYOUT_NB,
};

typedef void (*upsample_argb_block_func)(
    const uint8_t *top_y, const uint8_t *bottom_y, const uint8_t *top_u,
    const uint8_t *top_v, const uint8_t *cur_u, const uint8_t *cur_v,
    uint8_t *top_dst, uint8_t *bottom_dst, int num_blocks);

typedef void (*dispatch_alpha_func)(uint8_t *dst, const uint8_t *src,
                                    int num_pixels);

/* Reorders an ARGB row into the packed output layout; dst must not alias
   src. */
typedef void (*pack_row_func)(uint8_t *dst, const uint8_t *src, int num_pixels);

typedef void (*premultiply_row_func)(uint8_t *rgba, int alpha_first,
                                     int num_pixels);

/* The same in the packed 4-bit layout, which libwebp multiplies in its own
   precision rather than in 8-bit and then truncating. */
typedef void (*premultiply_4444_row_func)(uint8_t *rgba4444, int num_pixels);

typedef struct WPDYUVDSP {
    upsample_argb_block_func upsample_block[WPD_LAYOUT_NB];
    dispatch_alpha_func      dispatch_alpha;
    pack_row_func            pack_rgba;
    pack_row_func            pack_bgra;
    pack_row_func            pack_rgb;
    pack_row_func            pack_bgr;
    /* These two write two bytes per pixel, not three or four. */
    pack_row_func             pack_rgb565;
    pack_row_func             pack_rgba4444;
    premultiply_row_func      premultiply_row;
    premultiply_4444_row_func premultiply_row_4444;
} WPDYUVDSP;

void wpd_yuv_dsp_init(WPDYUVDSP *dsp);

void wpd_yuv420_to_packed(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                          ptrdiff_t dst_stride, const uint8_t *y,
                          ptrdiff_t y_stride, const uint8_t *u,
                          const uint8_t *v, ptrdiff_t uv_stride,
                          const uint8_t *a, ptrdiff_t a_stride, int width,
                          int height);

#endif
