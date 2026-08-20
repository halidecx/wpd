#ifndef WPD_RESCALER_H
#define WPD_RESCALER_H

#include <stddef.h>
#include <stdint.h>

/* libwebp-compatible area rescaler; work holds 2 * dst_width * num_channels u32s. */
void wpd_rescale_plane(uint8_t *dst, int dst_stride, int dst_width,
                       int dst_height, const uint8_t *src, int src_stride,
                       int src_width, int src_height, int num_channels,
                       uint32_t *work);

#endif
