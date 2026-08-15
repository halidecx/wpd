#ifndef WPD_RESCALER_H
#define WPD_RESCALER_H

#include <stddef.h>
#include <stdint.h>

/* A row-at-a-time area rescaler, matching libwebp's WebPRescaler bit for bit.
   The state and the incremental import/export pair live in the Rust core; what
   is declared here is the one entry point tests/parity.c drives.

   Runs a whole plane through in one go. 'work' must hold
   2 * dst_width * num_channels uint32_t. */
void wpd_rescale_plane(uint8_t *dst, int dst_stride, int dst_width,
                       int dst_height, const uint8_t *src, int src_stride,
                       int src_width, int src_height, int num_channels,
                       uint32_t *work);

#endif
