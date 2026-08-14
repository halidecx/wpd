
#include "wpd.h"

#include "anim.h"
#include "container.h"
#include "convert.h"
#include "export.h"
#include "image.h"
#include "lossy.h"
#include "rescaler.h"
#include "vp8.h"
#include "vp8l_dsp.h"
#include "wpd_codec.h"
#include "wpd_dec.h"
#include "wpd_internal.h"
#include "yuvdsp.h"

#include <stdio.h>
#include <stdlib.h>

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
size_t frame_extent(const WPDFrame *frame) {
    return frame->struct_size >= WPD_FIELD_END(WPDFrame, has_alpha)
        ? WPD_FIELD_END(WPDFrame, has_alpha)
        : WPD_FIELD_END(WPDFrame, private_data);
}

/* The decoder's answers to the questions the export asks, gathered at the call
   rather than reached for: the export owns no decoder state, so nothing it
   sees can drift from what the frame was decoded as. */
static ExportSettings export_settings(const WPDDecoder *s) {
    ExportSettings set = {
        .out_format  = s->out_format,
        .premultiply = s->premultiply,
        .animation   = s->animation,
        .anim_mode   = s->anim_mode,
        .ext_active  = s->ext_active,
        .duration    = s->frame_duration,
        .pos_x       = s->pos_x,
        .pos_y       = s->pos_y,
        .anmf_flags  = s->anmf_flags,
        /* An animation latches each sub-frame's alpha as it decodes it; a
           still has only the one image, whose two decoders report it
           separately. */
        .has_alpha = s->animation ? s->frame_has_alpha
                                  : s->has_alpha || s->lossless_has_alpha,
        .timestamp = s->frame_timestamp - s->frame_duration,
    };

    return set;
}

static ExportTargets export_targets(WPDDecoder *s) {
    ExportTargets t = {
        .dsp              = &s->ydsp,
        .options          = &s->options,
        .rescale          = &s->rescale,
        .transformed      = &s->transformed,
        .output           = &s->output,
        .converted        = &s->converted,
        .ext              = s->ext,
        .converted_rows   = &s->converted_rows,
        .converted_format = &s->converted_format,
    };

    return t;
}

void frame_clear(WPDFrame *frame) {
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
    decoder->vp8l = vp8l_alloc();
    decoder->scan = scan_alloc();
    if (!decoder->vp8l || !decoder->scan) {
        vp8l_free(&decoder->vp8l);
        scan_free(&decoder->scan);
        free(decoder);
        return NULL;
    }
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
    if (mode == WPD_ANIM_SUBFRAME && options_transform(&decoder->options))
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
    vp8l_reset(decoder->vp8l);
    image_free(&decoder->canvas);
    decoder->still_done       = 0;
    decoder->vp8_active       = 0;
    decoder->still_lossy      = 0;
    decoder->alpha_pending    = 0;
    decoder->converted_rows   = 0;
    decoder->converted_format = WPD_PIX_FMT_NONE;
    decoder->still_lossless   = 0;
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
    vp8l_release(decoder->vp8l);
    image_free(&decoder->converted);
    image_free(&decoder->output);
    image_free(&decoder->transformed);
    memset(&decoder->subframe, 0, sizeof(decoder->subframe));
    memset(&decoder->argb, 0, sizeof(decoder->argb));
    memset(&decoder->alpha_argb, 0, sizeof(decoder->alpha_argb));
    decoder->file_size = 0;
    decoder->discarded = 0;
    decoder->file      = decoder->file_alloc;
    scan_reset(decoder->scan);
    memset(&decoder->scanned, 0, sizeof(decoder->scanned));
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
    const ScanInfo *hs = &decoder->scanned;

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
    const ScanInfo *hs = &decoder->scanned;
    WPDStatus       status, meta;

    status = scan_headers(decoder->scan,
                          decoder->file,
                          decoder->discarded,
                          decoder->file_size,
                          decoder->streaming,
                          1);
    /* Read back whatever the walk reached, error or not: a stream whose
       headers are merely incomplete keeps decoding from what has arrived. */
    scan_info(decoder->scan, &decoder->scanned);
    meta = capture_metadata(decoder);

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
    const ScanInfo *hs = &decoder->scanned;

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
    info->metadata        = decoder->scanned.metadata;
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
    decoder->pos      = decoder->scanned.raw_kind ? 0 : 12;
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
    const ScanInfo *hs;
    FrameEntry      entry;
    const size_t    struct_size = info ? info->struct_size : 0;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_info_valid(info) || !decoder->opened)
        return set_error((WPDDecoder *)decoder,
                         "invalid decoder state",
                         WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid)
        return set_error(
            (WPDDecoder *)decoder, "headers incomplete", WPD_ERR_TRUNCATED);

    hs = &decoder->scanned;
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

    if (!scan_frame(decoder->scan, index, &entry))
        return set_error(
            (WPDDecoder *)decoder, "no such frame", WPD_ERR_INVALID_ARG);

    info->pos_x     = entry.pos_x;
    info->pos_y     = entry.pos_y;
    info->width     = entry.width;
    info->height    = entry.height;
    info->duration  = entry.duration;
    info->dispose   = entry.dispose;
    info->blend     = entry.blend;
    info->has_alpha = entry.has_alpha;
    info->complete  = entry.complete;
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

/* The resumable lossless path, plus the copies the container keeps of what it
   left behind: which picture is being filled in, and whether there is one. */
static int lossless_step(WPDDecoder *decoder, const uint8_t *payload,
                         unsigned avail, unsigned size, int complete) {
    int ret;

    lossless_canvas_in(decoder);
    ret = vp8l_still_step(decoder->vp8l, payload, avail, size, complete);
    lossless_canvas_out(decoder);
    if (ret >= 0 && (vp8l_still_active(decoder->vp8l) || ret == 1)) {
        decoder->still_lossless = 1;
        vp8l_still_frame(decoder->vp8l, &decoder->argb);
        decoder->lossless_frame = &decoder->argb;
    }
    return ret;
}

static int lossless_peek(WPDDecoder *decoder) {
    int ret = vp8l_still_peek(decoder->vp8l);

    if (ret >= 0) {
        vp8l_still_frame(decoder->vp8l, &decoder->argb);
        decoder->lossless_frame = &decoder->argb;
    }
    return ret;
}

static int emit_still_lossless(WPDDecoder *decoder, WPDFrame *frame) {
    const ExportSettings set = export_settings(decoder);
    const ExportTargets  t   = export_targets(decoder);
    int                  ret;

    decoder->still_done = 1;
    if (options_transform(&decoder->options))
        ret = export_packed(&set, &t, decoder->lossless_frame, frame);
    else
        ret = export_still_lossless(&set,
                                    &t,
                                    decoder->lossless_frame,
                                    frame,
                                    decoder->lossless_frame->height);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int emit_still_lossy(WPDDecoder *decoder, WPDFrame *frame) {
    const ExportSettings set = export_settings(decoder);
    const ExportTargets  t   = export_targets(decoder);
    int                  ret;

    decoder->still_done = 1;
    if (options_transform(&decoder->options))
        ret = export_packed(&set, &t, &decoder->subframe, frame);
    else if (format_is_packed(decoder->out_format))
        ret = export_still_packed(
            &set, &t, &decoder->subframe, frame, decoder->subframe.height);
    else
        ret = export_packed(&set, &t, &decoder->subframe, frame);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int decode_raw(WPDDecoder *decoder, WPDFrame *frame) {
    const ScanInfo *hs   = &decoder->scanned;
    const uint8_t  *data = file_at(decoder, hs->raw_image_offset);
    int             ret;

    if (!decoder->eos)
        return 0;
    if (hs->truncated)
        return set_error(decoder, "raw image is truncated", WPD_ERR_TRUNCATED);
    if (hs->raw_image_size > INT_MAX)
        return set_error(decoder, "raw image is too large", WPD_ERR_TOO_LARGE);

    decoder->width = decoder->height = 0;
    if (hs->raw_kind == 1) {
        lossless_canvas_in(decoder);
        ret = vp8l_decode_frame(decoder->vp8l,
                                VP8L_TARGET_ARGB,
                                &decoder->argb,
                                data,
                                (unsigned)hs->raw_image_size,
                                0);
        lossless_canvas_out(decoder);
        if (ret < 0)
            return set_error(decoder, "VP8L decode failed", ret);
        decoder->still_done     = 1;
        decoder->still_lossless = 1;
        decoder->lossless_frame = &decoder->argb;
        decoder->converted_rows = decoder->argb.height;
        {
            const ExportSettings set = export_settings(decoder);
            const ExportTargets  t   = export_targets(decoder);

            ret = export_packed(&set, &t, &decoder->argb, frame);
        }
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
        {
            const ExportSettings set = export_settings(decoder);
            const ExportTargets  t   = export_targets(decoder);

            ret = export_packed(&set, &t, &decoder->subframe, frame);
        }
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
    if (decoder->scanned.raw_kind)
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
                    ret = lossless_step(
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
            if (vp8l_still_active(decoder->vp8l)) {
                ret = lossless_step(decoder, payload, size, size, 1);
                if (ret == 0)
                    ret = WPD_ERROR_INVALID_DATA;
                if (ret < 0)
                    return set_error(decoder, "VP8L decode failed", ret);
                return emit_still_lossless(decoder, frame);
            }
            decoder->width = decoder->height = 0;
            lossless_canvas_in(decoder);
            ret = vp8l_decode_frame(decoder->vp8l,
                                    VP8L_TARGET_ARGB,
                                    &decoder->argb,
                                    payload,
                                    size,
                                    0);
            lossless_canvas_out(decoder);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
            decoder->still_done = 1;
            {
                const ExportSettings set = export_settings(decoder);
                const ExportTargets  t   = export_targets(decoder);

                ret = export_packed(&set, &t, &decoder->argb, frame);
            }
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
            {
                const ExportSettings set = export_settings(decoder);
                const ExportTargets  t   = export_targets(decoder);

                ret = export_packed(&set,
                                    &t,
                                    decoder->anim_mode == WPD_ANIM_SUBFRAME
                                        ? decoder->subframe_out
                                        : &decoder->canvas,
                                    frame);
            }
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

    const ExportSettings set = export_settings(decoder);
    const ExportTargets  t   = export_targets(decoder);

    frame_clear(frame);
    if (rows_valid)
        *rows_valid = 0;

    if (options_transform(&decoder->options)) {
        if (decoder->still_lossless) {
            if (vp8l_still_active(decoder->vp8l)) {
                ret = lossless_peek(decoder);
                if (ret < 0)
                    return set_error(decoder, "VP8L decode failed", ret);
            }
            rows = vp8l_still_active(decoder->vp8l)
                ? vp8l_still_rows_out(decoder->vp8l)
                : decoder->lossless_frame->height;
            if (rows < decoder->lossless_frame->height)
                return WPD_OK;
            ret = export_packed(&set, &t, decoder->lossless_frame, frame);
        } else if (decoder->still_lossy) {
            rows = decoder->vp8_active ? vp8_rows_finalized(&decoder->codec)
                                       : decoder->subframe.height;
            if (rows < decoder->subframe.height)
                return WPD_OK;
            ret = export_packed(&set, &t, &decoder->subframe, frame);
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
        if (vp8l_still_active(decoder->vp8l)) {
            ret = lossless_peek(decoder);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
        }
        ret = export_still_lossless(&set,
                                    &t,
                                    decoder->lossless_frame,
                                    frame,
                                    vp8l_still_active(decoder->vp8l)
                                        ? vp8l_still_rows_out(decoder->vp8l)
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
            ret = ensure_yuva_rows(&decoder->ydsp,
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
                &set, &t, plane, format, frame, first, rows);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
        } else {
            export_frame(&set, plane, format, frame);
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

    ret = export_still_packed(&set, &t, &decoder->subframe, frame, rows);
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
    vp8l_free(&decoder->vp8l);
    image_free(&decoder->canvas);
    image_free(&decoder->converted);
    image_free(&decoder->output);
    image_free(&decoder->transformed);
    for (int i = 0; i < WPD_METADATA_NB; i++) free(decoder->meta[i]);
    scan_free(&decoder->scan);
    image_scratch_free(&decoder->rescale);
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
