
#include "anim.h"
#include "lossy.h"

/* The decoder's answers to what the compositor asks, gathered at the call.
   'key_frame' is the one field it does not know yet: anim_is_key_frame()
   decides it from the rest. */
static Placement anim_placement(const WPDDecoder *s) {
    Placement pl = {
        .canvas_width        = s->canvas_width,
        .canvas_height       = s->canvas_height,
        .pos_x               = s->pos_x,
        .pos_y               = s->pos_y,
        .anmf_flags          = s->anmf_flags,
        .frame_index         = s->frame_index,
        .frame_has_alpha     = s->frame_has_alpha,
        .key_frame           = 0,
        .prev_anmf_flags     = s->prev_anmf_flags,
        .prev_width          = s->prev_width,
        .prev_height         = s->prev_height,
        .prev_pos_x          = s->prev_pos_x,
        .prev_pos_y          = s->prev_pos_y,
        .prev_key_frame      = s->prev_key_frame,
        .premultiply         = s->premultiply,
        .no_fancy_upsampling = s->options.no_fancy_upsampling,
    };

    memcpy(pl.clear_argb, s->clear_argb, 4);
    memcpy(pl.clear_yuva, s->clear_yuva, 4);
    return pl;
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
            s->alpha_data_offset = input_discarded(s->input) +
                (size_t)(p + 1 - input_at(s->input, input_discarded(s->input)));
            s->alpha_data_size = payload_size - 1;

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

    Placement pl = anim_placement(s);

    s->key_frame = anim_is_key_frame(&pl, sub->width, sub->height);
    pl.key_frame = s->key_frame;

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
        const CompositeTargets ct = {&s->ldsp, &s->ydsp, &s->canvas};

        ret = anim_composite(&pl, &ct, sub, target);
        if (ret < 0)
            return ret;
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
