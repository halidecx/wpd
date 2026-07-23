#ifndef FFVP8_INTERNAL_H
#define FFVP8_INTERNAL_H

#include "compat.h"

int vp8_decode_init(AVCodecContext *context);
int vp8_decode_frame(AVCodecContext *context, void *frame, int *got_frame,
                     AVPacket *packet);
int vp8_decode_free(AVCodecContext *context);

#endif
