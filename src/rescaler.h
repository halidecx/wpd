#ifndef WPD_RESCALER_H
#define WPD_RESCALER_H

#include <stddef.h>
#include <stdint.h>

/* libwebp-compatible area rescaler; work holds 2 * dst_width * num_channels u32s. */
void wpd_rescale_plane(uint8_t *dst, int dst_stride, int dst_width,
                       int dst_height, const uint8_t *src, int src_stride,
                       int src_width, int src_height, int num_channels,
                       uint32_t *work);

/* The row kernels behind it. Imports fill frow from one source row; exports
 * turn the accumulator into one destination row. */
typedef void (*rescale_import_row_func)(uint32_t *frow, const uint8_t *src,
                                        int dst_width, int src_width,
                                        int num_channels, uint32_t x_add,
                                        uint32_t x_sub, uint32_t fx_scale);
typedef void (*rescale_export_expand_row_func)(uint8_t        *dst,
                                               const uint32_t *irow,
                                               const uint32_t *frow, int width,
                                               int y_accum, uint32_t y_sub,
                                               uint32_t fy_scale);
typedef void (*rescale_export_shrink_row_func)(uint8_t *dst, uint32_t *irow,
                                               const uint32_t *frow, int width,
                                               int y_accum, uint32_t fy_scale,
                                               uint32_t fxy_scale);

typedef struct WPDRESCALEDSP {
    rescale_import_row_func        import_row_expand;
    rescale_import_row_func        import_row_shrink;
    rescale_export_expand_row_func export_row_expand;
    rescale_export_shrink_row_func export_row_shrink;
} WPDRESCALEDSP;

void wpd_rescale_dsp_init(WPDRESCALEDSP *dsp);

#endif
