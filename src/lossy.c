
#include "lossy.h"

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
        int direct;

        lossless_canvas_in(s);
        vp8l_set_alpha_dst(s->vp8l, p->data[3], p->linesize[3]);
        ret    = vp8l_decode_frame(s->vp8l,
                                   VP8L_TARGET_ALPHA,
                                   &s->alpha_argb,
                                   data_start,
                                   data_size,
                                   1);
        direct = vp8l_alpha_dst_used(s->vp8l);
        vp8l_set_alpha_dst(s->vp8l, NULL, 0);
        if (ret < 0)
            return ret;

        if (!direct)
            for (y = 0; y < s->height; y++)
                s->ldsp.extract_green(p->data[3] + p->linesize[3] * y,
                                      GET_PIXEL(&s->alpha_argb, 0, y),
                                      s->width);
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
    ret = vp8_decode_init(&s->codec);
    if (ret < 0)
        return ret;
    s->vp8_initialized = 1;
    return 0;
}

/* libwebp rounds an inferred dimension up, not to nearest. */
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
            &s->options,
            s->options.use_cropping ? s->options.crop_width : s->canvas_width,
            s->options.use_cropping ? s->options.crop_height : s->canvas_height,
            &width,
            &height) < 0)
        return;
    if (width < s->canvas_width * 3 / 4 && height < s->canvas_height * 3 / 4)
        s->codec.bypass_filtering = 1;
}

/* Returns 1 when the frame is complete, 0 when more of the chunk is needed. */
int vp8_lossy_step(WPDDecoder *s, WebPImage *out, const uint8_t *data_start,
                   unsigned int avail, unsigned int data_size) {
    WpdFrame current, decoded;
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
        vp8_current_frame(&s->codec, &current);
        vp8_lossy_export_planes(s, out, &current);
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

int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                           const uint8_t *data_start, unsigned int data_size) {
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
