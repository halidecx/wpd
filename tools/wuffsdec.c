#define WUFFS_IMPLEMENTATION
#define WUFFS_CONFIG__STATIC_FUNCTIONS
#define WUFFS_CONFIG__MODULES
#define WUFFS_CONFIG__MODULE__BASE
#define WUFFS_CONFIG__MODULE__VP8
#define WUFFS_CONFIG__MODULE__WEBP
#include "wuffs-unsupported-snapshot.c"

#include <errno.h>
#include <getopt.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef WUFFSDEC_REVISION
#define WUFFSDEC_REVISION "unknown"
#endif

static const char short_options[] = "hr:f:";

static const struct option long_options[] = {
    {"help", no_argument, NULL, 'h'},
    {"repeat", required_argument, NULL, 'r'},
    {"fmt", required_argument, NULL, 'f'},
    {NULL, 0, NULL, 0},
};

/* Wuffs swizzles straight into these; argb, which it has no pixel format for,
 * is shuffled out of rgba on the way to the output file, the way libwebpdec
 * converts what libwebp cannot emit directly. off[] holds where alpha, red,
 * green and blue sit in a pixel, -1 if absent. */
typedef struct Layout {
    const char *name;
    uint32_t    pixfmt;
    int         bpp;
    int         off[4];
} Layout;

static const Layout layouts[] = {
    {"rgba", WUFFS_BASE__PIXEL_FORMAT__RGBA_NONPREMUL, 4, {3, 0, 1, 2}},
    {"bgra", WUFFS_BASE__PIXEL_FORMAT__BGRA_NONPREMUL, 4, {3, 2, 1, 0}},
    {"rgb", WUFFS_BASE__PIXEL_FORMAT__RGB, 3, {-1, 0, 1, 2}},
    {"bgr", WUFFS_BASE__PIXEL_FORMAT__BGR, 3, {-1, 2, 1, 0}},
    {"rgbA", WUFFS_BASE__PIXEL_FORMAT__RGBA_PREMUL, 4, {3, 0, 1, 2}},
    {"bgrA", WUFFS_BASE__PIXEL_FORMAT__BGRA_PREMUL, 4, {3, 2, 1, 0}},
    {"argb", WUFFS_BASE__PIXEL_FORMAT__RGBA_NONPREMUL, 4, {0, 1, 2, 3}},
};

static const Layout *find_layout(const char *name) {
    for (size_t i = 0; i < sizeof(layouts) / sizeof(*layouts); i++)
        if (!strcmp(layouts[i].name, name))
            return &layouts[i];
    return NULL;
}

static void print_banner(void) {
    fprintf(stderr,
            "wuffsdec by Halide Compression, LLC | wuffs %s\n",
            WUFFSDEC_REVISION);
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
            "    auto, rgba, bgra, rgb, bgr, rgbA, bgrA, argb\n",
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

static int write_frame(FILE *output, const uint8_t *pixels, const Layout *have,
                       const Layout *want, size_t width, size_t height) {
    size_t   row_size = width * want->bpp;
    uint8_t *row;

    if (have == want)
        return fwrite(pixels, 1, row_size * height, output) == row_size * height
            ? 0
            : -1;

    row = malloc(row_size);
    if (!row) {
        fprintf(stderr, "out of memory\n");
        return -1;
    }
    for (size_t y = 0; y < height; y++) {
        const uint8_t *src = pixels + y * width * have->bpp;

        for (size_t x = 0; x < width; x++)
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

/* Wuffs' webp decoder has no animation support: it skips ANMF chunks, so an
 * animation fails as a truncated still. Name the real reason instead. */
static int is_animation(const uint8_t *data, size_t size) {
    return size >= 21 && !memcmp(data, "RIFF", 4) &&
        !memcmp(data + 8, "WEBPVP8X", 8) && (data[20] & 0x02);
}

static int decode(wuffs_webp__decoder *dec, const char *input_name,
                  uint8_t *data, size_t size, FILE *sink, const Layout *want) {
    wuffs_base__io_buffer    src = wuffs_base__ptr_u8__reader(data, size, true);
    wuffs_base__image_config ic;
    wuffs_base__pixel_buffer pb;
    wuffs_base__status       status;
    const Layout            *have;
    uint8_t                 *pixels = NULL, *workbuf = NULL;
    uint64_t                 pixels_len, workbuf_len;
    uint32_t                 width, height;
    int                      ret = -1;

    status = wuffs_webp__decoder__initialize(
        dec,
        sizeof__wuffs_webp__decoder(),
        WUFFS_VERSION,
        WUFFS_INITIALIZE__LEAVE_INTERNAL_BUFFERS_UNINITIALIZED);
    if (status.repr) {
        fprintf(stderr,
                "%s: %s\n",
                input_name,
                wuffs_base__status__message(&status));
        return -1;
    }
    status = wuffs_webp__decoder__decode_image_config(dec, &ic, &src);
    if (status.repr) {
        fprintf(stderr,
                "%s: %s\n",
                input_name,
                wuffs_base__status__message(&status));
        return -1;
    }

    width  = wuffs_base__pixel_config__width(&ic.pixcfg);
    height = wuffs_base__pixel_config__height(&ic.pixcfg);
    if (!want) {
        wuffs_base__pixel_format pixfmt =
            wuffs_base__pixel_config__pixel_format(&ic.pixcfg);

        want = find_layout(wuffs_base__pixel_format__transparency(&pixfmt) ==
                                   WUFFS_BASE__PIXEL_ALPHA_TRANSPARENCY__OPAQUE
                               ? "rgb"
                               : "rgba");
    }
    have = strcmp(want->name, "argb") ? want : find_layout("rgba");
    wuffs_base__pixel_config__set(&ic.pixcfg,
                                  have->pixfmt,
                                  WUFFS_BASE__PIXEL_SUBSAMPLING__NONE,
                                  width,
                                  height);

    pixels_len  = (uint64_t)width * height * have->bpp;
    workbuf_len = wuffs_webp__decoder__workbuf_len(dec).max_incl;
    if (pixels_len > SIZE_MAX || workbuf_len > SIZE_MAX) {
        fprintf(stderr, "%s: image is too large\n", input_name);
        return -1;
    }
    pixels  = malloc((size_t)pixels_len);
    workbuf = malloc(workbuf_len ? (size_t)workbuf_len : 1);
    if (!pixels || !workbuf) {
        fprintf(stderr, "out of memory\n");
        goto done;
    }

    status = wuffs_base__pixel_buffer__set_from_slice(
        &pb, &ic.pixcfg, wuffs_base__make_slice_u8(pixels, (size_t)pixels_len));
    if (!status.repr)
        status = wuffs_webp__decoder__decode_frame(
            dec,
            &pb,
            &src,
            WUFFS_BASE__PIXEL_BLEND__SRC,
            wuffs_base__make_slice_u8(workbuf, (size_t)workbuf_len),
            NULL);
    if (status.repr) {
        fprintf(stderr,
                "%s: %s\n",
                input_name,
                wuffs_base__status__message(&status));
        goto done;
    }

    if (sink && write_frame(sink, pixels, have, want, width, height) < 0)
        goto done;
    ret = 0;
done:
    free(workbuf);
    free(pixels);
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
    FILE                *input = NULL, *output = NULL;
    uint8_t             *data = NULL;
    size_t               size;
    const Layout        *want = NULL;
    const char          *input_name, *output_name;
    wuffs_webp__decoder *dec = NULL;
    int                  discard_output, repeat = 1, status = 1;

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
            if (!strcmp(optarg, "auto")) {
                want = NULL;
            } else if (!(want = find_layout(optarg))) {
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
    if (is_animation(data, size)) {
        fprintf(stderr, "%s: wuffs cannot decode animations\n", input_name);
        goto done;
    }

    dec = malloc(sizeof__wuffs_webp__decoder());
    if (!dec) {
        fprintf(stderr, "out of memory\n");
        goto done;
    }
    for (int iter = 0; iter < repeat; iter++)
        if (decode(
                dec, input_name, data, size, iter == 0 ? output : NULL, want) <
            0)
            goto done;
    status = 0;
done:
    free(dec);
    free(data);
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
