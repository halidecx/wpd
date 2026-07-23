/*
 * Standalone FFmpeg-derived VP8 decoder API.
 *
 * The returned image is planar 8-bit 4:2:0 (Y, U, V).  Its storage belongs
 * to the decoder and remains valid until the next decode/reset/free call.
 */
#ifndef FFVP8_H
#define FFVP8_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FFVP8Decoder FFVP8Decoder;

typedef struct FFVP8Frame {
    const uint8_t *data[3];
    ptrdiff_t stride[3];
    int width;
    int height;
} FFVP8Frame;

FFVP8Decoder *ffvp8_decoder_create(void);

/*
 * Decode exactly one VP8 compressed frame. Returns 0 with a visible frame,
 * 1 when an invisible reference frame was consumed, and -1 on failure.
 */
int ffvp8_decoder_decode(FFVP8Decoder *decoder,
                         const uint8_t *data, size_t size,
                         FFVP8Frame *frame);

/* Drop reference pictures and restart at the next keyframe. */
void ffvp8_decoder_reset(FFVP8Decoder *decoder);

/* A short description of the last failure, owned by decoder. */
const char *ffvp8_decoder_error(const FFVP8Decoder *decoder);

void ffvp8_decoder_free(FFVP8Decoder *decoder);

#ifdef __cplusplus
}
#endif
#endif
