#ifndef WPD_YUVDSP_H
#define WPD_YUVDSP_H

#include "wpd_codec.h"

#include <stddef.h>
#include <stdint.h>

#define WPD_UPSAMPLE_BLOCK 32

/* Three-byte packed layouts drop alpha, as libwebp's RGB and BGR do. */
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

/* Reorders an ARGB row; dst must not alias src. */
typedef void (*pack_row_func)(uint8_t *dst, const uint8_t *src, int num_pixels);

typedef void (*premultiply_row_func)(uint8_t *rgba, int alpha_first,
                                     int num_pixels);

/* libwebp premultiplies packed 4-bit output at 4-bit precision. */
typedef void (*premultiply_4444_row_func)(uint8_t *rgba4444, int num_pixels);

typedef void (*argb_to_y_func)(uint8_t *y, const uint8_t *argb, int num_pixels);

typedef void (*argb_to_yuv444_func)(uint8_t *y, uint8_t *u, uint8_t *v,
                                    const uint8_t *argb, int num_pixels);

/* A zero argb_stride repeats an odd image's final row. */
typedef void (*argb_to_uv_func)(uint8_t *u, uint8_t *v, const uint8_t *argb,
                                ptrdiff_t argb_stride, int num_pixels,
                                int weight_alpha);

/* Forward alpha multiplies only; the inverse divides per pixel and stays
 * scalar. */
typedef void (*multiply_row_func)(uint8_t *plane, const uint8_t *alpha,
                                  int num_pixels);
typedef void (*multiply_argb_row_func)(uint8_t *argb, int num_pixels);

extern const uint16_t wpd_gamma_to_linear_tab[257];
extern const uint16_t wpd_linear_to_gamma_tab[33];

typedef struct WPDYUVDSP {
    upsample_argb_block_func  upsample_block[WPD_LAYOUT_NB];
    dispatch_alpha_func       dispatch_alpha_first;
    dispatch_alpha_func       dispatch_alpha_last;
    pack_row_func             pack_rgba;
    pack_row_func             pack_bgra;
    pack_row_func             pack_rgb;
    pack_row_func             pack_bgr;
    pack_row_func             pack_rgb565;
    pack_row_func             pack_rgba4444;
    pack_row_func             pack_bgr565;
    pack_row_func             pack_bgra4444;
    premultiply_row_func      premultiply_row;
    premultiply_4444_row_func premultiply_row_4444;
    premultiply_4444_row_func premultiply_row_4444_swap;
    argb_to_y_func            argb_to_y;
    argb_to_yuv444_func       argb_to_yuv444;
    argb_to_uv_func           argb_to_uv;
    multiply_row_func         multiply_row;
    multiply_argb_row_func    premultiply_argb_row;
} WPDYUVDSP;

void wpd_yuv_dsp_init(WPDYUVDSP *dsp);

int wpd_yuv420_to_packed_rows(int layout, uint8_t *dst, ptrdiff_t dst_stride,
                              const uint8_t *y, ptrdiff_t y_stride,
                              const uint8_t *u, const uint8_t *v,
                              ptrdiff_t uv_stride, const uint8_t *a,
                              ptrdiff_t a_stride, int width, int height,
                              int row_start, int row_end);

void wpd_yuv420_to_packed(int layout, uint8_t *dst, ptrdiff_t dst_stride,
                          const uint8_t *y, ptrdiff_t y_stride,
                          const uint8_t *u, const uint8_t *v,
                          ptrdiff_t uv_stride, const uint8_t *a,
                          ptrdiff_t a_stride, int width, int height);

#endif
