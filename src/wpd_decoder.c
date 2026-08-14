
#include "wpd.h"

#include "container.h"
#include "huffman.h"
#include "image.h"
#include "rescaler.h"
#include "vp8.h"
#include "vp8l_dsp.h"
#include "wpd_codec.h"
#include "wpd_dec.h"
#include "wpd_internal.h"
#include "yuvdsp.h"

#include <stdio.h>
#include <stdlib.h>

#define ANMF_FLAG_DISPOSE (1 << 0)
#define ANMF_FLAG_NO_BLEND (1 << 1)

#define SCALEBITS 10
#define ONE_HALF (1 << (SCALEBITS - 1))
#define FIX(x) ((int)((x) * (1 << SCALEBITS) + 0.5))

#define YUV_TO_RGB1_CCIR(cb1, cr1)                            \
    do {                                                      \
        cb    = (cb1) - 128;                                  \
        cr    = (cr1) - 128;                                  \
        r_add = FIX(1.40200 * 255.0 / 224.0) * cr + ONE_HALF; \
        g_add = -FIX(0.34414 * 255.0 / 224.0) * cb -          \
            FIX(0.71414 * 255.0 / 224.0) * cr + ONE_HALF;     \
        b_add = FIX(1.77200 * 255.0 / 224.0) * cb + ONE_HALF; \
    } while (0)

#define YUV_TO_RGB2_CCIR(r, g, b, y1)                 \
    do {                                              \
        y = ((y1) - 16) * FIX(255.0 / 219.0);         \
        r = wpd_clip_uint8((y + r_add) >> SCALEBITS); \
        g = wpd_clip_uint8((y + g_add) >> SCALEBITS); \
        b = wpd_clip_uint8((y + b_add) >> SCALEBITS); \
    } while (0)

#define RGB_TO_Y_CCIR(r, g, b)                                                \
    ((FIX(0.29900 * 219.0 / 255.0) * (r) +                                    \
      FIX(0.58700 * 219.0 / 255.0) * (g) +                                    \
      FIX(0.11400 * 219.0 / 255.0) * (b) + (ONE_HALF + (16 << SCALEBITS))) >> \
     SCALEBITS)

#define RGB_TO_U_CCIR(r1, g1, b1, shift)                                       \
    (((-FIX(0.16874 * 224.0 / 255.0) * r1 -                                    \
       FIX(0.33126 * 224.0 / 255.0) * g1 + FIX(0.50000 * 224.0 / 255.0) * b1 + \
       (ONE_HALF << shift) - 1) >>                                             \
      (SCALEBITS + shift)) +                                                   \
     128)

#define RGB_TO_V_CCIR(r1, g1, b1, shift)                                       \
    (((FIX(0.50000 * 224.0 / 255.0) * r1 - FIX(0.41869 * 224.0 / 255.0) * g1 - \
       FIX(0.08131 * 224.0 / 255.0) * b1 + (ONE_HALF << shift) - 1) >>         \
      (SCALEBITS + shift)) +                                                   \
     128)

static int frame_valid(const WPDFrame *frame) {
    return frame && frame->struct_size >= WPD_FIELD_END(WPDFrame, private_data);
}

/* How much of the caller's frame this build may touch: the newest revision of
   the struct it declares room for in full, capped at the newest this build
   knows about. A size landing between two revisions rounds down to the older
   one rather than writing part of a field pair the caller may not have. */
static size_t frame_extent(const WPDFrame *frame) {
    return frame->struct_size >= WPD_FIELD_END(WPDFrame, has_alpha)
        ? WPD_FIELD_END(WPDFrame, has_alpha)
        : WPD_FIELD_END(WPDFrame, private_data);
}

static void frame_clear(WPDFrame *frame) {
    const size_t struct_size = frame->struct_size;

    memset((uint8_t *)frame + sizeof(frame->struct_size),
           0,
           frame_extent(frame) - sizeof(frame->struct_size));
    frame->struct_size = struct_size;
}

/* Copies past struct_size rather than assigning: the caller's frame may be a
   newer, longer revision of the struct, and its own size has to survive. */
static void frame_copy(WPDFrame *dst, const WPDFrame *src) {
    const size_t extent = WPD_MIN(frame_extent(dst), frame_extent(src));

    memcpy((uint8_t *)dst + sizeof(dst->struct_size),
           (const uint8_t *)src + sizeof(src->struct_size),
           extent - sizeof(dst->struct_size));
}

static void alpha_inverse_prediction(WebPImage *frame, enum AlphaFilter m) {
    int      x, y, ls;
    uint8_t *dec;

    ls = frame->linesize[3];

    dec = frame->data[3] + 1;
    for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - 1);

    dec = frame->data[3] + ls;
    for (y = 1; y < frame->height; y++, dec += ls) *dec += *(dec - ls);

    switch (m) {
    case ALPHA_FILTER_HORIZONTAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - 1);
        }
        break;
    case ALPHA_FILTER_VERTICAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - ls);
        }
        break;
    case ALPHA_FILTER_GRADIENT:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++)
                dec[0] += wpd_clip_uint8(*(dec - 1) + *(dec - ls) -
                                         *(dec - ls - 1));
        }
        break;
    case ALPHA_FILTER_NONE: break;
    }
}

static int vp8_lossy_decode_alpha(WPDDecoder *s, WebPImage *p,
                                  const uint8_t *data_start,
                                  unsigned int   data_size) {
    int y, ret;

    if (s->alpha_compression == ALPHA_COMPRESSION_NONE) {
        const uint8_t *src  = data_start;
        size_t         left = data_size;

        for (y = 0; y < s->height; y++) {
            size_t n = WPD_MIN((size_t)s->width, left);
            memcpy(p->data[3] + p->linesize[3] * y, src, n);
            src += n;
            left -= n;
        }
    } else if (s->alpha_compression == ALPHA_COMPRESSION_VP8L) {
        s->alpha_dst        = p->data[3];
        s->alpha_dst_stride = p->linesize[3];
        s->alpha_dst_used   = 0;

        ret = vp8_lossless_decode_frame(
            s, &s->alpha_argb, data_start, data_size, 1);
        s->alpha_dst = NULL;
        if (ret < 0) {
            image_free(&s->alpha_argb);
            return ret;
        }

        if (!s->alpha_dst_used)
            for (y = 0; y < s->height; y++)
                s->ldsp.extract_green(p->data[3] + p->linesize[3] * y,
                                      GET_PIXEL(&s->alpha_argb, 0, y),
                                      s->width);
        image_free(&s->alpha_argb);
    }

    if (s->alpha_filter)
        alpha_inverse_prediction(p, s->alpha_filter);

    return 0;
}

static void vp8_lossy_export_planes(const WPDDecoder *s, WebPImage *out,
                                    const WpdFrame *decoded) {
    memset(out, 0, sizeof(*out));
    out->width  = s->width;
    out->height = s->height;
    out->format = WPD_PIX_FMT_YUV420P;
    for (int plane = 0; plane < 3; plane++) {
        out->data[plane]     = decoded->data[plane];
        out->linesize[plane] = decoded->linesize[plane];
    }
    if (s->has_alpha) {
        out->data[3]     = s->alpha_plane;
        out->linesize[3] = s->width;
        out->format      = WPD_PIX_FMT_YUVA420P;
    }
}

static int vp8_lossy_alpha_plane(WPDDecoder *s, WebPImage *out) {
    const size_t alpha_size = (size_t)s->width * s->height;
    int          ret;

    if (s->alpha_plane_size < alpha_size) {
        uint8_t *plane = realloc(s->alpha_plane, alpha_size);
        if (!plane)
            return WPD_ERROR(ENOMEM);
        s->alpha_plane      = plane;
        s->alpha_plane_size = alpha_size;
    }
    memset(s->alpha_plane, 0, alpha_size);
    out->data[3]     = s->alpha_plane;
    out->linesize[3] = s->width;
    out->format      = WPD_PIX_FMT_YUVA420P;
    ret              = vp8_lossy_decode_alpha(
        s, out, file_at(s, s->alpha_data_offset), s->alpha_data_size);
    s->alpha_pending = 0;
    return ret;
}

static int vp8_lossy_init(WPDDecoder *s) {
    int ret;

    if (s->vp8_initialized)
        return 0;
    s->codec.priv_data = &s->vp8;
    ret                = vp8_decode_init(&s->codec);
    if (ret < 0)
        return ret;
    s->vp8_initialized = 1;
    return 0;
}

/* libwebp rounds an inferred dimension up, not to nearest. */
static int scaled_size(const WPDDecoder *s, int src_width, int src_height,
                       int *width, int *height) {
    int w = s->options.scaled_width;
    int h = s->options.scaled_height;

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

/* libwebp drops the in-loop filter once a scaled decode shrinks the frame past
   three quarters in both directions, on the grounds that nothing survives the
   downscale, so a scaled lossy frame only matches it if the filter goes too.
   The threshold is measured against the whole frame, not the cropped part. */
static void update_filter_bypass(WPDDecoder *s) {
    int width, height;

    s->codec.bypass_filtering = s->options.bypass_filtering;
    if (!s->options.use_scaling || !s->canvas_width || !s->canvas_height)
        return;
    if (scaled_size(
            s,
            s->options.use_cropping ? s->options.crop_width : s->canvas_width,
            s->options.use_cropping ? s->options.crop_height : s->canvas_height,
            &width,
            &height) < 0)
        return;
    if (width < s->canvas_width * 3 / 4 && height < s->canvas_height * 3 / 4)
        s->codec.bypass_filtering = 1;
}

/* Returns 1 when the frame is complete, 0 when more of the chunk is needed. */
static int vp8_lossy_step(WPDDecoder *s, WebPImage *out,
                          const uint8_t *data_start, unsigned int avail,
                          unsigned int data_size) {
    WpdFrame decoded;
    int      ret;

    if ((ret = vp8_lossy_init(s)) < 0)
        return ret;
    update_filter_bypass(s);

    if (!s->vp8_active) {
        ret = vp8_decode_frame_init(
            &s->codec, data_start, (int)avail, (int)data_size);
        if (ret < 0)
            return ret;
        if (ret)
            return 0;

        update_canvas_size(s, s->codec.width, s->codec.height);
        vp8_lossy_export_planes(s, out, &s->vp8.frame);
        if (s->has_alpha && (ret = vp8_lossy_alpha_plane(s, out)) < 0)
            return ret;
        s->still_lossy = !s->animation;
        s->vp8_active  = 1;
    } else {
        vp8_decode_extend(&s->codec, data_start, (int)avail);
    }

    ret = vp8_decode_rows(&s->codec, &decoded);
    if (ret < 0)
        return ret;
    vp8_lossy_export_planes(s, out, &decoded);
    if (ret)
        return 0;

    s->vp8_active = 0;
    return 1;
}

static int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                                  const uint8_t *data_start,
                                  unsigned int   data_size) {
    WpdPacket packet;
    WpdFrame  decoded;
    int       ret;

    if ((ret = vp8_lossy_init(s)) < 0)
        return ret;
    update_filter_bypass(s);

    packet.data = data_start;
    packet.size = data_size;
    ret         = vp8_decode_frame(&s->codec, &decoded, &packet);
    if (ret < 0)
        return ret;

    update_canvas_size(s, s->codec.width, s->codec.height);
    vp8_lossy_export_planes(s, out, &decoded);
    if (s->has_alpha && (ret = vp8_lossy_alpha_plane(s, out)) < 0)
        return ret;
    s->still_lossy = !s->animation;
    return 0;
}

static int image_nb_components(const WebPImage *img) {
    switch (img->format) {
    case WPD_PIX_FMT_YUV420P: return 3;
    case WPD_PIX_FMT_YUVA420P: return 4;
    default: return 1;
    }
}

typedef struct SubRect {
    int x, y, w, h;
} SubRect;

static int format_is_packed(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_ARGB;
}

static int format_bpp(WPDPixelFormat format) {
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

static int format_is_premultiplied(WPDPixelFormat format) {
    return format == WPD_PIX_FMT_ARGB_PRE || format == WPD_PIX_FMT_RGBA_PRE ||
        format == WPD_PIX_FMT_BGRA_PRE || format == WPD_PIX_FMT_RGBA4444_PRE ||
        format == WPD_PIX_FMT_BGRA4444_PRE;
}

static int format_valid(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_YUV420P && format <= WPD_PIX_FMT_BGRA4444_PRE;
}

static pack_row_func format_packer(const WPDDecoder *s, WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGBA:
    case WPD_PIX_FMT_RGBA_PRE: return s->ydsp.pack_rgba;
    case WPD_PIX_FMT_BGRA:
    case WPD_PIX_FMT_BGRA_PRE: return s->ydsp.pack_bgra;
    case WPD_PIX_FMT_RGB: return s->ydsp.pack_rgb;
    case WPD_PIX_FMT_BGR: return s->ydsp.pack_bgr;
    case WPD_PIX_FMT_RGB565: return s->ydsp.pack_rgb565;
    case WPD_PIX_FMT_RGBA4444:
    case WPD_PIX_FMT_RGBA4444_PRE: return s->ydsp.pack_rgba4444;
    case WPD_PIX_FMT_BGR565: return s->ydsp.pack_bgr565;
    case WPD_PIX_FMT_BGRA4444:
    case WPD_PIX_FMT_BGRA4444_PRE: return s->ydsp.pack_bgra4444;
    default: return NULL;
    }
}

static premultiply_4444_row_func format_premultiplier_4444(
    const WPDDecoder *s, WPDPixelFormat format) {
    return format == WPD_PIX_FMT_BGRA4444_PRE
        ? s->ydsp.premultiply_row_4444_swap
        : s->ydsp.premultiply_row_4444;
}

static int premultiply_after_pack(const WPDDecoder *s) {
    return !s->animation || s->anim_mode == WPD_ANIM_SUBFRAME;
}

/* The byte layouts the upsampler can emit without a second pass. */
static int format_layout(WPDPixelFormat format) {
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

static int options_transform(const WPDDecoder *s) {
    return s->options.use_cropping || s->options.use_scaling || s->options.flip;
}

static int crop_image(const WPDDecoder *s, const WebPImage *src,
                      WebPImage *view) {
    const int align = format_is_packed(src->format) ? 0 : 1;
    int       left  = s->options.crop_left & ~align;
    int       top   = s->options.crop_top & ~align;

    *view = *src;
    if (!s->options.use_cropping)
        return 0;
    if (left > src->width || top > src->height ||
        s->options.crop_width > src->width - left ||
        s->options.crop_height > src->height - top)
        return WPD_ERR_INVALID_ARG;
    for (int p = 0; p < image_nb_components(src); p++) {
        const int shift = p == 1 || p == 2;
        const int bpp = format_is_packed(src->format) ? format_bpp(src->format)
                                                      : 1;

        view->data[p] += (ptrdiff_t)(top >> shift) * src->linesize[p] +
            (ptrdiff_t)(left >> shift) * bpp;
    }
    view->width  = s->options.crop_width;
    view->height = s->options.crop_height;
    return 0;
}

static int rescale_work(WPDDecoder *s, int dst_width, int src_width,
                        int channels) {
    const size_t need = 2 * (size_t)dst_width * (size_t)channels;
    const size_t row  = (size_t)src_width * (size_t)channels;

    if (s->rescale_work_size < need) {
        uint32_t *grown = realloc(s->rescale_work, need * sizeof(*grown));

        if (!grown)
            return WPD_ERROR(ENOMEM);
        s->rescale_work      = grown;
        s->rescale_work_size = need;
    }
    if (s->rescale_row_size < row) {
        uint8_t *grown = realloc(s->rescale_row, row);

        if (!grown)
            return WPD_ERROR(ENOMEM);
        s->rescale_row      = grown;
        s->rescale_row_size = row;
    }
    return 0;
}

/* libwebp carries alpha-weighted samples across the rescaler, so the plane it
   feeds in is not the plane it decoded. Building each row into scratch keeps
   the decoded image untouched, which matters because an animation blends the
   next frame onto it and a still can be exported more than once. */
static void rescale_plane_weighted(WPDDecoder *s, uint8_t *dst, int dst_stride,
                                   int dst_width, int dst_height,
                                   const uint8_t *src, int src_stride,
                                   const uint8_t *alpha, int alpha_stride,
                                   int src_width, int src_height,
                                   int channels) {
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
                      s->rescale_work);
    while (y < src_height) {
        memcpy(s->rescale_row,
               src + (ptrdiff_t)y * src_stride,
               (size_t)src_width * channels);
        if (alpha)
            wpd_multiply_row(s->rescale_row,
                             alpha + (ptrdiff_t)y * alpha_stride,
                             src_width,
                             0);
        else
            wpd_premultiply_argb_row(s->rescale_row, src_width, 0);
        if (wpd_rescaler_import(&r, 1, s->rescale_row, 0))
            y++;
        wpd_rescaler_export(&r);
    }
}

/* Scales the way libwebp does: an area rescaler over each plane, with the
   colour channels premultiplied across it so a transparent edge does not
   bleed. 'chroma_full' brings U and V up to the output size instead of half
   it, which is what libwebp feeds its point converter when a scaled lossy
   frame is going to a packed format. */
static int scale_image(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       int width, int height, int chroma_full,
                       int weight_luma) {
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
    ret         = rescale_work(s, width, src->width, bpp);
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
            rescale_plane_weighted(s,
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
                              s->rescale_work);
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
        wpd_free(dst->alloc[3]);
        dst->alloc[3]      = NULL;
        dst->alloc_size[3] = 0;
        dst->data[3]       = NULL;
        dst->linesize[3]   = 0;
        dst->format        = WPD_PIX_FMT_YUV420P;
    }
    dst->chroma_full   = !packed && chroma_full;
    dst->premultiplied = src->premultiplied;
    return 0;
}

static void flip_image(WebPImage *view) {
    for (int p = 0; p < image_nb_components(view); p++) {
        const int shift = p == 1 || p == 2;
        const int h     = CEIL_RSHIFT(view->height, shift);

        view->data[p] += (ptrdiff_t)(h - 1) * view->linesize[p];
        view->linesize[p] = -view->linesize[p];
    }
}

static int transform_image(WPDDecoder *s, const WebPImage *src, WebPImage *view,
                           WebPImage **result, WPDPixelFormat format) {
    int width, height, ret;

    ret = crop_image(s, src, view);
    if (ret < 0)
        return ret;
    *result = view;
    if (s->options.use_scaling) {
        const int planar = !format_is_packed(src->format);
        /* Going to a packed format, libwebp brings U and V all the way up to
           the output size and point-converts; staying planar, it keeps them
           half size and weights the luma by alpha across the rescaler. */
        const int chroma_full = planar && format_is_packed(format);
        const int weight_luma = planar && !format_is_packed(format) &&
            format != WPD_PIX_FMT_YUV420P && image_nb_components(src) == 4;

        ret = scaled_size(s, view->width, view->height, &width, &height);
        if (ret < 0)
            return ret;
        ret = scale_image(
            s, &s->transformed, view, width, height, chroma_full, weight_luma);
        if (ret < 0)
            return ret;
        *result = &s->transformed;
    }
    return 0;
}

static void blend_argb_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(s->pos_y + r.y + y) * dst->linesize[0] +
            (s->pos_x + r.x) * 4;

        if (s->premultiply)
            s->ldsp.blend_row_argb_premult(dst_argb, src_argb, r.w);
        else
            s->ldsp.blend_row_argb(dst_argb, src_argb, r.w);
    }
}

static void copy_argb_region(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(s->pos_y + r.y + y) * dst->linesize[0] +
            (s->pos_x + r.x) * 4;

        memcpy(dst_argb, src_argb, (size_t)r.w * 4);
    }
}

static void blend_yuva_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    int base_x = s->pos_x + r.x, base_y = s->pos_y + r.y;

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

static void copy_yuva_region(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, SubRect r) {
    int nb_components = image_nb_components(src);
    int base_x = s->pos_x + r.x, base_y = s->pos_y + r.y;

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

static int convert_to_packed(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, WPDPixelFormat format) {
    const int layout = format_layout(format);
    int       ret;

    if (format_bpp(format) == 2) {
        WebPImage        temp = {0};
        const WebPImage *argb = src;

        if (src->format != WPD_PIX_FMT_ARGB) {
            ret = convert_to_packed(s, &temp, src, WPD_PIX_FMT_ARGB);
            if (ret < 0)
                return ret;
            argb = &temp;
        }
        ret = image_alloc_packed(dst, argb->width, argb->height, 2, format);
        if (ret >= 0) {
            const pack_row_func pack = format_packer(s, format);

            for (int y = 0; y < argb->height; y++)
                pack(dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                     argb->data[0] + (ptrdiff_t)y * argb->linesize[0],
                     argb->width);
            if (format_is_premultiplied(format) && premultiply_after_pack(s)) {
                const premultiply_4444_row_func premultiply =
                    format_premultiplier_4444(s, format);

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
                (layout == WPD_LAYOUT_ARGB ? s->ydsp.dispatch_alpha_first
                                           : s->ydsp.dispatch_alpha_last)(
                    dst->data[0] + (ptrdiff_t)y * dst->linesize[0],
                    src->data[3] + (ptrdiff_t)y * src->linesize[3],
                    src->width);
        return 0;
    }
    if (s->options.no_fancy_upsampling)
        wpd_yuv420_to_packed_simple(&s->ydsp,
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
        wpd_yuv420_to_packed(&s->ydsp,
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

static int convert_to_argb(WPDDecoder *s, WebPImage *dst,
                           const WebPImage *src) {
    return convert_to_packed(s, dst, src, WPD_PIX_FMT_ARGB);
}

static int convert_argb_to_yuva(WPDDecoder *s, WebPImage *dst,
                                const WebPImage *src, int want_alpha,
                                int row_start, int row_end) {
    int ret;

    if (!row_start &&
        (ret = image_alloc_yuva(dst, src->width, src->height)) < 0)
        return ret;
    wpd_argb_to_yuva(&s->ydsp,
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

static int ensure_yuva_rows(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                            int want_alpha, int row_start, int row_end) {
    int ret;

    if (src->format == WPD_PIX_FMT_ARGB)
        return convert_argb_to_yuva(
            s, dst, src, want_alpha, row_start, row_end);
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

static int ensure_yuva(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       int want_alpha) {
    return ensure_yuva_rows(s, dst, src, want_alpha, 0, src->height);
}

static void composite_region(WPDDecoder *s, const WebPImage *frame, SubRect r,
                             int blend) {
    WebPImage *canvas = &s->canvas;

    if (r.w <= 0 || r.h <= 0)
        return;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        if (blend)
            blend_argb_region(s, canvas, frame, r);
        else
            copy_argb_region(s, canvas, frame, r);
    } else {
        if (blend)
            blend_yuva_region(s, canvas, frame, r);
        else
            copy_yuva_region(s, canvas, frame, r);
    }
}

// libwebp overwrites the frame rect and alpha-blends only where the prev
// canvas can be non-transparent, blending elsewhere would round down
static void composite_subframe(WPDDecoder *s, const WebPImage *frame) {
    SubRect full = {0, 0, frame->width, frame->height};
    SubRect keep = {0, 0, 0, 0};

    // frames w no alpha plane cannot blend
    if (!s->key_frame && !(s->anmf_flags & ANMF_FLAG_NO_BLEND) &&
        frame->format != WPD_PIX_FMT_YUV420P) {
        if (!(s->prev_anmf_flags & ANMF_FLAG_DISPOSE)) {
            composite_region(s, frame, full, 1);
            return;
        }
        keep.x = WPD_MAX(s->pos_x, s->prev_pos_x) - s->pos_x;
        keep.y = WPD_MAX(s->pos_y, s->prev_pos_y) - s->pos_y;
        keep.w = WPD_MIN(s->pos_x + frame->width,
                         s->prev_pos_x + s->prev_width) -
            s->pos_x - keep.x;
        keep.h = WPD_MIN(s->pos_y + frame->height,
                         s->prev_pos_y + s->prev_height) -
            s->pos_y - keep.y;
        if (keep.w <= 0 || keep.h <= 0) {
            composite_region(s, frame, full, 1);
            return;
        }
        if (s->canvas.format != WPD_PIX_FMT_ARGB) {
            keep.w &= ~1;
            keep.h &= ~1;
            if (!keep.w || !keep.h) {
                composite_region(s, frame, full, 1);
                return;
            }
        }

        SubRect top    = {0, 0, full.w, keep.y};
        SubRect bottom = {0, keep.y + keep.h, full.w, full.h - keep.y - keep.h};
        SubRect left   = {0, keep.y, keep.x, keep.h};
        SubRect right  = {
            keep.x + keep.w, keep.y, full.w - keep.x - keep.w, keep.h};

        composite_region(s, frame, top, 1);
        composite_region(s, frame, bottom, 1);
        composite_region(s, frame, left, 1);
        composite_region(s, frame, right, 1);
        composite_region(s, frame, keep, 0);
        return;
    }

    composite_region(s, frame, full, 0);
}

static void clear_canvas_rect(WPDDecoder *s, int pos_x, int pos_y, int width,
                              int height) {
    WebPImage *canvas = &s->canvas;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        uint8_t *const base     = canvas->data[0];
        const int      linesize = canvas->linesize[0];
        uint32_t       bg;

        memcpy(&bg, s->clear_argb, 4);
        for (int y = 0; y < height; y++) {
            uint32_t *dst = (uint32_t *)(base +
                                         (size_t)(pos_y + y) * linesize) +
                pos_x;

            for (int x = 0; x < width; x++) dst[x] = bg;
        }
    } else {
        for (int comp = 0; comp < 4; comp++) {
            int      shift = (comp == 1 || comp == 2) ? 1 : 0;
            uint8_t *dst   = canvas->data[comp] +
                (ptrdiff_t)(pos_y >> shift) * canvas->linesize[comp] +
                (pos_x >> shift);
            for (int y = 0; y < CEIL_RSHIFT(height, shift); y++) {
                memset(dst, s->clear_yuva[comp], CEIL_RSHIFT(width, shift));
                dst += canvas->linesize[comp];
            }
        }
    }
}

static int allocate_canvas(WPDDecoder *s, WPDPixelFormat format) {
    int ret;

    if (format == WPD_PIX_FMT_ARGB)
        ret = image_alloc_argb(&s->canvas, s->canvas_width, s->canvas_height);
    else
        ret = image_alloc_yuva(&s->canvas, s->canvas_width, s->canvas_height);
    return ret;
}

static int is_full_frame(const WPDDecoder *s, int width, int height) {
    return width == s->canvas_width && height == s->canvas_height;
}

static int is_key_frame(const WPDDecoder *s, const WebPImage *frame) {
    if (s->frame_index == 0)
        return 1;
    if ((!s->frame_has_alpha || (s->anmf_flags & ANMF_FLAG_NO_BLEND)) &&
        s->pos_x == 0 && s->pos_y == 0 &&
        is_full_frame(s, frame->width, frame->height))
        return 1;
    return (s->prev_anmf_flags & ANMF_FLAG_DISPOSE) &&
        (is_full_frame(s, s->prev_width, s->prev_height) || s->prev_key_frame);
}

/* The canvas holds whichever alpha convention the output format asked for when
   its pixels were composited, and the caller may change that format between
   frames. Bring what is already there into the convention the next frame will
   be blended in, so the two are never mixed. */
static void reconcile_canvas_alpha(WPDDecoder *s) {
    if (s->canvas.data[0] && s->canvas.format == WPD_PIX_FMT_ARGB &&
        s->canvas.premultiplied != s->premultiply)
        for (int y = 0; y < s->canvas.height; y++) {
            uint8_t *row = s->canvas.data[0] +
                (ptrdiff_t)y * s->canvas.linesize[0];

            if (s->premultiply)
                s->ydsp.premultiply_row(row, 1, s->canvas.width);
            else
                wpd_premultiply_argb_row(row, s->canvas.width, 1);
        }
    s->canvas.premultiplied = s->premultiply;
}

static int prepare_canvas(WPDDecoder *s, const WebPImage *frame,
                          WPDPixelFormat format) {
    int covers_canvas = s->pos_x == 0 && s->pos_y == 0 &&
        is_full_frame(s, frame->width, frame->height);
    int ret;

    if (s->key_frame && s->canvas.data[0] && s->canvas.format != format)
        image_free(&s->canvas);

    if (!s->canvas.data[0]) {
        ret = allocate_canvas(s, format);
        if (ret < 0)
            return ret;
        s->canvas.premultiplied = s->premultiply;
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else if (s->key_frame) {
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else {
        if (format == WPD_PIX_FMT_ARGB &&
            s->canvas.format == WPD_PIX_FMT_YUVA420P) {
            WebPImage yuva_canvas = s->canvas;
            memset(&s->canvas, 0, sizeof(s->canvas));
            ret = convert_to_argb(s, &s->canvas, &yuva_canvas);
            image_free(&yuva_canvas);
            if (ret < 0)
                return ret;
        }
        if (s->prev_anmf_flags & ANMF_FLAG_DISPOSE)
            clear_canvas_rect(
                s, s->prev_pos_x, s->prev_pos_y, s->prev_width, s->prev_height);
    }

    reconcile_canvas_alpha(s);
    return 0;
}

static int decode_anmf(WPDDecoder *s, const uint8_t *data, size_t size) {
    const uint8_t *p = data, *end = data + size;
    WebPImage     *sub = NULL;
    int            declared_width, declared_height;
    int            ret;

    if (size < 16)
        return WPD_ERROR_INVALID_DATA;

    s->pos_x          = WPD_RL24(p) * 2;
    s->pos_y          = WPD_RL24(p + 3) * 2;
    declared_width    = WPD_RL24(p + 6) + 1;
    declared_height   = WPD_RL24(p + 9) + 1;
    s->frame_duration = WPD_RL24(p + 12);
    s->anmf_flags     = p[15];
    p += 16;

    if (s->pos_x + declared_width > s->canvas_width ||
        s->pos_y + declared_height > s->canvas_height) {
        wpd_log(NULL,
                WPD_LOG_ERROR,
                "Frame (%dx%d at pos %dx%d) does not fit into canvas (%dx%d)\n",
                declared_width,
                declared_height,
                s->pos_x,
                s->pos_y,
                s->canvas_width,
                s->canvas_height);
        return WPD_ERROR_INVALID_DATA;
    }

    s->has_alpha = 0;
    s->width     = 0;
    s->height    = 0;

    while (end - p >= 8) {
        uint32_t chunk_type   = WPD_RL32(p);
        uint32_t payload_size = WPD_RL32(p + 4);
        uint32_t padded_size;

        if (payload_size == UINT32_MAX)
            return WPD_ERROR_INVALID_DATA;
        padded_size = payload_size + (payload_size & 1);
        p += 8;

        if ((size_t)(end - p) < padded_size) {
            break;
        }

        switch (chunk_type) {
        case MKTAG('A', 'L', 'P', 'H'): {
            if (payload_size == 0) {
                wpd_log(NULL, WPD_LOG_ERROR, "invalid ALPHA chunk size\n");
                return WPD_ERROR_INVALID_DATA;
            }
            int alpha_header     = p[0];
            s->alpha_data_offset = s->discarded + (size_t)(p + 1 - s->file);
            s->alpha_data_size   = payload_size - 1;

            int filter_m    = (alpha_header >> 2) & 0x03;
            int compression = alpha_header & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL,
                        WPD_LOG_WARNING,
                        "skipping unsupported ALPHA chunk\n");
            } else {
                s->has_alpha         = 1;
                s->alpha_compression = compression;
                s->alpha_filter      = filter_m;
            }
            break;
        }
        case MKTAG('V', 'P', '8', ' '):
            if (sub)
                break;
            ret = vp8_lossy_decode_frame(s, &s->subframe, p, payload_size);
            if (ret < 0)
                return ret;
            sub                = &s->subframe;
            s->frame_has_alpha = s->has_alpha;
            break;
        case MKTAG('V', 'P', '8', 'L'):
            if (sub)
                break;
            ret = vp8_lossless_decode_frame(s, &s->argb, p, payload_size, 0);
            if (ret < 0)
                return ret;
            sub                = &s->argb;
            s->frame_has_alpha = s->lossless_has_alpha;
            break;
        default: break;
        }
        p += padded_size;
    }

    if (!sub) {
        wpd_log(NULL, WPD_LOG_ERROR, "image data not found\n");
        return WPD_ERROR_INVALID_DATA;
    }

    if (sub->width != declared_width || sub->height != declared_height)
        wpd_log(NULL,
                WPD_LOG_WARNING,
                "ANMF declares %dx%d but the image is %dx%d\n",
                declared_width,
                declared_height,
                sub->width,
                sub->height);

    if (s->pos_x + sub->width > s->canvas_width ||
        s->pos_y + sub->height > s->canvas_height) {
        wpd_log(NULL,
                WPD_LOG_ERROR,
                "Frame (%dx%d at pos %dx%d) does not fit into canvas (%dx%d)\n",
                sub->width,
                sub->height,
                s->pos_x,
                s->pos_y,
                s->canvas_width,
                s->canvas_height);
        return WPD_ERROR_INVALID_DATA;
    }

    s->key_frame = is_key_frame(s, sub);

    WPDPixelFormat target = WPD_PIX_FMT_YUVA420P;
    if (sub->format == WPD_PIX_FMT_ARGB || format_is_packed(s->out_format) ||
        (!s->key_frame && s->canvas.data[0] &&
         s->canvas.format == WPD_PIX_FMT_ARGB))
        target = WPD_PIX_FMT_ARGB;

    if (target == WPD_PIX_FMT_ARGB && sub->format != WPD_PIX_FMT_ARGB) {
        ret = convert_to_argb(s, &s->converted, sub);
        if (ret < 0)
            return ret;
        sub = &s->converted;
    }

    /* libwebp premultiplies each frame before compositing it, which is not
       the same as premultiplying the finished canvas. Premultiplying only ever
       goes with a packed output format, which forces the ARGB target above, so
       'sub' is four-byte ARGB here whatever the frame coded as. A sub-frame
       feeds no canvas, so a two-byte output premultiplies after the pack
       instead, in the four-bit domain a still uses. */
    if (s->premultiply &&
        !(premultiply_after_pack(s) && format_bpp(s->out_format) == 2))
        for (int y = 0; y < sub->height; y++)
            s->ydsp.premultiply_row(
                sub->data[0] + (size_t)y * sub->linesize[0], 1, sub->width);

    s->subframe_out = sub;

    /* Sub-frame mode owns no canvas, so it skips the allocation and the blend
       altogether; the dispose latch below is bookkeeping the canvas never fed.
       Nothing above reads the canvas except the ARGB target rule, which wants
       a canvas to stay compatible with and correctly declines when there is
       none. Switching modes mid-animation is refused for that reason. */
    if (s->anim_mode != WPD_ANIM_SUBFRAME) {
        ret = prepare_canvas(s, sub, target);
        if (ret < 0)
            return ret;

        composite_subframe(s, sub);
    }

    s->frame_timestamp += s->frame_duration;
    s->prev_anmf_flags = s->anmf_flags;
    s->prev_width      = sub->width;
    s->prev_height     = sub->height;
    s->prev_pos_x      = s->pos_x;
    s->prev_pos_y      = s->pos_y;
    s->prev_key_frame  = s->key_frame;
    s->frame_index++;

    return 0;
}

static void export_frame(const WPDDecoder *s, const WebPImage *img,
                         WPDPixelFormat format, WPDFrame *frame) {
    int planes = format == WPD_PIX_FMT_YUVA420P ? 4
        : format == WPD_PIX_FMT_YUV420P         ? 3
                                                : 1;

    frame_clear(frame);
    for (int p = 0; p < planes; p++) {
        frame->data[p]   = img->data[p];
        frame->stride[p] = img->linesize[p];
    }
    frame->width     = img->width;
    frame->height    = img->height;
    frame->format    = format;
    frame->duration  = s->frame_duration;
    frame->timestamp = s->frame_timestamp - s->frame_duration;
    if (frame_extent(frame) < WPD_FIELD_END(WPDFrame, has_alpha))
        return;
    frame->pos_x   = s->pos_x;
    frame->pos_y   = s->pos_y;
    frame->dispose = s->anmf_flags & ANMF_FLAG_DISPOSE ? WPD_DISPOSE_BACKGROUND
                                                       : WPD_DISPOSE_NONE;
    frame->blend   = s->anmf_flags & ANMF_FLAG_NO_BLEND ? WPD_BLEND_NONE
                                                        : WPD_BLEND_ALPHA;
    /* An animation latches each sub-frame's alpha as it decodes it; a still
       has only the one image, whose two decoders report it separately. */
    frame->has_alpha = s->animation ? s->frame_has_alpha
                                    : s->has_alpha || s->lossless_has_alpha;
}

static size_t stride_magnitude(ptrdiff_t stride) {
    return stride < 0 ? (size_t)(-(stride + 1)) + 1 : (size_t)stride;
}

static int export_external_rows(WPDDecoder *s, const WebPImage *img,
                                WPDPixelFormat format, WPDFrame *frame,
                                int row_start, int row_end) {
    const size_t  row     = (size_t)img->width * format_bpp(format);
    const size_t  advance = stride_magnitude(s->ext[0].stride);
    pack_row_func pack    = img->format == format ? NULL
                                                  : format_packer(s, format);
    uint8_t      *dst     = s->ext[0].data;

    if (!pack && format_bpp(img->format) != format_bpp(format))
        return WPD_ERR_UNSUPPORTED;
    if (advance < row || (size_t)img->height > s->ext[0].size / advance)
        return WPD_ERR_BUFFER_TOO_SMALL;

    dst += (ptrdiff_t)row_start * s->ext[0].stride;
    for (int y = row_start; y < row_end; y++) {
        const uint8_t *src = img->data[0] + (ptrdiff_t)y * img->linesize[0];

        if (pack) {
            pack(dst, src, img->width);
        } else {
            memcpy(dst, src, row);
        }
        dst += s->ext[0].stride;
    }

    export_frame(s, img, format, frame);
    for (int p = 1; p < 4; p++) {
        frame->data[p]   = NULL;
        frame->stride[p] = 0;
    }
    frame->data[0]   = s->ext[0].data;
    frame->stride[0] = s->ext[0].stride;
    return 0;
}

static int export_external_planar_rows(WPDDecoder *s, const WebPImage *img,
                                       WPDPixelFormat format, WPDFrame *frame,
                                       int row_start, int row_end) {
    const int planes = format == WPD_PIX_FMT_YUVA420P ? 4 : 3;

    for (int p = 0; p < planes; p++) {
        const int    shift = p == 1 || p == 2;
        const int    w     = CEIL_RSHIFT(img->width, shift);
        const int    h     = CEIL_RSHIFT(img->height, shift);
        const size_t step  = stride_magnitude(s->ext[p].stride);

        if (!s->ext[p].data || !s->ext[p].stride || step < (size_t)w ||
            (size_t)h > s->ext[p].size / step)
            return WPD_ERR_BUFFER_TOO_SMALL;
    }

    for (int p = 0; p < planes; p++) {
        const int shift = p == 1 || p == 2;
        const int w     = CEIL_RSHIFT(img->width, shift);
        const int y0    = row_start >> shift;
        const int h     = CEIL_RSHIFT(row_end, shift);
        uint8_t  *dst   = s->ext[p].data + (ptrdiff_t)y0 * s->ext[p].stride;

        for (int y = y0; y < h; y++) {
            memcpy(
                dst, img->data[p] + (ptrdiff_t)y * img->linesize[p], (size_t)w);
            dst += s->ext[p].stride;
        }
    }

    export_frame(s, img, format, frame);
    for (int p = 0; p < 4; p++) {
        frame->data[p]   = p < planes ? s->ext[p].data : NULL;
        frame->stride[p] = p < planes ? s->ext[p].stride : 0;
    }
    return 0;
}

static int export_external_planar(WPDDecoder *s, const WebPImage *img,
                                  WPDPixelFormat format, WPDFrame *frame) {
    return export_external_planar_rows(s, img, format, frame, 0, img->height);
}

static int export_external(WPDDecoder *s, const WebPImage *img,
                           WPDPixelFormat format, WPDFrame *frame) {
    return export_external_rows(s, img, format, frame, 0, img->height);
}

static int export_packed(WPDDecoder *s, WebPImage *img, WPDFrame *frame) {
    const WPDPixelFormat format = s->out_format;
    WebPImage            view;
    WebPImage           *processed;
    WebPImage           *planar;
    pack_row_func        pack;
    int                  ret;

    ret = transform_image(s, img, &view, &processed, format);
    if (ret < 0)
        return ret;
    img = processed;

    if (format == WPD_PIX_FMT_YUV420P || format == WPD_PIX_FMT_YUVA420P) {
        if ((img->format == WPD_PIX_FMT_YUV420P &&
             format == WPD_PIX_FMT_YUV420P) ||
            (img->format == WPD_PIX_FMT_YUVA420P)) {
            planar = img;
        } else {
            ret = ensure_yuva(
                s, &s->output, img, format == WPD_PIX_FMT_YUVA420P);
            if (ret < 0)
                return ret;
            planar = &s->output;
        }
        if (s->options.flip) {
            view = *planar;
            flip_image(&view);
            planar = &view;
        }
        if (s->ext_active)
            return export_external_planar(s, planar, format, frame);
        export_frame(s, planar, format, frame);
        return 0;
    }
    if (!format_is_packed(format)) {
        if (s->options.flip) {
            view = *img;
            flip_image(&view);
            img = &view;
        }
        if (!s->ext_active) {
            export_frame(s, img, img->format, frame);
            return 0;
        }
        if (!format_is_packed(img->format))
            return export_external_planar(s, img, img->format, frame);
        return export_external(s, img, img->format, frame);
    }
    if (!format_is_packed(img->format) || format_bpp(format) == 2) {
        ret = convert_to_packed(s, &s->output, img, format);
        if (ret < 0)
            return ret;
        img = &s->output;
    } else if (img->format != format) {
        pack = format_packer(s, format);
        if (!pack) {
            if (format != WPD_PIX_FMT_ARGB_PRE ||
                img->format != WPD_PIX_FMT_ARGB)
                return WPD_ERR_UNSUPPORTED;
            if (s->animation) {
                view        = *img;
                view.format = format;
                img         = &view;
            } else {
                ret = image_alloc_packed(
                    &s->output, img->width, img->height, 4, format);
                if (ret < 0)
                    return ret;
                for (int y = 0; y < img->height; y++)
                    memcpy(s->output.data[0] +
                               (ptrdiff_t)y * s->output.linesize[0],
                           img->data[0] + (ptrdiff_t)y * img->linesize[0],
                           (size_t)img->width * 4);
                img = &s->output;
            }
        } else {
            ret = image_alloc_packed(&s->output,
                                     img->width,
                                     img->height,
                                     format_bpp(format),
                                     format);
            if (ret < 0)
                return ret;
            for (int y = 0; y < img->height; y++)
                pack(s->output.data[0] + (ptrdiff_t)y * s->output.linesize[0],
                     img->data[0] + (ptrdiff_t)y * img->linesize[0],
                     img->width);
            img = &s->output;
        }
    }
    if (s->premultiply && !s->animation && format_bpp(format) != 2)
        for (int y = 0; y < img->height; y++)
            s->ydsp.premultiply_row(
                img->data[0] + (ptrdiff_t)y * img->linesize[0],
                format_layout(img->format) == WPD_LAYOUT_ARGB,
                img->width);
    if (s->options.flip) {
        view = *img;
        flip_image(&view);
        img = &view;
    }
    if (s->ext_active)
        return export_external(s, img, format, frame);
    export_frame(s, img, format, frame);
    return 0;
}

/* Converts and hands out rows [0, upto) of the still lossy frame, converting
   each row exactly once however many times it is asked for. */
static int export_still_packed(WPDDecoder *s, WPDFrame *frame, int upto) {
    const WPDPixelFormat format = s->out_format;
    const WebPImage     *src    = &s->subframe;
    WebPImage           *dst    = &s->converted;
    const int first = s->converted_format == format ? s->converted_rows : 0;
    int       converted_from = first;
    int       ret;

    if (upto < s->converted_rows)
        upto = s->converted_rows;

    /* The two-byte formats are packed from ARGB, so the intermediate has to be
       carried between calls too, rather than rebuilt for the whole frame. */
    if (format_bpp(format) == 2) {
        WebPImage *argb = &s->output;

        if (!first) {
            ret = image_alloc_argb(argb, src->width, src->height);
            if (ret < 0)
                return ret;
            ret = image_alloc_packed(dst, src->width, src->height, 2, format);
            if (ret < 0)
                return ret;
        }
        if (upto > first) {
            const pack_row_func             pack = format_packer(s, format);
            const premultiply_4444_row_func premultiply =
                format_premultiplier_4444(s, format);

            if (s->options.no_fancy_upsampling)
                wpd_yuv420_to_packed_simple(&s->ydsp,
                                            WPD_LAYOUT_ARGB,
                                            argb->data[0],
                                            argb->linesize[0],
                                            src->data[0],
                                            src->linesize[0],
                                            src->data[1],
                                            src->data[2],
                                            src->linesize[1],
                                            src->data[3],
                                            src->linesize[3],
                                            src->width,
                                            first,
                                            upto);
            else
                converted_from = wpd_yuv420_to_packed_rows(&s->ydsp,
                                                           WPD_LAYOUT_ARGB,
                                                           argb->data[0],
                                                           argb->linesize[0],
                                                           src->data[0],
                                                           src->linesize[0],
                                                           src->data[1],
                                                           src->data[2],
                                                           src->linesize[1],
                                                           src->data[3],
                                                           src->linesize[3],
                                                           src->width,
                                                           src->height,
                                                           first,
                                                           upto);
            for (int y = converted_from; y < upto; y++) {
                uint8_t *row = dst->data[0] + (ptrdiff_t)y * dst->linesize[0];

                pack(row,
                     argb->data[0] + (ptrdiff_t)y * argb->linesize[0],
                     src->width);
                if (s->premultiply)
                    premultiply(row, src->width);
            }
        }
        if (s->ext_active) {
            ret = export_external_rows(
                s, dst, format, frame, converted_from, upto);
            if (ret < 0)
                return ret;
        } else {
            export_frame(s, dst, format, frame);
        }
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (!first) {
        ret = image_alloc_packed(
            dst, src->width, src->height, format_bpp(format), format);
        if (ret < 0)
            return ret;
    }

    if (s->options.no_fancy_upsampling) {
        wpd_yuv420_to_packed_simple(&s->ydsp,
                                    format_layout(format),
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
                                    first,
                                    upto);
    } else if (upto > first) {
        converted_from = wpd_yuv420_to_packed_rows(&s->ydsp,
                                                   format_layout(format),
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
                                                   src->height,
                                                   first,
                                                   upto);
    }
    if (s->premultiply)
        for (int y = converted_from; y < upto; y++)
            s->ydsp.premultiply_row(dst->data[0] + (size_t)y * dst->linesize[0],
                                    format_layout(format) == WPD_LAYOUT_ARGB,
                                    dst->width);

    if (s->ext_active) {
        ret = export_external_rows(s, dst, format, frame, converted_from, upto);
        if (ret < 0)
            return ret;
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }
    s->converted_rows   = upto;
    s->converted_format = format;
    export_frame(s, dst, format, frame);
    return 0;
}

/* Hands out rows [0, upto) of the still lossless frame, premultiplying and
   packing each row exactly once however many times it is asked for. */
static int export_still_lossless(WPDDecoder *s, WPDFrame *frame, int upto) {
    const WPDPixelFormat format = s->out_format;
    WebPImage           *img    = s->lossless_frame;
    const int first = s->converted_format == format ? s->converted_rows : 0;
    const premultiply_4444_row_func premultiply = format_premultiplier_4444(
        s, format);
    pack_row_func pack;
    int           ret;

    if (upto < s->converted_rows)
        upto = s->converted_rows;

    if (format == WPD_PIX_FMT_YUV420P || format == WPD_PIX_FMT_YUVA420P) {
        ret = ensure_yuva_rows(
            s, &s->output, img, format == WPD_PIX_FMT_YUVA420P, first, upto);
        if (ret < 0)
            return ret;
        if (s->ext_active)
            ret = export_external_planar_rows(
                s, &s->output, format, frame, first, upto);
        else {
            export_frame(s, &s->output, format, frame);
            ret = 0;
        }
        if (ret < 0)
            return ret;
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (!format_is_packed(format)) {
        if (!s->ext_active) {
            export_frame(s, img, img->format, frame);
            s->converted_rows   = upto;
            s->converted_format = format;
            return 0;
        }
        ret = export_external_rows(s, img, img->format, frame, first, upto);
        if (ret < 0)
            return ret;
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (s->ext_active) {
        ret = export_external_rows(s, img, format, frame, first, upto);
        if (ret < 0)
            return ret;
        if (s->premultiply)
            for (int y = first; y < upto; y++) {
                uint8_t *row = s->ext[0].data + (ptrdiff_t)y * s->ext[0].stride;

                if (format_bpp(format) == 2)
                    premultiply(row, img->width);
                else
                    s->ydsp.premultiply_row(
                        row,
                        format_layout(format) == WPD_LAYOUT_ARGB,
                        img->width);
            }
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    pack = format_packer(s, format);
    if (!s->premultiply && (!pack || img->format == format)) {
        export_frame(s, img, format, frame);
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (!first) {
        ret = image_alloc_packed(
            &s->output, img->width, img->height, format_bpp(format), format);
        if (ret < 0)
            return ret;
    }
    for (int y = first; y < upto; y++) {
        uint8_t *dst = s->output.data[0] + (size_t)y * s->output.linesize[0];
        const uint8_t *src = img->data[0] + (size_t)y * img->linesize[0];

        if (pack)
            pack(dst, src, img->width);
        else
            memcpy(dst, src, (size_t)img->width * 4);
        if (s->premultiply) {
            if (format_bpp(format) == 2)
                premultiply(dst, img->width);
            else
                s->ydsp.premultiply_row(
                    dst, format_layout(format) == WPD_LAYOUT_ARGB, img->width);
        }
    }
    export_frame(s, &s->output, format, frame);
    s->converted_rows   = upto;
    s->converted_format = format;
    return 0;
}

const char *wpd_status_string(WPDStatus status) {
    switch (status) {
    case WPD_OK: return "success";
    case WPD_ERR_INVALID_ARG: return "invalid argument";
    case WPD_ERR_NOT_WEBP: return "not a WebP file";
    case WPD_ERR_BITSTREAM: return "invalid bitstream";
    case WPD_ERR_TRUNCATED: return "truncated file";
    case WPD_ERR_UNSUPPORTED: return "unsupported feature";
    case WPD_ERR_NO_MEMORY: return "out of memory";
    case WPD_ERR_TOO_LARGE: return "image too large";
    case WPD_ERR_BUFFER_TOO_SMALL: return "output buffer too small";
    }
    return "unknown error";
}

/* Internal failures are either a WPDStatus or a negated errno. */
static WPDStatus status_from_internal(int code) {
    switch (code) {
    case 0: return WPD_OK;
    case WPD_ERROR_INVALID_DATA: return WPD_ERR_BITSTREAM;
    case WPD_ERROR(ENOMEM): return WPD_ERR_NO_MEMORY;
    case WPD_ERROR_TOO_LARGE: return WPD_ERR_TOO_LARGE;
    case WPD_ERROR(EINVAL): return WPD_ERR_INVALID_ARG;
    default: break;
    }
    if (code <= WPD_ERR_INVALID_ARG && code >= WPD_ERR_BUFFER_TOO_SMALL)
        return (WPDStatus)code;
    return WPD_ERR_BITSTREAM;
}

static WPDStatus set_error(WPDDecoder *decoder, const char *message, int code) {
    decoder->status = status_from_internal(code);
    snprintf(decoder->error,
             sizeof(decoder->error),
             "%s (%s)",
             message,
             wpd_status_string(decoder->status));
    return decoder->status;
}

WPDDecoder *wpd_decoder_create(void) {
    WPDDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    wpd_init_cpu();
    wpd_vp8l_dsp_init(&decoder->ldsp);
    wpd_yuv_dsp_init(&decoder->ydsp);
    decoder->out_format          = WPD_PIX_FMT_NONE;
    decoder->options.struct_size = sizeof(decoder->options);
    return decoder;
}

WPDStatus wpd_decoder_set_options(WPDDecoder              *decoder,
                                  const WPDDecoderOptions *options) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!options ||
        options->struct_size < WPD_FIELD_END(WPDDecoderOptions, flip))
        return set_error(
            decoder, "invalid decoder options", WPD_ERR_INVALID_ARG);
    if ((options->bypass_filtering != 0 && options->bypass_filtering != 1) ||
        (options->no_fancy_upsampling != 0 &&
         options->no_fancy_upsampling != 1) ||
        (options->use_cropping != 0 && options->use_cropping != 1) ||
        (options->use_scaling != 0 && options->use_scaling != 1) ||
        (options->flip != 0 && options->flip != 1) ||
        (options->use_cropping &&
         (options->crop_left < 0 || options->crop_top < 0 ||
          options->crop_width <= 0 || options->crop_height <= 0)) ||
        (options->use_scaling &&
         (options->scaled_width < 0 || options->scaled_height < 0 ||
          (!options->scaled_width && !options->scaled_height))))
        return set_error(
            decoder, "invalid decoder options", WPD_ERR_INVALID_ARG);
    if (decoder->anim_mode == WPD_ANIM_SUBFRAME &&
        (options->use_cropping || options->use_scaling || options->flip))
        return set_error(decoder,
                         "cropping, scaling and flipping are defined against "
                         "the canvas, which sub-frame mode does not produce",
                         WPD_ERR_INVALID_ARG);
    decoder->options                = *options;
    decoder->options.struct_size    = sizeof(decoder->options);
    decoder->codec.bypass_filtering = options->bypass_filtering;
    return WPD_OK;
}

WPDStatus wpd_decoder_set_animation_mode(WPDDecoder      *decoder,
                                         WPDAnimationMode mode) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (mode != WPD_ANIM_COMPOSITED && mode != WPD_ANIM_SUBFRAME)
        return set_error(
            decoder, "invalid animation mode", WPD_ERR_INVALID_ARG);
    if (mode == WPD_ANIM_SUBFRAME && options_transform(decoder))
        return set_error(decoder,
                         "sub-frame mode cannot be combined with cropping, "
                         "scaling or flipping",
                         WPD_ERR_INVALID_ARG);
    /* Sub-frame mode never builds the canvas the composited one carries from
       frame to frame, so the two cannot be swapped part-way through an
       animation. wpd_decoder_rewind() clears the frame index and reopens the
       choice. */
    if (mode != decoder->anim_mode && decoder->animation &&
        decoder->frame_index)
        return set_error(decoder,
                         "the animation mode cannot change mid-animation",
                         WPD_ERR_INVALID_ARG);
    decoder->anim_mode = mode;
    return WPD_OK;
}

WPDStatus wpd_decoder_set_output_format(WPDDecoder    *decoder,
                                        WPDPixelFormat format) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (format != WPD_PIX_FMT_NONE && !format_valid(format))
        return set_error(decoder, "invalid output format", WPD_ERR_INVALID_ARG);
    decoder->out_format  = format;
    decoder->premultiply = format_is_premultiplied(format);
    return WPD_OK;
}

static int same_output_planes(const WPDOutputPlane *a,
                              const WPDOutputPlane *b) {
    for (int p = 0; p < 4; p++)
        if (a[p].data != b[p].data || a[p].size != b[p].size ||
            a[p].stride != b[p].stride)
            return 0;
    return 1;
}

/* Rows already handed out live in whichever buffer was current at the time, so
   a new destination has to be filled from the top again. */
static void drop_converted_rows(WPDDecoder *decoder) {
    decoder->converted_rows   = 0;
    decoder->converted_format = WPD_PIX_FMT_NONE;
}

WPDStatus wpd_decoder_set_output_buffer(WPDDecoder            *decoder,
                                        const WPDOutputBuffer *buffer) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!buffer) {
        if (decoder->ext_active)
            drop_converted_rows(decoder);
        decoder->ext_active = 0;
        memset(decoder->ext, 0, sizeof(decoder->ext));
        return WPD_OK;
    }
    if (buffer->struct_size < WPD_FIELD_END(WPDOutputBuffer, plane) ||
        !buffer->plane[0].data || !buffer->plane[0].stride)
        return set_error(decoder, "invalid output buffer", WPD_ERR_INVALID_ARG);
    for (int p = 0; p < 4; p++) {
        if ((buffer->plane[p].data && !buffer->plane[p].stride) ||
            (!buffer->plane[p].data && buffer->plane[p].stride))
            return set_error(
                decoder, "invalid output buffer", WPD_ERR_INVALID_ARG);
    }
    if (!decoder->ext_active ||
        !same_output_planes(decoder->ext, buffer->plane))
        drop_converted_rows(decoder);
    memcpy(decoder->ext, buffer->plane, sizeof(decoder->ext));
    decoder->ext_active = 1;
    return WPD_OK;
}

/* Everything a decode builds up as it walks the frames, which both opening a
   new file and rewinding the current one have to put back. The buffers the
   frames are decoded into are kept: they are sized on use and reused. */
static void anim_state_reset(WPDDecoder *decoder) {
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&decoder->image[i]);
    image_free(&decoder->canvas);
    decoder->still_done       = 0;
    decoder->vp8_active       = 0;
    decoder->still_lossy      = 0;
    decoder->alpha_pending    = 0;
    decoder->converted_rows   = 0;
    decoder->converted_format = WPD_PIX_FMT_NONE;
    decoder->vp8l_active      = 0;
    decoder->still_lossless   = 0;
    decoder->vp8l_next_try    = 0;
    decoder->vp8l_peeked      = 0;
    decoder->lossless_frame   = NULL;
    decoder->subframe_out     = NULL;
    decoder->frame_index      = 0;
    decoder->width = decoder->height = 0;
    decoder->has_alpha               = 0;
    decoder->lossless_has_alpha      = 0;
    decoder->frame_has_alpha         = 0;
    decoder->key_frame = decoder->prev_key_frame = 0;
    decoder->prev_anmf_flags = decoder->anmf_flags = 0;
    decoder->prev_width = decoder->prev_height = 0;
    decoder->prev_pos_x = decoder->prev_pos_y = 0;
    decoder->pos_x = decoder->pos_y = 0;
    decoder->frame_duration         = 0;
    decoder->frame_timestamp        = 0;
}

/* Clears everything derived from a file but keeps the input allocation, which
   a stream grows across many calls. */
static void decoder_reset(WPDDecoder *decoder) {
    for (int i = 0; i < WPD_METADATA_NB; i++) {
        free(decoder->meta[i]);
        decoder->meta[i]      = NULL;
        decoder->meta_size[i] = 0;
    }
    anim_state_reset(decoder);
    image_free(&decoder->argb);
    image_free(&decoder->lossless_out);
    image_free(&decoder->converted);
    image_free(&decoder->output);
    image_free(&decoder->transformed);
    memset(&decoder->subframe, 0, sizeof(decoder->subframe));
    decoder->file_size = 0;
    decoder->discarded = 0;
    decoder->file      = decoder->file_alloc;
    scan_free(&decoder->scan);
    memset(&decoder->scan, 0, sizeof(decoder->scan));
    decoder->pos = decoder->end = 0;
    decoder->opened             = 0;
    decoder->streaming          = 0;
    decoder->eos                = 0;
    decoder->headers_valid      = 0;
    decoder->truncated          = 0;
    decoder->borrowed           = 0;
    decoder->input_mode         = 0;
    decoder->animation          = 0;
    decoder->canvas_width = decoder->canvas_height = 0;
    decoder->anim_loop_count = decoder->anim_frame_count = 0;
    decoder->anim_background_argb                        = 0;
    memset(decoder->clear_argb, 0, sizeof(decoder->clear_argb));
    decoder->clear_yuva[0]  = RGB_TO_Y_CCIR(0, 0, 0);
    decoder->clear_yuva[1]  = RGB_TO_U_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[2]  = RGB_TO_V_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[3]  = 0;
    decoder->info_has_alpha = 0;
    decoder->info_coding    = WPD_CODING_UNKNOWN;
    decoder->status         = WPD_OK;
    decoder->error[0]       = 0;
}

/* Drops input the decoder can no longer look at. The chunk at 'pos' is kept
   whole: a VP8 chunk decoded row by row keeps range coders pointing into it
   until the frame is done, and those are rebased on the next step. */
static void file_compact(WPDDecoder *decoder) {
    size_t keep = decoder->pos;

    if (decoder->alpha_pending && decoder->alpha_data_offset < keep)
        keep = decoder->alpha_data_offset;
    if (keep < decoder->discarded || keep - decoder->discarded < 1 << 16)
        return;

    memmove(decoder->file_alloc,
            file_at(decoder, keep),
            decoder->file_size - keep + WPD_FILE_PADDING);
    decoder->file      = decoder->file_alloc;
    decoder->discarded = keep;
}

static WPDStatus file_reserve(WPDDecoder *decoder, size_t size) {
    const size_t buffered = file_buffered(decoder);
    const size_t needed   = buffered + size + WPD_FILE_PADDING;
    size_t       capacity;
    uint8_t     *grown;

    if (size > (size_t)INT_MAX - WPD_FILE_PADDING ||
        buffered > (size_t)INT_MAX - WPD_FILE_PADDING - size)
        return WPD_ERR_TOO_LARGE;
    if (decoder->file_capacity >= needed)
        return WPD_OK;

    capacity = decoder->file_capacity ? decoder->file_capacity : 1 << 16;
    while (capacity < needed) capacity *= 2;
    grown = realloc(decoder->file_alloc, capacity);
    if (!grown)
        return WPD_ERR_NO_MEMORY;
    decoder->file_alloc    = grown;
    decoder->file          = grown;
    decoder->file_capacity = capacity;
    return WPD_OK;
}

/* Takes a copy of each metadata chunk the scanner has reached, since the
   buffer it sits in is dropped as the stream moves past it. */
static WPDStatus capture_metadata(WPDDecoder *decoder) {
    const HeaderScan *hs = &decoder->scan;

    for (int i = 0; i < WPD_METADATA_NB; i++) {
        const size_t offset = hs->meta_offset[i];
        const size_t size   = hs->meta_size[i];

        if (!offset || decoder->meta[i])
            continue;
        if (offset < decoder->discarded || offset > decoder->file_size ||
            size > decoder->file_size - offset)
            continue;
        decoder->meta[i] = malloc(size);
        if (!decoder->meta[i])
            return WPD_ERR_NO_MEMORY;
        memcpy(decoder->meta[i], file_at(decoder, offset), size);
        decoder->meta_size[i] = size;
    }
    return WPD_OK;
}

static WPDStatus rescan_headers(WPDDecoder *decoder) {
    const HeaderScan *hs = &decoder->scan;
    WPDStatus         status, meta;

    decoder->scan.collect_frames = 1;
    status                       = scan_headers(&decoder->scan,
                                                decoder->file,
                                                decoder->discarded,
                                                decoder->file_size,
                                                decoder->streaming);
    meta                         = capture_metadata(decoder);

    if (status != WPD_OK)
        return status;
    if (meta != WPD_OK)
        return meta;

    decoder->end                  = hs->end;
    decoder->canvas_width         = hs->width;
    decoder->canvas_height        = hs->height;
    decoder->animation            = hs->animation;
    decoder->anim_frame_count     = hs->frame_count;
    decoder->anim_loop_count      = hs->loop_count;
    decoder->anim_background_argb = hs->background_argb;
    decoder->info_has_alpha       = hs->has_alpha;
    decoder->info_coding          = hs->coding;
    decoder->truncated            = hs->truncated;
    if (!decoder->headers_valid) {
        decoder->pos           = hs->raw_kind ? 0 : 12;
        decoder->headers_valid = 1;
    }
    return WPD_OK;
}

/* No more input is coming, so a chunk list that stops short of what it
   promised, or that never carried an image, cannot be completed. */
static WPDStatus check_final_headers(WPDDecoder *decoder, const char *message) {
    const HeaderScan *hs = &decoder->scan;

    if (hs->truncated)
        return set_error(decoder, message, WPD_ERR_TRUNCATED);
    if (!hs->images && !hs->frame_count)
        return set_error(decoder, "no image data found", WPD_ERR_BITSTREAM);
    return WPD_OK;
}

WPDStatus wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data,
                           size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);

    decoder_reset(decoder);

    status = file_reserve(decoder, size);
    if (status != WPD_OK)
        return set_error(decoder, "cannot buffer input", status);
    memcpy(decoder->file_alloc, data, size);
    memset(decoder->file_alloc + size, 0, WPD_FILE_PADDING);
    decoder->file      = decoder->file_alloc;
    decoder->file_size = size;
    decoder->discarded = 0;

    status = rescan_headers(decoder);
    if (status != WPD_OK) {
        decoder->file_size = 0;
        return set_error(decoder, "cannot read headers", status);
    }
    status = check_final_headers(decoder, "file ends inside a chunk");
    if (status != WPD_OK) {
        decoder->file_size     = 0;
        decoder->headers_valid = 0;
        return status;
    }
    decoder->opened = 1;
    decoder->eos    = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_open_borrowed(WPDDecoder *decoder, const uint8_t *data,
                                    size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);

    decoder_reset(decoder);
    decoder->file      = data;
    decoder->file_size = size;
    decoder->borrowed  = 1;

    status = rescan_headers(decoder);
    if (status != WPD_OK)
        status = set_error(decoder, "cannot read headers", status);
    else
        status = check_final_headers(decoder, "file ends inside a chunk");
    if (status != WPD_OK) {
        decoder->file          = decoder->file_alloc;
        decoder->file_size     = 0;
        decoder->borrowed      = 0;
        decoder->headers_valid = 0;
        return status;
    }
    decoder->opened = 1;
    decoder->eos    = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_open_stream(WPDDecoder *decoder) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    decoder_reset(decoder);
    decoder->opened    = 1;
    decoder->streaming = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_append(WPDDecoder *decoder, const uint8_t *data,
                             size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);
    if (!decoder->streaming || decoder->eos)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);
    if (!size)
        return WPD_OK;
    if (decoder->input_mode == 2)
        return set_error(
            decoder, "cannot mix append and update", WPD_ERR_INVALID_ARG);
    decoder->input_mode = 1;

    file_compact(decoder);
    status = file_reserve(decoder, size);
    if (status != WPD_OK)
        return set_error(decoder, "cannot buffer input", status);
    memcpy(decoder->file_alloc + file_buffered(decoder), data, size);
    decoder->file_size += size;
    memset(decoder->file_alloc + file_buffered(decoder), 0, WPD_FILE_PADDING);

    status = rescan_headers(decoder);
    /* Headers that are merely incomplete are the normal state of a stream. */
    if (status == WPD_ERR_TRUNCATED)
        return WPD_OK;
    if (status != WPD_OK)
        return set_error(decoder, "cannot read headers", status);
    return WPD_OK;
}

WPDStatus wpd_decoder_update(WPDDecoder *decoder, const uint8_t *data,
                             size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);
    if (!decoder->streaming || decoder->eos)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);
    if (decoder->input_mode == 1)
        return set_error(
            decoder, "cannot mix append and update", WPD_ERR_INVALID_ARG);
    if (size < decoder->file_size)
        return set_error(decoder, "stream buffer shrank", WPD_ERR_INVALID_ARG);

    decoder->input_mode = 2;
    decoder->borrowed   = 1;
    decoder->file       = data;
    decoder->file_size  = size;
    decoder->discarded  = 0;

    status = rescan_headers(decoder);
    if (status == WPD_ERR_TRUNCATED)
        return WPD_OK;
    if (status != WPD_OK) {
        decoder->file          = decoder->file_alloc;
        decoder->file_size     = 0;
        decoder->borrowed      = 0;
        decoder->headers_valid = 0;
        return set_error(decoder, "cannot read headers", status);
    }
    return WPD_OK;
}

WPDStatus wpd_decoder_end_of_stream(WPDDecoder *decoder) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!decoder->streaming)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);

    decoder->eos = 1;
    status       = rescan_headers(decoder);
    if (status != WPD_OK)
        return set_error(decoder, "cannot read headers", status);
    return check_final_headers(decoder, "stream ended early");
}

WPDStatus wpd_decoder_get_info(const WPDDecoder *decoder, WPDImageInfo *info) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!info_valid(info) || !decoder->opened)
        return set_error((WPDDecoder *)decoder,
                         "invalid decoder state",
                         WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid)
        return set_error(
            (WPDDecoder *)decoder, "headers incomplete", WPD_ERR_TRUNCATED);

    info_clear(info);
    info->width           = decoder->canvas_width;
    info->height          = decoder->canvas_height;
    info->has_alpha       = decoder->info_has_alpha;
    info->is_animation    = decoder->animation;
    info->frame_count     = decoder->anim_frame_count;
    info->loop_count      = decoder->anim_loop_count;
    info->background_argb = decoder->anim_background_argb;
    info->coding          = decoder->info_coding;
    info->metadata        = decoder->scan.metadata;
    return WPD_OK;
}

WPDStatus wpd_decoder_rewind(WPDDecoder *decoder) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!decoder->opened || !decoder->headers_valid)
        return set_error(decoder, "invalid decoder state", WPD_ERR_INVALID_ARG);
    /* wpd_decoder_append() is free to drop bytes the decoder has moved past,
       so the head of the file may simply no longer be there. */
    if (decoder->input_mode == 1)
        return set_error(decoder,
                         "an appended stream cannot be rewound",
                         WPD_ERR_UNSUPPORTED);

    anim_state_reset(decoder);
    decoder->pos      = decoder->scan.raw_kind ? 0 : 12;
    decoder->status   = WPD_OK;
    decoder->error[0] = 0;
    return WPD_OK;
}

/* The oldest WPDFrameInfo this build accepts, and equally how much of the
   caller's struct it may touch. Appending a field leaves this where it is and
   adds a longer extent above it, the way frame_extent() does for WPDFrame, so
   a caller compiled against the shorter struct keeps working. */
#define WPD_FRAME_INFO_V1 WPD_FIELD_END(WPDFrameInfo, complete)

static int frame_info_valid(const WPDFrameInfo *info) {
    return info && info->struct_size >= WPD_FRAME_INFO_V1;
}

WPDStatus wpd_decoder_frame_info(const WPDDecoder *decoder, int index,
                                 WPDFrameInfo *info) {
    const HeaderScan *hs;
    const size_t      struct_size = info ? info->struct_size : 0;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_info_valid(info) || !decoder->opened)
        return set_error((WPDDecoder *)decoder,
                         "invalid decoder state",
                         WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid)
        return set_error(
            (WPDDecoder *)decoder, "headers incomplete", WPD_ERR_TRUNCATED);

    hs = &decoder->scan;
    memset((uint8_t *)info + sizeof(info->struct_size),
           0,
           WPD_FRAME_INFO_V1 - sizeof(info->struct_size));
    info->struct_size = struct_size;

    /* A still image is one frame covering the whole canvas, which is what
       libwebp's demuxer reports for it too. */
    if (!decoder->animation) {
        if (index != 0)
            return set_error(
                (WPDDecoder *)decoder, "no such frame", WPD_ERR_INVALID_ARG);
        info->width  = decoder->canvas_width;
        info->height = decoder->canvas_height;
        /* The image's own alpha, not the VP8X declaration WPDImageInfo
           reports, so that this agrees with the frame decoding produces. */
        info->has_alpha = hs->image_has_alpha;
        info->complete  = hs->raw_kind ? decoder->eos : hs->images != 0;
        return WPD_OK;
    }

    if (index < 0 || index >= hs->nb_frames + hs->partial_frame)
        return set_error(
            (WPDDecoder *)decoder, "no such frame", WPD_ERR_INVALID_ARG);

    info->pos_x     = hs->frames[index].pos_x;
    info->pos_y     = hs->frames[index].pos_y;
    info->width     = hs->frames[index].width;
    info->height    = hs->frames[index].height;
    info->duration  = hs->frames[index].duration;
    info->dispose   = hs->frames[index].dispose;
    info->blend     = hs->frames[index].blend;
    info->has_alpha = hs->frames[index].has_alpha;
    info->complete  = hs->frames[index].complete;
    return WPD_OK;
}

WPDStatus wpd_decoder_metadata(const WPDDecoder *decoder, WPDMetadata which,
                               const uint8_t **data, size_t *size) {
    int index;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data || !size || !decoder->opened)
        return set_error((WPDDecoder *)decoder,
                         "invalid decoder state",
                         WPD_ERR_INVALID_ARG);
    if (which <= 0 || (which & (which - 1)) || which >> WPD_METADATA_NB)
        return set_error((WPDDecoder *)decoder,
                         "invalid metadata type",
                         WPD_ERR_INVALID_ARG);

    for (index = 0; !(which >> index & 1); index++) continue;
    *data = decoder->meta[index];
    *size = decoder->meta_size[index];
    return WPD_OK;
}

static int still_lossy_pending(const WPDDecoder *decoder, uint32_t chunk_type) {
    return chunk_type == MKTAG('V', 'P', '8', ' ') && !decoder->animation &&
        !decoder->still_done;
}

static int still_lossless_pending(const WPDDecoder *decoder,
                                  uint32_t          chunk_type) {
    return chunk_type == MKTAG('V', 'P', '8', 'L') && !decoder->animation &&
        !decoder->still_done;
}

static int emit_still_lossless(WPDDecoder *decoder, WPDFrame *frame) {
    int ret;

    decoder->still_done = 1;
    if (options_transform(decoder))
        ret = export_packed(decoder, decoder->lossless_frame, frame);
    else
        ret = export_still_lossless(
            decoder, frame, decoder->lossless_frame->height);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int emit_still_lossy(WPDDecoder *decoder, WPDFrame *frame) {
    int ret;

    decoder->still_done = 1;
    if (options_transform(decoder))
        ret = export_packed(decoder, &decoder->subframe, frame);
    else if (format_is_packed(decoder->out_format))
        ret = export_still_packed(decoder, frame, decoder->subframe.height);
    else
        ret = export_packed(decoder, &decoder->subframe, frame);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int decode_raw(WPDDecoder *decoder, WPDFrame *frame) {
    const HeaderScan *hs   = &decoder->scan;
    const uint8_t    *data = file_at(decoder, hs->raw_image_offset);
    int               ret;

    if (!decoder->eos)
        return 0;
    if (hs->truncated)
        return set_error(decoder, "raw image is truncated", WPD_ERR_TRUNCATED);
    if (hs->raw_image_size > INT_MAX)
        return set_error(decoder, "raw image is too large", WPD_ERR_TOO_LARGE);

    decoder->width = decoder->height = 0;
    if (hs->raw_kind == 1) {
        ret = vp8_lossless_decode_frame(
            decoder, &decoder->argb, data, (unsigned)hs->raw_image_size, 0);
        if (ret < 0)
            return set_error(decoder, "VP8L decode failed", ret);
        decoder->still_done     = 1;
        decoder->still_lossless = 1;
        decoder->lossless_frame = &decoder->argb;
        decoder->converted_rows = decoder->argb.height;
        ret                     = export_packed(decoder, &decoder->argb, frame);
    } else {
        if (hs->raw_kind == 3) {
            const uint8_t *alpha = file_at(decoder, hs->raw_alpha_offset);
            int            header;

            if (!hs->raw_alpha_size)
                return set_error(
                    decoder, "invalid ALPHA chunk", WPD_ERR_BITSTREAM);
            header = alpha[0];
            if ((header & 3) > ALPHA_COMPRESSION_VP8L)
                return set_error(decoder,
                                 "unsupported ALPHA compression",
                                 WPD_ERR_UNSUPPORTED);
            decoder->has_alpha         = 1;
            decoder->alpha_compression = header & 3;
            decoder->alpha_filter      = header >> 2 & 3;
            decoder->alpha_data_offset = hs->raw_alpha_offset + 1;
            decoder->alpha_data_size   = (int)hs->raw_alpha_size - 1;
        }
        ret = vp8_lossy_decode_frame(
            decoder, &decoder->subframe, data, (unsigned)hs->raw_image_size);
        if (ret < 0)
            return set_error(decoder, "VP8 decode failed", ret);
        decoder->still_done = 1;
        ret                 = export_packed(decoder, &decoder->subframe, frame);
    }
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_valid(frame))
        return set_error(decoder, "invalid frame", WPD_ERR_INVALID_ARG);
    if (!decoder->opened)
        return set_error(decoder, "no file opened", WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid) {
        if (!decoder->eos)
            return 0; /* the headers have not arrived yet */
        return set_error(decoder, "no image data found", WPD_ERR_TRUNCATED);
    }
    if (decoder->scan.raw_kind)
        return decoder->still_done ? 0 : decode_raw(decoder, frame);

    while (decoder->pos + 8 <= decoder->end) {
        const size_t   chunk_pos  = decoder->pos;
        const uint8_t *chunk      = file_at(decoder, chunk_pos);
        uint32_t       chunk_type = WPD_RL32(chunk);
        uint32_t       size       = WPD_RL32(chunk + 4);
        uint32_t       padded_size;
        const uint8_t *payload = chunk + 8;
        int            ret;

        if (size == UINT32_MAX)
            return set_error(
                decoder, "invalid chunk size", WPD_ERROR_INVALID_DATA);
        padded_size = size + (size & 1);

        if (decoder->end - (decoder->pos + 8) < padded_size) {
            if (!decoder->eos) {
                if (still_lossy_pending(decoder, chunk_type)) {
                    ret = vp8_lossy_step(
                        decoder,
                        &decoder->subframe,
                        payload,
                        (unsigned)(decoder->end - (decoder->pos + 8)),
                        size);
                    if (ret < 0)
                        return set_error(decoder, "VP8 decode failed", ret);
                    if (ret)
                        return emit_still_lossy(decoder, frame);
                } else if (still_lossless_pending(decoder, chunk_type)) {
                    ret = vp8l_still_step(
                        decoder,
                        payload,
                        (unsigned)(decoder->end - (decoder->pos + 8)),
                        size,
                        0);
                    if (ret < 0)
                        return set_error(decoder, "VP8L decode failed", ret);
                    if (ret)
                        return emit_still_lossless(decoder, frame);
                }
                return 0; /* the rest of this chunk has not arrived yet */
            }
            return set_error(decoder,
                             "chunk runs past the end of the file",
                             WPD_ERR_TRUNCATED);
        }
        decoder->pos += 8 + padded_size;

        switch (chunk_type) {
        case MKTAG('A', 'L', 'P', 'H'): {
            int alpha_header, filter_m, compression;

            if (size == 0)
                return set_error(decoder,
                                 "invalid ALPHA chunk size",
                                 WPD_ERROR_INVALID_DATA);
            alpha_header               = payload[0];
            decoder->alpha_data_offset = chunk_pos + 9;
            decoder->alpha_pending     = 1;
            decoder->alpha_data_size   = size - 1;

            filter_m    = (alpha_header >> 2) & 0x03;
            compression = alpha_header & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL,
                        WPD_LOG_WARNING,
                        "skipping unsupported ALPHA chunk\n");
            } else {
                decoder->has_alpha         = 1;
                decoder->alpha_compression = compression;
                decoder->alpha_filter      = filter_m;
            }
            break;
        }
        case MKTAG('V', 'P', '8', ' '):
            if (decoder->animation || decoder->still_done)
                break;
            if (decoder->vp8_active) {
                ret = vp8_lossy_step(
                    decoder, &decoder->subframe, payload, size, size);
                if (ret == 0)
                    ret = WPD_ERROR_INVALID_DATA;
            } else {
                decoder->width = decoder->height = 0;
                ret                              = vp8_lossy_decode_frame(
                    decoder, &decoder->subframe, payload, size);
            }
            if (ret < 0)
                return set_error(decoder, "VP8 decode failed", ret);
            return emit_still_lossy(decoder, frame);
        case MKTAG('V', 'P', '8', 'L'):
            if (decoder->animation || decoder->still_done)
                break;
            if (decoder->vp8l_active) {
                ret = vp8l_still_step(decoder, payload, size, size, 1);
                if (ret == 0)
                    ret = WPD_ERROR_INVALID_DATA;
                if (ret < 0)
                    return set_error(decoder, "VP8L decode failed", ret);
                return emit_still_lossless(decoder, frame);
            }
            decoder->width = decoder->height = 0;
            ret                              = vp8_lossless_decode_frame(
                decoder, &decoder->argb, payload, size, 0);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
            decoder->still_done = 1;
            ret                 = export_packed(decoder, &decoder->argb, frame);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            decoder->still_lossless = 1;
            decoder->lossless_frame = &decoder->argb;
            decoder->converted_rows = decoder->argb.height;
            return 1;
        case MKTAG('A', 'N', 'M', 'F'):
            if (!decoder->animation || !decoder->canvas_width ||
                !decoder->canvas_height)
                return set_error(decoder,
                                 "ANMF chunk without animation header",
                                 WPD_ERROR_INVALID_DATA);
            ret = decode_anmf(decoder, payload, size);
            if (ret < 0)
                return set_error(decoder, "animation frame decode failed", ret);
            ret = export_packed(decoder,
                                decoder->anim_mode == WPD_ANIM_SUBFRAME
                                    ? decoder->subframe_out
                                    : &decoder->canvas,
                                frame);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            return 1;
        default: break;
        }
    }

    return 0;
}

WPDStatus wpd_decoder_partial_frame(WPDDecoder *decoder, WPDFrame *frame,
                                    int *rows_valid) {
    int rows, ret;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_valid(frame))
        return set_error(decoder, "invalid frame", WPD_ERR_INVALID_ARG);
    if (!decoder->opened)
        return set_error(decoder, "no file opened", WPD_ERR_INVALID_ARG);

    frame_clear(frame);
    if (rows_valid)
        *rows_valid = 0;

    if (options_transform(decoder)) {
        if (decoder->still_lossless) {
            if (decoder->vp8l_active) {
                ret = vp8l_still_peek(decoder);
                if (ret < 0)
                    return set_error(decoder, "VP8L decode failed", ret);
            }
            rows = decoder->vp8l_active ? decoder->vp8l_rows_out
                                        : decoder->lossless_frame->height;
            if (rows < decoder->lossless_frame->height)
                return WPD_OK;
            ret = export_packed(decoder, decoder->lossless_frame, frame);
        } else if (decoder->still_lossy) {
            rows = decoder->vp8_active ? vp8_rows_finalized(&decoder->codec)
                                       : decoder->subframe.height;
            if (rows < decoder->subframe.height)
                return WPD_OK;
            ret = export_packed(decoder, &decoder->subframe, frame);
        } else {
            return WPD_OK;
        }
        if (ret < 0)
            return set_error(decoder, "cannot output frame", ret);
        if (rows_valid)
            *rows_valid = frame->height;
        return WPD_OK;
    }

    if (decoder->still_lossless) {
        if (decoder->vp8l_active) {
            ret = vp8l_still_peek(decoder);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
        }
        ret = export_still_lossless(decoder,
                                    frame,
                                    decoder->vp8l_active
                                        ? decoder->vp8l_rows_out
                                        : decoder->lossless_frame->height);
        if (ret < 0)
            return set_error(decoder, "cannot output frame", ret);
        if (rows_valid)
            *rows_valid = decoder->converted_rows;
        return WPD_OK;
    }
    if (!decoder->still_lossy)
        return WPD_OK;

    rows = decoder->vp8_active ? vp8_rows_finalized(&decoder->codec)
                               : decoder->subframe.height;

    if (!format_is_packed(decoder->out_format)) {
        const WPDPixelFormat format = decoder->out_format == WPD_PIX_FMT_NONE
            ? decoder->subframe.format
            : decoder->out_format;
        const WPDPixelFormat have   = decoder->subframe.format;
        const WebPImage     *plane  = &decoder->subframe;
        const int            first  = decoder->converted_format == format
            ? decoder->converted_rows
            : 0;

        if (rows < first)
            rows = first;
        if (have != WPD_PIX_FMT_YUVA420P && format != have) {
            ret = ensure_yuva_rows(decoder,
                                   &decoder->output,
                                   &decoder->subframe,
                                   format == WPD_PIX_FMT_YUVA420P,
                                   first,
                                   rows);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            plane = &decoder->output;
        }
        if (decoder->ext_active) {
            ret = export_external_planar_rows(
                decoder, plane, format, frame, first, rows);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
        } else {
            export_frame(decoder, plane, format, frame);
        }
        decoder->converted_rows   = rows;
        decoder->converted_format = format;
        if (rows_valid)
            *rows_valid = rows;
        return WPD_OK;
    }

    /* The fancy upsampler pairs a row with the one below it, so the last
       finished row cannot be converted until the row after it exists. */
    if (rows && rows < decoder->subframe.height)
        rows--;

    ret = export_still_packed(decoder, frame, rows);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    if (rows_valid)
        *rows_valid = decoder->converted_rows;
    return WPD_OK;
}

WPDStatus wpd_decoder_status(const WPDDecoder *decoder) {
    return decoder ? decoder->status : WPD_ERR_INVALID_ARG;
}

const char *wpd_decoder_error(const WPDDecoder *decoder) {
    return decoder && decoder->error[0] ? decoder->error
                                        : "unknown decoder error";
}

void wpd_decoder_free(WPDDecoder *decoder) {
    if (!decoder)
        return;
    if (decoder->vp8_initialized)
        vp8_decode_free(&decoder->codec);
    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    image_free(&decoder->converted);
    image_free(&decoder->output);
    image_free(&decoder->transformed);
    image_free(&decoder->alpha_argb);
    image_free(&decoder->lossless_out);
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&decoder->image[i]);
    for (int i = 0; i < WPD_METADATA_NB; i++) free(decoder->meta[i]);
    scan_free(&decoder->scan);
    free(decoder->rescale_work);
    free(decoder->rescale_row);
    free(decoder->lossless_top);
    free(decoder->alpha_plane);
    free(decoder->file_alloc);
    free(decoder);
}

typedef struct WPDFrameOwner {
    uint8_t *plane[4];
} WPDFrameOwner;

WPDStatus wpd_decode_into(const uint8_t *data, size_t size,
                          WPDPixelFormat           format,
                          const WPDDecoderOptions *options,
                          const WPDOutputBuffer *buffer, WPDFrame *frame) {
    WPDDecoder *decoder;
    WPDStatus   status;
    int         ret;

    if (!data || !buffer || !frame_valid(frame))
        return WPD_ERR_INVALID_ARG;
    if (frame->private_data)
        wpd_frame_free(frame);
    decoder = wpd_decoder_create();
    if (!decoder)
        return WPD_ERR_NO_MEMORY;
    status = options ? wpd_decoder_set_options(decoder, options) : WPD_OK;
    if (status == WPD_OK)
        status = wpd_decoder_set_output_format(decoder, format);
    if (status == WPD_OK)
        status = wpd_decoder_set_output_buffer(decoder, buffer);
    if (status == WPD_OK)
        status = wpd_decoder_open_borrowed(decoder, data, size);
    ret = status == WPD_OK ? wpd_decoder_next_frame(decoder, frame) : status;
    if (ret == 0)
        status = WPD_ERR_BITSTREAM;
    else if (ret < 0)
        status = (WPDStatus)ret;
    wpd_decoder_free(decoder);
    return status;
}

WPDStatus wpd_decode(const uint8_t *data, size_t size, WPDPixelFormat format,
                     const WPDDecoderOptions *options, WPDFrame *frame) {
    WPDFrameOwner *owner;
    WPDDecoder    *decoder;
    WPDFrame       decoded = WPD_FRAME_INIT;
    WPDStatus      status;
    int            planes, ret;

    if (!data || !frame_valid(frame))
        return WPD_ERR_INVALID_ARG;
    if (frame->private_data)
        wpd_frame_free(frame);
    decoder = wpd_decoder_create();
    if (!decoder)
        return WPD_ERR_NO_MEMORY;
    status = options ? wpd_decoder_set_options(decoder, options) : WPD_OK;
    if (status == WPD_OK)
        status = wpd_decoder_set_output_format(decoder, format);
    if (status == WPD_OK)
        status = wpd_decoder_open_borrowed(decoder, data, size);
    ret = status == WPD_OK ? wpd_decoder_next_frame(decoder, &decoded) : status;
    if (ret <= 0) {
        wpd_decoder_free(decoder);
        return ret < 0 ? (WPDStatus)ret : WPD_ERR_BITSTREAM;
    }

    owner = calloc(1, sizeof(*owner));
    if (!owner) {
        wpd_decoder_free(decoder);
        return WPD_ERR_NO_MEMORY;
    }
    frame_clear(frame);
    frame_copy(frame, &decoded);
    frame->private_data = owner;
    planes              = decoded.format == WPD_PIX_FMT_YUVA420P ? 4
        : decoded.format == WPD_PIX_FMT_YUV420P                  ? 3
                                                                 : 1;
    for (int p = 0; p < planes; p++) {
        const int shift = p == 1 || p == 2;
        const int w = planes == 1 ? decoded.width * format_bpp(decoded.format)
                                  : CEIL_RSHIFT(decoded.width, shift);
        const int h = CEIL_RSHIFT(decoded.height, shift);
        size_t    bytes;

        if ((size_t)h > SIZE_MAX / (size_t)w) {
            status = WPD_ERR_TOO_LARGE;
            goto fail;
        }
        bytes           = (size_t)w * (size_t)h;
        owner->plane[p] = malloc(bytes);
        if (!owner->plane[p]) {
            status = WPD_ERR_NO_MEMORY;
            goto fail;
        }
        for (int y = 0; y < h; y++)
            memcpy(owner->plane[p] + (size_t)y * w,
                   decoded.data[p] + (ptrdiff_t)y * decoded.stride[p],
                   (size_t)w);
        frame->data[p]   = owner->plane[p];
        frame->stride[p] = w;
    }
    wpd_decoder_free(decoder);
    return WPD_OK;

fail:
    wpd_decoder_free(decoder);
    wpd_frame_free(frame);
    return status;
}

void wpd_frame_free(WPDFrame *frame) {
    WPDFrameOwner *owner;

    if (!frame_valid(frame))
        return;
    owner = frame->private_data;
    if (owner) {
        for (int p = 0; p < 4; p++) free(owner->plane[p]);
        free(owner);
    }
    frame_clear(frame);
}
