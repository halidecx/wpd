#ifndef WPD_CONVERT_H
#define WPD_CONVERT_H

#include "image.h"
#include "rescaler.h"
#include "vp8l_dsp.h"
#include "yuvdsp.h"

typedef struct SubRect {
    int x, y, w, h;
} SubRect;

int           image_nb_components(const WebPImage *img);
int           format_is_packed(WPDPixelFormat format);
int           format_bpp(WPDPixelFormat format);
int           format_is_premultiplied(WPDPixelFormat format);
int           format_valid(WPDPixelFormat format);
pack_row_func format_packer(const WPDYUVDSP *dsp, WPDPixelFormat format);
premultiply_4444_row_func format_premultiplier_4444(const WPDYUVDSP *dsp,
                                                    WPDPixelFormat   format);
/* Whether a two-byte output premultiplies after the pack, in the four-bit
   domain, rather than before it in eight-bit ARGB. */
int premultiply_after_pack(int animation, WPDAnimationMode anim_mode);
int format_layout(WPDPixelFormat format);
int options_transform(const WPDDecoderOptions *options);
int scaled_size(const WPDDecoderOptions *options, int src_width, int src_height,
                int *width, int *height);
void flip_image(WebPImage *view);
int  transform_image(const WPDDecoderOptions *options, RescaleScratch *scratch,
                     WebPImage *scaled, const WebPImage *src, WebPImage *view,
                     WebPImage **result, WPDPixelFormat format);
int  convert_to_packed(const WPDYUVDSP *dsp, WebPImage *dst,
                       const WebPImage *src, WPDPixelFormat format,
                       int no_fancy_upsampling, int premultiply_packed);
int  convert_to_argb(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                     int no_fancy_upsampling);
int  ensure_yuva(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                 int want_alpha);
int ensure_yuva_rows(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                     int want_alpha, int row_start, int row_end);

/* The region blitters take the destination corner separately: the rectangle is
   in source coordinates, and an animation lands it at the sub-frame's position
   on the canvas. */
void blend_argb_region(const WPDLosslessDSP *dsp, int premultiply,
                       WebPImage *dst, const WebPImage *src, SubRect r,
                       int dst_x, int dst_y);
void copy_argb_region(WebPImage *dst, const WebPImage *src, SubRect r,
                      int dst_x, int dst_y);
void blend_yuva_region(WebPImage *dst, const WebPImage *src, SubRect r,
                       int dst_x, int dst_y);
void copy_yuva_region(WebPImage *dst, const WebPImage *src, SubRect r,
                      int dst_x, int dst_y);

#endif
