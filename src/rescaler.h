#ifndef WPD_RESCALER_H
#define WPD_RESCALER_H

#include <stddef.h>
#include <stdint.h>

/* A row-at-a-time area rescaler, matching libwebp's WebPRescaler bit for bit.
   Rows are pushed in with wpd_rescaler_import() and pulled out with
   wpd_rescaler_export(); the caller drives both, since the number of input
   rows one output row needs varies. */

#define WPD_RESCALER_RFIX 32
#define WPD_RESCALER_ONE (1ull << WPD_RESCALER_RFIX)

typedef struct WPDRescaler {
    int      x_expand;
    int      y_expand;
    int      num_channels;
    uint32_t fx_scale;
    uint32_t fy_scale;
    uint32_t fxy_scale;
    int      y_accum;
    int      y_add, y_sub;
    int      x_add, x_sub;
    int      src_width, src_height;
    int      dst_width, dst_height;
    int      src_y, dst_y;
    uint8_t *dst;
    int      dst_stride;
    /* 2 * dst_width * num_channels entries, owned by the caller. */
    uint32_t *irow;
    uint32_t *frow;
} WPDRescaler;

void wpd_rescaler_init(WPDRescaler *r, int src_width, int src_height,
                       uint8_t *dst, int dst_width, int dst_height,
                       int dst_stride, int num_channels, uint32_t *work);

static inline int wpd_rescaler_input_done(const WPDRescaler *r) {
    return r->src_y >= r->src_height;
}

static inline int wpd_rescaler_output_done(const WPDRescaler *r) {
    return r->dst_y >= r->dst_height;
}

static inline int wpd_rescaler_has_pending_output(const WPDRescaler *r) {
    return !wpd_rescaler_output_done(r) && r->y_accum <= 0;
}

int  wpd_rescaler_needed_lines(const WPDRescaler *r, int max_num_lines);
int  wpd_rescaler_import(WPDRescaler *r, int num_lines, const uint8_t *src,
                         int src_stride);
void wpd_rescaler_export_row(WPDRescaler *r);
int  wpd_rescaler_export(WPDRescaler *r);

/* Runs a whole plane through in one go. 'work' must hold
   2 * dst_width * num_channels uint32_t. */
void wpd_rescale_plane(uint8_t *dst, int dst_stride, int dst_width,
                       int dst_height, const uint8_t *src, int src_stride,
                       int src_width, int src_height, int num_channels,
                       uint32_t *work);

/* libwebp scales alpha-bearing ARGB with the colour channels premultiplied,
   then undoes it on the way out. Both directions round the same way it does. */
void wpd_premultiply_argb_row(uint8_t *argb, int num_pixels, int inverse);

/* The same for one plane against a separate alpha plane, which is how a scaled
   YUVA frame's luma is carried across the rescaler. */
void wpd_multiply_row(uint8_t *plane, const uint8_t *alpha, int num_pixels,
                      int inverse);

#endif
