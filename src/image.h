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

/* Scratch the area rescaler needs, grown to fit and kept between frames. */
typedef struct RescaleScratch {
    uint32_t *work;
    size_t    work_size;
    uint8_t  *row;
    size_t    row_size;
} RescaleScratch;

#define GET_PIXEL(img, x, y) \
    ((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x))

#define GET_PIXEL_COMP(img, x, y, c) \
    (*((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x) + (c)))

void image_free(WebPImage *img);
/* Releases one plane, leaving the rest of the image as it was. */
void image_drop_plane(WebPImage *img, int p);
int  image_alloc_packed(WebPImage *img, int w, int h, int bpp,
                        WPDPixelFormat format);
int  image_alloc_argb(WebPImage *img, int w, int h);
int  image_alloc_yuv444(WebPImage *img, int w, int h);
int  image_alloc_yuva(WebPImage *img, int w, int h);
void image_scratch_free(RescaleScratch *scratch);
int  image_scratch_grow(RescaleScratch *scratch, int dst_width, int src_width,
                        int channels);

#endif
