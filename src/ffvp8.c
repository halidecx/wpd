#include "ffvp8.h"

#include "compat.h"
#include "vp8.h"
#include "vp8_internal.h"

#include <stdio.h>
#include <stdlib.h>

struct FFVP8Decoder {
    AVCodecContext codec;
    VP8Context vp8;
    AVFrame decoded;
    uint8_t *packet_buffer;
    size_t packet_capacity;
    char error[128];
};

#define FFVP8_PACKET_PADDING 64

FFVP8Decoder *ffvp8_decoder_create(void)
{
    FFVP8Decoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    decoder->codec.priv_data = &decoder->vp8;
    decoder->codec.flags = 0;
    decoder->codec.skip_frame = AVDISCARD_DEFAULT;
    decoder->codec.skip_loop_filter = AVDISCARD_DEFAULT;
    if (vp8_decode_init(&decoder->codec) < 0) {
        snprintf(decoder->error, sizeof(decoder->error), "decoder initialization failed");
    }
    return decoder;
}

int ffvp8_decoder_decode(FFVP8Decoder *decoder,
                         const uint8_t *data, size_t size,
                         FFVP8Frame *frame)
{
    AVPacket packet;
    int got_frame = 0;
    int result;
    if (!decoder || !data || !frame || size > INT_MAX)
        return -1;
    memset(frame, 0, sizeof(*frame));
    if (decoder->packet_capacity < size + FFVP8_PACKET_PADDING) {
        uint8_t *new_buffer = realloc(decoder->packet_buffer,
                                      size + FFVP8_PACKET_PADDING);
        if (!new_buffer) {
            snprintf(decoder->error, sizeof(decoder->error), "out of memory");
            return -1;
        }
        decoder->packet_buffer = new_buffer;
        decoder->packet_capacity = size + FFVP8_PACKET_PADDING;
    }
    memcpy(decoder->packet_buffer, data, size);
    memset(decoder->packet_buffer + size, 0, FFVP8_PACKET_PADDING);
    packet.data = decoder->packet_buffer;
    packet.size = (int)size;
    result = vp8_decode_frame(&decoder->codec, &decoder->decoded, &got_frame, &packet);
    if (result < 0) {
        snprintf(decoder->error, sizeof(decoder->error),
                 "invalid VP8 frame (%d)", result);
        return -1;
    }
    decoder->error[0] = 0;
    if (!got_frame)
        return 1;
    for (int plane = 0; plane < 3; plane++) {
        frame->data[plane] = decoder->decoded.data[plane];
        frame->stride[plane] = decoder->decoded.linesize[plane];
    }
    frame->width = decoder->codec.width;
    frame->height = decoder->codec.height;
    return 0;
}

void ffvp8_decoder_reset(FFVP8Decoder *decoder)
{
    if (!decoder)
        return;
    vp8_decode_free(&decoder->codec);
    memset(&decoder->vp8, 0, sizeof(decoder->vp8));
    memset(&decoder->decoded, 0, sizeof(decoder->decoded));
    decoder->codec.width = decoder->codec.height = 0;
    decoder->codec.coded_width = decoder->codec.coded_height = 0;
    decoder->codec.priv_data = &decoder->vp8;
    if (vp8_decode_init(&decoder->codec) < 0)
        snprintf(decoder->error, sizeof(decoder->error),
                 "decoder reinitialization failed");
    else
        decoder->error[0] = 0;
}

const char *ffvp8_decoder_error(const FFVP8Decoder *decoder)
{
    return decoder && decoder->error[0] ? decoder->error : "unknown decoder error";
}

void ffvp8_decoder_free(FFVP8Decoder *decoder)
{
    if (decoder) {
        vp8_decode_free(&decoder->codec);
        free(decoder->packet_buffer);
        free(decoder);
    }
}
