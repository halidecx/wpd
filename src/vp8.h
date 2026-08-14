#ifndef WPD_VP8_H
#define WPD_VP8_H

#include "wpd_codec.h"

#define VP8_NEED_MORE 1

int  vp8_decode_init(WpdCodecContext *context);
int  vp8_decode_frame(WpdCodecContext *context, void *frame, WpdPacket *packet);
int  vp8_decode_frame_init(WpdCodecContext *context, const uint8_t *chunk,
                           int avail, int size);
void vp8_decode_extend(WpdCodecContext *context, const uint8_t *chunk,
                       int avail);
int  vp8_decode_rows(WpdCodecContext *context, void *frame);
int  vp8_rows_finalized(const WpdCodecContext *context);
int  vp8_decode_free(WpdCodecContext *context);
void vp8_current_frame(const WpdCodecContext *context, WpdFrame *frame);

#endif
