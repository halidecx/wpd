
#include "export.h"

void export_frame(const WPDDecoder *s, const WebPImage *img,
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
    pack_row_func pack    = img->format == format
        ? NULL
        : format_packer(&s->ydsp, format);
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

int export_external_planar_rows(WPDDecoder *s, const WebPImage *img,
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

int export_packed(WPDDecoder *s, WebPImage *img, WPDFrame *frame) {
    const WPDPixelFormat format = s->out_format;
    WebPImage            view;
    WebPImage           *processed;
    WebPImage           *planar;
    pack_row_func        pack;
    int                  ret;

    ret = transform_image(&s->options,
                          &s->rescale,
                          &s->transformed,
                          img,
                          &view,
                          &processed,
                          format);
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
                &s->ydsp, &s->output, img, format == WPD_PIX_FMT_YUVA420P);
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
        ret = convert_to_packed(
            &s->ydsp,
            &s->output,
            img,
            format,
            s->options.no_fancy_upsampling,
            premultiply_after_pack(s->animation, s->anim_mode));
        if (ret < 0)
            return ret;
        img = &s->output;
    } else if (img->format != format) {
        pack = format_packer(&s->ydsp, format);
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
int export_still_packed(WPDDecoder *s, WPDFrame *frame, int upto) {
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
            const pack_row_func pack = format_packer(&s->ydsp, format);
            const premultiply_4444_row_func premultiply =
                format_premultiplier_4444(&s->ydsp, format);

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
int export_still_lossless(WPDDecoder *s, WPDFrame *frame, int upto) {
    const WPDPixelFormat format = s->out_format;
    WebPImage           *img    = s->lossless_frame;
    const int first = s->converted_format == format ? s->converted_rows : 0;
    const premultiply_4444_row_func premultiply = format_premultiplier_4444(
        &s->ydsp, format);
    pack_row_func pack;
    int           ret;

    if (upto < s->converted_rows)
        upto = s->converted_rows;

    if (format == WPD_PIX_FMT_YUV420P || format == WPD_PIX_FMT_YUVA420P) {
        ret = ensure_yuva_rows(&s->ydsp,
                               &s->output,
                               img,
                               format == WPD_PIX_FMT_YUVA420P,
                               first,
                               upto);
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

    pack = format_packer(&s->ydsp, format);
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
