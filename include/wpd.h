#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WPDDecoder WPDDecoder;

typedef enum WPDPixelFormat {
    WPD_PIX_FMT_YUV420P,  /* planar Y, U, V; chroma subsampled 2x2 */
    WPD_PIX_FMT_YUVA420P, /* planar Y, U, V, A; full-size alpha plane */
    WPD_PIX_FMT_ARGB,     /* packed 8:8:8:8, byte order A, R, G, B */
} WPDPixelFormat;

typedef struct WPDFrame {
    const uint8_t *data[4];
    ptrdiff_t stride[4];
    int width;
    int height;
    WPDPixelFormat format;
} WPDFrame;

WPDDecoder *wpd_decoder_create(void);

/*
 * Start decoding a complete WebP file (still or animated, lossy or
 * lossless). The decoder keeps its own copy of the data.
 * Returns 0 on success, -1 on failure.
 */
int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data, size_t size);

/*
 * Decode the next frame. A still image produces exactly one frame; an
 * animation produces one frame per ANMF chunk, each a fully composited
 * canvas. The frame data is owned by the decoder and is valid until the
 * next call into it.
 * Returns 1 and fills in frame on success, 0 at end of file, -1 on failure.
 */
int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

/* A short description of the last failure, owned by decoder. */
const char *wpd_decoder_error(const WPDDecoder *decoder);

void wpd_decoder_free(WPDDecoder *decoder);

#ifdef __cplusplus
}
#endif
#endif
