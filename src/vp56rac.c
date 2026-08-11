
#include "vp56rac.h"
#include "wpd_codec.h"

#if WPD_RAC_64

void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size) {
    c->value   = 0;
    c->range   = 255 - 1;
    c->bits    = -8;
    c->buffer  = buf;
    c->end     = buf + buf_size;
    c->buf_max = buf_size >= 8 ? buf + buf_size - 8 + 1 : buf;
    c->eof     = 0;
}

void wpd_vp56_extend(VP56RangeCoder *c, const uint8_t *end) {
    c->end     = end;
    c->buf_max = end - c->buffer >= 8 ? end - 7 : c->buffer;
}

void wpd_vp56_save_offsets(const VP56RangeCoder *c, const uint8_t *base,
                           VP56RacOffsets *offsets) {
    offsets->buffer  = c->buffer - base;
    offsets->buf_max = c->buf_max - base;
    offsets->end     = c->end - base;
}

void wpd_vp56_restore_offsets(VP56RangeCoder *c, const uint8_t *base,
                              const VP56RacOffsets *offsets) {
    c->buffer  = base + offsets->buffer;
    c->buf_max = base + offsets->buf_max;
    c->end     = base + offsets->end;
}

void wpd_vp56_load_final_bytes(VP56RangeCoder *c) {
    if (c->buffer < c->end) {
        c->value = (c->value << 8) | *c->buffer++;
        c->bits += 8;
    } else if (!c->eof) {
        c->value <<= 8;
        c->bits += 8;
        c->eof = 1;
    } else {
        c->bits = 0;
    }
}

#else

const uint8_t wpd_vp56_norm_shift[256] = {
    8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
};

void wpd_vp56_init_range_decoder(VP56RangeCoder *c, const uint8_t *buf,
                                 int buf_size) {
    c->high   = 255;
    c->bits   = -16;
    c->buffer = buf;
    c->end    = buf + buf_size;
    if (buf_size >= 3) {
        c->code_word = wpd_bytestream_get_be24(&c->buffer);
        c->eof       = 0;
    } else {
        unsigned word = 0;

        for (int i = 0; i < 3; i++)
            word = word << 8 | (i < buf_size ? *c->buffer++ : 0u);
        c->code_word = word;
        c->eof       = 1;
    }
}

void wpd_vp56_extend(VP56RangeCoder *c, const uint8_t *end) { c->end = end; }

void wpd_vp56_save_offsets(const VP56RangeCoder *c, const uint8_t *base,
                           VP56RacOffsets *offsets) {
    offsets->buffer  = c->buffer - base;
    offsets->buf_max = 0;
    offsets->end     = c->end - base;
}

void wpd_vp56_restore_offsets(VP56RangeCoder *c, const uint8_t *base,
                              const VP56RacOffsets *offsets) {
    c->buffer = base + offsets->buffer;
    c->end    = base + offsets->end;
}

#endif
