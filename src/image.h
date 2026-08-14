#ifndef WPD_IMAGE_H
#define WPD_IMAGE_H

#include "wpd_internal.h"

typedef struct WebPImage {
    /* Set when the rescaler has brought U and V up to full resolution. */
    int chroma_full;
    /* Set when the colour channels already carry alpha, as the animation
       canvas does for a premultiplied output format. */
    int            premultiplied;
    uint8_t       *data[4];
    uint8_t       *alloc[4];
    size_t         alloc_size[4];
    int            linesize[4];
    int            width, height;
    WPDPixelFormat format;
} WebPImage;

#define GET_PIXEL(img, x, y) \
    ((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x))

#define GET_PIXEL_COMP(img, x, y, c) \
    (*((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x) + (c)))

void image_free(WebPImage *img);
int  image_alloc_packed(WebPImage *img, int w, int h, int bpp,
                        WPDPixelFormat format);
int  image_alloc_argb(WebPImage *img, int w, int h);
int  image_alloc_yuv444(WebPImage *img, int w, int h);
int  image_alloc_yuva(WebPImage *img, int w, int h);

#endif
