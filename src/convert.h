#ifndef WPD_CONVERT_H
#define WPD_CONVERT_H

#include "wpd_dec.h"

typedef struct SubRect {
    int x, y, w, h;
} SubRect;

int           image_nb_components(const WebPImage *img);
int           format_is_packed(WPDPixelFormat format);
int           format_bpp(WPDPixelFormat format);
int           format_is_premultiplied(WPDPixelFormat format);
int           format_valid(WPDPixelFormat format);
pack_row_func format_packer(const WPDDecoder *s, WPDPixelFormat format);
premultiply_4444_row_func format_premultiplier_4444(const WPDDecoder *s,
                                                    WPDPixelFormat    format);
int                       premultiply_after_pack(const WPDDecoder *s);
int                       format_layout(WPDPixelFormat format);
int                       options_transform(const WPDDecoder *s);
void                      flip_image(WebPImage *view);
int  transform_image(WPDDecoder *s, const WebPImage *src, WebPImage *view,
                     WebPImage **result, WPDPixelFormat format);
int  convert_to_packed(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       WPDPixelFormat format);
int  convert_to_argb(WPDDecoder *s, WebPImage *dst, const WebPImage *src);
int  ensure_yuva(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                 int want_alpha);
void blend_argb_region(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       SubRect r);
void copy_argb_region(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                      SubRect r);
void blend_yuva_region(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       SubRect r);
void copy_yuva_region(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                      SubRect r);

int ensure_yuva_rows(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                     int want_alpha, int row_start, int row_end);

#endif
