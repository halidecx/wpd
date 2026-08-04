#include "wpd.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int write_plane(FILE *output, const uint8_t *data, ptrdiff_t stride,
                       int width, int height)
{
    for (int y = 0; y < height; y++)
        if (fwrite(data + y * stride, 1, width, output) != (size_t)width)
            return -1;
    return 0;
}

static const char *format_name(WPDPixelFormat format)
{
    switch (format) {
    case WPD_PIX_FMT_YUV420P:  return "yuv420p";
    case WPD_PIX_FMT_YUVA420P: return "yuva420p";
    case WPD_PIX_FMT_ARGB:     return "argb";
    }
    return "unknown";
}

/*
 * Write one raw frame. Requesting yuv420p from a yuva420p frame drops the
 * alpha plane; any other conversion is rejected.
 */
static int write_frame(FILE *output, const WPDFrame *frame,
                       const char *pixel_format)
{
    int planes;

    if (!pixel_format)
        pixel_format = format_name(frame->format);

    if (frame->format == WPD_PIX_FMT_ARGB) {
        if (strcmp(pixel_format, "argb")) {
            fprintf(stderr, "cannot convert argb frame to %s\n", pixel_format);
            return -1;
        }
        return write_plane(output, frame->data[0], frame->stride[0],
                           frame->width * 4, frame->height);
    }

    if (!strcmp(pixel_format, "yuv420p")) {
        planes = 3;
    } else if (!strcmp(pixel_format, "yuva420p")) {
        planes = 4;
    } else {
        fprintf(stderr, "cannot convert %s frame to %s\n",
                format_name(frame->format), pixel_format);
        return -1;
    }
    if (planes == 4 && frame->format != WPD_PIX_FMT_YUVA420P) {
        fprintf(stderr, "frame has no alpha plane\n");
        return -1;
    }

    for (int p = 0; p < planes; p++) {
        int width  = p == 1 || p == 2 ? (frame->width + 1) / 2 : frame->width;
        int height = p == 1 || p == 2 ? (frame->height + 1) / 2 : frame->height;
        if (write_plane(output, frame->data[p], frame->stride[p],
                        width, height) < 0)
            return -1;
    }
    return 0;
}

static uint8_t *read_file(const char *name, FILE *input, size_t *size)
{
    uint8_t *data = NULL;
    size_t capacity = 0, used = 0;

    for (;;) {
        size_t n;
        if (used == capacity) {
            uint8_t *grown;
            capacity = capacity ? capacity * 2 : 1 << 16;
            grown = realloc(data, capacity);
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

int main(int argc, char **argv)
{
    FILE *input = NULL, *output = NULL;
    WPDDecoder *decoder = NULL;
    uint8_t *data = NULL;
    size_t size;
    const char *pixel_format = NULL;
    int discard_output, frames = 0, ret, status = 1;
    WPDFrame frame;

    if (argc < 3 || argc > 4) {
        fprintf(stderr, "usage: %s input.webp output.yuv [pixel_format]\n",
                argv[0]);
        return 2;
    }
    if (argc == 4)
        pixel_format = argv[3];
    discard_output = !strcmp(argv[2], "/dev/null");
    input = fopen(argv[1], "rb");
    if (!discard_output)
        output = fopen(argv[2], "wb");
    if (!input || (!discard_output && !output)) {
        perror(!input ? argv[1] : argv[2]);
        goto done;
    }

    data = read_file(argv[1], input, &size);
    if (!data)
        goto done;

    decoder = wpd_decoder_create();
    if (!decoder) {
        fprintf(stderr, "out of memory\n");
        goto done;
    }
    if (wpd_decoder_open(decoder, data, size) < 0) {
        fprintf(stderr, "%s: %s\n", argv[1], wpd_decoder_error(decoder));
        goto done;
    }
    while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
        if (output && write_frame(output, &frame, pixel_format) < 0) {
            if (ferror(output))
                perror("write");
            goto done;
        }
        frames++;
    }
    if (ret < 0) {
        fprintf(stderr, "%s: %s\n", argv[1], wpd_decoder_error(decoder));
        goto done;
    }
    if (!frames) {
        fprintf(stderr, "%s: no image data found\n", argv[1]);
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
