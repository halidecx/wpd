#ifndef WPD_DEC_H
#define WPD_DEC_H

#include "container.h"
#include "rescaler.h"
#include "vp8.h"
#include "vp8l.h"
#include "vp8l_dsp.h"
#include "yuvdsp.h"

enum AlphaCompression {
    ALPHA_COMPRESSION_NONE,
    ALPHA_COMPRESSION_VP8L,
};

enum AlphaFilter {
    ALPHA_FILTER_NONE,
    ALPHA_FILTER_HORIZONTAL,
    ALPHA_FILTER_VERTICAL,
    ALPHA_FILTER_GRADIENT,
};

struct WPDDecoder {
    WpdCodecContext   codec;
    int               vp8_initialized;
    WPDLosslessDSP    ldsp;
    WPDYUVDSP         ydsp;
    WPDPixelFormat    out_format;
    int               premultiply;
    WPDDecoderOptions options;

    const uint8_t *file;
    uint8_t       *file_alloc;
    size_t         file_size;
    size_t         discarded;
    size_t         pos, end;
    HeaderScan    *scan;
    /* What the last scan found, refreshed on every rescan. */
    ScanInfo       scanned;
    int            animation;
    int            still_done;
    int            vp8_active;
    int            still_lossy;
    int            alpha_pending;
    int            converted_rows;
    WPDPixelFormat converted_format;
    int            still_lossless;
    int            frame_index;
    int            canvas_width, canvas_height;

    int                   has_alpha;
    enum AlphaCompression alpha_compression;
    enum AlphaFilter      alpha_filter;
    /* An offset, not a pointer: appending to a stream can move 'file'. */
    size_t   alpha_data_offset;
    int      alpha_data_size;
    uint8_t *alpha_plane;
    size_t   alpha_plane_size;

    VP8LContext *vp8l;
    int          width, height;
    int          lossless_has_alpha;

    /* Views of pictures the lossless decoder owns; never freed from here. */
    WebPImage  argb;
    WebPImage  alpha_argb;
    WebPImage *lossless_frame;
    WebPImage  subframe;
    WebPImage  converted;
    WebPImage  output;
    WebPImage  transformed;
    uint32_t  *rescale_work;
    size_t     rescale_work_size;
    uint8_t   *rescale_row;
    size_t     rescale_row_size;

    WebPImage canvas;
    /* The sub-frame WPD_ANIM_SUBFRAME hands out, borrowed from whichever of
       subframe, argb and converted decode_anmf() finished with. */
    WebPImage       *subframe_out;
    WPDAnimationMode anim_mode;
    int              anmf_flags, pos_x, pos_y;
    int              frame_has_alpha, key_frame;
    int     prev_anmf_flags, prev_width, prev_height, prev_pos_x, prev_pos_y;
    int     prev_key_frame;
    uint8_t clear_argb[4];
    uint8_t clear_yuva[4];

    int      anim_loop_count, anim_frame_count;
    uint32_t anim_background_argb;
    int      frame_duration;
    int64_t  frame_timestamp;

    int       info_has_alpha;
    WPDCoding info_coding;

    uint8_t *meta[WPD_METADATA_NB];
    size_t   meta_size[WPD_METADATA_NB];

    size_t file_capacity;
    int    opened;
    int    streaming;
    int    eos;
    int    headers_valid;
    int    truncated;
    int    borrowed;
    int    input_mode;

    WPDOutputPlane ext[4];
    int            ext_active;

    WPDStatus status;
    char      error[128];
};

static inline size_t file_buffered(const WPDDecoder *decoder) {
    return decoder->file_size - decoder->discarded;
}

static inline const uint8_t *file_at(const WPDDecoder *decoder, size_t offset) {
    return decoder->file + (offset - decoder->discarded);
}

static inline void update_canvas_size(WPDDecoder *s, int w, int h) {
    if (s->width && s->width != w)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Width mismatch. %d != %d\n", s->width, w);
    s->width = w;
    if (s->height && s->height != h)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Height mismatch. %d != %d\n", s->height, h);
    s->height = h;
}

/* The canvas is negotiated between the container and the lossless module: the
   container knows what the file declared, the module knows what the frame
   header said, and either may be the first to learn it. */
static inline void lossless_canvas_in(WPDDecoder *s) {
    vp8l_set_canvas(s->vp8l, s->width, s->height);
}

static inline void lossless_canvas_out(WPDDecoder *s) {
    s->width              = vp8l_width(s->vp8l);
    s->height             = vp8l_height(s->vp8l);
    s->lossless_has_alpha = vp8l_has_alpha(s->vp8l);
}

void   frame_clear(WPDFrame *frame);
size_t frame_extent(const WPDFrame *frame);

#endif
