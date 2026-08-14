#ifndef WPD_LOSSY_H
#define WPD_LOSSY_H

#include "wpd_dec.h"

int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                           const uint8_t *data_start, unsigned int data_size);
int vp8_lossy_step(WPDDecoder *s, WebPImage *out, const uint8_t *data_start,
                   unsigned int avail, unsigned int data_size);
int scaled_size(const WPDDecoder *s, int src_width, int src_height, int *width,
                int *height);

#endif
