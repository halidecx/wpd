#include "wpd.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t rl16(const uint8_t *p)
{
    return (uint16_t)(p[0] | p[1] << 8);
}

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

int main(int argc, char **argv)
{
    uint8_t header[32], frame_header[12];
    WPDDecoder *decoder = NULL;
    FILE *input = NULL, *output = NULL;
    int discard_output, status = 1, wrote_header = 0;
    uint32_t frame_rate, time_scale;

    if (argc != 3) {
        fprintf(stderr, "usage: %s input.ivf output.y4m\n", argv[0]);
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
    if (fread(header, 1, sizeof(header), input) != sizeof(header) ||
        memcmp(header, "DKIF", 4) || memcmp(header + 8, "VP80", 4) ||
        rl16(header + 6) < sizeof(header)) {
        fprintf(stderr, "%s: unsupported or invalid VP8 IVF file\n", argv[1]);
        goto done;
    }
    if (rl16(header + 6) > sizeof(header) &&
        fseek(input, rl16(header + 6) - (long)sizeof(header), SEEK_CUR) != 0) {
        fprintf(stderr, "%s: invalid extended IVF header\n", argv[1]);
        goto done;
    }
    frame_rate = rl32(header + 16);
    time_scale = rl32(header + 20);
    if (!frame_rate || !time_scale) {
        frame_rate = 1;
        time_scale = 1;
    }
    decoder = wpd_decoder_create();
    if (!decoder) {
        fprintf(stderr, "out of memory\n");
        goto done;
    }
    while (fread(frame_header, 1, sizeof(frame_header), input) == sizeof(frame_header)) {
        uint32_t size = rl32(frame_header);
        uint8_t *compressed = malloc(size ? size : 1);
        WPDFrame frame;
        int decode_result;
        if (!compressed || fread(compressed, 1, size, input) != size) {
            fprintf(stderr, "%s: truncated IVF frame\n", argv[1]);
            free(compressed);
            goto done;
        }
        decode_result = wpd_decoder_decode(decoder, compressed, size, &frame);
        if (decode_result < 0) {
            fprintf(stderr, "VP8 decode failed: %s\n", wpd_decoder_error(decoder));
            free(compressed);
            goto done;
        }
        free(compressed);
        if (decode_result > 0)
            continue;
        if (discard_output) {
            wrote_header = 1;
            continue;
        }
        if (!wrote_header) {
            if (fprintf(output, "YUV4MPEG2 W%d H%d F%u:%u Ip A0:0 C420jpeg\n",
                        frame.width, frame.height, frame_rate, time_scale) < 0)
                goto done;
            wrote_header = 1;
        }
        if (fputs("FRAME\n", output) == EOF ||
            write_plane(output, frame.data[0], frame.stride[0],
                        frame.width, frame.height) < 0 ||
            write_plane(output, frame.data[1], frame.stride[1],
                        (frame.width + 1) / 2, (frame.height + 1) / 2) < 0 ||
            write_plane(output, frame.data[2], frame.stride[2],
                        (frame.width + 1) / 2, (frame.height + 1) / 2) < 0) {
            perror(argv[2]);
            goto done;
        }
    }
    if (ferror(input) || !wrote_header) {
        fprintf(stderr, "%s: no complete IVF frames\n", argv[1]);
        goto done;
    }
    status = 0;
done:
    wpd_decoder_free(decoder);
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
