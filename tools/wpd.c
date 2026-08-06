#include "wpd.h"
#include "vcs_version.h"

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

static void print_banner(void) {
    fprintf(stderr, "wpd by Halide Compression, LLC | %s\n", WPD_VERSION);
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
            "    output pixel format (auto, yuv420p, yuva420p, argb); "
            "default auto\n",
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

static int write_plane(FILE *output, const uint8_t *data, ptrdiff_t stride,
                       int width, int height) {
    for (int y = 0; y < height; y++)
        if (fwrite(data + y * stride, 1, width, output) != (size_t)width)
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

static int write_frame(FILE *output, const WPDFrame *frame,
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
    FILE       *input = NULL, *output = NULL;
    WPDDecoder *decoder = NULL;
    uint8_t    *data    = NULL;
    size_t      size;
    const char *pixel_format = NULL;
    const char *input_name, *output_name;
    int         discard_output, frames = 0, repeat = 1, ret, status = 1;
    WPDFrame    frame;

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
        FILE *sink = iter == 0 ? output : NULL;

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
                if (ferror(sink))
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
    status = 0;
done:
    wpd_decoder_free(decoder);
    free(data);
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
