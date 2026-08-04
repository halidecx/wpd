#include "wpd.h"

#include "wpd_codec.h"
#include "vp8.h"

#include <stdio.h>
#include <stdlib.h>

struct WPDDecoder {
    WpdCodecContext codec;
    VP8Context vp8;
    WpdFrame decoded;
    uint8_t *packet_buffer;
    size_t packet_capacity;
    char error[128];
};

#define WPD_PACKET_PADDING 64

WPDDecoder *wpd_decoder_create(void)
{
    WPDDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    decoder->codec.priv_data = &decoder->vp8;
    if (vp8_decode_init(&decoder->codec) < 0) {
        snprintf(decoder->error, sizeof(decoder->error), "decoder initialization failed");
    }
    return decoder;
}

int wpd_decoder_decode(WPDDecoder *decoder,
                         const uint8_t *data, size_t size,
                         WPDFrame *frame)
{
    WpdPacket packet;
    int result;
    if (!decoder || !data || !frame || size > INT_MAX)
        return -1;
    memset(frame, 0, sizeof(*frame));
    if (decoder->packet_capacity < size + WPD_PACKET_PADDING) {
        uint8_t *new_buffer = realloc(decoder->packet_buffer,
                                      size + WPD_PACKET_PADDING);
        if (!new_buffer) {
            snprintf(decoder->error, sizeof(decoder->error), "out of memory");
            return -1;
        }
        decoder->packet_buffer = new_buffer;
        decoder->packet_capacity = size + WPD_PACKET_PADDING;
    }
    memcpy(decoder->packet_buffer, data, size);
    memset(decoder->packet_buffer + size, 0, WPD_PACKET_PADDING);
    packet.data = decoder->packet_buffer;
    packet.size = (int)size;
    result = vp8_decode_frame(&decoder->codec, &decoder->decoded, &packet);
    if (result < 0) {
        snprintf(decoder->error, sizeof(decoder->error),
                 "invalid VP8 keyframe (%d)", result);
        return -1;
    }
    decoder->error[0] = 0;
    for (int plane = 0; plane < 3; plane++) {
        frame->data[plane] = decoder->decoded.data[plane];
        frame->stride[plane] = decoder->decoded.linesize[plane];
    }
    frame->width = decoder->codec.width;
    frame->height = decoder->codec.height;
    return 0;
}

const char *wpd_decoder_error(const WPDDecoder *decoder)
{
    return decoder && decoder->error[0] ? decoder->error : "unknown decoder error";
}

void wpd_decoder_free(WPDDecoder *decoder)
{
    if (decoder) {
        vp8_decode_free(&decoder->codec);
        free(decoder->packet_buffer);
        free(decoder);
    }
}
