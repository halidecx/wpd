#include "wpd.h"

#include "testutil.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Feeds damaged and truncated files through every decoding path there is,
   looking for a crash rather than for particular output. Built to be run
   under a sanitizer; scripts/fuzz.sh does that. */

static const WPDPixelFormat formats[] = {
    WPD_PIX_FMT_NONE,
    WPD_PIX_FMT_RGBA,
    WPD_PIX_FMT_ARGB_PRE,
    WPD_PIX_FMT_RGB565,
    WPD_PIX_FMT_RGBA4444_PRE,
    WPD_PIX_FMT_YUVA420P,
    WPD_PIX_FMT_BGR,
    WPD_PIX_FMT_BGR565,
    WPD_PIX_FMT_BGRA4444_PRE,
};

static uint32_t seed = 0x9e3779b9u;

static uint32_t rnd(void) {
    seed ^= seed << 13;
    seed ^= seed >> 17;
    seed ^= seed << 5;
    return seed;
}

static void free_planes(uint8_t *planes[4]) {
    for (int p = 0; p < 4; p++) {
        free(planes[p]);
        planes[p] = NULL;
    }
}

/* Points the decoder at caller-owned output sized from headers that damaged
   input makes unreliable, which is the point: a buffer that cannot hold the
   frame has to be refused rather than written past. Plane 0 is sized for four
   bytes per pixel so one buffer serves the packed and the planar formats. */
static int attach_output(WPDDecoder *decoder, const WPDImageInfo *info,
                         uint8_t *planes[4]) {
    WPDOutputBuffer buffer = WPD_OUTPUT_BUFFER_INIT;

    if (info->width <= 0 || info->height <= 0 || info->width > 4096 ||
        info->height > 4096)
        return 0;
    for (int p = 0; p < 4; p++) {
        const int    shift = p == 1 || p == 2;
        const size_t row   = (size_t)((info->width + shift) >> shift) *
            (p ? 1 : 4);
        const size_t rows = (size_t)((info->height + shift) >> shift);

        planes[p] = calloc(rows, row);
        if (!planes[p]) {
            free_planes(planes);
            return 0;
        }
        buffer.plane[p].data   = planes[p];
        buffer.plane[p].size   = row * rows;
        buffer.plane[p].stride = (ptrdiff_t)row;
    }
    if (wpd_decoder_set_output_buffer(decoder, &buffer) != WPD_OK) {
        free_planes(planes);
        return 0;
    }
    return 1;
}

/* Whole file at once, then the same bytes streamed in pieces with the
   progressive row query interleaved, then the zero-copy variants. */
static void decode_every_way(const uint8_t *data, size_t size,
                             WPDPixelFormat format, size_t chunk) {
    WPDDecoder       *decoder;
    WPDFrame          frame     = WPD_FRAME_INIT;
    WPDImageInfo      info      = WPD_IMAGE_INFO_INIT;
    WPDDecoderOptions options   = WPD_DECODER_OPTIONS_INIT;
    uint8_t          *planes[4] = {NULL, NULL, NULL, NULL};

    wpd_get_info(data, size, &info);

    decoder = wpd_decoder_create();
    if (!decoder)
        return;
    wpd_decoder_set_output_format(decoder, format);
    attach_output(decoder, &info, planes);
    if (wpd_decoder_open(decoder, data, size) == WPD_OK)
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    wpd_decoder_free(decoder);
    free_planes(planes);

    decoder = wpd_decoder_create();
    if (!decoder)
        return;
    wpd_decoder_set_output_format(decoder, format);
    if (wpd_decoder_open(decoder, data, size) == WPD_OK)
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    wpd_decoder_free(decoder);

    /* Uncomposited sub-frames, the frame table and a replay, all of which read
       geometry straight out of a damaged ANMF header. */
    decoder = wpd_decoder_create();
    if (!decoder)
        return;
    wpd_decoder_set_output_format(decoder, format);
    wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME);
    if (wpd_decoder_open(decoder, data, size) == WPD_OK) {
        for (int i = 0;; i++) {
            WPDFrameInfo entry = WPD_FRAME_INFO_INIT;

            if (wpd_decoder_frame_info(decoder, i, &entry) != WPD_OK)
                break;
        }
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
        if (wpd_decoder_rewind(decoder) == WPD_OK)
            while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    }
    wpd_decoder_free(decoder);

    decoder = wpd_decoder_create();
    if (!decoder)
        return;
    wpd_decoder_set_output_format(decoder, format);
    if (wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK)
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    wpd_decoder_free(decoder);

    decoder = wpd_decoder_create();
    if (!decoder)
        return;
    wpd_decoder_set_output_format(decoder, format);
    if (rnd() & 1)
        attach_output(decoder, &info, planes);
    wpd_decoder_open_stream(decoder);
    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n       = size - offset < chunk ? size - offset : chunk;
        WPDFrame     partial = WPD_FRAME_INIT;
        int          rows    = 0;

        if (wpd_decoder_append(decoder, data + offset, n) < 0)
            break;
        wpd_decoder_partial_frame(decoder, &partial, &rows);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    }
    if (wpd_decoder_end_of_stream(decoder) == WPD_OK)
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    wpd_decoder_free(decoder);
    free_planes(planes);

    /* wpd_decoder_update() hands over a growing prefix instead of copying,
       so each step needs its own exactly sized buffer for a sanitizer to see
       a read past the end. */
    decoder = wpd_decoder_create();
    if (!decoder) {
        return;
    }
    wpd_decoder_set_output_format(decoder, format);
    wpd_decoder_open_stream(decoder);
    {
        uint8_t *held[64] = {NULL};
        int      n_held   = 0;

        for (size_t offset = 0; offset < size && n_held < 64; offset += chunk) {
            const size_t have = size - offset < chunk ? size : offset + chunk;
            uint8_t     *next = malloc(have);

            if (!next)
                break;
            memcpy(next, data, have);
            /* The decoder borrows this until it is freed or replaced, so it
               has to stay alive for the whole stream, not just this call. */
            held[n_held++] = next;
            if (wpd_decoder_update(decoder, next, have) < 0)
                break;
            while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
        }
        wpd_decoder_end_of_stream(decoder);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
        wpd_decoder_free(decoder);
        for (int i = 0; i < n_held; i++) free(held[i]);
    }

    options.use_cropping = 1;
    options.crop_left    = (int)(rnd() % 8);
    options.crop_top     = (int)(rnd() % 8);
    options.crop_width   = 1 + (int)(rnd() % 64);
    options.crop_height  = 1 + (int)(rnd() % 64);
    if (rnd() & 1) {
        options.use_scaling   = 1;
        options.scaled_width  = 1 + (int)(rnd() % 128);
        options.scaled_height = 1 + (int)(rnd() % 128);
    }
    options.flip                = (int)(rnd() & 1);
    options.no_fancy_upsampling = (int)(rnd() & 1);
    options.bypass_filtering    = (int)(rnd() & 1);
    if (wpd_decode(data, size, format, &options, &frame) == WPD_OK)
        wpd_frame_free(&frame);

    /* The point-sampled converter has a row-range path of its own, which only
       a progressive still reaches, and only without the geometry options. */
    options                     = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.no_fancy_upsampling = (int)(rnd() & 1);
    options.bypass_filtering    = (int)(rnd() & 1);
    decoder                     = wpd_decoder_create();
    if (decoder) {
        size_t offset = 0;

        wpd_decoder_set_options(decoder, &options);
        wpd_decoder_set_output_format(decoder, format);
        wpd_decoder_open_stream(decoder);
        for (; offset < size; offset += chunk) {
            const size_t n = size - offset < chunk ? size - offset : chunk;
            WPDFrame     partial = WPD_FRAME_INIT;
            int          rows    = 0;

            if (wpd_decoder_append(decoder, data + offset, n) < 0)
                break;
            wpd_decoder_partial_frame(decoder, &partial, &rows);
            while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
        }
        wpd_decoder_end_of_stream(decoder);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
        wpd_decoder_free(decoder);
    }
}

int main(int argc, char **argv) {
    int trials = 300;
    int first  = 1;

    if (argc > 2 && !strcmp(argv[1], "-n")) {
        trials = atoi(argv[2]);
        first  = 3;
    }
    if (argc <= first) {
        fprintf(stderr, "usage: %s [-n trials] file.webp...\n", argv[0]);
        return 2;
    }
    wpd_set_log_callback(NULL, NULL);

    for (int a = first; a < argc; a++) {
        size_t   size;
        uint8_t *original = read_file(argv[a], &size);

        if (!original || !size) {
            fprintf(stderr, "%s: cannot read\n", argv[a]);
            free(original);
            continue;
        }
        for (int trial = 0; trial < trials; trial++) {
            /* Truncate somewhere, then flip a few bits in what is left. An
               undamaged prefix is a valid case too, so the cut alone is
               sometimes the only change. */
            const size_t cut  = 1 + rnd() % size;
            uint8_t     *copy = malloc(cut);
            const int    bits = (int)(rnd() % 6);

            if (!copy)
                break;
            memcpy(copy, original, cut);
            for (int i = 0; i < bits; i++)
                copy[rnd() % cut] ^= (uint8_t)(1u << (rnd() % 8));
            /* Walk the formats rather than drawing them, so every one gets an
               even share of the trials and the choice does not run in lockstep
               with the other draws made from the same generator. */
            decode_every_way(
                copy,
                cut,
                formats[(size_t)trial % (sizeof(formats) / sizeof(*formats))],
                1 + rnd() % 4096);
            free(copy);
        }
        free(original);
        printf("%s: %d trial(s)\n", argv[a], trials);
        fflush(stdout);
    }
    return 0;
}
