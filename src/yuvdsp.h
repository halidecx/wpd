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

/* Luma for one ARGB row. */
typedef void (*argb_to_y_func)(uint8_t *y, const uint8_t *argb, int num_pixels);

/* Chroma for one pair of ARGB rows, averaged 2x2 in linear light. 'argb' is
   the top row and 'argb_stride' reaches the bottom one; a stride of 0 repeats
   the row, which is how the last row of an odd-height image is handled. When
   'weight_alpha' is set, a partly transparent block is averaged weighted by
   alpha, as libwebp does whenever it is also producing an alpha plane. */
typedef void (*argb_to_uv_func)(uint8_t *u, uint8_t *v, const uint8_t *argb,
                                ptrdiff_t argb_stride, int num_pixels,
                                int weight_alpha);

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
    argb_to_y_func            argb_to_y;
    argb_to_uv_func           argb_to_uv;
} WPDYUVDSP;

void wpd_yuv_dsp_init(WPDYUVDSP *dsp);

/* Converts rows [row_start, row_end). The fancy upsampler emits an (odd, even)
   row pair at a time, so an even row_start also rewrites row_start - 1; the
   first row actually written is returned, and a caller with a per-row pass of
   its own must run that pass from there, not from row_start. */
int wpd_yuv420_to_packed_rows(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                              ptrdiff_t dst_stride, const uint8_t *y,
                              ptrdiff_t y_stride, const uint8_t *u,
                              const uint8_t *v, ptrdiff_t uv_stride,
                              const uint8_t *a, ptrdiff_t a_stride, int width,
                              int height, int row_start, int row_end);

void wpd_yuv420_to_packed(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                          ptrdiff_t dst_stride, const uint8_t *y,
                          ptrdiff_t y_stride, const uint8_t *u,
                          const uint8_t *v, ptrdiff_t uv_stride,
                          const uint8_t *a, ptrdiff_t a_stride, int width,
                          int height);
/* Point sampling, which libwebp uses when fancy upsampling is turned off.
   Every output row stands alone here, so rows [row_start, row_end) may be cut
   anywhere. */
void wpd_yuv420_to_packed_simple(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                                 ptrdiff_t dst_stride, const uint8_t *y,
                                 ptrdiff_t y_stride, const uint8_t *u,
                                 const uint8_t *v, ptrdiff_t uv_stride,
                                 const uint8_t *a, ptrdiff_t a_stride,
                                 int width, int row_start, int row_end);

/* Point conversion from full-resolution planes, which is what libwebp uses
   once the rescaler has brought chroma up to the output size. */
void wpd_yuv444_to_packed(int layout, uint8_t *dst, ptrdiff_t dst_stride,
                          const uint8_t *y, ptrdiff_t y_stride,
                          const uint8_t *u, const uint8_t *v,
                          ptrdiff_t uv_stride, int width, int height);

/* Converts rows [row_start, row_end) of a packed ARGB image to planar 4:2:0.
   Pass a NULL 'a' when the caller wants no alpha plane; chroma is then
   averaged without weighting it, which is what libwebp does for its YUV, as
   opposed to YUVA, colorspace.

   Chroma pairs rows, so row_start must be even, and row_end even or the image
   height; within that, splitting a conversion is bit-identical to doing it at
   once. */
void wpd_argb_to_yuva(const WPDYUVDSP *dsp, uint8_t *y, ptrdiff_t y_stride,
                      uint8_t *u, uint8_t *v, ptrdiff_t uv_stride, uint8_t *a,
                      ptrdiff_t a_stride, const uint8_t *argb,
                      ptrdiff_t argb_stride, int width, int row_start,
                      int row_end);

#endif
