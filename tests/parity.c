#include "wpd.h"

#include "rescaler.h"
#include "webp/decode.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;
static int comparisons;
static int skipped;

static const struct {
    WPDPixelFormat format;
    WEBP_CSP_MODE  mode;
    const char    *name;
    int            bpp;
} formats[] = {
    {WPD_PIX_FMT_RGBA, MODE_RGBA, "rgba", 4},
    {WPD_PIX_FMT_BGRA, MODE_BGRA, "bgra", 4},
    {WPD_PIX_FMT_ARGB, MODE_ARGB, "argb", 4},
    {WPD_PIX_FMT_RGB, MODE_RGB, "rgb", 3},
    {WPD_PIX_FMT_BGR, MODE_BGR, "bgr", 3},
    {WPD_PIX_FMT_RGBA_PRE, MODE_rgbA, "rgbA", 4},
    {WPD_PIX_FMT_BGRA_PRE, MODE_bgrA, "bgrA", 4},
    {WPD_PIX_FMT_ARGB_PRE, MODE_Argb, "Argb", 4},
    {WPD_PIX_FMT_RGB565, MODE_RGB_565, "rgb565", 2},
    {WPD_PIX_FMT_RGBA4444, MODE_RGBA_4444, "rgba4444", 2},
    {WPD_PIX_FMT_RGBA4444_PRE, MODE_rgbA_4444, "rgbA4444", 2},
};

static uint8_t *read_file(const char *path, size_t *size) {
    FILE    *file = fopen(path, "rb");
    uint8_t *data;
    long     length;

    if (!file)
        return NULL;
    if (fseek(file, 0, SEEK_END) || (length = ftell(file)) < 0) {
        fclose(file);
        return NULL;
    }
    rewind(file);
    data = malloc((size_t)length);
    if (data && fread(data, 1, (size_t)length, file) != (size_t)length) {
        free(data);
        data = NULL;
    }
    fclose(file);
    *size = (size_t)length;
    return data;
}

static void report(const char *file, const char *what, const char *detail,
                   long differing, long total) {
    comparisons++;
    if (!differing)
        return;
    failures++;
    fprintf(stderr,
            "%s: %s %s: %ld of %ld bytes differ from libwebp\n",
            file,
            what,
            detail,
            differing,
            total);
}

static long compare_packed(const uint8_t *got, ptrdiff_t got_stride,
                           const uint8_t *want, int want_stride, int width,
                           int height, int bpp) {
    long differing = 0;

    for (int y = 0; y < height; y++)
        for (int x = 0; x < width * bpp; x++)
            differing += got[(ptrdiff_t)y * got_stride + x] !=
                want[(ptrdiff_t)y * want_stride + x];
    return differing;
}

/* Runs one wpd decode and the matching libwebp decode and compares them. */
static void check(const char *file, const uint8_t *data, size_t size,
                  const WPDDecoderOptions  *options,
                  const WebPDecoderOptions *webp_options, int index,
                  const char *what, const char *detail) {
    WPDFrame          frame = WPD_FRAME_INIT;
    WebPDecoderConfig config;
    WPDStatus         status;
    long              differing;

    status = wpd_decode(data, size, formats[index].format, options, &frame);
    if (!WebPInitDecoderConfig(&config)) {
        fprintf(stderr, "%s: libwebp version mismatch\n", file);
        failures++;
        return;
    }
    config.options           = *webp_options;
    config.output.colorspace = formats[index].mode;
    if (WebPDecode(data, size, &config) != VP8_STATUS_OK) {
        if (status == WPD_OK) {
            fprintf(stderr,
                    "%s: %s %s %s: libwebp refused what wpd decoded\n",
                    file,
                    what,
                    detail,
                    formats[index].name);
            failures++;
        }
        wpd_frame_free(&frame);
        WebPFreeDecBuffer(&config.output);
        return;
    }
    if (status != WPD_OK) {
        fprintf(stderr,
                "%s: %s %s %s: wpd failed (%s) where libwebp succeeded\n",
                file,
                what,
                detail,
                formats[index].name,
                wpd_status_string(status));
        failures++;
        WebPFreeDecBuffer(&config.output);
        return;
    }
    if (frame.width != config.output.width ||
        frame.height != config.output.height) {
        fprintf(stderr,
                "%s: %s %s %s: %dx%d, libwebp %dx%d\n",
                file,
                what,
                detail,
                formats[index].name,
                frame.width,
                frame.height,
                config.output.width,
                config.output.height);
        failures++;
        comparisons++;
        wpd_frame_free(&frame);
        WebPFreeDecBuffer(&config.output);
        return;
    }

    differing = compare_packed(frame.data[0],
                               frame.stride[0],
                               config.output.u.RGBA.rgba,
                               config.output.u.RGBA.stride,
                               frame.width,
                               frame.height,
                               formats[index].bpp);
    {
        char label[64];

        snprintf(label, sizeof(label), "%s %s", detail, formats[index].name);
        report(file,
               what,
               label,
               differing,
               (long)frame.width * formats[index].bpp * frame.height);
    }
    wpd_frame_free(&frame);
    WebPFreeDecBuffer(&config.output);
}

static long compare_plane(const uint8_t *got, ptrdiff_t got_stride,
                          const uint8_t *want, int want_stride, int width,
                          int height) {
    return compare_packed(got, got_stride, want, want_stride, width, height, 1);
}

/* libwebp gained gamma-correct chroma downsampling for lossless-to-YUV after
   1.6.0 (upstream 0d14d84b, "Have lossless use ImportYUVAFromRGB"), and the
   release does not report a distinguishing version, so decide from behaviour:
   reproduce the old averaging and see whether the linked libwebp still does
   it. Only lossless sources are affected; a lossy one is already YUV. */
static int lossless_yuv_is_gamma = 1;

static int simple_chroma_u(const uint8_t *row, int x, int width) {
    const int x1 = x + 1 < width ? x + 1 : x;
    const int r  = 2 * (row[4 * x + 1] + row[4 * x1 + 1]);
    const int g  = 2 * (row[4 * x + 2] + row[4 * x1 + 2]);
    const int b  = 2 * (row[4 * x + 3] + row[4 * x1 + 3]);
    long      uv = -9719L * r - 19081L * g + 28800L * b;

    uv = (uv + (1 << 17) + (128L << 18)) >> 18;
    return uv < 0 ? 0 : uv > 255 ? 255 : (int)uv;
}

static int uses_simple_lossless_chroma(const uint8_t *data, size_t size) {
    WPDFrame          argb = WPD_FRAME_INIT;
    WebPDecoderConfig config;
    int               matches = 1;

    if (wpd_decode(data, size, WPD_PIX_FMT_ARGB, NULL, &argb) != WPD_OK)
        return 0;
    if (!WebPInitDecoderConfig(&config)) {
        wpd_frame_free(&argb);
        return 0;
    }
    config.output.colorspace = MODE_YUV;
    if (WebPDecode(data, size, &config) != VP8_STATUS_OK) {
        wpd_frame_free(&argb);
        return 0;
    }
    for (int y = 0; y + 1 < argb.height && matches; y += 2) {
        const uint8_t *top = argb.data[0] + (ptrdiff_t)y * argb.stride[0];
        const uint8_t *bot = argb.data[0] + (ptrdiff_t)(y + 1) * argb.stride[0];
        const uint8_t *u   = config.output.u.YUVA.u +
            (ptrdiff_t)(y >> 1) * config.output.u.YUVA.u_stride;

        for (int x = 0; x < argb.width; x += 2) {
            const int a = simple_chroma_u(top, x, argb.width);
            const int b = simple_chroma_u(bot, x, argb.width);

            if (u[x >> 1] != ((a + b + 1) >> 1)) {
                matches = 0;
                break;
            }
        }
    }
    wpd_frame_free(&argb);
    WebPFreeDecBuffer(&config.output);
    return matches;
}

/* The planar formats have no packed buffer to compare, so they get their own
   path over the three or four planes. */
static void check_planar(const char *file, const uint8_t *data, size_t size,
                         const WPDDecoderOptions  *options,
                         const WebPDecoderOptions *webp_options, int alpha,
                         const char *what, const char *detail,
                         WPDCoding coding) {
    const WPDPixelFormat format = alpha ? WPD_PIX_FMT_YUVA420P
                                        : WPD_PIX_FMT_YUV420P;
    WPDFrame             frame  = WPD_FRAME_INIT;
    WebPDecoderConfig    config;
    WPDStatus            status;
    long                 differing = 0;
    int                  uv_width, uv_height;
    char                 label[64];

    if (!lossless_yuv_is_gamma && coding == WPD_CODING_LOSSLESS)
        return;
    status = wpd_decode(data, size, format, options, &frame);
    if (!WebPInitDecoderConfig(&config))
        return;
    config.options           = *webp_options;
    config.output.colorspace = alpha ? MODE_YUVA : MODE_YUV;
    if (WebPDecode(data, size, &config) != VP8_STATUS_OK) {
        wpd_frame_free(&frame);
        WebPFreeDecBuffer(&config.output);
        return;
    }
    if (status != WPD_OK) {
        fprintf(stderr,
                "%s: %s %s %s: wpd failed (%s) where libwebp succeeded\n",
                file,
                what,
                detail,
                alpha ? "yuva420p" : "yuv420p",
                wpd_status_string(status));
        failures++;
        WebPFreeDecBuffer(&config.output);
        return;
    }

    uv_width  = (frame.width + 1) / 2;
    uv_height = (frame.height + 1) / 2;
    differing += compare_plane(frame.data[0],
                               frame.stride[0],
                               config.output.u.YUVA.y,
                               config.output.u.YUVA.y_stride,
                               frame.width,
                               frame.height);
    differing += compare_plane(frame.data[1],
                               frame.stride[1],
                               config.output.u.YUVA.u,
                               config.output.u.YUVA.u_stride,
                               uv_width,
                               uv_height);
    differing += compare_plane(frame.data[2],
                               frame.stride[2],
                               config.output.u.YUVA.v,
                               config.output.u.YUVA.v_stride,
                               uv_width,
                               uv_height);
    if (alpha && frame.data[3] && config.output.u.YUVA.a)
        differing += compare_plane(frame.data[3],
                                   frame.stride[3],
                                   config.output.u.YUVA.a,
                                   config.output.u.YUVA.a_stride,
                                   frame.width,
                                   frame.height);
    snprintf(
        label, sizeof(label), "%s %s", detail, alpha ? "yuva420p" : "yuv420p");
    report(file,
           what,
           label,
           differing,
           (long)frame.width * frame.height * (alpha ? 3 : 2));
    wpd_frame_free(&frame);
    WebPFreeDecBuffer(&config.output);
}

static void check_all_formats(const char *file, const uint8_t *data,
                              size_t size, const WPDDecoderOptions *options,
                              const WebPDecoderOptions *webp_options,
                              const char *what, const char *detail,
                              WPDCoding coding) {
    for (size_t i = 0; i < sizeof(formats) / sizeof(*formats); i++)
        check(file, data, size, options, webp_options, (int)i, what, detail);
    check_planar(
        file, data, size, options, webp_options, 0, what, detail, coding);
    check_planar(
        file, data, size, options, webp_options, 1, what, detail, coding);
}

/* libwebp's rescaler, driven directly, against ours. wpd only ever asks for
   one or four channels; libwebp's own SSE2 path diverges from its C at two and
   three, so those are left out. */
#if (defined(__GNUC__) || defined(__clang__)) && !defined(_WIN32)
#define PARITY_HAVE_RESCALER 1
#define MAYBE_WEAK __attribute__((weak))

/* Only a static libwebp exposes these; against a shared one they resolve to
   NULL and the direct comparison is skipped. */
MAYBE_WEAK int WebPRescalerInit(void *r, int src_width, int src_height,
                                uint8_t *dst, int dst_width, int dst_height,
                                int dst_stride, int num_channels,
                                uint32_t *work);
MAYBE_WEAK int WebPRescalerImport(void *r, int num_lines, const uint8_t *src,
                                  int stride);
MAYBE_WEAK int WebPRescalerExport(void *r);
MAYBE_WEAK int WebPRescalerGetScaledDimensions(int src_width, int src_height,
                                               int *scaled_width,
                                               int *scaled_height);

static int rescaler_available(void) {
    return WebPRescalerInit && WebPRescalerImport && WebPRescalerExport &&
        WebPRescalerGetScaledDimensions;
}

/* Rescales the same pseudo-random source both ways. 1 if libwebp agrees with
   us, 0 if it does not, -1 if the run could not be made. */
static int rescaler_agrees(int sw, int sh, int dw, int dh, int ch,
                           unsigned *seed) {
    uint8_t  *src  = malloc((size_t)sw * sh * ch);
    uint8_t  *ours = calloc((size_t)dw * dh * ch, 1);
    uint8_t  *ref  = calloc((size_t)dw * dh * ch, 1);
    uint32_t *w1   = calloc(2 * (size_t)dw * ch, 4);
    uint32_t *w2   = calloc(2 * (size_t)dw * ch, 4);
    char      state[4096];
    int       row    = 0;
    int       result = -1;

    if (src && ours && ref && w1 && w2) {
        for (size_t k = 0; k < (size_t)sw * sh * ch; k++) {
            *seed  = *seed * 1103515245u + 12345u;
            src[k] = (uint8_t)(*seed >> 16);
        }
        wpd_rescale_plane(ours, dw * ch, dw, dh, src, sw * ch, sw, sh, ch, w1);
        memset(state, 0, sizeof(state));
        if (WebPRescalerInit(state, sw, sh, ref, dw, dh, dw * ch, ch, w2)) {
            while (row < sh) {
                row += WebPRescalerImport(
                    state, sh - row, src + (size_t)row * sw * ch, sw * ch);
                WebPRescalerExport(state);
            }
            result = memcmp(ours, ref, (size_t)dw * dh * ch) == 0;
        }
    }
    free(src);
    free(ours);
    free(ref);
    free(w1);
    free(w2);
    return result;
}

/* Whether libwebp rescales this geometry the way its own C rescaler would,
   which is the arithmetic wpd implements.

   libwebp's SIMD rescalers are not bit-exact with its C one. Both compute
   MULT_FIX(x, scale), which the C spells as an exact 64-bit
   ((uint64_t)x * scale + (1 << 31)) >> 32; rescaler_neon.c instead halves the
   scale into a constant (MAKE_HALF_CST) and reaches for vqrdmulhq_s32, which
   drops the scale's low bit -- so an odd scale comes out one too low -- and is
   signed, so an accumulator at or above 2^31 is read as negative. Whether
   either fires depends on the ratio, so most geometries agree and a few do
   not; 576x576 -> 64x1 is one that does not.

   It cannot be dodged by asking libwebp for its C rescaler, because on aarch64
   WEBP_NEON_OMIT_C_CODE leaves the C export functions unbuilt and installs the
   NEON ones without consulting VP8GetCPUInfo. So the reference itself is what
   varies here, and the honest thing is to leave out the geometries where it
   disagrees with the arithmetic it documents rather than to loosen the
   comparison everywhere. Probing keeps that narrow, and re-enables these
   automatically if libwebp makes its SIMD exact -- where it already is, as on
   the SSE2 path at the channel counts below, nothing is left out at all.

   The probe rescales a pseudo-random source rather than the file being
   decoded, so which geometries drop out does not depend on image content. That
   costs a little: a geometry where the two implementations can disagree is
   skipped even for a file whose pixels would not have made them. Reproducing
   the exact intermediate the decoder hands its rescaler is not worth that. */
static int scaling_is_comparable(const WPDDecoderOptions *options,
                                 int full_width, int full_height) {
    static const int channels[] = {1, 4};
    unsigned         seed       = 12345;
    int              sw = full_width, sh = full_height;
    int              dw, dh;

    if (!options || !options->use_scaling || !rescaler_available())
        return 1;
    if (options->use_cropping) {
        sw = options->crop_width;
        sh = options->crop_height;
    }
    dw = options->scaled_width;
    dh = options->scaled_height;
    if (!WebPRescalerGetScaledDimensions(sw, sh, &dw, &dh))
        return 1;
    for (size_t c = 0; c < sizeof(channels) / sizeof(*channels); c++)
        if (rescaler_agrees(sw, sh, dw, dh, channels[c], &seed) == 0)
            return 0;
    return 1;
}
#else
#define PARITY_HAVE_RESCALER 0
#define rescaler_available() 0
#define scaling_is_comparable(options, full_width, full_height) 1
#endif

/* As check_all_formats, minus the one combination scaling has to leave out:
   libwebp has not moved its rescaler onto the gamma-correct conversion, so a
   lossless source reaching YUV through it still goes via the low-quality
   duplicate that upstream 0d14d84b replaced everywhere else. */
static void check_scaled_formats(const char *file, const uint8_t *data,
                                 size_t size, const WPDDecoderOptions *options,
                                 const WebPDecoderOptions *webp_options,
                                 const char *what, const char *detail,
                                 WPDCoding coding, int full_width,
                                 int full_height) {
    if (!scaling_is_comparable(options, full_width, full_height)) {
        fprintf(stderr,
                "%s: %s %s: skipped, libwebp's own rescaler is not exact "
                "here\n",
                file,
                what,
                detail);
        skipped++;
        return;
    }
    for (size_t i = 0; i < sizeof(formats) / sizeof(*formats); i++)
        check(file, data, size, options, webp_options, (int)i, what, detail);
    if (coding == WPD_CODING_LOSSLESS)
        return;
    check_planar(
        file, data, size, options, webp_options, 0, what, detail, coding);
    check_planar(
        file, data, size, options, webp_options, 1, what, detail, coding);
}

static void check_file(const char *dir, const char *name) {
    char               path[4096];
    size_t             size;
    uint8_t           *data;
    WPDImageInfo       info    = WPD_IMAGE_INFO_INIT;
    WPDDecoderOptions  options = WPD_DECODER_OPTIONS_INIT;
    WebPDecoderOptions webp_options;
    static const int   scales[][2] = {
        {32, 32},
        {17, 23},
        {200, 200},
        {64, 0},
        {0, 48},
        {300, 100},
        /* The ends of the range: a single pixel, a single row or column, where
           the ratio stops fitting the rescaler's fixed point, and a large
           expansion in both directions. */
        {1, 1},
        {1, 64},
        {64, 1},
        {2, 1},
        {512, 512},
    };
    static const int crops[][4] = {
        {0, 0, 16, 16},
        {2, 4, 32, 24},
        {1, 1, 31, 29},
        {3, 5, 31, 29},
        {5, 3, 17, 19},
        {7, 7, 9, 11},
    };

    snprintf(path, sizeof(path), "%s/%s", dir, name);
    data = read_file(path, &size);
    if (!data) {
        fprintf(stderr, "%s: cannot read\n", path);
        return;
    }
    if (wpd_get_info(data, size, &info) != WPD_OK || info.is_animation) {
        free(data);
        return;
    }

    memset(&webp_options, 0, sizeof(webp_options));
    check_all_formats(
        name, data, size, NULL, &webp_options, "plain", "", info.coding);

    for (size_t i = 0; i < sizeof(crops) / sizeof(*crops); i++) {
        char detail[64];

        if (crops[i][0] + crops[i][2] > info.width ||
            crops[i][1] + crops[i][3] > info.height)
            continue;
        options              = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
        options.use_cropping = 1;
        options.crop_left    = crops[i][0];
        options.crop_top     = crops[i][1];
        options.crop_width   = crops[i][2];
        options.crop_height  = crops[i][3];
        memset(&webp_options, 0, sizeof(webp_options));
        webp_options.use_cropping = 1;
        webp_options.crop_left    = crops[i][0];
        webp_options.crop_top     = crops[i][1];
        webp_options.crop_width   = crops[i][2];
        webp_options.crop_height  = crops[i][3];
        snprintf(detail,
                 sizeof(detail),
                 "%d,%d %dx%d",
                 crops[i][0],
                 crops[i][1],
                 crops[i][2],
                 crops[i][3]);
        check_all_formats(name,
                          data,
                          size,
                          &options,
                          &webp_options,
                          "crop",
                          detail,
                          info.coding);
    }

    for (size_t i = 0; i < sizeof(scales) / sizeof(*scales); i++) {
        char detail[64];

        options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
        options.use_scaling   = 1;
        options.scaled_width  = scales[i][0];
        options.scaled_height = scales[i][1];
        memset(&webp_options, 0, sizeof(webp_options));
        webp_options.use_scaling   = 1;
        webp_options.scaled_width  = scales[i][0];
        webp_options.scaled_height = scales[i][1];
        snprintf(detail, sizeof(detail), "%dx%d", scales[i][0], scales[i][1]);
        check_scaled_formats(name,
                             data,
                             size,
                             &options,
                             &webp_options,
                             "scale",
                             detail,
                             info.coding,
                             info.width,
                             info.height);
    }

    /* Cropping and scaling together, because the two interact: libwebp scales
       the cropped region but decides whether the downscale is steep enough to
       drop the in-loop filter from the size of the whole frame. */
    for (size_t i = 0; i < sizeof(crops) / sizeof(*crops); i++) {
        static const int combined[][2] = {{8, 8}, {40, 40}, {0, 12}};
        char             detail[64];

        if (crops[i][0] + crops[i][2] > info.width ||
            crops[i][1] + crops[i][3] > info.height)
            continue;
        for (size_t k = 0; k < sizeof(combined) / sizeof(*combined); k++) {
            options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
            options.use_cropping  = 1;
            options.crop_left     = crops[i][0];
            options.crop_top      = crops[i][1];
            options.crop_width    = crops[i][2];
            options.crop_height   = crops[i][3];
            options.use_scaling   = 1;
            options.scaled_width  = combined[k][0];
            options.scaled_height = combined[k][1];
            memset(&webp_options, 0, sizeof(webp_options));
            webp_options.use_cropping  = 1;
            webp_options.crop_left     = crops[i][0];
            webp_options.crop_top      = crops[i][1];
            webp_options.crop_width    = crops[i][2];
            webp_options.crop_height   = crops[i][3];
            webp_options.use_scaling   = 1;
            webp_options.scaled_width  = combined[k][0];
            webp_options.scaled_height = combined[k][1];
            snprintf(detail,
                     sizeof(detail),
                     "%d,%d %dx%d to %dx%d",
                     crops[i][0],
                     crops[i][1],
                     crops[i][2],
                     crops[i][3],
                     combined[k][0],
                     combined[k][1]);
            check_scaled_formats(name,
                                 data,
                                 size,
                                 &options,
                                 &webp_options,
                                 "crop-scale",
                                 detail,
                                 info.coding,
                                 info.width,
                                 info.height);
        }
    }

    options                     = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.no_fancy_upsampling = 1;
    memset(&webp_options, 0, sizeof(webp_options));
    webp_options.no_fancy_upsampling = 1;
    check_all_formats(
        name, data, size, &options, &webp_options, "no-fancy", "", info.coding);

    options                  = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.bypass_filtering = 1;
    memset(&webp_options, 0, sizeof(webp_options));
    webp_options.bypass_filtering = 1;
    check_all_formats(name,
                      data,
                      size,
                      &options,
                      &webp_options,
                      "bypass-filter",
                      "",
                      info.coding);

    options      = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.flip = 1;
    memset(&webp_options, 0, sizeof(webp_options));
    webp_options.flip = 1;
    check_all_formats(
        name, data, size, &options, &webp_options, "flip", "", info.coding);

    /* Flip runs last, over whatever cropping and scaling produced, and offsets
       each plane by its own height, which halves for chroma. An odd height is
       what tells the two roundings apart. */
    for (size_t i = 0; i < sizeof(scales) / sizeof(*scales); i++) {
        char detail[64];

        options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
        options.use_scaling   = 1;
        options.scaled_width  = scales[i][0];
        options.scaled_height = scales[i][1];
        options.flip          = 1;
        memset(&webp_options, 0, sizeof(webp_options));
        webp_options.use_scaling   = 1;
        webp_options.scaled_width  = scales[i][0];
        webp_options.scaled_height = scales[i][1];
        webp_options.flip          = 1;
        snprintf(detail, sizeof(detail), "%dx%d", scales[i][0], scales[i][1]);
        check_scaled_formats(name,
                             data,
                             size,
                             &options,
                             &webp_options,
                             "flip-scale",
                             detail,
                             info.coding,
                             info.width,
                             info.height);
    }

    for (size_t i = 0; i < sizeof(crops) / sizeof(*crops); i++) {
        char detail[64];

        if (crops[i][0] + crops[i][2] > info.width ||
            crops[i][1] + crops[i][3] > info.height)
            continue;
        options              = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
        options.use_cropping = 1;
        options.crop_left    = crops[i][0];
        options.crop_top     = crops[i][1];
        options.crop_width   = crops[i][2];
        options.crop_height  = crops[i][3];
        options.flip         = 1;
        memset(&webp_options, 0, sizeof(webp_options));
        webp_options.use_cropping = 1;
        webp_options.crop_left    = crops[i][0];
        webp_options.crop_top     = crops[i][1];
        webp_options.crop_width   = crops[i][2];
        webp_options.crop_height  = crops[i][3];
        webp_options.flip         = 1;
        snprintf(detail,
                 sizeof(detail),
                 "%d,%d %dx%d",
                 crops[i][0],
                 crops[i][1],
                 crops[i][2],
                 crops[i][3]);
        check_all_formats(name,
                          data,
                          size,
                          &options,
                          &webp_options,
                          "flip-crop",
                          detail,
                          info.coding);
    }

    free(data);
}

static void check_rescaler(void) {
#if PARITY_HAVE_RESCALER
    if (rescaler_available()) {
        static const int dims[][4] = {{64, 64, 32, 32},
                                      {64, 64, 128, 128},
                                      {57, 31, 13, 97},
                                      {1, 1, 5, 5},
                                      {100, 80, 100, 80},
                                      {2, 2, 1, 1},
                                      {7, 5, 3, 2},
                                      {31, 29, 64, 64},
                                      {16, 16, 1, 1},
                                      {1, 7, 3, 1},
                                      {200, 150, 37, 41},
                                      {3, 3, 300, 7},
                                      {640, 480, 320, 240},
                                      {5, 5, 10, 3},
                                      {1024, 1024, 32, 32},
                                      {1024, 1024, 17, 23},
                                      {576, 576, 200, 200}};
        unsigned         seed      = 12345;

        for (size_t i = 0; i < sizeof(dims) / sizeof(*dims); i++) {
            static const int channels[] = {1, 4};

            for (size_t c = 0; c < sizeof(channels) / sizeof(*channels); c++) {
                const int ch = channels[c];
                const int sw = dims[i][0], sh = dims[i][1];
                const int dw = dims[i][2], dh = dims[i][3];
                const int agrees = rescaler_agrees(sw, sh, dw, dh, ch, &seed);

                if (agrees < 0) {
                    failures++;
                    continue;
                }
                comparisons++;
                if (!agrees) {
                    fprintf(stderr,
                            "rescaler %dx%d -> %dx%d ch%d differs\n",
                            sw,
                            sh,
                            dw,
                            dh,
                            ch);
                    failures++;
                }
            }
        }
        return;
    }
#endif
    fprintf(stderr,
            "libwebp rescaler internals unavailable; skipping the direct "
            "rescaler comparison\n");
}

int main(int argc, char **argv) {
    static const char *const stills[] = {
        "lossy.webp",
        "lossless.webp",
        "a_lossy.webp",
        "odd_lossy.webp",
        "odd_a_lossy.webp",
        "simplelf-lossy.webp",
        "palette_rgb.webp",
        "palette2bpp_rgb.webp",
        "palette4bpp_rgb.webp",
        "transforms_before_palette.webp",
    };

    if (argc < 2) {
        fprintf(stderr, "usage: %s <wpd-test-data dir>\n", argv[0]);
        return 2;
    }
    wpd_set_log_callback(NULL, NULL);
    check_rescaler();

    {
        char     path[4096];
        size_t   size;
        uint8_t *probe;

        snprintf(path, sizeof(path), "%s/lossless.webp", argv[1]);
        probe = read_file(path, &size);
        if (probe) {
            lossless_yuv_is_gamma = !uses_simple_lossless_chroma(probe, size);
            free(probe);
        }
        if (!lossless_yuv_is_gamma)
            fprintf(stderr,
                    "libwebp predates gamma-correct lossless chroma "
                    "(upstream 0d14d84b); skipping planar checks on lossless "
                    "sources\n");
    }

    for (size_t i = 0; i < sizeof(stills) / sizeof(*stills); i++)
        check_file(argv[1], stills[i]);

    fprintf(stderr,
            "%d comparison(s) against libwebp, %d mismatch(es), %d group(s) "
            "skipped\n",
            comparisons,
            failures,
            skipped);
    return failures ? 1 : 0;
}
