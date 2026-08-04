#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WPDDecoder WPDDecoder;

typedef struct WPDFrame {
    const uint8_t *data[3];
    ptrdiff_t stride[3];
    int width;
    int height;
} WPDFrame;

WPDDecoder *wpd_decoder_create(void);

/*
 * Decode one VP8 keyframe, the bitstream carried by a lossy WebP file.
 * Returns 0 and fills in frame on success, -1 on failure.
 */
int wpd_decoder_decode(WPDDecoder *decoder,
                         const uint8_t *data, size_t size,
                         WPDFrame *frame);

/* A short description of the last failure, owned by decoder. */
const char *wpd_decoder_error(const WPDDecoder *decoder);

void wpd_decoder_free(WPDDecoder *decoder);

#ifdef __cplusplus
}
#endif
#endif
