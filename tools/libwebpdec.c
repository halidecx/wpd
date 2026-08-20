#include <webp/decode.h>
#include <webp/demux.h>

#include <errno.h>
#include <getopt.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char short_options[] = "hr:f:";

static const struct option long_options[] = {
    {"help", no_argument, NULL, 'h'},
    {"repeat", required_argument, NULL, 'r'},
    {"fmt", required_argument, NULL, 'f'},
    {NULL, 0, NULL, 0},
};

typedef enum PixelFormat {
    PIX_FMT_YUV420P,
    PIX_FMT_YUVA420P,
    PIX_FMT_PACKED,
} PixelFormat;

/* libwebp has no RGB animation mode, so RGB/BGR discard alpha after conversion. */
typedef struct Layout {
    const char   *name;
    WEBP_CSP_MODE still_mode;
    WEBP_CSP_MODE anim_mode;
    int           bpp;
    int           off[4];
} Layout;

static const Layout layouts[] = {
    {"argb", MODE_ARGB, MODE_RGBA, 4, {0, 1, 2, 3}},
    {"rgba", MODE_RGBA, MODE_RGBA, 4, {3, 0, 1, 2}},
    {"bgra", MODE_BGRA, MODE_BGRA, 4, {3, 2, 1, 0}},
    {"rgb", MODE_RGB, MODE_RGBA, 3, {-1, 0, 1, 2}},
    {"bgr", MODE_BGR, MODE_BGRA, 3, {-1, 2, 1, 0}},
    {"Argb", MODE_Argb, MODE_rgbA, 4, {0, 1, 2, 3}},
    {"rgbA", MODE_rgbA, MODE_rgbA, 4, {3, 0, 1, 2}},
    {"bgrA", MODE_bgrA, MODE_bgrA, 4, {3, 2, 1, 0}},
};

static const Layout *find_layout(const char *name) {
    for (size_t i = 0; i < sizeof(layouts) / sizeof(*layouts); i++)
        if (!strcmp(layouts[i].name, name))
            return &layouts[i];
    return NULL;
}

static const Layout *layout_for_mode(WEBP_CSP_MODE mode) {
    for (size_t i = 0; i < sizeof(layouts) / sizeof(*layouts); i++)
        if (layouts[i].still_mode == mode)
            return &layouts[i];
    return NULL;
}

typedef struct Frame {
    const uint8_t *data[4];
    ptrdiff_t      stride[4];
    int            width;
    int            height;
    PixelFormat    format;
    const Layout  *layout;
} Frame;

static void print_banner(void) {
    int version = WebPGetDecoderVersion();

    fprintf(stderr,
            "libwebpdec by Halide Compression, LLC | libwebp %d.%d.%d\n",
            (version >> 16) & 0xff,
            (version >> 8) & 0xff,
            version & 0xff);
}

static void usage(const char *app, const char *reason) {
    if (reason)
        fprintf(stderr, "\n%s\n", reason);
    fprintf(stderr,
            "\nusage:  %s [options] input output\n"
            "\noptions:\n"
            " -h, --help\n"
            "    view help menu\n"
            " -r, --repeat u32\n"
            "    repeat decode for benchmarking (1..INT_MAX); default 1\n"
            " -f, --fmt str\n"
            "    output pixel format; default auto. one of\n"
            "    auto, yuv420p, yuva420p,\n"
            "    argb, rgba, bgra, rgb, bgr, Argb, rgbA, bgrA\n",
            app);
}

static int parse_repeat(const char *value, int *repeat) {
    char         *end;
    unsigned long parsed;

    errno  = 0;
    parsed = strtoul(value, &end, 10);
    if (errno == ERANGE || end == value || *end || value[0] == '-' ||
        parsed < 1 || parsed > INT_MAX)
        return -1;
    *repeat = (int)parsed;
    return 0;
}

static int parse_format(const char *value, const char **pixel_format) {
    if (!strcmp(value, "auto")) {
        *pixel_format = NULL;
    } else if (!strcmp(value, "yuv420p") || !strcmp(value, "yuva420p") ||
               find_layout(value)) {
        *pixel_format = value;
    } else {
        return -1;
    }
    return 0;
}

static int write_plane(FILE *output, const uint8_t *data, ptrdiff_t stride,
                       int width, int height) {
    for (int y = 0; y < height; y++)
        if (fwrite(data + y * stride, 1, width, output) != (size_t)width)
            return -1;
    return 0;
}

static int write_converted(FILE *output, const Frame *frame,
                           const Layout *want) {
    const Layout *have     = frame->layout;
    size_t        row_size = (size_t)frame->width * want->bpp;
    uint8_t      *row      = malloc(row_size);

    if (!row) {
        fprintf(stderr, "out of memory\n");
        return -1;
    }
    for (int y = 0; y < frame->height; y++) {
        const uint8_t *src = frame->data[0] + y * frame->stride[0];

        for (int x = 0; x < frame->width; x++)
            for (int c = 0; c < 4; c++) {
                if (want->off[c] < 0)
                    continue;
                row[want->bpp * x + want->off[c]] = have->off[c] < 0
                    ? 0xff
                    : src[have->bpp * x + have->off[c]];
            }
        if (fwrite(row, 1, row_size, output) != row_size) {
            free(row);
            return -1;
        }
    }
    free(row);
    return 0;
}

static const char *format_name(const Frame *frame) {
    switch (frame->format) {
    case PIX_FMT_YUV420P: return "yuv420p";
    case PIX_FMT_YUVA420P: return "yuva420p";
    case PIX_FMT_PACKED: return frame->layout->name;
    }
    return "unknown";
}

static int write_frame(FILE *output, const Frame *frame,
                       const char *pixel_format) {
    int planes;

    if (!pixel_format)
        pixel_format = format_name(frame);

    if (frame->format == PIX_FMT_PACKED) {
        const Layout *want = find_layout(pixel_format);

        if (!want) {
            fprintf(stderr,
                    "cannot convert %s frame to %s\n",
                    format_name(frame),
                    pixel_format);
            return -1;
        }
        if (want == frame->layout)
            return write_plane(output,
                               frame->data[0],
                               frame->stride[0],
                               frame->width * want->bpp,
                               frame->height);
        return write_converted(output, frame, want);
    }

    if (!strcmp(pixel_format, "yuv420p")) {
        planes = 3;
    } else if (!strcmp(pixel_format, "yuva420p")) {
        planes = 4;
    } else {
        fprintf(stderr,
                "cannot convert %s frame to %s\n",
                format_name(frame),
                pixel_format);
        return -1;
    }
    if (planes == 4 && frame->format != PIX_FMT_YUVA420P) {
        fprintf(stderr, "frame has no alpha plane\n");
        return -1;
    }

    for (int p = 0; p < planes; p++) {
        int width  = p == 1 || p == 2 ? (frame->width + 1) / 2 : frame->width;
        int height = p == 1 || p == 2 ? (frame->height + 1) / 2 : frame->height;
        if (write_plane(
                output, frame->data[p], frame->stride[p], width, height) < 0)
            return -1;
    }
    return 0;
}

static const char *status_name(VP8StatusCode status) {
    switch (status) {
    case VP8_STATUS_OK: return "ok";
    case VP8_STATUS_OUT_OF_MEMORY: return "out of memory";
    case VP8_STATUS_INVALID_PARAM: return "invalid parameter";
    case VP8_STATUS_BITSTREAM_ERROR: return "bitstream error";
    case VP8_STATUS_UNSUPPORTED_FEATURE: return "unsupported feature";
    case VP8_STATUS_SUSPENDED: return "suspended";
    case VP8_STATUS_USER_ABORT: return "user abort";
    case VP8_STATUS_NOT_ENOUGH_DATA: return "not enough data";
    }
    return "unknown error";
}

static int decode_still(const char *input_name, const uint8_t *data,
                        size_t size, const WebPBitstreamFeatures *features,
                        FILE *sink, const char *pixel_format, int *frames) {
    WebPDecoderConfig config;
    VP8StatusCode     status;
    Frame             frame = {0};

    if (!WebPInitDecoderConfig(&config)) {
        fprintf(stderr, "libwebp decoder ABI mismatch\n");
        return -1;
    }
    config.input = *features;
    if (pixel_format && find_layout(pixel_format)) {
        frame.layout             = find_layout(pixel_format);
        config.output.colorspace = frame.layout->still_mode;
        frame.format             = PIX_FMT_PACKED;
    } else if (features->format == 2) {
        frame.layout             = find_layout("argb");
        config.output.colorspace = MODE_ARGB;
        frame.format             = PIX_FMT_PACKED;
    } else if (features->has_alpha) {
        config.output.colorspace = MODE_YUVA;
        frame.format             = PIX_FMT_YUVA420P;
    } else {
        config.output.colorspace = MODE_YUV;
        frame.format             = PIX_FMT_YUV420P;
    }

    status = WebPDecode(data, size, &config);
    if (status != VP8_STATUS_OK) {
        fprintf(stderr, "%s: %s\n", input_name, status_name(status));
        WebPFreeDecBuffer(&config.output);
        return -1;
    }

    frame.width  = config.output.width;
    frame.height = config.output.height;
    if (frame.format == PIX_FMT_PACKED) {
        frame.data[0]   = config.output.u.RGBA.rgba;
        frame.stride[0] = config.output.u.RGBA.stride;
    } else {
        frame.data[0]   = config.output.u.YUVA.y;
        frame.data[1]   = config.output.u.YUVA.u;
        frame.data[2]   = config.output.u.YUVA.v;
        frame.data[3]   = config.output.u.YUVA.a;
        frame.stride[0] = config.output.u.YUVA.y_stride;
        frame.stride[1] = config.output.u.YUVA.u_stride;
        frame.stride[2] = config.output.u.YUVA.v_stride;
        frame.stride[3] = config.output.u.YUVA.a_stride;
    }

    if (sink && write_frame(sink, &frame, pixel_format) < 0) {
        WebPFreeDecBuffer(&config.output);
        return -1;
    }
    *frames = 1;
    WebPFreeDecBuffer(&config.output);
    return 0;
}

static int decode_animation(const char *input_name, const uint8_t *data,
                            size_t size, FILE *sink, const char *pixel_format,
                            int *frames) {
    WebPData               webp_data = {data, size};
    WebPAnimDecoderOptions options;
    WebPAnimDecoder       *decoder = NULL;
    WebPAnimInfo           info;
    Frame                  frame = {0};
    const Layout          *layout;
    int                    ret = -1;

    if (!WebPAnimDecoderOptionsInit(&options)) {
        fprintf(stderr, "libwebp demux ABI mismatch\n");
        return -1;
    }
    layout = pixel_format ? find_layout(pixel_format) : find_layout("rgba");
    if (!layout) {
        fprintf(stderr,
                "%s: animations have no %s output\n",
                input_name,
                pixel_format);
        return -1;
    }
    options.color_mode = layout->anim_mode;
    decoder            = WebPAnimDecoderNew(&webp_data, &options);
    if (!decoder) {
        fprintf(stderr, "%s: cannot create animation decoder\n", input_name);
        return -1;
    }
    if (!WebPAnimDecoderGetInfo(decoder, &info)) {
        fprintf(stderr, "%s: cannot read animation information\n", input_name);
        goto done;
    }
    if (info.canvas_width > INT_MAX / 4 || info.canvas_height > INT_MAX) {
        fprintf(stderr, "%s: animation canvas is too large\n", input_name);
        goto done;
    }

    frame.width     = (int)info.canvas_width;
    frame.height    = (int)info.canvas_height;
    frame.format    = PIX_FMT_PACKED;
    frame.layout    = layout_for_mode(layout->anim_mode);
    frame.stride[0] = frame.width * 4;
    while (WebPAnimDecoderHasMoreFrames(decoder)) {
        uint8_t *rgba;
        int      timestamp;

        if (!WebPAnimDecoderGetNext(decoder, &rgba, &timestamp)) {
            fprintf(stderr, "%s: animation decode failed\n", input_name);
            goto done;
        }
        frame.data[0] = rgba;
        if (sink && write_frame(sink, &frame, pixel_format) < 0)
            goto done;
        (*frames)++;
    }
    ret = 0;
done:
    WebPAnimDecoderDelete(decoder);
    return ret;
}

static uint8_t *read_file(const char *name, FILE *input, size_t *size) {
    uint8_t *data     = NULL;
    size_t   capacity = 0, used = 0;

    for (;;) {
        size_t n;
        if (used == capacity) {
            uint8_t *grown;
            capacity = capacity ? capacity * 2 : 1 << 16;
            grown    = realloc(data, capacity);
            if (!grown) {
                free(data);
                return NULL;
            }
            data = grown;
        }
        n = fread(data + used, 1, capacity - used, input);
        used += n;
        if (n == 0) {
            if (ferror(input)) {
                perror(name);
                free(data);
                return NULL;
            }
            break;
        }
    }
    *size = used;
    return data;
}

int main(int argc, char **argv) {
    FILE       *input = NULL, *output = NULL;
    uint8_t    *data = NULL;
    size_t      size;
    const char *pixel_format = NULL;
    const char *input_name, *output_name;
    int         discard_output, frames = 0, repeat = 1, status = 1;

    print_banner();
    opterr = 0;
    for (;;) {
        int option = getopt_long(argc, argv, short_options, long_options, NULL);
        if (option == -1)
            break;
        switch (option) {
        case 'h': usage(argv[0], NULL); return 0;
        case 'r':
            if (parse_repeat(optarg, &repeat) < 0) {
                usage(argv[0], "invalid repeat value; expected 1..INT_MAX");
                return 2;
            }
            break;
        case 'f':
            if (parse_format(optarg, &pixel_format) < 0) {
                usage(argv[0], "invalid output pixel format");
                return 2;
            }
            break;
        default:
            usage(argv[0], "unknown option or missing option value");
            return 2;
        }
    }
    if (argc - optind != 2) {
        usage(argv[0],
              argc - optind < 2 ? "input and output are required"
                                : "unexpected argument");
        return 2;
    }

    input_name     = argv[optind];
    output_name    = argv[optind + 1];
    discard_output = !strcmp(output_name, "/dev/null");
    input          = fopen(input_name, "rb");
    if (!discard_output)
        output = fopen(output_name, "wb");
    if (!input || (!discard_output && !output)) {
        perror(!input ? input_name : output_name);
        goto done;
    }

    data = read_file(input_name, input, &size);
    if (!data)
        goto done;

    for (int iter = 0; iter < repeat; iter++) {
        WebPBitstreamFeatures features;
        VP8StatusCode         decode_status;
        FILE                 *sink = iter == 0 ? output : NULL;

        frames        = 0;
        decode_status = WebPGetFeatures(data, size, &features);
        if (decode_status != VP8_STATUS_OK) {
            fprintf(stderr, "%s: %s\n", input_name, status_name(decode_status));
            goto done;
        }
        if (features.has_animation) {
            if (decode_animation(
                    input_name, data, size, sink, pixel_format, &frames) < 0)
                goto done;
        } else if (decode_still(input_name,
                                data,
                                size,
                                &features,
                                sink,
                                pixel_format,
                                &frames) < 0) {
            goto done;
        }
    }
    if (!frames) {
        fprintf(stderr, "%s: no image data found\n", input_name);
        goto done;
    }
    status = 0;
done:
    free(data);
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
