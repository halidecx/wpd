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

static int decode_ivf(const char *input_name, FILE *input, FILE *output,
                      int raw_yuv, const uint8_t *header)
{
    uint8_t frame_header[12];
    WPDDecoder *decoder = NULL;
    int status = 1, wrote_header = 0;
    uint32_t frame_rate, time_scale;

    if (memcmp(header, "DKIF", 4) || memcmp(header + 8, "VP80", 4) ||
        rl16(header + 6) < 32) {
        fprintf(stderr, "%s: unsupported or invalid VP8 IVF file\n", input_name);
        goto done;
    }
    if (rl16(header + 6) > 32 &&
        fseek(input, rl16(header + 6) - 32L, SEEK_CUR) != 0) {
        fprintf(stderr, "%s: invalid extended IVF header\n", input_name);
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
            fprintf(stderr, "%s: truncated IVF frame\n", input_name);
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
        if (!output) {
            wrote_header = 1;
            continue;
        }
        if (!wrote_header && !raw_yuv) {
            if (fprintf(output, "YUV4MPEG2 W%d H%d F%u:%u Ip A0:0 C420jpeg\n",
                        frame.width, frame.height, frame_rate, time_scale) < 0)
                goto done;
        }
        wrote_header = 1;
        if ((!raw_yuv && fputs("FRAME\n", output) == EOF) ||
            write_frame(output, &frame) < 0) {
            perror("write");
            goto done;
        }
    }
    if (ferror(input) || !wrote_header) {
        fprintf(stderr, "%s: no complete IVF frames\n", input_name);
        goto done;
    }
    status = 0;
done:
    wpd_decoder_free(decoder);
    return status;
}

static int decode_webp(const char *input_name, FILE *input, FILE *output,
                       int raw_yuv, const uint8_t *header)
{
    uint8_t chunk_header[8];
    WPDDecoder *decoder = NULL;
    int status = 1;
    uint32_t riff_size, consumed = 4;

    if (memcmp(header, "RIFF", 4) || memcmp(header + 8, "WEBP", 4)) {
        fprintf(stderr, "%s: invalid WebP file\n", input_name);
        return 1;
    }
    riff_size = rl32(header + 4);
    /* Extra bytes from the 32-byte sniff buffer belong to the first chunk. */
    if (fseek(input, 12, SEEK_SET) != 0) {
        fprintf(stderr, "%s: unseekable input\n", input_name);
        return 1;
    }
    while (consumed + 8 <= riff_size &&
           fread(chunk_header, 1, sizeof(chunk_header), input) == sizeof(chunk_header)) {
        uint32_t size = rl32(chunk_header + 4);
        uint32_t padded_size = size + (size & 1);
        consumed += 8;
        if (!memcmp(chunk_header, "VP8 ", 4)) {
            uint8_t *compressed = malloc(size ? size : 1);
            WPDFrame frame;
            int decode_result;
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
            decode_result = wpd_decoder_decode(decoder, compressed, size, &frame);
            free(compressed);
            if (decode_result != 0) {
                fprintf(stderr, "VP8 decode failed: %s\n",
                        decode_result < 0 ? wpd_decoder_error(decoder)
                                          : "no visible frame");
                goto done;
            }
            if (output) {
                if (!raw_yuv &&
                    fprintf(output, "YUV4MPEG2 W%d H%d F1:1 Ip A0:0 C420jpeg\nFRAME\n",
                            frame.width, frame.height) < 0) {
                    perror("write");
                    goto done;
                }
                if (write_frame(output, &frame) < 0) {
                    perror("write");
                    goto done;
                }
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
    uint8_t header[32];
    FILE *input = NULL, *output = NULL;
    int discard_output, raw_yuv, status = 1;
    size_t output_name_len;

    if (argc != 3) {
        fprintf(stderr, "usage: %s input.{ivf,webp} output.{y4m,yuv}\n", argv[0]);
        return 2;
    }
    discard_output = !strcmp(argv[2], "/dev/null");
    output_name_len = strlen(argv[2]);
    raw_yuv = output_name_len >= 4 &&
              !strcmp(argv[2] + output_name_len - 4, ".yuv");
    input = fopen(argv[1], "rb");
    if (!discard_output)
        output = fopen(argv[2], "wb");
    if (!input || (!discard_output && !output)) {
        perror(!input ? argv[1] : argv[2]);
        goto done;
    }
    if (fread(header, 1, sizeof(header), input) != sizeof(header)) {
        fprintf(stderr, "%s: unsupported or invalid input file\n", argv[1]);
        goto done;
    }
    if (!memcmp(header, "RIFF", 4))
        status = decode_webp(argv[1], input, output, raw_yuv, header);
    else
        status = decode_ivf(argv[1], input, output, raw_yuv, header);
done:
    if (input)
        fclose(input);
    if (output && fclose(output) && !status)
        status = 1;
    return status;
}
