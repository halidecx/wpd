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
    OUTPUT_RAW,
    OUTPUT_MD5,
    OUTPUT_NULL,
} OutputType;

typedef struct OutputContext {
    OutputType    type;
    FILE         *file;
    WPDMD5Context md5;
} OutputContext;

static const char short_options[] = "hr:f:";

static const struct option long_options[] = {
    {"help", no_argument, NULL, 'h'},
    {"repeat", required_argument, NULL, 'r'},
    {"fmt", required_argument, NULL, 'f'},
    {"muxer", required_argument, NULL, ARG_MUXER},
    {"verify", required_argument, NULL, ARG_VERIFY},
    {"cpumask", required_argument, NULL, ARG_CPUMASK},
    {NULL, 0, NULL, 0},
};

static void print_banner(void) {
    fprintf(stderr, "wpd by Halide Compression, LLC | %s\n", WPD_VERSION);
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
            "    output pixel format (auto, yuv420p, yuva420p, argb); "
            "default auto\n"
            " --muxer str\n"
            "    output muxer (raw, md5); default raw\n"
            " --verify md5\n"
            "    verify decoded md5; implies --muxer md5 and no output\n"
            " --cpumask str\n"
            "    restrict the instruction sets used; " CPU_MASK_NAMES
            ",\n"
            "    or a number; default all detected\n",
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
               !strcmp(value, "argb")) {
        *pixel_format = value;
    } else {
        return -1;
    }
    return 0;
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

static int output_open(OutputContext *output, const char *muxer,
                       const char *filename) {
    if (!muxer)
        muxer = "raw";

    output->file = NULL;
    if (!strcmp(muxer, "md5")) {
        output->type = OUTPUT_MD5;
        wpd_md5_init(&output->md5);
        if (!filename)
            return 0;
    } else if (!strcmp(filename, "/dev/null")) {
        output->type = OUTPUT_NULL;
        return 0;
    } else {
        output->type = OUTPUT_RAW;
    }

    output->file = !strcmp(filename, "-") ? stdout : fopen(filename, "wb");
    return output->file ? 0 : -1;
}

static int output_write(OutputContext *output, const uint8_t *data,
                        size_t size) {
    if (output->type == OUTPUT_MD5) {
        wpd_md5_update(&output->md5, data, size);
    } else if (output->type == OUTPUT_RAW &&
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

static const char *format_name(WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_YUV420P: return "yuv420p";
    case WPD_PIX_FMT_YUVA420P: return "yuva420p";
    case WPD_PIX_FMT_ARGB: return "argb";
    }
    return "unknown";
}

static int write_frame(OutputContext *output, const WPDFrame *frame,
                       const char *pixel_format) {
    int planes;

    if (!pixel_format)
        pixel_format = format_name(frame->format);

    if (frame->format == WPD_PIX_FMT_ARGB) {
        if (strcmp(pixel_format, "argb")) {
            fprintf(stderr, "cannot convert argb frame to %s\n", pixel_format);
            return -1;
        }
        return write_plane(output,
                           frame->data[0],
                           frame->stride[0],
                           frame->width * 4,
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
    FILE         *input = NULL;
    OutputContext output;
    WPDDecoder   *decoder = NULL;
    uint8_t      *data    = NULL;
    uint8_t       expected_md5[16];
    size_t        size;
    const char   *muxer = NULL, *pixel_format = NULL, *verify = NULL;
    const char   *input_name, *output_name;
    int           frames = 0, output_opened = 0, repeat = 1, ret, status = 1;
    unsigned      cpumask;
    WPDFrame      frame;

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
        case ARG_MUXER:
            if (strcmp(optarg, "raw") && strcmp(optarg, "md5")) {
                usage(argv[0], "invalid output muxer; expected raw or md5");
                return 2;
            }
            muxer = optarg;
            break;
        case ARG_VERIFY: verify = optarg; break;
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
    if (argc - optind != (verify ? 1 : 2)) {
        usage(argv[0],
              verify ? argc - optind < 1 ? "input is required"
                                         : "verification does not accept output"
                  : argc - optind < 2 ? "input and output are required"
                                      : "unexpected argument");
        return 2;
    }

    input_name  = argv[optind];
    output_name = verify ? NULL : argv[optind + 1];
    input       = fopen(input_name, "rb");
    if (!input) {
        perror(input_name);
        goto done;
    }
    if (output_open(&output, verify ? "md5" : muxer, output_name) < 0) {
        perror(output_name);
        goto done;
    }
    output_opened = 1;

    data = read_file(input_name, input, &size);
    if (!data)
        goto done;

    for (int iter = 0; iter < repeat; iter++) {
        OutputContext *sink = iter == 0 && output.type != OUTPUT_NULL ? &output
                                                                      : NULL;

        wpd_decoder_free(decoder);
        frames = 0;

        decoder = wpd_decoder_create();
        if (!decoder) {
            fprintf(stderr, "out of memory\n");
            goto done;
        }
        if (wpd_decoder_open(decoder, data, size) < 0) {
            fprintf(stderr, "%s: %s\n", input_name, wpd_decoder_error(decoder));
            goto done;
        }
        while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
            if (sink && write_frame(sink, &frame, pixel_format) < 0) {
                if (sink->file && ferror(sink->file))
                    perror("write");
                goto done;
            }
            frames++;
        }
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
    } else if (output_close(&output) < 0) {
        perror("write");
    } else {
        status = 0;
    }
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
