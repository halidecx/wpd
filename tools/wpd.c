#include "wpd.h"
#include "cpu.h"
#include "md5.h"
#include "vcs_version.h"

#include <errno.h>
#include <getopt.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum {
    ARG_MUXER = 256,
    ARG_VERIFY,
    ARG_CPUMASK,
    ARG_INFO,
    ARG_STREAM,
    ARG_SUBFRAME,
    ARG_LOOPS,
};

typedef struct CpuMask {
    const char *name;
    unsigned    mask;
} CpuMask;

#if WPD_ARCH_X86
#define CPU_MASK_NAMES "sse, sse2, ssse3, sse41, avx2, none"
#elif WPD_ARCH_ARM
#define CPU_MASK_NAMES "armv6, neon, none"
#elif WPD_ARCH_AARCH64
#define CPU_MASK_NAMES "neon, none"
#else
#define CPU_MASK_NAMES "none"
#endif

static const CpuMask cpu_masks[] = {
#if WPD_ARCH_X86
    {"sse", WPD_X86_CPU_FLAG_SSE},
    {"sse2", WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE},
    {"ssse3",
     WPD_X86_CPU_FLAG_SSSE3 | WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE},
    {"sse41",
     WPD_X86_CPU_FLAG_SSE41 | WPD_X86_CPU_FLAG_SSSE3 | WPD_X86_CPU_FLAG_SSE2 |
         WPD_X86_CPU_FLAG_SSE},
    {"avx2",
     WPD_X86_CPU_FLAG_AVX2 | WPD_X86_CPU_FLAG_SSE41 | WPD_X86_CPU_FLAG_SSSE3 |
         WPD_X86_CPU_FLAG_SSE2 | WPD_X86_CPU_FLAG_SSE},
#elif WPD_ARCH_ARM
    {"armv6", WPD_ARM_CPU_FLAG_ARMV6},
    {"neon", WPD_ARM_CPU_FLAG_NEON | WPD_ARM_CPU_FLAG_ARMV6},
#elif WPD_ARCH_AARCH64
    {"neon", WPD_ARM_CPU_FLAG_NEON},
#endif
    {"none", 0},
};

typedef enum OutputType {
    OUTPUT_FILE,
    OUTPUT_MD5,
    OUTPUT_NULL,
} OutputType;

typedef enum OutputMuxer {
    MUXER_RAW,
    MUXER_PPM,
    MUXER_PAM,
    MUXER_Y4M,
} OutputMuxer;

typedef struct OutputContext {
    OutputType     type;
    OutputMuxer    muxer;
    FILE          *file;
    WPDMD5Context  md5;
    int            frames;
    int            width;
    int            height;
    WPDPixelFormat format;
} OutputContext;

static const char short_options[] = "hr:f:";

static const struct option long_options[] = {
    {"help", no_argument, NULL, 'h'},
    {"repeat", required_argument, NULL, 'r'},
    {"fmt", required_argument, NULL, 'f'},
    {"muxer", required_argument, NULL, ARG_MUXER},
    {"verify", required_argument, NULL, ARG_VERIFY},
    {"cpumask", required_argument, NULL, ARG_CPUMASK},
    {"info", no_argument, NULL, ARG_INFO},
    {"stream", required_argument, NULL, ARG_STREAM},
    {"subframe", no_argument, NULL, ARG_SUBFRAME},
    {"loops", required_argument, NULL, ARG_LOOPS},
    {NULL, 0, NULL, 0},
};

static void print_banner(void) {
    fprintf(stderr,
            "wpd by Halide Compression, LLC | %s | %s\n",
            wpd_version_string(),
            WPD_VCS_VERSION);
}

static void usage(const char *app, const char *reason) {
    if (reason)
        fprintf(stderr, "\n%s\n", reason);
    fprintf(stderr,
            "\nusage:  %s [options] input [output]\n"
            "\noptions:\n"
            " -h, --help\n"
            "    view help menu\n"
            " -r, --repeat u32\n"
            "    repeat decode for benchmarking (1..INT_MAX); default 1\n"
            " -f, --fmt str\n"
            "    output pixel format; default auto. one of\n"
            "    auto, yuv420p, yuva420p,\n"
            "    argb, rgba, bgra, rgb, bgr, Argb, rgbA, bgrA,\n"
            "    rgb565, rgba4444, rgbA4444,\n"
            "    bgr565, bgra4444, bgrA4444\n"
            "    the packed formats convert lossy frames and match the\n"
            "    like-named libwebp colorspace bit-exactly; a lowercase\n"
            "    letter marks the channels alpha is multiplied into, and\n"
            "    the bgr 16-bit ones swap the two bytes of every pixel\n"
            " --muxer str\n"
            "    output muxer (raw, md5, ppm, pam, y4m); default is selected\n"
            "    from a .ppm, .pam or .y4m output extension, or raw\n"
            " --verify md5\n"
            "    verify decoded md5; implies --muxer md5 and no output\n"
            " --cpumask str\n"
            "    restrict the instruction sets used; " CPU_MASK_NAMES
            ",\n"
            "    or a number; default all detected\n"
            " --info\n"
            "    print canvas, animation, the frame table and per-frame\n"
            "    timing to stdout\n"
            " --stream u32\n"
            "    decode incrementally, appending this many bytes at a time,\n"
            "    instead of opening the file whole\n"
            " --subframe\n"
            "    yield each animation sub-frame uncomposited, with its own\n"
            "    dimensions and canvas offset, instead of a finished canvas\n"
            " --loops u32\n"
            "    replay the animation this many times, rewinding between\n"
            "    passes; --stream, which cannot be rewound, reopens instead.\n"
            "    only the first pass is written out. default 1\n",
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

static const struct {
    const char    *name;
    WPDPixelFormat format;
} pixel_formats[] = {
    {"yuv420p", WPD_PIX_FMT_YUV420P},
    {"yuva420p", WPD_PIX_FMT_YUVA420P},
    {"argb", WPD_PIX_FMT_ARGB},
    {"rgba", WPD_PIX_FMT_RGBA},
    {"bgra", WPD_PIX_FMT_BGRA},
    {"rgb", WPD_PIX_FMT_RGB},
    {"bgr", WPD_PIX_FMT_BGR},
    {"Argb", WPD_PIX_FMT_ARGB_PRE},
    {"rgbA", WPD_PIX_FMT_RGBA_PRE},
    {"bgrA", WPD_PIX_FMT_BGRA_PRE},
    {"rgb565", WPD_PIX_FMT_RGB565},
    {"rgba4444", WPD_PIX_FMT_RGBA4444},
    {"rgbA4444", WPD_PIX_FMT_RGBA4444_PRE},
    {"bgr565", WPD_PIX_FMT_BGR565},
    {"bgra4444", WPD_PIX_FMT_BGRA4444},
    {"bgrA4444", WPD_PIX_FMT_BGRA4444_PRE},
};

static int parse_format(const char *value, const char **pixel_format,
                        WPDPixelFormat *format) {
    if (!strcmp(value, "auto")) {
        *pixel_format = NULL;
        *format       = WPD_PIX_FMT_NONE;
        return 0;
    }
    for (size_t i = 0; i < sizeof(pixel_formats) / sizeof(*pixel_formats); i++)
        if (!strcmp(pixel_formats[i].name, value)) {
            *pixel_format = pixel_formats[i].name;
            *format       = pixel_formats[i].format;
            return 0;
        }
    return -1;
}

static int parse_cpumask(const char *value, unsigned *mask) {
    char         *end;
    unsigned long parsed;

    for (size_t i = 0; i < sizeof(cpu_masks) / sizeof(*cpu_masks); i++) {
        if (!strcmp(cpu_masks[i].name, value)) {
            *mask = cpu_masks[i].mask;
            return 0;
        }
    }

    errno  = 0;
    parsed = strtoul(value, &end, 0);
    if (errno == ERANGE || end == value || *end || value[0] == '-' ||
        parsed != (unsigned long)(unsigned)parsed)
        return -1;
    *mask = (unsigned)parsed;
    return 0;
}

static void warn_baseline_cpumask(unsigned mask) {
#if WPD_TRIM_DSP_FUNCTIONS
    unsigned forced = wpd_get_default_cpu_flags() & ~mask;

    if (forced)
        fprintf(stderr,
                "warning: cannot disable flags 0x%x below the build target; "
                "reconfigure with -Dtrim_dsp=false\n",
                forced);
#else
    (void)mask;
#endif
}

static int parse_md5(const char *value, uint8_t digest[16]) {
    if (strlen(value) != 32)
        return -1;
    for (int i = 0; i < 16; i++) {
        int hi = value[i * 2];
        int lo = value[i * 2 + 1];
        hi     = hi >= '0' && hi <= '9'  ? hi - '0'
                : hi >= 'a' && hi <= 'f' ? hi - 'a' + 10
                : hi >= 'A' && hi <= 'F' ? hi - 'A' + 10
                                         : -1;
        lo     = lo >= '0' && lo <= '9'  ? lo - '0'
                : lo >= 'a' && lo <= 'f' ? lo - 'a' + 10
                : lo >= 'A' && lo <= 'F' ? lo - 'A' + 10
                                         : -1;
        if (hi < 0 || lo < 0)
            return -1;
        digest[i] = hi << 4 | lo;
    }
    return 0;
}

static const char *filename_extension(const char *filename) {
    const char *slash     = strrchr(filename, '/');
    const char *backslash = strrchr(filename, '\\');
    const char *dot       = strrchr(filename, '.');

    if (backslash && (!slash || backslash > slash))
        slash = backslash;

    return dot && (!slash || dot > slash) ? dot + 1 : NULL;
}

static int output_open(OutputContext *output, const char *muxer,
                       const char *filename) {
    const char *extension;

    memset(output, 0, sizeof(*output));
    if (!muxer) {
        extension = filename_extension(filename);
        muxer     = extension &&
                (!strcmp(extension, "ppm") || !strcmp(extension, "pam") ||
                 !strcmp(extension, "y4m"))
            ? extension
            : "raw";
    }

    output->file = NULL;
    if (!strcmp(muxer, "md5")) {
        output->type  = OUTPUT_MD5;
        output->muxer = MUXER_RAW;
        wpd_md5_init(&output->md5);
        if (!filename)
            return 0;
    } else {
        output->type  = !strcmp(filename, "/dev/null") ? OUTPUT_NULL
                                                       : OUTPUT_FILE;
        output->muxer = !strcmp(muxer, "ppm") ? MUXER_PPM
            : !strcmp(muxer, "pam")           ? MUXER_PAM
            : !strcmp(muxer, "y4m")           ? MUXER_Y4M
                                              : MUXER_RAW;
        if (output->type == OUTPUT_NULL)
            return 0;
    }

    output->file = !strcmp(filename, "-") ? stdout : fopen(filename, "wb");
    return output->file ? 0 : -1;
}

static int output_select_format(const OutputContext *output,
                                const WPDImageInfo  *info,
                                const char         **pixel_format,
                                WPDPixelFormat      *format) {
    const char    *required_name;
    WPDPixelFormat required;

    switch (output->muxer) {
    case MUXER_PPM:
        required_name = "rgb";
        required      = WPD_PIX_FMT_RGB;
        break;
    case MUXER_PAM:
        required_name = "rgba";
        required      = WPD_PIX_FMT_RGBA;
        break;
    case MUXER_Y4M:
        if (*format == WPD_PIX_FMT_YUV420P || *format == WPD_PIX_FMT_YUVA420P)
            return 0;
        if (*format != WPD_PIX_FMT_NONE) {
            fprintf(stderr, "y4m requires yuv420p or yuva420p output\n");
            return -1;
        }
        required_name = info->has_alpha ? "yuva420p" : "yuv420p";
        required = info->has_alpha ? WPD_PIX_FMT_YUVA420P : WPD_PIX_FMT_YUV420P;
        break;
    default: return 0;
    }

    if (*format != WPD_PIX_FMT_NONE && *format != required) {
        fprintf(stderr,
                "%s requires %s output\n",
                output->muxer == MUXER_PPM ? "ppm" : "pam",
                required_name);
        return -1;
    } else {
        *pixel_format = required_name;
        *format       = required;
    }
    return 0;
}

static int output_write(OutputContext *output, const uint8_t *data,
                        size_t size) {
    if (output->type == OUTPUT_MD5) {
        wpd_md5_update(&output->md5, data, size);
    } else if (output->type == OUTPUT_FILE &&
               fwrite(data, 1, size, output->file) != size) {
        return -1;
    }
    return 0;
}

static int output_close_file(OutputContext *output) {
    int ret;

    if (!output->file)
        return 0;
    if (output->file == stdout) {
        ret = fflush(output->file);
    } else {
        ret = fclose(output->file);
    }
    output->file = NULL;
    return ret;
}

static int output_close(OutputContext *output) {
    int status = 0;

    if (output->type == OUTPUT_MD5) {
        uint8_t digest[16];
        wpd_md5_final(&output->md5, digest);
        for (int i = 0; i < 16; i++)
            if (fprintf(output->file, "%02x", digest[i]) < 0)
                status = -1;
        if (fputc('\n', output->file) == EOF)
            status = -1;
    }
    if (output_close_file(output) < 0)
        status = -1;
    return status;
}

static int output_verify(OutputContext *output, const uint8_t expected[16]) {
    uint8_t digest[16];

    wpd_md5_final(&output->md5, digest);
    return memcmp(digest, expected, sizeof(digest)) != 0;
}

static int write_plane(OutputContext *output, const uint8_t *data,
                       ptrdiff_t stride, int width, int height) {
    for (int y = 0; y < height; y++)
        if (output_write(output, data + y * stride, width) < 0)
            return -1;
    return 0;
}

static int write_header(OutputContext *output, const char *header) {
    return output_write(output, (const uint8_t *)header, strlen(header));
}

static int write_chroma_444(OutputContext *output, const uint8_t *data,
                            ptrdiff_t stride, int width, int height) {
    uint8_t *row = malloc((size_t)width);

    if (!row)
        return -1;
    for (int y = 0; y < height; y++) {
        const uint8_t *src = data + (y / 2) * stride;
        for (int x = 0; x < width; x++) row[x] = src[x / 2];
        if (output_write(output, row, (size_t)width) < 0) {
            free(row);
            return -1;
        }
    }
    free(row);
    return 0;
}

static const char *format_name(WPDPixelFormat format) {
    for (size_t i = 0; i < sizeof(pixel_formats) / sizeof(*pixel_formats); i++)
        if (pixel_formats[i].format == format)
            return pixel_formats[i].name;
    return "unknown";
}

static int write_frame(OutputContext *output, const WPDFrame *frame,
                       const char *pixel_format) {
    int  planes;
    char header[128];
    int  header_size;

    if (!pixel_format)
        pixel_format = format_name(frame->format);

    if (output->muxer == MUXER_PPM || output->muxer == MUXER_PAM) {
        const WPDPixelFormat required = output->muxer == MUXER_PPM
            ? WPD_PIX_FMT_RGB
            : WPD_PIX_FMT_RGBA;

        if (frame->format != required) {
            fprintf(stderr,
                    "%s requires %s output\n",
                    output->muxer == MUXER_PPM ? "ppm" : "pam",
                    output->muxer == MUXER_PPM ? "rgb" : "rgba");
            return -1;
        }
        header_size = output->muxer == MUXER_PPM
            ? snprintf(header,
                       sizeof(header),
                       "P6\n%d %d\n255\n",
                       frame->width,
                       frame->height)
            : snprintf(header,
                       sizeof(header),
                       "P7\nWIDTH %d\nHEIGHT %d\nDEPTH 4\nMAXVAL 255\n"
                       "TUPLTYPE RGB_ALPHA\nENDHDR\n",
                       frame->width,
                       frame->height);
        if (header_size < 0 || (size_t)header_size >= sizeof(header) ||
            write_header(output, header) < 0)
            return -1;
        return write_plane(output,
                           frame->data[0],
                           frame->stride[0],
                           frame->width * (output->muxer == MUXER_PPM ? 3 : 4),
                           frame->height);
    }

    if (output->muxer == MUXER_Y4M) {
        if (frame->format != WPD_PIX_FMT_YUV420P &&
            frame->format != WPD_PIX_FMT_YUVA420P) {
            fprintf(stderr, "y4m requires yuv420p or yuva420p output\n");
            return -1;
        }
        if (!output->frames) {
            output->width  = frame->width;
            output->height = frame->height;
            output->format = frame->format;
            header_size    = snprintf(
                header,
                sizeof(header),
                "YUV4MPEG2 W%d H%d F0:0 Ip A0:0 C%s\n",
                frame->width,
                frame->height,
                frame->format == WPD_PIX_FMT_YUVA420P ? "444alpha" : "420jpeg");
            if (header_size < 0 || (size_t)header_size >= sizeof(header) ||
                write_header(output, header) < 0)
                return -1;
        } else if (frame->width != output->width ||
                   frame->height != output->height ||
                   frame->format != output->format) {
            fprintf(stderr, "y4m frames must have one size and format\n");
            return -1;
        }
        output->frames++;
        if (write_header(output, "FRAME\n") < 0 ||
            write_plane(output,
                        frame->data[0],
                        frame->stride[0],
                        frame->width,
                        frame->height) < 0)
            return -1;
        if (frame->format == WPD_PIX_FMT_YUVA420P) {
            if (write_chroma_444(output,
                                 frame->data[1],
                                 frame->stride[1],
                                 frame->width,
                                 frame->height) < 0 ||
                write_chroma_444(output,
                                 frame->data[2],
                                 frame->stride[2],
                                 frame->width,
                                 frame->height) < 0 ||
                write_plane(output,
                            frame->data[3],
                            frame->stride[3],
                            frame->width,
                            frame->height) < 0)
                return -1;
        } else {
            const int chroma_width  = (frame->width + 1) / 2;
            const int chroma_height = (frame->height + 1) / 2;
            if (write_plane(output,
                            frame->data[1],
                            frame->stride[1],
                            chroma_width,
                            chroma_height) < 0 ||
                write_plane(output,
                            frame->data[2],
                            frame->stride[2],
                            chroma_width,
                            chroma_height) < 0)
                return -1;
        }
        return 0;
    }

    if (frame->format >= WPD_PIX_FMT_ARGB) {
        const int bpp = frame->format == WPD_PIX_FMT_RGB ||
                frame->format == WPD_PIX_FMT_BGR
            ? 3
            : frame->format == WPD_PIX_FMT_RGB565 ||
                frame->format == WPD_PIX_FMT_RGBA4444 ||
                frame->format == WPD_PIX_FMT_RGBA4444_PRE ||
                frame->format == WPD_PIX_FMT_BGR565 ||
                frame->format == WPD_PIX_FMT_BGRA4444 ||
                frame->format == WPD_PIX_FMT_BGRA4444_PRE
            ? 2
            : 4;

        if (strcmp(pixel_format, format_name(frame->format))) {
            fprintf(stderr,
                    "cannot convert %s frame to %s\n",
                    format_name(frame->format),
                    pixel_format);
            return -1;
        }
        return write_plane(output,
                           frame->data[0],
                           frame->stride[0],
                           frame->width * bpp,
                           frame->height);
    }

    if (!strcmp(pixel_format, "yuv420p")) {
        planes = 3;
    } else if (!strcmp(pixel_format, "yuva420p")) {
        planes = 4;
    } else {
        fprintf(stderr,
                "cannot convert %s frame to %s\n",
                format_name(frame->format),
                pixel_format);
        return -1;
    }
    if (planes == 4 && frame->format != WPD_PIX_FMT_YUVA420P) {
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

typedef struct DecodeContext {
    OutputContext *sink;
    const char    *pixel_format;
    int            info;
    int            frames;
    int            info_printed;
} DecodeContext;

static void print_image_info(WPDDecoder *decoder, DecodeContext *ctx) {
    static const char *const codings[] = {"unknown", "lossy", "lossless"};
    WPDImageInfo             image     = WPD_IMAGE_INFO_INIT;

    if (ctx->info_printed || wpd_decoder_get_info(decoder, &image) != WPD_OK)
        return;
    ctx->info_printed = 1;
    printf("canvas: %dx%d\n", image.width, image.height);
    printf("coding: %s\n", codings[image.coding]);
    printf("alpha: %d\n", image.has_alpha);
    printf("animation: %d\n", image.is_animation);
    printf("frames: %d\n", image.frame_count);
    printf("loops: %d\n", image.loop_count);
    printf("background: 0x%08x\n", image.background_argb);

    for (int i = 0;; i++) {
        WPDFrameInfo entry = WPD_FRAME_INFO_INIT;

        if (wpd_decoder_frame_info(decoder, i, &entry) != WPD_OK)
            break;
        printf(
            "table %d: %dx%d at %d,%d duration %d dispose %d blend %d "
            "alpha %d complete %d\n",
            i,
            entry.width,
            entry.height,
            entry.pos_x,
            entry.pos_y,
            entry.duration,
            entry.dispose,
            entry.blend,
            entry.has_alpha,
            entry.complete);
    }
}

static void print_metadata(WPDDecoder *decoder) {
    static const struct {
        WPDMetadata which;
        const char *name;
    } kinds[] = {
        {WPD_METADATA_ICCP, "iccp"},
        {WPD_METADATA_EXIF, "exif"},
        {WPD_METADATA_XMP, "xmp"},
    };

    for (size_t i = 0; i < sizeof(kinds) / sizeof(*kinds); i++) {
        const uint8_t *data;
        size_t         size;

        if (wpd_decoder_metadata(decoder, kinds[i].which, &data, &size) ==
                WPD_OK &&
            size)
            printf("%s: %zu bytes\n", kinds[i].name, size);
    }
}

/* Pulls every frame currently available. Returns 0 when the decoder has
   nothing more for now, or negative on error. */
static int drain_frames(WPDDecoder *decoder, DecodeContext *ctx) {
    WPDFrame frame = WPD_FRAME_INIT;
    int      ret;

    while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
        if (ctx->info)
            printf(
                "frame %d: %dx%d %s duration %d timestamp %lld at %d,%d "
                "dispose %d blend %d alpha %d\n",
                ctx->frames,
                frame.width,
                frame.height,
                format_name(frame.format),
                frame.duration,
                (long long)frame.timestamp,
                frame.pos_x,
                frame.pos_y,
                frame.dispose,
                frame.blend,
                frame.has_alpha);
        if (ctx->sink &&
            write_frame(ctx->sink, &frame, ctx->pixel_format) < 0) {
            if (ctx->sink->file && ferror(ctx->sink->file))
                perror("write");
            return -1;
        }
        ctx->frames++;
    }
    return ret;
}

static int decode_stream(WPDDecoder *decoder, const uint8_t *data, size_t size,
                         size_t chunk, DecodeContext *ctx) {
    int last_rows = 0;

    if (wpd_decoder_open_stream(decoder) < 0)
        return -1;

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n = size - offset < chunk ? size - offset : chunk;

        if (wpd_decoder_append(decoder, data + offset, n) < 0)
            return -1;
        if (drain_frames(decoder, ctx) < 0)
            return -1;
        if (ctx->info) {
            WPDFrame partial = WPD_FRAME_INIT;
            int      rows    = 0;

            if (wpd_decoder_partial_frame(decoder, &partial, &rows) == WPD_OK &&
                rows > 0 && rows != last_rows) {
                printf("partial: %d of %d rows\n", rows, partial.height);
                last_rows = rows;
            }
        }
    }
    if (wpd_decoder_end_of_stream(decoder) < 0)
        return -1;
    if (ctx->info)
        print_image_info(decoder, ctx);
    return drain_frames(decoder, ctx);
}

static WPDDecoder *create_decoder(WPDPixelFormat out_format,
                                  const char *pixel_format, int subframe) {
    WPDDecoder *decoder = wpd_decoder_create();

    if (!decoder) {
        fprintf(stderr, "out of memory\n");
        return NULL;
    }
    if (out_format != WPD_PIX_FMT_NONE &&
        wpd_decoder_set_output_format(decoder, out_format) < 0) {
        fprintf(stderr, "cannot select %s output\n", pixel_format);
        wpd_decoder_free(decoder);
        return NULL;
    }
    if (subframe &&
        wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) < 0) {
        fprintf(stderr, "cannot select sub-frame output\n");
        wpd_decoder_free(decoder);
        return NULL;
    }
    return decoder;
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
    FILE          *input = NULL;
    OutputContext  output;
    WPDDecoder    *decoder = NULL;
    uint8_t       *data    = NULL;
    uint8_t        expected_md5[16];
    size_t         size;
    const char    *muxer = NULL, *pixel_format = NULL, *verify = NULL;
    WPDPixelFormat out_format = WPD_PIX_FMT_NONE;
    const char    *input_name, *output_name;
    int            info = 0, stream = 0, subframe = 0, loops = 1;
    int            frames = 0, output_opened = 0, repeat = 1, ret, status = 1;
    unsigned       cpumask;

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
            if (parse_format(optarg, &pixel_format, &out_format) < 0) {
                usage(argv[0], "invalid output pixel format");
                return 2;
            }
            break;
        case ARG_MUXER:
            if (strcmp(optarg, "raw") && strcmp(optarg, "md5") &&
                strcmp(optarg, "ppm") && strcmp(optarg, "pam") &&
                strcmp(optarg, "y4m")) {
                usage(argv[0],
                      "invalid output muxer; expected raw, md5, ppm, pam or "
                      "y4m");
                return 2;
            }
            muxer = optarg;
            break;
        case ARG_VERIFY: verify = optarg; break;
        case ARG_INFO: info = 1; break;
        case ARG_SUBFRAME: subframe = 1; break;
        case ARG_LOOPS:
            if (parse_repeat(optarg, &loops) < 0) {
                usage(argv[0], "invalid loop count; expected 1..INT_MAX");
                return 2;
            }
            break;
        case ARG_STREAM:
            if (parse_repeat(optarg, &stream) < 0) {
                usage(argv[0],
                      "invalid stream chunk size; expected 1..INT_MAX");
                return 2;
            }
            break;
        case ARG_CPUMASK:
            if (parse_cpumask(optarg, &cpumask) < 0) {
                usage(argv[0],
                      "invalid cpu mask; expected " CPU_MASK_NAMES
                      ", or a number");
                return 2;
            }
            warn_baseline_cpumask(cpumask);
            wpd_set_cpu_flags_mask(cpumask);
            break;
        default:
            usage(argv[0], "unknown option or missing option value");
            return 2;
        }
    }
    if (verify && muxer && strcmp(muxer, "md5")) {
        usage(argv[0], "verification requires the md5 muxer");
        return 2;
    }
    if (verify && parse_md5(verify, expected_md5) < 0) {
        usage(argv[0], "invalid md5; expected exactly 32 hexadecimal digits");
        return 2;
    }
    if (argc - optind < 1 || argc - optind > (verify ? 1 : 2) ||
        (!verify && !info && argc - optind != 2)) {
        usage(argv[0],
              verify ? argc - optind < 1 ? "input is required"
                                         : "verification does not accept output"
                  : argc - optind < 1 ? "input is required"
                                      : "unexpected argument");
        return 2;
    }

    input_name  = argv[optind];
    output_name = verify || argc - optind < 2 ? NULL : argv[optind + 1];
    input       = fopen(input_name, "rb");
    if (!input) {
        perror(input_name);
        goto done;
    }
    if (verify || output_name) {
        if (output_open(&output, verify ? "md5" : muxer, output_name) < 0) {
            perror(output_name);
            goto done;
        }
        output_opened = 1;
    } else {
        output.type = OUTPUT_NULL;
        output.file = NULL;
    }

    data = read_file(input_name, input, &size);
    if (!data)
        goto done;

    if (output_opened && output.muxer != MUXER_RAW) {
        WPDImageInfo image = WPD_IMAGE_INFO_INIT;

        if (wpd_get_info(data, size, &image) < 0) {
            fprintf(stderr, "%s: cannot read image header\n", input_name);
            goto done;
        }
        if (output_select_format(&output, &image, &pixel_format, &out_format) <
            0)
            goto done;
    }

    for (int iter = 0; iter < repeat; iter++) {
        DecodeContext ctx = {0};

        ctx.sink = iter == 0 && output.type != OUTPUT_NULL ? &output : NULL;
        ctx.pixel_format = pixel_format;
        ctx.info         = info && iter == 0;

        wpd_decoder_free(decoder);
        frames = 0;

        decoder = create_decoder(out_format, pixel_format, subframe);
        if (!decoder)
            goto done;
        if (stream) {
            for (int loop = 0; loop < loops; loop++) {
                if (loop) {
                    wpd_decoder_free(decoder);
                    decoder = create_decoder(
                        out_format, pixel_format, subframe);
                    if (!decoder)
                        goto done;
                    ctx.sink   = NULL;
                    ctx.frames = 0;
                }
                ret = decode_stream(decoder, data, size, (size_t)stream, &ctx);
                if (ret < 0)
                    break;
            }
        } else if (wpd_decoder_open(decoder, data, size) < 0) {
            ret = -1;
        } else {
            if (ctx.info)
                print_image_info(decoder, &ctx);
            ret = drain_frames(decoder, &ctx);
            for (int loop = 1; loop < loops && ret >= 0; loop++) {
                ctx.sink   = NULL;
                ctx.frames = 0;
                ret        = wpd_decoder_rewind(decoder);
                if (ret >= 0)
                    ret = drain_frames(decoder, &ctx);
            }
        }
        if (ctx.info && ret >= 0)
            print_metadata(decoder);
        frames = ctx.frames;
        if (ret < 0) {
            fprintf(stderr, "%s: %s\n", input_name, wpd_decoder_error(decoder));
            goto done;
        }
    }
    if (!frames) {
        fprintf(stderr, "%s: no image data found\n", input_name);
        goto done;
    }
    if (verify) {
        status        = output_verify(&output, expected_md5);
        output_opened = 0;
    } else if (!output_opened)
        status = 0;
    else if (output_close(&output) < 0)
        perror("write");
    else
        status = 0;
    output_opened = 0;
done:
    wpd_decoder_free(decoder);
    free(data);
    if (input)
        fclose(input);
    if (output_opened)
        output_close_file(&output);
    return status;
}
