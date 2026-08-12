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

/* Every entry point that can fail reports one of these. Negative values are
   errors, so 'result < 0' is a valid test even where a function returns extra
   non-negative values. */
typedef enum WPDStatus {
    WPD_OK = 0,
    /* A NULL pointer, or an argument outside its permitted range. */
    WPD_ERR_INVALID_ARG = -1,
    /* The data is not a RIFF WebP or supported raw WebP bitstream. */
    WPD_ERR_NOT_WEBP = -2,
    /* The file is a WebP but its contents are malformed. */
    WPD_ERR_BITSTREAM = -3,
    /* The file ends inside a chunk. More data may complete it. */
    WPD_ERR_TRUNCATED = -4,
    /* Well-formed, but uses something this decoder cannot produce. */
    WPD_ERR_UNSUPPORTED = -5,
    WPD_ERR_NO_MEMORY   = -6,
    /* Dimensions or allocation size exceed the decoder's safe limits. */
    WPD_ERR_TOO_LARGE = -7,
    /* The buffer given to wpd_decoder_set_output_buffer() cannot hold the
       frame at the stride requested. */
    WPD_ERR_BUFFER_TOO_SMALL = -8,
} WPDStatus;

/* A short, static description of 'status'. Never NULL. */
WPD_API const char *wpd_status_string(WPDStatus status);

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
    /* Two bytes per pixel, most-significant component bits first. The
       premultiplied 4444 formats are multiplied in the four-bit domain, after
       the quantization, as libwebp's rgbA4444 is. The exception is a
       composited animation frame, whose canvas is premultiplied in eight bits
       before each frame is blended into it and packed afterwards. */
    WPD_PIX_FMT_RGB565,
    WPD_PIX_FMT_RGBA4444,
    WPD_PIX_FMT_RGBA4444_PRE,
    /* The two bytes of each unit in the opposite order to the three above,
       which is what Skia's kRGB_565_SkColorType and kARGB_4444_SkColorType
       expect on a little-endian host: its libwebp is built with
       WEBP_SWAP_16BIT_CSP. Not a channel swizzle; the fields keep their
       widths and their order within the sixteen bits. */
    WPD_PIX_FMT_BGR565, /* byte0 = gggbbbbb, byte1 = rrrrrggg */
    WPD_PIX_FMT_BGRA4444, /* byte0 = bbbbaaaa, byte1 = rrrrgggg */
    WPD_PIX_FMT_BGRA4444_PRE,
} WPDPixelFormat;

/* What the canvas keeps of a frame once the next one is decoded. */
typedef enum WPDDispose {
    WPD_DISPOSE_NONE       = 0,
    WPD_DISPOSE_BACKGROUND = 1,
} WPDDispose;

/* How a frame is combined with what the canvas already holds. */
typedef enum WPDBlend {
    WPD_BLEND_ALPHA = 0,
    WPD_BLEND_NONE  = 1,
} WPDBlend;

typedef struct WPDFrame {
    /* Set to sizeof(WPDFrame), normally with WPD_FRAME_INIT. */
    size_t         struct_size;
    const uint8_t *data[4];
    ptrdiff_t      stride[4];
    int            width;
    int            height;
    WPDPixelFormat format;
    /* Milliseconds this frame is shown, and since the animation started.
       Both are 0 for still images. */
    int     duration;
    int64_t timestamp;
    /* Private ownership used only by wpd_decode(). */
    void *private_data;
    /* The sub-frame this frame was built from, in canvas coordinates. Filled
       in both animation modes: under WPD_ANIM_COMPOSITED they describe the
       sub-frame that produced the canvas handed over, and 'width' and 'height'
       above are still the canvas; under WPD_ANIM_SUBFRAME 'width' and 'height'
       are the sub-frame's own. All zero for a still image apart from
       'has_alpha'. */
    int pos_x, pos_y;
    int dispose; /* WPDDispose, this frame's own flag */
    int blend; /* WPDBlend */
    int has_alpha; /* whether this frame itself carries transparency */
} WPDFrame;

#define WPD_FRAME_INIT         \
    {sizeof(WPDFrame),         \
     {NULL, NULL, NULL, NULL}, \
     {0, 0, 0, 0},             \
     0,                        \
     0,                        \
     WPD_PIX_FMT_NONE,         \
     0,                        \
     0,                        \
     NULL,                     \
     0,                        \
     0,                        \
     WPD_DISPOSE_NONE,         \
     WPD_BLEND_ALPHA,          \
     0}

/* How the image data is coded. Reported as WPD_CODING_UNKNOWN for animations,
   whose frames may mix the two, matching libwebp's WebPBitstreamFeatures. */
typedef enum WPDCoding {
    WPD_CODING_UNKNOWN  = 0,
    WPD_CODING_LOSSY    = 1,
    WPD_CODING_LOSSLESS = 2,
} WPDCoding;

/* The metadata a WebP file can carry alongside the image. These are bits in
   WPDImageInfo.metadata, and one of them selects a chunk in
   wpd_decoder_metadata(). The decoder never acts on any of it: an EXIF
   orientation does not rotate the frames, and an ICC profile does not change
   how they are converted. */
typedef enum WPDMetadata {
    WPD_METADATA_ICCP = 1 << 0, /* "ICCP", an ICC colour profile */
    WPD_METADATA_EXIF = 1 << 1, /* "EXIF" */
    WPD_METADATA_XMP  = 1 << 2, /* "XMP " */
} WPDMetadata;

typedef struct WPDImageInfo {
    /* Set to sizeof(WPDImageInfo), normally with WPD_IMAGE_INFO_INIT. */
    size_t struct_size;
    /* Canvas dimensions. Every decoded frame is this size. */
    int width;
    int height;
    /* Set if any frame can carry transparency. A frame may still be fully
       opaque. */
    int has_alpha;
    int is_animation;
    /* 1 for a still image. May undercount a truncated animation. */
    int frame_count;
    int loop_count; /* 0 repeats forever */
    /* Advisory only: frames are composited onto transparent black, matching
       libwebp. */
    uint32_t  background_argb;
    WPDCoding coding;
    /* WPDMetadata bits for the metadata the file says it carries. A bit can be
       set before the chunk itself has arrived in a stream. */
    int metadata;
} WPDImageInfo;

#define WPD_IMAGE_INFO_INIT \
    {sizeof(WPDImageInfo), 0, 0, 0, 0, 0, 0, 0, WPD_CODING_UNKNOWN, 0}

/* Read the headers of 'data' without decoding, allocating, or retaining it.
   This is the cheap way to learn an image's dimensions and whether it has
   alpha or animation before committing to a decode. RIFF WebP files, bare VP8
   and VP8L payloads, and an ALPH chunk followed by a VP8 chunk are accepted.

   Once a canvas header has given the dimensions this succeeds, as libwebp
   does, even if the image data behind it is short or absent; only opening the
   file for decoding judges it complete.

   Returns WPD_OK and fills 'info', or WPD_ERR_NOT_WEBP, WPD_ERR_TRUNCATED if
   the headers are incomplete, or WPD_ERR_BITSTREAM. */
WPD_API WPDStatus wpd_get_info(const uint8_t *data, size_t size,
                               WPDImageInfo *info);

WPD_API WPDDecoder *wpd_decoder_create(void);

typedef struct WPDDecoderOptions {
    /* Set to sizeof(WPDDecoderOptions), normally with WPD_DECODER_OPTIONS_INIT. */
    size_t struct_size;
    /* Skip the lossy in-loop filter. */
    int bypass_filtering;
    /* Use point-sampled chroma instead of the default fancy upsampler. */
    int no_fancy_upsampling;
    /* Crop to this rectangle when nonzero. */
    int use_cropping;
    int crop_left;
    int crop_top;
    int crop_width;
    int crop_height;
    /* Scale to these dimensions when nonzero. Taking a lossy frame down past
       three quarters in both directions turns the in-loop filter off, as it
       does in libwebp. */
    int use_scaling;
    int scaled_width;
    int scaled_height;
    /* Reverse the final output vertically. */
    int flip;
} WPDDecoderOptions;

#define WPD_DECODER_OPTIONS_INIT \
    {sizeof(WPDDecoderOptions), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}

/* Replace the decoder's processing options. Cropping is applied before
   scaling. A lossy frame is cropped in its native YUV, whose chroma is
   subsampled, so crop_left and crop_top are rounded down to even values there;
   a lossless frame is cropped in ARGB and takes the origin exactly. libwebp
   does the same. A zero scaled width or height is inferred from the other
   dimension while preserving aspect ratio. Options take effect on the next
   frame decoded. Returns WPD_OK or WPD_ERR_INVALID_ARG. */
WPD_API WPDStatus wpd_decoder_set_options(WPDDecoder              *decoder,
                                          const WPDDecoderOptions *options);

/* Force the pixel format frames are produced in. The default,
   WPD_PIX_FMT_NONE, reports whatever the bitstream decodes to: ARGB for
   lossless, YUV420P or YUVA420P for lossy.

   Any packed format converts lossy frames with the same fixed-point BT.601
   coefficients and fancy chroma upsampler libwebp uses, so output is bit-exact
   with the matching libwebp colorspace, including for animations, which
   libwebp only ever composites in RGB. Conversion is not free; leave it off if
   YUV output is acceptable.

   Planar output from a lossless source includes an RGB-to-YUV conversion.
   Takes effect on the next wpd_decoder_next_frame(); call it before decoding an
   animation, since compositing depends on it. Returns WPD_OK, or
   WPD_ERR_INVALID_ARG on an unsupported format. */
WPD_API WPDStatus wpd_decoder_set_output_format(WPDDecoder    *decoder,
                                                WPDPixelFormat format);

/* What wpd_decoder_next_frame() hands over for an animation. */
typedef enum WPDAnimationMode {
    /* A finished canvas, composited as libwebp's WebPAnimDecoder does. */
    WPD_ANIM_COMPOSITED = 0,
    /* Each ANMF sub-frame on its own, uncomposited, which is what a host
       running its own frame-buffer cache wants. */
    WPD_ANIM_SUBFRAME = 1,
} WPDAnimationMode;

/* Choose between a composited canvas and raw sub-frames. In
   WPD_ANIM_SUBFRAME, a frame's 'width' and 'height' are the sub-frame's, not
   the canvas', and 'pos_x', 'pos_y', 'dispose' and 'blend' say where and how
   the host should draw it; nothing is composited on the caller's behalf.

   Nothing normalizes the sub-frames either, so under the automatic output
   format a frame's 'format' is whatever its own image chunk decoded to and may
   differ from frame to frame: an animation mixing lossy frames with and
   without an ALPH chunk yields WPD_PIX_FMT_YUV420P for some and
   WPD_PIX_FMT_YUVA420P for others, which do not even agree on their plane
   count. A host that reads 'format' once should pin it with
   wpd_decoder_set_output_format(); the composited mode has one canvas and so
   one format throughout.

   The sub-frames are the bitstream's own, so cropping, scaling and flipping,
   which are all defined against the canvas, cannot be combined with this mode:
   setting either one while the other is in force returns WPD_ERR_INVALID_ARG.
   Still images are unaffected by the mode.

   Sub-frame mode never builds the canvas the composited mode carries from
   frame to frame, so the mode has to be settled before an animation's first
   frame: changing it once one has been decoded returns WPD_ERR_INVALID_ARG
   until wpd_decoder_rewind() or a fresh open. Returns WPD_OK or
   WPD_ERR_INVALID_ARG. */
WPD_API WPDStatus wpd_decoder_set_animation_mode(WPDDecoder      *decoder,
                                                 WPDAnimationMode mode);

typedef struct WPDOutputPlane {
    uint8_t  *data;
    size_t    size;
    ptrdiff_t stride;
} WPDOutputPlane;

/* Caller-owned memory to write decoded frames into. Packed formats use
   plane[0]. Planar formats use planes Y, U, V and optionally A in that order.
   Each size must cover abs(stride) times that plane's height, and each stride
   must be at least the plane width in magnitude. Negative strides flip planes
   vertically; data must then point at the first byte of the last row. */
typedef struct WPDOutputBuffer {
    /* Set to sizeof(WPDOutputBuffer), normally with WPD_OUTPUT_BUFFER_INIT. */
    size_t         struct_size;
    WPDOutputPlane plane[4];
} WPDOutputBuffer;

#define WPD_OUTPUT_BUFFER_INIT                                     \
    {                                                              \
        sizeof(WPDOutputBuffer), {                                 \
            {NULL, 0, 0}, {NULL, 0, 0}, {NULL, 0, 0}, {NULL, 0, 0} \
        }                                                          \
    }

/* Decode into memory the caller owns instead of into decoder-owned memory.
   Frames returned by wpd_decoder_next_frame() then point into 'buffer', and
   stay valid for as long as the caller keeps it alive. Pass NULL to go back to
   decoder-owned memory.

   Animations are composited internally and each finished canvas is written
   out, so the caller may freely overwrite the buffer between frames.

   The buffer is measured against the final cropped and scaled dimensions on
   every frame, so it is fine to set it before wpd_decoder_open(). Changing it
   part way through a progressive still decode is allowed: the rows already
   written stay in the old buffer, and the new one is filled from the top by
   the next wpd_decoder_partial_frame(). Returns WPD_OK or WPD_ERR_INVALID_ARG;
   a buffer too small for the image is reported by wpd_decoder_next_frame() as
   WPD_ERR_BUFFER_TOO_SMALL. */
WPD_API WPDStatus wpd_decoder_set_output_buffer(WPDDecoder            *decoder,
                                                const WPDOutputBuffer *buffer);

/* 'data' is copied, so it need not outlive the call. The whole file has to be
   here: a chunk list that stops short of what it promised is
   WPD_ERR_TRUNCATED, and one that carries no image at all is
   WPD_ERR_BITSTREAM. Use wpd_decoder_open_stream() for a file still arriving.
   Returns WPD_OK or an error. */
WPD_API WPDStatus wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data,
                                   size_t size);

/* Open caller-owned input without copying it. The bytes must remain at the
   same address and unchanged until the decoder is reopened or freed. */
WPD_API WPDStatus wpd_decoder_open_borrowed(WPDDecoder    *decoder,
                                            const uint8_t *data, size_t size);

/* Begin decoding a file that is not all here yet, for callers reading from a
   socket or a pipe. Use instead of wpd_decoder_open(), then alternate
   wpd_decoder_append() with wpd_decoder_next_frame():

     wpd_decoder_open_stream(decoder);
     while ((n = read(fd, buf, sizeof(buf))) > 0) {
         if (wpd_decoder_append(decoder, buf, n) < 0)
             break;
         while (wpd_decoder_next_frame(decoder, &frame) > 0)
             present(&frame);
     }
     if (wpd_decoder_end_of_stream(decoder) == WPD_OK)
         while (wpd_decoder_next_frame(decoder, &frame) > 0)
             present(&frame);

   An animation yields each frame as soon as that frame's bytes have arrived,
   so a long animation starts playing while the rest downloads. A still image
   is handed over only once it is complete, but it decodes as the bytes arrive
   and its finished rows can be displayed meanwhile; see
   wpd_decoder_partial_frame(). Everything else behaves as it does for a file
   opened whole, including the output format and output buffer.

   A bare VP8 or VP8L payload with no RIFF header around it carries no length,
   so nothing about it can be decoded until wpd_decoder_end_of_stream() says
   where it ends. Such a stream produces no frame and no partial rows before
   then. */
WPD_API WPDStatus wpd_decoder_open_stream(WPDDecoder *decoder);

/* Add the next 'size' bytes of the file, which are copied. Returns WPD_OK,
   including when the headers are still incomplete, or an error once the data
   is known to be unusable. */
WPD_API WPDStatus wpd_decoder_append(WPDDecoder *decoder, const uint8_t *data,
                                     size_t size);

/* Supply a cumulative caller-owned stream buffer without copying it. Each call
   contains the complete prefix of the file and must not shrink. The bytes must
   remain unchanged until the next update or until the decoder is freed, except
   that a call reporting an error hands the memory straight back: the decoder
   keeps no reference to a buffer it has rejected. Use instead of
   wpd_decoder_append() for a stream opened normally. */
WPD_API WPDStatus wpd_decoder_update(WPDDecoder *decoder, const uint8_t *data,
                                     size_t size);

/* Declare the file complete, so that wpd_decoder_next_frame() reporting 0
   means end of stream rather than "not yet". Returns WPD_OK,
   WPD_ERR_TRUNCATED if the file ended inside a chunk, or WPD_ERR_BITSTREAM if
   it carried no image at all. */
WPD_API WPDStatus wpd_decoder_end_of_stream(WPDDecoder *decoder);

/* Same as wpd_get_info() for the file currently open. Returns WPD_OK,
   WPD_ERR_INVALID_ARG if no file is open, or WPD_ERR_TRUNCATED if a stream has
   not yet delivered enough of its headers. For a stream, 'frame_count' counts
   the frames seen so far and grows as more arrives. */
WPD_API WPDStatus wpd_decoder_get_info(const WPDDecoder *decoder,
                                       WPDImageInfo     *info);

typedef struct WPDFrameInfo {
    /* Set to sizeof(WPDFrameInfo), normally with WPD_FRAME_INFO_INIT. */
    size_t struct_size;
    /* Where the sub-frame sits on the canvas, and how big it is. */
    int pos_x, pos_y;
    int width, height;
    int duration;
    int dispose; /* WPDDispose */
    int blend; /* WPDBlend */
    /* Whether the frame's own image data carries transparency, which is what
       the decoded WPDFrame reports and may be narrower than the whole file's
       WPDImageInfo.has_alpha, a VP8X declaration the image need not honour.
       Only final once 'complete' is set. */
    int has_alpha;
    /* The frame's payload is entirely buffered, so decoding it will not stall.
       0 for the last entry of a stream still arriving. */
    int complete;
} WPDFrameInfo;

#define WPD_FRAME_INFO_INIT \
    {sizeof(WPDFrameInfo),  \
     0,                     \
     0,                     \
     0,                     \
     0,                     \
     0,                     \
     WPD_DISPOSE_NONE,      \
     WPD_BLEND_ALPHA,       \
     0,                     \
     0}

/* Describe one frame from the chunk list alone, without decoding it, so a host
   can lay out its own frame cache up front. This is what libwebp callers get
   from WebPDemuxGetFrame() and WebPIterator.

   Indices run from 0. An animation being streamed grows an entry as soon as
   that frame's 16-byte ANMF header has arrived, so the last one may report
   'complete' as 0 and flip to 1 later; everything but 'has_alpha' is final
   from the moment the entry appears. A still image has exactly one entry
   covering the whole canvas.

   The table stops growing after 2^20 entries, far above any animation a player
   would sit through. Such a file still decodes in full, and
   WPDImageInfo.frame_count still counts every frame, so a host laying out a
   cache from that count should treat the indices past the table the way it
   treats a frame that has not arrived.

   Returns WPD_OK, WPD_ERR_INVALID_ARG if no file is open or 'index' names no
   frame yet, or WPD_ERR_TRUNCATED if a stream has not delivered enough of its
   headers. */
WPD_API WPDStatus wpd_decoder_frame_info(const WPDDecoder *decoder, int index,
                                         WPDFrameInfo *info);

/* Go back to frame 0 without re-parsing the file, for a player looping an
   animation. The canvas is cleared and the frame index, timestamp and dispose
   latch are reset; the headers, the frame table and the metadata are kept, as
   are the output format, options and output buffer.

   Returns WPD_OK, WPD_ERR_INVALID_ARG if no file is open, or
   WPD_ERR_UNSUPPORTED for a stream driven by wpd_decoder_append(), which is
   free to drop bytes the decoder has moved past. Streams fed through
   wpd_decoder_update(), and files opened whole or borrowed, can always be
   rewound. */
WPD_API WPDStatus wpd_decoder_rewind(WPDDecoder *decoder);

/* Point '*data' and '*size' at one metadata chunk's payload, where 'which' is
   a single WPDMetadata bit. The bytes belong to the decoder and stay valid
   until the next wpd_decoder_open(), wpd_decoder_open_stream(),
   wpd_decoder_open_borrowed(), or wpd_decoder_free(); appending to a stream
   does not move them.

   Sets *data to NULL and *size to 0 when the file has no such chunk, or when a
   stream has not reached it yet: EXIF and XMP follow the image data, so they
   arrive last. WPDImageInfo.metadata says what to expect. Only the first chunk
   of a kind is kept, as libwebp does.

   Returns WPD_OK, or WPD_ERR_INVALID_ARG if no file is open or 'which' is not
   a single metadata bit. */
WPD_API WPDStatus wpd_decoder_metadata(const WPDDecoder *decoder,
                                       WPDMetadata which, const uint8_t **data,
                                       size_t *size);

/* Returns 1 and fills 'frame', 0 when no further frame is available, or a
   negative WPDStatus. Unless an external output buffer is set, the frame
   borrows decoder memory that the next call invalidates.

   For a file opened whole, 0 means end of stream. For one being streamed, 0
   means no frame can be produced from the bytes appended so far; it means end
   of stream only once wpd_decoder_end_of_stream() has been called. */
WPD_API int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

/* Look at the rows of a still image decoded so far, for progressive display.
   Fills 'frame' as wpd_decoder_next_frame() would and sets *rows_valid to the
   number of rows that are finished; those rows will not change again. Rows at
   or past *rows_valid hold whatever the decoder has written so far and must
   not be displayed.

   rows_valid is 0 until something is decodable, and stays 0 for animations,
   which decode a whole frame at a time. It reaches the image height once the
   frame is complete, whether or not wpd_decoder_next_frame() has handed that
   frame over yet. A lossless still gives rows away in blocks of sixteen.

   Nothing is consumed, so the same frame still arrives from
   wpd_decoder_next_frame(). An output buffer set with
   wpd_decoder_set_output_buffer() does receive the finished rows, since that
   is where the caller asked for output to go. Cropped, scaled or flipped
   output is withheld until the complete source frame is available. Returns
   WPD_OK, or WPD_ERR_INVALID_ARG if no file is open. */
WPD_API WPDStatus wpd_decoder_partial_frame(WPDDecoder *decoder,
                                            WPDFrame *frame, int *rows_valid);

/* The status of the last failed call on 'decoder', or WPD_OK. */
WPD_API WPDStatus wpd_decoder_status(const WPDDecoder *decoder);

/* A human-readable description of the last failure, for logs. Callers that
   need to branch on the failure should use wpd_decoder_status(). */
WPD_API const char *wpd_decoder_error(const WPDDecoder *decoder);

WPD_API void wpd_decoder_free(WPDDecoder *decoder);

/* Decode a still image or the first animation frame into caller-owned memory.
   Any allocation previously owned by 'frame' is released first. */
WPD_API WPDStatus wpd_decode_into(const uint8_t *data, size_t size,
                                  WPDPixelFormat           format,
                                  const WPDDecoderOptions *options,
                                  const WPDOutputBuffer   *buffer,
                                  WPDFrame                *frame);

/* Decode and allocate a still image or the first animation frame. Release it
   with wpd_frame_free(). Any allocation previously owned by 'frame' is
   released first. */
WPD_API WPDStatus wpd_decode(const uint8_t *data, size_t size,
                             WPDPixelFormat           format,
                             const WPDDecoderOptions *options, WPDFrame *frame);

WPD_API void wpd_frame_free(WPDFrame *frame);

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
