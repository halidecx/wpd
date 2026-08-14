
#include "convert.h"

int image_nb_components(const WebPImage *img) {
    switch (img->format) {
    case WPD_PIX_FMT_YUV420P: return 3;
    case WPD_PIX_FMT_YUVA420P: return 4;
    default: return 1;
    }
}

int format_is_packed(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_ARGB;
}

int format_bpp(WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGB565:
    case WPD_PIX_FMT_RGBA4444:
    case WPD_PIX_FMT_RGBA4444_PRE:
    case WPD_PIX_FMT_BGR565:
    case WPD_PIX_FMT_BGRA4444:
    case WPD_PIX_FMT_BGRA4444_PRE: return 2;
    case WPD_PIX_FMT_RGB:
    case WPD_PIX_FMT_BGR: return 3;
    default: return 4;
    }
}

int format_is_premultiplied(WPDPixelFormat format) {
    return format == WPD_PIX_FMT_ARGB_PRE || format == WPD_PIX_FMT_RGBA_PRE ||
        format == WPD_PIX_FMT_BGRA_PRE || format == WPD_PIX_FMT_RGBA4444_PRE ||
        format == WPD_PIX_FMT_BGRA4444_PRE;
}

int format_valid(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_YUV420P && format <= WPD_PIX_FMT_BGRA4444_PRE;
}

pack_row_func format_packer(const WPDYUVDSP *dsp, WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGBA:
    case WPD_PIX_FMT_RGBA_PRE: return dsp->pack_rgba;
    case WPD_PIX_FMT_BGRA:
    case WPD_PIX_FMT_BGRA_PRE: return dsp->pack_bgra;
    case WPD_PIX_FMT_RGB: return dsp->pack_rgb;
    case WPD_PIX_FMT_BGR: return dsp->pack_bgr;
    case WPD_PIX_FMT_RGB565: return dsp->pack_rgb565;
    case WPD_PIX_FMT_RGBA4444:
    case WPD_PIX_FMT_RGBA4444_PRE: return dsp->pack_rgba4444;
    case WPD_PIX_FMT_BGR565: return dsp->pack_bgr565;
    case WPD_PIX_FMT_BGRA4444:
    case WPD_PIX_FMT_BGRA4444_PRE: return dsp->pack_bgra4444;
    default: return NULL;
    }
}

premultiply_4444_row_func format_premultiplier_4444(const WPDYUVDSP *dsp,
                                                    WPDPixelFormat   format) {
    return format == WPD_PIX_FMT_BGRA4444_PRE ? dsp->premultiply_row_4444_swap
                                              : dsp->premultiply_row_4444;
}

int premultiply_after_pack(int animation, WPDAnimationMode anim_mode) {
    return !animation || anim_mode == WPD_ANIM_SUBFRAME;
}

/* The byte layouts the upsampler can emit without a second pass. */
int format_layout(WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGBA:
    case WPD_PIX_FMT_RGBA_PRE: return WPD_LAYOUT_RGBA;
    case WPD_PIX_FMT_BGRA:
    case WPD_PIX_FMT_BGRA_PRE: return WPD_LAYOUT_BGRA;
    case WPD_PIX_FMT_RGB: return WPD_LAYOUT_RGB;
    case WPD_PIX_FMT_BGR: return WPD_LAYOUT_BGR;
    default: return WPD_LAYOUT_ARGB;
    }
}

int options_transform(const WPDDecoderOptions *options) {
    return options->use_cropping || options->use_scaling || options->flip;
}

int scaled_size(const WPDDecoderOptions *options, int src_width, int src_height,
                int *width, int *height) {
    int w = options->scaled_width;
    int h = options->scaled_height;

    if (!w)
        w = (int)(((int64_t)src_width * h + src_height - 1) / src_height);
    if (!h)
        h = (int)(((int64_t)src_height * w + src_width - 1) / src_width);
    if (w <= 0 || h <= 0 || w > 16384 || h > 16384 ||
        (uint64_t)w * h >= 1ULL << 32)
        return WPD_ERR_TOO_LARGE;
    *width  = w;
    *height = h;
    return 0;
}

static int crop_image(const WPDDecoderOptions *options, const WebPImage *src,
                      WebPImage *view) {
    const int align = format_is_packed(src->format) ? 0 : 1;
    int       left  = options->crop_left & ~align;
    int       top   = options->crop_top & ~align;

    *view = *src;
    if (!options->use_cropping)
        return 0;
    if (left > src->width || top > src->height ||
        options->crop_width > src->width - left ||
        options->crop_height > src->height - top)
        return WPD_ERR_INVALID_ARG;
    for (int p = 0; p < image_nb_components(src); p++) {
        const int shift = p == 1 || p == 2;
        const int bpp = format_is_packed(src->format) ? format_bpp(src->format)
                                                      : 1;

        view->data[p] += (ptrdiff_t)(top >> shift) * src->linesize[p] +
            (ptrdiff_t)(left >> shift) * bpp;
    }
    view->width  = options->crop_width;
    view->height = options->crop_height;
    return 0;
}

/* libwebp carries alpha-weighted samples across the rescaler, so the plane it
   feeds in is not the plane it decoded. Building each row into scratch keeps
   the decoded image untouched, which matters because an animation blends the
   next frame onto it and a still can be exported more than once. */
static void rescale_plane_weighted(RescaleScratch *scratch, uint8_t *dst,
                                   int dst_stride, int dst_width,
                                   int dst_height, const uint8_t *src,
                                   int src_stride, const uint8_t *alpha,
                                   int alpha_stride, int src_width,
                                   int src_height, int channels) {
    WPDRescaler r;
    int         y = 0;

    wpd_rescaler_init(&r,
                      src_width,
                      src_height,
                      dst,
                      dst_width,
                      dst_height,
                      dst_stride,
                      channels,
                      scratch->work);
    while (y < src_height) {
        memcpy(scratch->row,
               src + (ptrdiff_t)y * src_stride,
               (size_t)src_width * channels);
        if (alpha)
            wpd_multiply_row(scratch->row,
                             alpha + (ptrdiff_t)y * alpha_stride,
                             src_width,
                             0);
        else
            wpd_premultiply_argb_row(scratch->row, src_width, 0);
        if (wpd_rescaler_import(&r, 1, scratch->row, 0))
            y++;
        wpd_rescaler_export(&r);
    }
}

/* Scales the way libwebp does: an area rescaler over each plane, with the
   colour channels premultiplied across it so a transparent edge does not
   bleed. 'chroma_full' brings U and V up to the output size instead of half
   it, which is what libwebp feeds its point converter when a scaled lossy
   frame is going to a packed format. */
static int scale_image(RescaleScratch *scratch, WebPImage *dst,
                       const WebPImage *src, int width, int height,
                       int chroma_full, int weight_luma) {
    const int packed = format_is_packed(src->format);
    const int bpp    = packed ? format_bpp(src->format) : 1;
    /* An already premultiplied source resamples correctly on its own: the
       weighted average of alpha-weighted colour is what the rescaler outputs
       directly, so weighting it a second time would skew it. */
    const int premult = packed && src->format == WPD_PIX_FMT_ARGB &&
        !src->premultiplied;
    int ret;

    if (packed)
        ret = image_alloc_packed(dst, width, height, bpp, src->format);
    else if (chroma_full)
        ret = image_alloc_yuv444(dst, width, height);
    else
        ret = image_alloc_yuva(dst, width, height);
    if (ret < 0)
        return ret;
    dst->format = src->format;
    ret         = image_scratch_grow(scratch, width, src->width, bpp);
    if (ret < 0)
        return ret;

    for (int p = 0; p < image_nb_components(src); p++) {
        const int chroma = p == 1 || p == 2;
        const int shift  = chroma && !chroma_full;
        const int sw = packed ? src->width : CEIL_RSHIFT(src->width, chroma);
        const int sh = packed ? src->height : CEIL_RSHIFT(src->height, chroma);
        const int dw = CEIL_RSHIFT(width, shift);
        const int dh = CEIL_RSHIFT(height, shift);

        if (premult || (weight_luma && p == 0))
            rescale_plane_weighted(scratch,
                                   dst->data[p],
                                   dst->linesize[p],
                                   dw,
                                   dh,
                                   src->data[p],
                                   src->linesize[p],
                                   premult ? NULL : src->data[3],
                                   premult ? 0 : src->linesize[3],
                                   sw,
                                   sh,
                                   bpp);
        else
            wpd_rescale_plane(dst->data[p],
                              dst->linesize[p],
                              dw,
                              dh,
                              src->data[p],
                              src->linesize[p],
                              sw,
                              sh,
                              bpp,
                              scratch->work);
    }

    if (premult)
        for (int y = 0; y < height; y++)
            wpd_premultiply_argb_row(
                dst->data[0] + (ptrdiff_t)y * dst->linesize[0], width, 1);
    else if (weight_luma)
        for (int y = 0; y < height; y++)
            wpd_multiply_row(dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                             dst->data[3] + (ptrdiff_t)y * dst->linesize[3],
                             width,
                             1);
    if (!packed && image_nb_components(src) < 4) {
        image_drop_plane(dst, 3);
        dst->format = WPD_PIX_FMT_YUV420P;
    }
    dst->chroma_full   = !packed && chroma_full;
    dst->premultiplied = src->premultiplied;
    return 0;
}

void flip_image(WebPImage *view) {
    for (int p = 0; p < image_nb_components(view); p++) {
        const int shift = p == 1 || p == 2;
        const int h     = CEIL_RSHIFT(view->height, shift);

        view->data[p] += (ptrdiff_t)(h - 1) * view->linesize[p];
        view->linesize[p] = -view->linesize[p];
    }
}

int transform_image(const WPDDecoderOptions *options, RescaleScratch *scratch,
                    WebPImage *scaled, const WebPImage *src, WebPImage *view,
                    WebPImage **result, WPDPixelFormat format) {
    int width, height, ret;

    ret = crop_image(options, src, view);
    if (ret < 0)
        return ret;
    *result = view;
    if (options->use_scaling) {
        const int planar = !format_is_packed(src->format);
        /* Going to a packed format, libwebp brings U and V all the way up to
           the output size and point-converts; staying planar, it keeps them
           half size and weights the luma by alpha across the rescaler. */
        const int chroma_full = planar && format_is_packed(format);
        const int weight_luma = planar && !format_is_packed(format) &&
            format != WPD_PIX_FMT_YUV420P && image_nb_components(src) == 4;

        ret = scaled_size(options, view->width, view->height, &width, &height);
        if (ret < 0)
            return ret;
        ret = scale_image(
            scratch, scaled, view, width, height, chroma_full, weight_luma);
        if (ret < 0)
            return ret;
        *result = scaled;
    }
    return 0;
}

void blend_argb_region(const WPDLosslessDSP *dsp, int premultiply,
                       WebPImage *dst, const WebPImage *src, SubRect r,
                       int dst_x, int dst_y) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(dst_y + r.y + y) * dst->linesize[0] + (dst_x + r.x) * 4;

        if (premultiply)
            dsp->blend_row_argb_premult(dst_argb, src_argb, r.w);
        else
            dsp->blend_row_argb(dst_argb, src_argb, r.w);
    }
}

void copy_argb_region(WebPImage *dst, const WebPImage *src, SubRect r,
                      int dst_x, int dst_y) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(dst_y + r.y + y) * dst->linesize[0] + (dst_x + r.x) * 4;

        memcpy(dst_argb, src_argb, (size_t)r.w * 4);
    }
}

void blend_yuva_region(WebPImage *dst, const WebPImage *src, SubRect r,
                       int dst_x, int dst_y) {
    int base_x = dst_x + r.x, base_y = dst_y + r.y;

    for (int y = 0; y < CEIL_RSHIFT(r.h, 1); y++) {
        int            tile_h = WPD_MIN(r.h - y * 2, 2);
        const uint8_t *src_u  = src->data[1] +
            (ptrdiff_t)((r.y >> 1) + y) * src->linesize[1] + (r.x >> 1);
        const uint8_t *src_v = src->data[2] +
            (ptrdiff_t)((r.y >> 1) + y) * src->linesize[2] + (r.x >> 1);
        uint8_t *dst_u = dst->data[1] +
            (ptrdiff_t)((base_y >> 1) + y) * dst->linesize[1] + (base_x >> 1);
        uint8_t *dst_v = dst->data[2] +
            (ptrdiff_t)((base_y >> 1) + y) * dst->linesize[2] + (base_x >> 1);
        for (int x = 0; x < CEIL_RSHIFT(r.w, 1); x++) {
            int tile_w    = WPD_MIN(r.w - x * 2, 2);
            int src_alpha = 0;
            int dst_alpha = 0;
            for (int yy = 0; yy < tile_h; yy++) {
                for (int xx = 0; xx < tile_w; xx++) {
                    src_alpha += src->data[3][(ptrdiff_t)(r.y + y * 2 + yy) *
                                                  src->linesize[3] +
                                              (r.x + x * 2 + xx)];
                    dst_alpha += dst->data[3][(ptrdiff_t)(base_y + y * 2 + yy) *
                                                  dst->linesize[3] +
                                              (base_x + x * 2 + xx)];
                }
            }
            int shift = (tile_h == 2) + (tile_w == 2);
            src_alpha = CEIL_RSHIFT(src_alpha, shift);
            dst_alpha = CEIL_RSHIFT(dst_alpha, shift);

            if (src_alpha == 255) {
                *dst_u = *src_u;
                *dst_v = *src_v;
            } else if (src_alpha == 0) {
            } else {
                int tmp_alpha   = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale       = (1UL << 24) / blend_alpha;
                *dst_u          = (((uint32_t)(*src_u * src_alpha +
                                               *dst_u * tmp_alpha)) *
                                   scale) >>
                    24;
                *dst_v = (((uint32_t)(*src_v * src_alpha +
                                      *dst_v * tmp_alpha)) *
                          scale) >>
                    24;
            }
            src_u += 1;
            src_v += 1;
            dst_u += 1;
            dst_v += 1;
        }
    }

    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_y = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x;
        const uint8_t *src_a = src->data[3] +
            (ptrdiff_t)(r.y + y) * src->linesize[3] + r.x;
        uint8_t *dst_y = dst->data[0] +
            (ptrdiff_t)(base_y + y) * dst->linesize[0] + base_x;
        uint8_t *dst_a = dst->data[3] +
            (ptrdiff_t)(base_y + y) * dst->linesize[3] + base_x;
        for (int x = 0; x < r.w; x++) {
            int src_alpha = *src_a;
            int dst_alpha = *dst_a;

            if (src_alpha == 255) {
                *dst_y = *src_y;
                *dst_a = 255;
            } else if (src_alpha == 0) {
            } else {
                int tmp_alpha   = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale       = (1UL << 24) / blend_alpha;
                *dst_y          = (((uint32_t)(*src_y * src_alpha +
                                               *dst_y * tmp_alpha)) *
                                   scale) >>
                    24;
                *dst_a = blend_alpha;
            }
            src_y += 1;
            src_a += 1;
            dst_y += 1;
            dst_a += 1;
        }
    }
}

void copy_yuva_region(WebPImage *dst, const WebPImage *src, SubRect r,
                      int dst_x, int dst_y) {
    int nb_components = image_nb_components(src);
    int base_x = dst_x + r.x, base_y = dst_y + r.y;

    for (int comp = 0; comp < nb_components; comp++) {
        int            shift = (comp == 1 || comp == 2) ? 1 : 0;
        const uint8_t *src_p = src->data[comp] +
            (ptrdiff_t)(r.y >> shift) * src->linesize[comp] + (r.x >> shift);
        uint8_t *dst_p = dst->data[comp] +
            (ptrdiff_t)(base_y >> shift) * dst->linesize[comp] +
            (base_x >> shift);

        for (int y = 0; y < CEIL_RSHIFT(r.h, shift); y++) {
            memcpy(dst_p, src_p, CEIL_RSHIFT(r.w, shift));
            src_p += src->linesize[comp];
            dst_p += dst->linesize[comp];
        }
    }

    if (nb_components < 4) {
        uint8_t *dst_a = dst->data[3] + base_y * dst->linesize[3] + base_x;

        for (int y = 0; y < r.h; y++) {
            memset(dst_a, 255, r.w);
            dst_a += dst->linesize[3];
        }
    }
}

int convert_to_packed(const WPDYUVDSP *dsp, WebPImage *dst,
                      const WebPImage *src, WPDPixelFormat format,
                      int no_fancy_upsampling, int premultiply_packed) {
    const int layout = format_layout(format);
    int       ret;

    if (format_bpp(format) == 2) {
        WebPImage        temp = {0};
        const WebPImage *argb = src;

        if (src->format != WPD_PIX_FMT_ARGB) {
            ret = convert_to_packed(dsp,
                                    &temp,
                                    src,
                                    WPD_PIX_FMT_ARGB,
                                    no_fancy_upsampling,
                                    premultiply_packed);
            if (ret < 0)
                return ret;
            argb = &temp;
        }
        ret = image_alloc_packed(dst, argb->width, argb->height, 2, format);
        if (ret >= 0) {
            const pack_row_func pack = format_packer(dsp, format);

            for (int y = 0; y < argb->height; y++)
                pack(dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                     argb->data[0] + (ptrdiff_t)y * argb->linesize[0],
                     argb->width);
            if (format_is_premultiplied(format) && premultiply_packed) {
                const premultiply_4444_row_func premultiply =
                    format_premultiplier_4444(dsp, format);

                for (int y = 0; y < argb->height; y++)
                    premultiply(dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                                argb->width);
            }
        }
        image_free(&temp);
        return ret;
    }

    ret = image_alloc_packed(
        dst, src->width, src->height, format_bpp(format), format);

    if (ret < 0)
        return ret;
    if (src->chroma_full) {
        wpd_yuv444_to_packed(layout,
                             dst->data[0],
                             dst->linesize[0],
                             src->data[0],
                             src->linesize[0],
                             src->data[1],
                             src->data[2],
                             src->linesize[1],
                             src->width,
                             src->height);
        if (image_nb_components(src) == 4 && layout != WPD_LAYOUT_RGB &&
            layout != WPD_LAYOUT_BGR)
            for (int y = 0; y < src->height; y++)
                (layout == WPD_LAYOUT_ARGB ? dsp->dispatch_alpha_first
                                           : dsp->dispatch_alpha_last)(
                    dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                    src->data[3] + (ptrdiff_t)y * src->linesize[3],
                    src->width);
        return 0;
    }
    if (no_fancy_upsampling)
        wpd_yuv420_to_packed_simple(dsp,
                                    layout,
                                    dst->data[0],
                                    dst->linesize[0],
                                    src->data[0],
                                    src->linesize[0],
                                    src->data[1],
                                    src->data[2],
                                    src->linesize[1],
                                    src->data[3],
                                    src->linesize[3],
                                    src->width,
                                    0,
                                    src->height);
    else
        wpd_yuv420_to_packed(dsp,
                             layout,
                             dst->data[0],
                             dst->linesize[0],
                             src->data[0],
                             src->linesize[0],
                             src->data[1],
                             src->data[2],
                             src->linesize[1],
                             src->data[3],
                             src->linesize[3],
                             src->width,
                             src->height);
    return 0;
}

int convert_to_argb(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                    int no_fancy_upsampling) {
    return convert_to_packed(
        dsp, dst, src, WPD_PIX_FMT_ARGB, no_fancy_upsampling, 0);
}

static int convert_argb_to_yuva(const WPDYUVDSP *dsp, WebPImage *dst,
                                const WebPImage *src, int want_alpha,
                                int row_start, int row_end) {
    int ret;

    if (!row_start &&
        (ret = image_alloc_yuva(dst, src->width, src->height)) < 0)
        return ret;
    wpd_argb_to_yuva(dsp,
                     dst->data[0],
                     dst->linesize[0],
                     dst->data[1],
                     dst->data[2],
                     dst->linesize[1],
                     want_alpha ? dst->data[3] : NULL,
                     dst->linesize[3],
                     src->data[0],
                     src->linesize[0],
                     src->width,
                     row_start,
                     row_end);
    if (!want_alpha)
        for (int y = row_start; y < row_end; y++)
            memset(dst->data[3] + (ptrdiff_t)y * dst->linesize[3],
                   255,
                   (size_t)src->width);
    return 0;
}

int ensure_yuva_rows(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                     int want_alpha, int row_start, int row_end) {
    int ret;

    if (src->format == WPD_PIX_FMT_ARGB)
        return convert_argb_to_yuva(
            dsp, dst, src, want_alpha, row_start, row_end);
    if (!row_start &&
        (ret = image_alloc_yuva(dst, src->width, src->height)) < 0)
        return ret;
    for (int p = 0; p < 4; p++) {
        const int shift = p == 1 || p == 2;
        const int w     = CEIL_RSHIFT(src->width, shift);
        const int h     = CEIL_RSHIFT(row_end, shift);

        for (int y = row_start >> shift; y < h; y++) {
            uint8_t *out = dst->data[p] + (ptrdiff_t)y * dst->linesize[p];

            if (p == 3 && src->format == WPD_PIX_FMT_YUV420P)
                memset(out, 255, (size_t)w);
            else
                memcpy(out,
                       src->data[p] + (ptrdiff_t)y * src->linesize[p],
                       (size_t)w);
        }
    }
    return 0;
}

int ensure_yuva(const WPDYUVDSP *dsp, WebPImage *dst, const WebPImage *src,
                int want_alpha) {
    return ensure_yuva_rows(dsp, dst, src, want_alpha, 0, src->height);
}
