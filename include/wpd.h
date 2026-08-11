#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Version of the headers being compiled against. wpd_version() reports the
   version of the library actually linked, which may differ. */
#define WPD_VERSION_MAJOR 0
#define WPD_VERSION_MINOR 1
#define WPD_VERSION_PATCH 0
#define WPD_VERSION_INT(major, minor, patch) \
    ((major) << 16 | (minor) << 8 | (patch))
#define WPD_VERSION_NUM \
    WPD_VERSION_INT(WPD_VERSION_MAJOR, WPD_VERSION_MINOR, WPD_VERSION_PATCH)
#define WPD_VERSION_STR "0.1.0"

/* Define WPD_STATIC before including this header when linking against the
   static library on Windows. */
#if defined(_WIN32) && !defined(WPD_STATIC)
#ifdef WPD_BUILDING
#define WPD_API __declspec(dllexport)
#else
#define WPD_API __declspec(dllimport)
#endif
#elif defined(__GNUC__) || defined(__clang__)
#define WPD_API __attribute__((visibility("default")))
#else
#define WPD_API
#endif

typedef struct WPDDecoder WPDDecoder;

/* WPD_VERSION_NUM and WPD_VERSION_STR for the linked library. */
WPD_API unsigned    wpd_version(void);
WPD_API const char *wpd_version_string(void);

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

WPD_API WPDDecoder *wpd_decoder_create(void);

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
WPD_API int wpd_decoder_set_output_format(WPDDecoder    *decoder,
                                          WPDPixelFormat format);

WPD_API int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data,
                             size_t size);

/* Valid after a successful wpd_decoder_open(). Returns 0, or -1 if no file
   is open. */
WPD_API int wpd_decoder_anim_info(const WPDDecoder *decoder,
                                  WPDAnimInfo      *info);

/* Returns 1 and fills 'frame', 0 at end of stream, or -1 on error. The frame
   borrows decoder memory that the next call invalidates. */
WPD_API int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

WPD_API const char *wpd_decoder_error(const WPDDecoder *decoder);

WPD_API void wpd_decoder_free(WPDDecoder *decoder);

typedef enum WPDLogLevel {
    WPD_LOG_ERROR   = 0,
    WPD_LOG_WARNING = 1,
} WPDLogLevel;

/* 'message' is a complete, NUL-terminated line with no trailing newline, valid
   only for the duration of the call. */
typedef void (*WPDLogCallback)(void *opaque, WPDLogLevel level,
                               const char *message);

/* Redirect diagnostics, which are silent by default. 'opaque' is handed back to
   every call. Passing a NULL callback silences them again.

   This is process-global. Install it before decoding starts: it may be called
   from any thread, and a decode that sees the callback is guaranteed to see the
   'opaque' installed alongside it, on that thread and every other. Replacing or
   clearing it while another thread is decoding is not supported. The callback
   and the opaque are read one after the other, so a decode already past the
   first read can pair the outgoing callback with the incoming opaque; only the
   initial install is ordered. */
WPD_API void wpd_set_log_callback(WPDLogCallback callback, void *opaque);

#ifdef __cplusplus
}
#endif
#endif
