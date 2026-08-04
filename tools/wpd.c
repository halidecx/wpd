#include "wpd.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t rl32(const uint8_t *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 |
           (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static int write_plane(FILE *output, const uint8_t *data, ptrdiff_t stride,
                       int width, int height)
{
    for (int y = 0; y < height; y++)
        if (fwrite(data + y * stride, 1, width, output) != (size_t)width)
            return -1;
    return 0;
}

/* Raw planar 4:2:0, the layout dwebp writes for -yuv. */
static int write_frame(FILE *output, const WPDFrame *frame)
{
    if (write_plane(output, frame->data[0], frame->stride[0],
                    frame->width, frame->height) < 0 ||
        write_plane(output, frame->data[1], frame->stride[1],
                    (frame->width + 1) / 2, (frame->height + 1) / 2) < 0 ||
        write_plane(output, frame->data[2], frame->stride[2],
                    (frame->width + 1) / 2, (frame->height + 1) / 2) < 0)
        return -1;
    return 0;
}

/*
 * Walk the RIFF chunks looking for the lossy bitstream. Chunk payloads are
 * padded to an even length, which the size field does not include.
 */
static int decode_webp(const char *input_name, FILE *input, FILE *output)
{
    uint8_t header[12], chunk_header[8];
    WPDDecoder *decoder = NULL;
    int status = 1;
    uint32_t riff_size, consumed = 4;

    if (fread(header, 1, sizeof(header), input) != sizeof(header) ||
        memcmp(header, "RIFF", 4) || memcmp(header + 8, "WEBP", 4)) {
        fprintf(stderr, "%s: not a WebP file\n", input_name);
        return 1;
    }
    riff_size = rl32(header + 4);

    while (consumed + 8 <= riff_size &&
           fread(chunk_header, 1, sizeof(chunk_header), input) == sizeof(chunk_header)) {
        uint32_t size = rl32(chunk_header + 4);
        uint32_t padded_size = size + (size & 1);
        consumed += 8;
        if (!memcmp(chunk_header, "VP8 ", 4)) {
            uint8_t *compressed = malloc(size ? size : 1);
            WPDFrame frame;
            if (!compressed || fread(compressed, 1, size, input) != size) {
                fprintf(stderr, "%s: truncated WebP file\n", input_name);
                free(compressed);
                goto done;
            }
            decoder = wpd_decoder_create();
            if (!decoder) {
                fprintf(stderr, "out of memory\n");
                free(compressed);
                goto done;
            }
            if (wpd_decoder_decode(decoder, compressed, size, &frame) < 0) {
                fprintf(stderr, "VP8 decode failed: %s\n", wpd_decoder_error(decoder));
                free(compressed);
                goto done;
            }
            free(compressed);
            if (output && write_frame(output, &frame) < 0) {
                perror("write");
                goto done;
            }
            status = 0;
            goto done;
        }
        if (!memcmp(chunk_header, "VP8L", 4)) {
            fprintf(stderr, "%s: lossless WebP is not supported\n", input_name);
            goto done;
        }
        if (!memcmp(chunk_header, "ALPH", 4))
            fprintf(stderr, "%s: warning: ignoring alpha plane\n", input_name);
        if (fseek(input, padded_size, SEEK_CUR) != 0) {
            fprintf(stderr, "%s: truncated WebP file\n", input_name);
            goto done;
        }
        consumed += padded_size;
    }
    fprintf(stderr, "%s: no lossy (VP8) bitstream found\n", input_name);
done:
    wpd_decoder_free(decoder);
    return status;
}

int main(int argc, char **argv)
{
    FILE *input = NULL, *output = NULL;
    int discard_output, status = 1;

    if (argc != 3) {
        fprintf(stderr, "usage: %s input.webp output.yuv\n", argv[0]);
        return 2;
    }
    discard_output = !strcmp(argv[2], "/dev/null");
    input = fopen(argv[1], "rb");
    if (!discard_output)
        output = fopen(argv[2], "wb");
    if (!input || (!discard_output && !output)) {
        perror(!input ? argv[1] : argv[2]);
        goto done;
    }
    status = decode_webp(argv[1], input, output);
done:
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
