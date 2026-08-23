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

/* The same kernels as the assembly exposes them: counts in elements, no
 * ratio gate and no scalar fallback, so every entry is null unless this
 * build has assembly the running CPU can use. The table above cannot stand
 * in for these, because the wrappers it hands out rebuild their arguments
 * and would absorb anything a caller planted in the undefined halves. */
typedef struct WPDRESCALERAWDSP {
    void (*import_expand)(uint32_t *frow, const uint8_t *src, int n,
                          int src_width, int num_channels, int x_add,
                          int x_sub);
    void (*import_shrink)(uint32_t *frow, const uint8_t *src, int n, int x_add,
                          int x_sub, uint32_t fx_scale);
    void (*export_direct)(uint8_t *dst, const uint32_t *frow, int n,
                          uint32_t fy_scale);
    void (*export_blend)(uint8_t *dst, const uint32_t *irow,
                         const uint32_t *frow, int n, uint32_t fy_scale,
                         uint32_t wa, uint32_t wb);
    void (*export_shrink)(uint8_t *dst, uint32_t *irow, const uint32_t *frow,
                          int n, uint32_t yscale, uint32_t fxy_scale);
    void (*export_shrink0)(uint8_t *dst, uint32_t *irow, int n,
                           uint32_t fxy_scale);
} WPDRESCALERAWDSP;

void wpd_rescale_raw_dsp_init(WPDRESCALERAWDSP *dsp);

#endif
