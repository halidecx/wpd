#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WPDDecoder WPDDecoder;

/* Packed formats are named after their byte order in memory, following
   libwebp: WPD_PIX_FMT_BGRA is B,G,R,A,B,G,R,A... A lowercase letter marks the
   channels the alpha has been multiplied into. */
typedef enum WPDPixelFormat {
    /* Only accepted by wpd_decoder_set_output_format(); never reported on a
       decoded frame. */
    WPD_PIX_FMT_NONE = -1,
    WPD_PIX_FMT_YUV420P,
    WPD_PIX_FMT_YUVA420P,
    WPD_PIX_FMT_ARGB,
    WPD_PIX_FMT_RGBA,
    WPD_PIX_FMT_BGRA,
    /* Transparency is dropped, not composited onto a background. */
    WPD_PIX_FMT_RGB,
    WPD_PIX_FMT_BGR,
    WPD_PIX_FMT_ARGB_PRE,
    WPD_PIX_FMT_RGBA_PRE,
    WPD_PIX_FMT_BGRA_PRE,
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

/* Force the pixel format frames are produced in. The default,
   WPD_PIX_FMT_NONE, reports whatever the bitstream decodes to: ARGB for
   lossless, YUV420P or YUVA420P for lossy.

   Any packed format converts lossy frames with the same fixed-point BT.601
   coefficients and fancy chroma upsampler libwebp uses, so output is bit-exact
   with the matching libwebp colorspace, including for animations, which
   libwebp only ever composites in RGB. Conversion is not free; leave it off if
   YUV output is acceptable.

   The planar formats are not accepted, since a lossless frame cannot be
   expressed in them. Takes effect on the next wpd_decoder_next_frame(); call
   it before decoding an animation, since compositing depends on it. Returns 0,
   or -1 on an unsupported format. */
int wpd_decoder_set_output_format(WPDDecoder *decoder, WPDPixelFormat format);

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
