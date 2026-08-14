#ifndef WPD_H
#define WPD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Version of the headers being compiled against.
 */
#define WPD_VERSION_MAJOR 0
#define WPD_VERSION_MINOR 1
#define WPD_VERSION_PATCH 0
#define WPD_VERSION_INT(major, minor, patch) \
    ((major) << 16 | (minor) << 8 | (patch))
#define WPD_VERSION_NUM \
    WPD_VERSION_INT(WPD_VERSION_MAJOR, WPD_VERSION_MINOR, WPD_VERSION_PATCH)
#define WPD_VERSION_STR "0.1.0"

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

/**
 * Status codes. Negative values are errors.
 */
typedef enum WPDStatus {
    WPD_OK                   = 0,
    WPD_ERR_INVALID_ARG      = -1,
    WPD_ERR_NOT_WEBP         = -2,
    WPD_ERR_BITSTREAM        = -3,
    WPD_ERR_TRUNCATED        = -4,
    WPD_ERR_UNSUPPORTED      = -5,
    WPD_ERR_NO_MEMORY        = -6,
    WPD_ERR_TOO_LARGE        = -7,
    WPD_ERR_BUFFER_TOO_SMALL = -8,
} WPDStatus;

/**
 * Get a static description of a status code.
 */
WPD_API const char *wpd_status_string(WPDStatus status);

/**
 * Get the linked library version.
 */
WPD_API unsigned    wpd_version(void);
WPD_API const char *wpd_version_string(void);

/**
 * Pixel formats. Packed formats use memory byte order; _PRE is premultiplied.
 */
typedef enum WPDPixelFormat {
    WPD_PIX_FMT_NONE = -1,
    WPD_PIX_FMT_YUV420P,
    WPD_PIX_FMT_YUVA420P,
    WPD_PIX_FMT_ARGB,
    WPD_PIX_FMT_RGBA,
    WPD_PIX_FMT_BGRA,
    WPD_PIX_FMT_RGB,
    WPD_PIX_FMT_BGR,
    WPD_PIX_FMT_ARGB_PRE,
    WPD_PIX_FMT_RGBA_PRE,
    WPD_PIX_FMT_BGRA_PRE,
    WPD_PIX_FMT_RGB565,
    WPD_PIX_FMT_RGBA4444,
    WPD_PIX_FMT_RGBA4444_PRE,
    WPD_PIX_FMT_BGR565,
    WPD_PIX_FMT_BGRA4444,
    WPD_PIX_FMT_BGRA4444_PRE,
} WPDPixelFormat;

/**
 * Frame disposal method.
 */
typedef enum WPDDispose {
    WPD_DISPOSE_NONE       = 0,
    WPD_DISPOSE_BACKGROUND = 1,
} WPDDispose;

/**
 * Frame blend method.
 */
typedef enum WPDBlend {
    WPD_BLEND_ALPHA = 0,
    WPD_BLEND_NONE  = 1,
} WPDBlend;

typedef struct WPDFrame {
    size_t         struct_size; ///< Set to sizeof(WPDFrame).
    const uint8_t *data[4];
    ptrdiff_t      stride[4];
    int            width;
    int            height;
    WPDPixelFormat format;
    int            duration; ///< Display duration in milliseconds.
    int64_t        timestamp; ///< Presentation timestamp in milliseconds.
    void          *private_data;
    int            pos_x, pos_y; ///< Sub-frame position in canvas coordinates.
    int            dispose; ///< WPDDispose.
    int            blend; ///< WPDBlend.
    int            has_alpha; ///< Whether the frame itself has alpha.
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

/**
 * Image coding type. Animations report WPD_CODING_UNKNOWN.
 */
typedef enum WPDCoding {
    WPD_CODING_UNKNOWN  = 0,
    WPD_CODING_LOSSY    = 1,
    WPD_CODING_LOSSLESS = 2,
} WPDCoding;

/**
 * Metadata flags. The decoder does not apply metadata.
 */
typedef enum WPDMetadata {
    WPD_METADATA_ICCP = 1 << 0,
    WPD_METADATA_EXIF = 1 << 1,
    WPD_METADATA_XMP  = 1 << 2,
} WPDMetadata;

typedef struct WPDImageInfo {
    size_t    struct_size; ///< Set to sizeof(WPDImageInfo).
    int       width;
    int       height;
    int       has_alpha; ///< Whether any frame can have alpha.
    int       is_animation;
    int       frame_count; ///< One for still images.
    int       loop_count; ///< Zero repeats indefinitely.
    uint32_t  background_argb;
    WPDCoding coding;
    int       metadata; ///< WPDMetadata flags.
} WPDImageInfo;

#define WPD_IMAGE_INFO_INIT \
    {sizeof(WPDImageInfo), 0, 0, 0, 0, 0, 0, 0, WPD_CODING_UNKNOWN, 0}

/**
 * Read image information without decoding. The input is not retained.
 *
 * @return WPD_OK on success, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_get_info(const uint8_t *data, size_t size,
                               WPDImageInfo *info);

/**
 * Allocate a decoder. Free it with wpd_decoder_free().
 */
WPD_API WPDDecoder *wpd_decoder_create(void);

typedef struct WPDDecoderOptions {
    size_t struct_size; ///< Set to sizeof(WPDDecoderOptions).
    int    bypass_filtering; ///< Disable the lossy in-loop filter.
    int    no_fancy_upsampling; ///< Use point-sampled chroma.
    int    use_cropping; ///< Enable cropping.
    int    crop_left;
    int    crop_top;
    int    crop_width;
    int    crop_height;
    int    use_scaling; ///< Enable scaling.
    int    scaled_width;
    int    scaled_height;
    int    flip; ///< Flip output vertically.
} WPDDecoderOptions;

#define WPD_DECODER_OPTIONS_INIT \
    {sizeof(WPDDecoderOptions), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}

/**
 * Set processing options. Cropping precedes scaling.
 *
 * A zero scaled dimension preserves aspect ratio. Options apply to the next
 * decoded frame.
 *
 * @return WPD_OK on success, or WPD_ERR_INVALID_ARG.
 */
WPD_API WPDStatus wpd_decoder_set_options(WPDDecoder              *decoder,
                                          const WPDDecoderOptions *options);

/**
 * Set the output pixel format.
 *
 * WPD_PIX_FMT_NONE preserves the native format. The format applies to the next
 * decoded frame and should be set before decoding an animation.
 *
 * @return WPD_OK on success, or WPD_ERR_INVALID_ARG.
 */
WPD_API WPDStatus wpd_decoder_set_output_format(WPDDecoder    *decoder,
                                                WPDPixelFormat format);

/**
 * Animation output mode.
 */
typedef enum WPDAnimationMode {
    WPD_ANIM_COMPOSITED = 0,
    WPD_ANIM_SUBFRAME   = 1,
} WPDAnimationMode;

/**
 * Set the animation output mode.
 *
 * Subframe output is not composited; position, disposal, and blend describe
 * how the caller must draw it. It cannot be combined with cropping, scaling,
 * or flipping, and must be selected before decoding an animation.
 *
 * @return WPD_OK on success, or WPD_ERR_INVALID_ARG.
 */
WPD_API WPDStatus wpd_decoder_set_animation_mode(WPDDecoder      *decoder,
                                                 WPDAnimationMode mode);

typedef struct WPDOutputPlane {
    uint8_t  *data;
    size_t    size;
    ptrdiff_t stride;
} WPDOutputPlane;

/**
 * Caller-owned output memory. Packed formats use plane[0]; planar formats use
 * Y, U, V, and optionally A. Negative strides are supported.
 */
typedef struct WPDOutputBuffer {
    size_t         struct_size; ///< Set to sizeof(WPDOutputBuffer).
    WPDOutputPlane plane[4];
} WPDOutputBuffer;

#define WPD_OUTPUT_BUFFER_INIT                                     \
    {                                                              \
        sizeof(WPDOutputBuffer), {                                 \
            {NULL, 0, 0}, {NULL, 0, 0}, {NULL, 0, 0}, {NULL, 0, 0} \
        }                                                          \
    }

/**
 * Set caller-owned output memory, or NULL for decoder-owned memory.
 *
 * The buffer must accommodate the final cropped and scaled frame. A too-small
 * buffer is reported by wpd_decoder_next_frame().
 *
 * @return WPD_OK on success, or WPD_ERR_INVALID_ARG.
 */
WPD_API WPDStatus wpd_decoder_set_output_buffer(WPDDecoder            *decoder,
                                                const WPDOutputBuffer *buffer);

/**
 * Open a complete image. The input is copied.
 *
 * @return WPD_OK on success, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data,
                                   size_t size);

/**
 * Open caller-owned input without copying it.
 *
 * The input must remain unchanged until the decoder is reopened or freed.
 */
WPD_API WPDStatus wpd_decoder_open_borrowed(WPDDecoder    *decoder,
                                            const uint8_t *data, size_t size);

/**
 * Begin a streamed decode.
 *
 * Supply data with wpd_decoder_append() or wpd_decoder_update(), then call
 * wpd_decoder_end_of_stream() when complete.
 *
 * @return WPD_OK on success, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_decoder_open_stream(WPDDecoder *decoder);

/**
 * Append copied bytes to a streamed decode.
 *
 * @return WPD_OK while more data may be needed, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_decoder_append(WPDDecoder *decoder, const uint8_t *data,
                                     size_t size);

/**
 * Update a streamed decode with a cumulative, caller-owned buffer.
 *
 * Each buffer must contain the complete input prefix and remain valid until the
 * next update or the decoder is freed.
 */
WPD_API WPDStatus wpd_decoder_update(WPDDecoder *decoder, const uint8_t *data,
                                     size_t size);

/**
 * Mark a streamed input complete.
 *
 * Thereafter, a zero result from wpd_decoder_next_frame() means end of stream.
 */
WPD_API WPDStatus wpd_decoder_end_of_stream(WPDDecoder *decoder);

/**
 * Get information for the open image.
 *
 * For a stream, frame_count grows as frames arrive.
 */
WPD_API WPDStatus wpd_decoder_get_info(const WPDDecoder *decoder,
                                       WPDImageInfo     *info);

typedef struct WPDFrameInfo {
    size_t struct_size; ///< Set to sizeof(WPDFrameInfo).
    int    pos_x, pos_y;
    int    width, height;
    int    duration;
    int    dispose; ///< WPDDispose.
    int    blend; ///< WPDBlend.
    int    has_alpha; ///< Valid when complete is set.
    int    complete; ///< Whether the complete payload is buffered.
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

/**
 * Get information about an image frame without decoding it.
 *
 * Frame indices start at zero. A stream may expose an incomplete final frame.
 *
 * @return WPD_OK on success, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_decoder_frame_info(const WPDDecoder *decoder, int index,
                                         WPDFrameInfo *info);

/**
 * Rewind the open image to its first frame.
 *
 * Appended streams cannot be rewound; updated streams and complete inputs can.
 *
 * @return WPD_OK on success, or a negative WPDStatus.
 */
WPD_API WPDStatus wpd_decoder_rewind(WPDDecoder *decoder);

/**
 * Get one metadata payload.
 *
 * The returned bytes belong to the decoder and remain valid until it is opened
 * again or freed. Missing or unavailable streamed metadata returns NULL and 0.
 */
WPD_API WPDStatus wpd_decoder_metadata(const WPDDecoder *decoder,
                                       WPDMetadata which, const uint8_t **data,
                                       size_t *size);

/**
 * Decode and return the next frame.
 *
 * @return 1 when a frame is returned, 0 when no frame is available, or a
 *         negative WPDStatus. Decoder-owned frame memory is invalidated by the
 *         next decoder call.
 */
WPD_API int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame);

/**
 * Get completed rows of a progressively decoded still image.
 *
 * rows_valid is zero for animations and for output requiring crop, scale, or
 * flip. This does not consume the frame.
 */
WPD_API WPDStatus wpd_decoder_partial_frame(WPDDecoder *decoder,
                                            WPDFrame *frame, int *rows_valid);

/**
 * Get the status of the last failed decoder call.
 */
WPD_API WPDStatus wpd_decoder_status(const WPDDecoder *decoder);

/**
 * Get a description of the last decoder failure.
 */
WPD_API const char *wpd_decoder_error(const WPDDecoder *decoder);

/**
 * Free a decoder.
 */
WPD_API void wpd_decoder_free(WPDDecoder *decoder);

/**
 * Decode into caller-owned output memory.
 *
 * Any allocation owned by frame is released first.
 */
WPD_API WPDStatus wpd_decode_into(const uint8_t *data, size_t size,
                                  WPDPixelFormat           format,
                                  const WPDDecoderOptions *options,
                                  const WPDOutputBuffer   *buffer,
                                  WPDFrame                *frame);

/**
 * Decode and allocate a frame. Release it with wpd_frame_free().
 *
 * Any allocation owned by frame is released first.
 */
WPD_API WPDStatus wpd_decode(const uint8_t *data, size_t size,
                             WPDPixelFormat           format,
                             const WPDDecoderOptions *options, WPDFrame *frame);

/**
 * Release a frame allocated by wpd_decode().
 */
WPD_API void wpd_frame_free(WPDFrame *frame);

typedef enum WPDLogLevel {
    WPD_LOG_ERROR   = 0,
    WPD_LOG_WARNING = 1,
} WPDLogLevel;

/**
 * Diagnostic callback. message is valid for the duration of the call.
 */
typedef void (*WPDLogCallback)(void *opaque, WPDLogLevel level,
                               const char *message);

/**
 * Set the process-global diagnostic callback. Passing NULL disables logging.
 *
 * Install the callback before decoding; changing it while decoding is not
 * supported.
 */
WPD_API void wpd_set_log_callback(WPDLogCallback callback, void *opaque);

#ifdef __cplusplus
}
#endif
#endif
