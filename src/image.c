
#include "image.h"

void image_free(WebPImage *img) {
    for (int p = 0; p < 4; p++) wpd_free(img->alloc[p]);
    memset(img, 0, sizeof(*img));
}

static uint8_t *image_alloc_plane(WebPImage *img, int p, size_t size) {
    if (img->alloc[p] && img->alloc_size[p] >= size) {
        memset(img->alloc[p], 0, size);
        return img->alloc[p];
    }
    wpd_free(img->alloc[p]);
    img->alloc[p]      = wpd_mallocz(size);
    img->alloc_size[p] = img->alloc[p] ? size : 0;
    return img->alloc[p];
}

int image_alloc_packed(WebPImage *img, int w, int h, int bpp,
                       WPDPixelFormat format) {
    size_t row, size;

    if (w <= 0 || h <= 0 || bpp <= 0 || (size_t)w > SIZE_MAX / (size_t)bpp)
        return WPD_ERROR_TOO_LARGE;
    row = (size_t)w * (size_t)bpp;
    if ((size_t)h > (SIZE_MAX - WPD_FILE_PADDING) / row)
        return WPD_ERROR_TOO_LARGE;
    size = row * (size_t)h + WPD_FILE_PADDING;
    if (row > INT_MAX)
        return WPD_ERROR_TOO_LARGE;

    for (int p = 1; p < 4; p++) {
        wpd_free(img->alloc[p]);
        img->alloc[p]      = NULL;
        img->alloc_size[p] = 0;
        img->data[p]       = NULL;
        img->linesize[p]   = 0;
    }
    img->linesize[0] = (int)row;
    img->data[0]     = image_alloc_plane(img, 0, size);
    if (!img->data[0])
        return WPD_ERROR(ENOMEM);
    img->width  = w;
    img->height = h;
    img->format = format;
    return 0;
}

int image_alloc_argb(WebPImage *img, int w, int h) {
    return image_alloc_packed(img, w, h, 4, WPD_PIX_FMT_ARGB);
}

int image_alloc_yuv444(WebPImage *img, int w, int h) {
    if (w <= 0 || h <= 0)
        return WPD_ERROR_TOO_LARGE;
    for (int p = 0; p < 4; p++) {
        size_t size;

        if ((size_t)h > (SIZE_MAX - WPD_FILE_PADDING) / (size_t)w) {
            image_free(img);
            return WPD_ERROR_TOO_LARGE;
        }
        size             = (size_t)w * (size_t)h + WPD_FILE_PADDING;
        img->linesize[p] = w;
        img->data[p]     = image_alloc_plane(img, p, size);
        if (!img->data[p]) {
            image_free(img);
            return WPD_ERROR(ENOMEM);
        }
    }
    img->width  = w;
    img->height = h;
    img->format = WPD_PIX_FMT_YUVA420P;
    return 0;
}

int image_alloc_yuva(WebPImage *img, int w, int h) {
    if (w <= 0 || h <= 0)
        return WPD_ERROR_TOO_LARGE;
    for (int p = 0; p < 4; p++) {
        int    pw = (p == 1 || p == 2) ? (w + 1) / 2 : w;
        int    ph = (p == 1 || p == 2) ? (h + 1) / 2 : h;
        size_t size;

        if ((size_t)ph > (SIZE_MAX - WPD_FILE_PADDING) / (size_t)pw) {
            image_free(img);
            return WPD_ERROR_TOO_LARGE;
        }
        size             = (size_t)pw * (size_t)ph + WPD_FILE_PADDING;
        img->linesize[p] = pw;
        img->data[p]     = image_alloc_plane(img, p, size);
        if (!img->data[p]) {
            image_free(img);
            return WPD_ERROR(ENOMEM);
        }
    }
    img->width  = w;
    img->height = h;
    img->format = WPD_PIX_FMT_YUVA420P;
    return 0;
}
