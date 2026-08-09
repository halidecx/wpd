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
    /* Milliseconds this frame is shown, and since the animation started.
       Both are 0 for still images. */
    int     duration;
    int64_t timestamp;
} WPDFrame;

typedef struct WPDAnimInfo {
    int canvas_width;
    int canvas_height;
    int frame_count;
    int loop_count; /* 0 repeats forever */
    /* Advisory only: frames are composited onto transparent black, matching
       libwebp. */
    uint32_t background_argb;
    int      is_animation;
} WPDAnimInfo;

WPDDecoder *wpd_decoder_create(void);

int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data, size_t size);

/* Valid after a successful wpd_decoder_open(). Returns 0, or -1 if no file
   is open. */
int wpd_decoder_anim_info(const WPDDecoder *decoder, WPDAnimInfo *info);

/* Returns 1 and fills 'frame', 0 at end of stream, or -1 on error. The frame
   borrows decoder memory that the next call invalidates. */
int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

const char *wpd_decoder_error(const WPDDecoder *decoder);

void wpd_decoder_free(WPDDecoder *decoder);

#ifdef __cplusplus
}
#endif
#endif
