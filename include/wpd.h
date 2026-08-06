#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WPDDecoder WPDDecoder;

typedef enum WPDPixelFormat {
    WPD_PIX_FMT_YUV420P,
    WPD_PIX_FMT_YUVA420P,
    WPD_PIX_FMT_ARGB,
} WPDPixelFormat;

typedef struct WPDFrame {
    const uint8_t *data[4];
    ptrdiff_t      stride[4];
    int            width;
    int            height;
    WPDPixelFormat format;
} WPDFrame;

WPDDecoder *wpd_decoder_create(void);

int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data, size_t size);

int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

const char *wpd_decoder_error(const WPDDecoder *decoder);

void wpd_decoder_free(WPDDecoder *decoder);

#ifdef __cplusplus
}
#endif
#endif
