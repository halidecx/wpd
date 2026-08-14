
#include "anim.h"
#include "lossy.h"

static void composite_region(WPDDecoder *s, const WebPImage *frame, SubRect r,
                             int blend) {
    WebPImage *canvas = &s->canvas;

    if (r.w <= 0 || r.h <= 0)
        return;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        if (blend)
            blend_argb_region(
                &s->ldsp, s->premultiply, canvas, frame, r, s->pos_x, s->pos_y);
        else
            copy_argb_region(canvas, frame, r, s->pos_x, s->pos_y);
    } else {
        if (blend)
            blend_yuva_region(canvas, frame, r, s->pos_x, s->pos_y);
        else
            copy_yuva_region(canvas, frame, r, s->pos_x, s->pos_y);
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
            ret = convert_to_argb(&s->ydsp,
                                  &s->canvas,
                                  &yuva_canvas,
                                  s->options.no_fancy_upsampling);
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

int decode_anmf(WPDDecoder *s, const uint8_t *data, size_t size) {
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
            lossless_canvas_in(s);
            ret = vp8l_decode_frame(
                s->vp8l, VP8L_TARGET_ARGB, &s->argb, p, payload_size, 0);
            lossless_canvas_out(s);
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
        ret = convert_to_argb(
            &s->ydsp, &s->converted, sub, s->options.no_fancy_upsampling);
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
        !(premultiply_after_pack(s->animation, s->anim_mode) &&
          format_bpp(s->out_format) == 2))
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
