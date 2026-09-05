#include "wpd.h"

#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;

#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            fprintf(stderr, "%s:%d: %s\n", __func__, __LINE__, #cond); \
            failures++;                                                \
        }                                                              \
    } while (0)

static void put32(uint8_t *p, uint32_t v) {
    p[0] = v & 0xff;
    p[1] = v >> 8 & 0xff;
    p[2] = v >> 16 & 0xff;
    p[3] = v >> 24 & 0xff;
}

static size_t make_vp8l(uint8_t *out, int width, int height, int alpha) {
    const uint32_t bits = (uint32_t)(width - 1) | (uint32_t)(height - 1) << 14 |
        (uint32_t)(alpha != 0) << 28;

    memcpy(out, "RIFF", 4);
    put32(out + 4, 20);
    memcpy(out + 8, "WEBP", 4);
    memcpy(out + 12, "VP8L", 4);
    put32(out + 16, 8);
    out[20] = 0x2f;
    put32(out + 21, bits);
    put32(out + 25, 0);
    return 29;
}

static void put24(uint8_t *p, uint32_t v) {
    p[0] = v & 0xff;
    p[1] = v >> 8 & 0xff;
    p[2] = v >> 16 & 0xff;
}

static size_t make_vp8x(uint8_t *out, int width, int height, int flags) {
    memcpy(out, "RIFF", 4);
    memcpy(out + 8, "WEBP", 4);
    memcpy(out + 12, "VP8X", 4);
    put32(out + 16, 10);
    memset(out + 20, 0, 10);
    out[20] = (uint8_t)flags;
    put24(out + 24, (uint32_t)(width - 1));
    put24(out + 27, (uint32_t)(height - 1));
    return 30;
}

static size_t make_truncated_chunk(uint8_t *out) {
    size_t size = make_vp8x(out, 8, 8, 0);

    memcpy(out + size, "VP8L", 4);
    put32(out + size + 4, 8);
    memset(out + size + 8, 0, 4);
    size += 12;
    put32(out + 4, (uint32_t)size - 8);
    return size;
}

static size_t put_chunk(uint8_t *out, const char *tag, const uint8_t *payload,
                        size_t size) {
    memcpy(out, tag, 4);
    put32(out + 4, (uint32_t)size);
    memcpy(out + 8, payload, size);
    if (size & 1)
        out[8 + size] = 0;
    return 8 + size + (size & 1);
}

static size_t make_vp8l_blank(uint8_t *out, int width, int height, int alpha) {
    out[0] = 0x2f;
    put32(out + 1,
          (uint32_t)(width - 1) | (uint32_t)(height - 1) << 14 |
              (uint32_t)(alpha != 0) << 28);
    out[5] = 0x88;
    out[6] = 0x88;
    out[7] = 0x08;
    return 8;
}

static size_t make_unsupported_alph(uint8_t *out) {
    uint8_t image[8];
    size_t  size = make_vp8x(out, 8, 8, 0x10);

    memcpy(out + size, "ALPH", 4);
    put32(out + size + 4, 2);
    out[size + 8] = 0x03;
    out[size + 9] = 0x00;
    size += 10;
    size += put_chunk(
        out + size, "VP8L", image, make_vp8l_blank(image, 8, 8, 0));
    put32(out + 4, (uint32_t)size - 8);
    return size;
}

static size_t make_odd_anmf(uint8_t *out) {
    uint8_t image[8], anmf[33];
    size_t  size = make_vp8x(out, 8, 8, 0x02);
    uint8_t anim[6];

    memset(anim, 0, sizeof(anim));
    size += put_chunk(out + size, "ANIM", anim, sizeof(anim));

    memset(anmf, 0, 16);
    put24(anmf + 6, 7);
    put24(anmf + 9, 7);
    put24(anmf + 12, 40);
    put_chunk(anmf + 16, "VP8L", image, make_vp8l_blank(image, 8, 8, 0));
    anmf[32] = 0xff;
    size += put_chunk(out + size, "ANMF", anmf, sizeof(anmf));
    put32(out + 4, (uint32_t)size - 8);
    return size;
}

enum {
    ANMF_SUB_ALPH,
    ANMF_SUB_VP8L_ALPHA,
    ANMF_SUB_VP8L_OPAQUE,
};

#define ANMF_SUBCHUNKS_SIZE(pad_chunks, frames) \
    (30 + 14 + (size_t)(frames) * (8 + 16 + 8 * (size_t)(pad_chunks) + 10 + 16))

static size_t put_anmf_subchunks(uint8_t *out, int pad_chunks, int kind) {
    uint8_t        image[8], alph[2];
    uint8_t *const anmf    = out + 8;
    size_t         payload = 16;

    memset(anmf, 0, 16);
    put24(anmf + 6, 7);
    put24(anmf + 9, 7);
    put24(anmf + 12, 40);
    for (int i = 0; i < pad_chunks; i++)
        payload += put_chunk(anmf + payload, "XTRA", image, 0);
    if (kind == ANMF_SUB_ALPH) {
        alph[0] = 0;
        alph[1] = 0;
        payload += put_chunk(anmf + payload, "ALPH", alph, sizeof(alph));
    }
    payload += put_chunk(
        anmf + payload,
        "VP8L",
        image,
        make_vp8l_blank(image, 8, 8, kind == ANMF_SUB_VP8L_ALPHA));
    memcpy(out, "ANMF", 4);
    put32(out + 4, (uint32_t)payload);
    return 8 + payload;
}

static size_t make_anmf_subchunks(uint8_t *out, int pad_chunks,
                                  const int *kinds, int nb_kinds) {
    uint8_t anim[6];
    size_t  size = make_vp8x(out, 8, 8, 0x02);

    memset(anim, 0, sizeof(anim));
    size += put_chunk(out + size, "ANIM", anim, sizeof(anim));
    for (int i = 0; i < nb_kinds; i++)
        size += put_anmf_subchunks(out + size, pad_chunks, kinds[i]);
    put32(out + 4, (uint32_t)size - 8);
    return size;
}

static size_t make_metadata_file(uint8_t *out, const uint8_t *iccp,
                                 size_t iccp_size, const uint8_t *exif,
                                 size_t exif_size, const uint8_t *xmp,
                                 size_t xmp_size) {
    uint8_t image[9];
    size_t  size = make_vp8x(out, 8, 8, 0x04 | 0x08 | 0x20);

    image[0] = 0x2f;
    put32(image + 1, 7u | 7u << 14);
    put32(image + 5, 0);
    size += put_chunk(out + size, "ICCP", iccp, iccp_size);
    size += put_chunk(out + size, "VP8L", image, sizeof(image));
    size += put_chunk(out + size, "EXIF", exif, exif_size);
    size += put_chunk(out + size, "XMP ", xmp, xmp_size);
    put32(out + 4, (uint32_t)size - 8);
    return size;
}

static void check_metadata(WPDDecoder *decoder, WPDMetadata which,
                           const uint8_t *want, size_t want_size) {
    const uint8_t *data = (const uint8_t *)1;
    size_t         size = 1;

    CHECK(wpd_decoder_metadata(decoder, which, &data, &size) == WPD_OK);
    CHECK(size == want_size);
    if (size == want_size && want_size)
        CHECK(data && !memcmp(data, want, want_size));
    if (!want_size)
        CHECK(data == NULL);
}

static void test_metadata(void) {
    static const uint8_t iccp[] = {0, 0, 1, 0x30, 'a', 'c', 's', 'p', 9};
    static const uint8_t exif[] = {'M', 'M', 0, 42, 0, 0, 0, 8};
    static const uint8_t xmp[]  = "<x:xmpmeta/>";
    uint8_t              file[256];
    size_t               size = make_metadata_file(
        file, iccp, sizeof(iccp), exif, sizeof(exif), xmp, sizeof(xmp) - 1);
    const uint8_t *data;
    size_t         n;
    WPDImageInfo   info    = WPD_IMAGE_INFO_INIT;
    WPDDecoder    *decoder = wpd_decoder_create();

    CHECK(decoder != NULL);
    if (!decoder)
        return;

    CHECK(wpd_get_info(file, size, &info) == WPD_OK);
    CHECK(info.metadata ==
          (WPD_METADATA_ICCP | WPD_METADATA_EXIF | WPD_METADATA_XMP));

    CHECK(wpd_decoder_metadata(decoder, WPD_METADATA_EXIF, &data, &n) ==
          WPD_ERR_INVALID_ARG);

    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(info.metadata ==
          (WPD_METADATA_ICCP | WPD_METADATA_EXIF | WPD_METADATA_XMP));
    check_metadata(decoder, WPD_METADATA_ICCP, iccp, sizeof(iccp));
    check_metadata(decoder, WPD_METADATA_EXIF, exif, sizeof(exif));
    check_metadata(decoder, WPD_METADATA_XMP, xmp, sizeof(xmp) - 1);

    CHECK(wpd_decoder_metadata(decoder, 0, &data, &n) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_metadata(
              decoder, WPD_METADATA_EXIF | WPD_METADATA_XMP, &data, &n) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_metadata(decoder, (WPDMetadata)8, &data, &n) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_metadata(decoder, (WPDMetadata)-1, &data, &n) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_metadata(NULL, WPD_METADATA_EXIF, &data, &n) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_metadata(decoder, WPD_METADATA_EXIF, NULL, &n) ==
          WPD_ERR_INVALID_ARG);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t offset = 0; offset < size; offset++) {
        CHECK(wpd_decoder_append(decoder, file + offset, 1) == WPD_OK);
        if (offset + 1 == 30) {
            CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
            CHECK(info.metadata ==
                  (WPD_METADATA_ICCP | WPD_METADATA_EXIF | WPD_METADATA_XMP));
            check_metadata(decoder, WPD_METADATA_XMP, NULL, 0);
        }
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    check_metadata(decoder, WPD_METADATA_ICCP, iccp, sizeof(iccp));
    check_metadata(decoder, WPD_METADATA_EXIF, exif, sizeof(exif));
    check_metadata(decoder, WPD_METADATA_XMP, xmp, sizeof(xmp) - 1);

    size = make_vp8l(file, 8, 8, 0);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(info.metadata == 0);
    check_metadata(decoder, WPD_METADATA_EXIF, NULL, 0);

    wpd_decoder_free(decoder);
}

static void test_version(void) {
    CHECK(wpd_version() == WPD_VERSION_NUM);
    CHECK(!strcmp(wpd_version_string(), WPD_VERSION_STR));
    CHECK(WPD_VERSION_NUM ==
          WPD_VERSION_INT(
              WPD_VERSION_MAJOR, WPD_VERSION_MINOR, WPD_VERSION_PATCH));
}

static void test_status_strings(void) {
    static const WPDStatus all[] = {
        WPD_OK,
        WPD_ERR_INVALID_ARG,
        WPD_ERR_NOT_WEBP,
        WPD_ERR_BITSTREAM,
        WPD_ERR_TRUNCATED,
        WPD_ERR_UNSUPPORTED,
        WPD_ERR_NO_MEMORY,
        WPD_ERR_TOO_LARGE,
        WPD_ERR_BUFFER_TOO_SMALL,
    };

    for (size_t i = 0; i < sizeof(all) / sizeof(*all); i++) {
        const char *text = wpd_status_string(all[i]);

        CHECK(text && text[0]);
        CHECK(strcmp(text, "unknown error") != 0);
    }
    CHECK(!strcmp(wpd_status_string((WPDStatus)-999), "unknown error"));
}

static void test_get_info(void) {
    uint8_t      file[64];
    size_t       size = make_vp8l(file, 100, 60, 1);
    WPDImageInfo info = WPD_IMAGE_INFO_INIT;

    CHECK(wpd_get_info(file, size, &info) == WPD_OK);
    CHECK(info.width == 100);
    CHECK(info.height == 60);
    CHECK(info.has_alpha == 1);
    CHECK(info.is_animation == 0);
    CHECK(info.frame_count == 1);
    CHECK(info.coding == WPD_CODING_LOSSLESS);

    size = make_vp8l(file, 1, 1, 0);
    CHECK(wpd_get_info(file, size, &info) == WPD_OK);
    CHECK(info.width == 1 && info.height == 1 && info.has_alpha == 0);

    file[24] = 0x20;
    CHECK(wpd_get_info(file, size, &info) == WPD_ERR_BITSTREAM);
    CHECK(wpd_get_info(file + 20, size - 20, &info) == WPD_ERR_BITSTREAM);

    memset(file, 0, sizeof(file));
    file[0] = 0x11;
    file[3] = 0x9d;
    file[4] = 0x01;
    file[5] = 0x2a;
    file[6] = 8;
    file[8] = 8;
    CHECK(wpd_get_info(file, 10, &info) == WPD_ERR_BITSTREAM);
    file[0] = 0x10;
    CHECK(wpd_get_info(file, 10, &info) == WPD_OK);
    file[0] = 0x18;
    CHECK(wpd_get_info(file, 10, &info) == WPD_ERR_BITSTREAM);
    file[0] = 0;
    CHECK(wpd_get_info(file, 10, &info) == WPD_ERR_BITSTREAM);
    file[0] = 0x30;
    CHECK(wpd_get_info(file, 10, &info) == WPD_OK);
    CHECK(info.width == 8 && info.height == 8);
    {
        WPDDecoder *decoder = wpd_decoder_create();

        CHECK(decoder != NULL);
        if (decoder)
            CHECK(wpd_decoder_open(decoder, file, 10) == WPD_ERR_BITSTREAM);
        wpd_decoder_free(decoder);
    }
    file[0] = 0x11;
    memcpy(file + 20, file, 10);
    memcpy(file, "RIFF", 4);
    put32(file + 4, 22);
    memcpy(file + 8, "WEBPVP8 ", 8);
    put32(file + 16, 10);
    CHECK(wpd_get_info(file, 30, &info) == WPD_ERR_BITSTREAM);

    CHECK(wpd_get_info(NULL, size, &info) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_get_info(file, size, NULL) == WPD_ERR_INVALID_ARG);

    CHECK(wpd_get_info(file, 8, &info) == WPD_ERR_TRUNCATED);
    CHECK(wpd_get_info(file, 20, &info) == WPD_ERR_TRUNCATED);

    memcpy(file, "RIFX", 4);
    CHECK(wpd_get_info(file, size, &info) == WPD_ERR_NOT_WEBP);
    memcpy(file, "RIFF", 4);
    memcpy(file + 8, "WEBQ", 4);
    CHECK(wpd_get_info(file, size, &info) == WPD_ERR_NOT_WEBP);
}

static int   log_count;
static char  log_last[256];
static void *log_last_opaque;

static void collect_log(void *opaque, WPDLogLevel level, const char *message) {
    (void)level;
    log_count++;
    log_last_opaque = opaque;
    snprintf(log_last, sizeof(log_last), "%s", message);
}

static void test_log_callback(void) {
    uint8_t     file[64];
    size_t      size    = make_unsupported_alph(file);
    int         opaque  = 0;
    WPDFrame    frame   = WPD_FRAME_INIT;
    WPDDecoder *decoder = wpd_decoder_create();

    CHECK(decoder != NULL);
    if (!decoder)
        return;

    wpd_set_log_callback(collect_log, &opaque);
    log_count = 0;
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) == 1);
    CHECK(log_count == 1);
    CHECK(log_last_opaque == &opaque);
    CHECK(strstr(log_last, "unsupported ALPHA") != NULL);
    CHECK(strchr(log_last, '\n') == NULL);

    wpd_set_log_callback(NULL, NULL);
    log_count = 0;
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) == 1);
    CHECK(log_count == 0);

    wpd_decoder_free(decoder);
}

static void test_decoder_errors(void) {
    uint8_t      file[64];
    size_t       size    = make_vp8l(file, 8, 8, 0);
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDDecoder  *decoder = wpd_decoder_create();

    CHECK(decoder != NULL);
    if (!decoder)
        return;
    wpd_set_log_callback(NULL, NULL);

    CHECK(wpd_decoder_open(decoder, NULL, size) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);

    memcpy(file, "RIFX", 4);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_ERR_NOT_WEBP);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_NOT_WEBP);
    CHECK(strstr(wpd_decoder_error(decoder), "not a WebP file") != NULL);

    memcpy(file, "RIFF", 4);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_status(decoder) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(info.width == 8 && info.coding == WPD_CODING_LOSSLESS);

    /* Match libwebp: incomplete image data can still expose canvas info. */
    size = make_truncated_chunk(file);
    CHECK(wpd_get_info(file, size, &info) == WPD_OK);
    CHECK(info.width == 8 && info.height == 8);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_ERR_TRUNCATED);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_TRUNCATED);
    CHECK(strstr(wpd_decoder_error(decoder), "truncated") != NULL);
    CHECK(wpd_decoder_next_frame(decoder, &(WPDFrame)WPD_FRAME_INIT) ==
          WPD_ERR_INVALID_ARG);

    size = make_vp8x(file, 8, 8, 0);
    put32(file + 4, (uint32_t)size - 8);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_ERR_BITSTREAM);
    CHECK(strstr(wpd_decoder_error(decoder), "no image data") != NULL);
    CHECK(wpd_decode(
              file, size, WPD_PIX_FMT_RGBA, NULL, &(WPDFrame)WPD_FRAME_INIT) ==
          WPD_ERR_BITSTREAM);

    size = make_vp8l(file, 8, 8, 0);
    put32(file + 4, (uint32_t)size);
    CHECK(wpd_decoder_open(decoder, file, size) == WPD_ERR_TRUNCATED);

    size = make_vp8l(file, 8, 8, 0);
    CHECK(wpd_decoder_open(decoder, file, size - 4) == WPD_ERR_TRUNCATED);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_next_frame(decoder, &(WPDFrame)WPD_FRAME_INIT) ==
          WPD_ERR_INVALID_ARG);

    CHECK(wpd_decoder_set_output_format(NULL, WPD_PIX_FMT_RGBA) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_YUV420P) ==
          WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_NONE) == WPD_OK);

    CHECK(wpd_decoder_set_output_buffer(NULL, NULL) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_output_buffer(decoder, NULL) == WPD_OK);
    CHECK(wpd_decoder_set_output_buffer(
              decoder,
              &(WPDOutputBuffer){
                  .struct_size = sizeof(WPDOutputBuffer),
                  .plane[0]    = {.data = NULL, .size = 16, .stride = 4}}) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(strstr(wpd_decoder_error(decoder), "invalid output buffer") != NULL);
    CHECK(wpd_decoder_set_output_buffer(
              decoder,
              &(WPDOutputBuffer){
                  .struct_size = sizeof(WPDOutputBuffer),
                  .plane[0]    = {.data = file, .size = 16, .stride = 0}}) ==
          WPD_ERR_INVALID_ARG);

    wpd_decoder_free(decoder);
}

static uint8_t *read_file(const char *path, size_t *size) {
    uint8_t *data;
    long     length;
    FILE    *f = fopen(path, "rb");

    if (!f) {
        fprintf(stderr, "cannot open %s\n", path);
        failures++;
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    length = ftell(f);
    fseek(f, 0, SEEK_SET);
    data = malloc((size_t)length);
    if (!data || fread(data, 1, (size_t)length, f) != (size_t)length) {
        fclose(f);
        free(data);
        failures++;
        return NULL;
    }
    fclose(f);
    *size = (size_t)length;
    return data;
}

static int packed_bpp(WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGB:
    case WPD_PIX_FMT_BGR: return 3;
    case WPD_PIX_FMT_RGB565:
    case WPD_PIX_FMT_RGBA4444:
    case WPD_PIX_FMT_RGBA4444_PRE:
    case WPD_PIX_FMT_BGR565:
    case WPD_PIX_FMT_BGRA4444:
    case WPD_PIX_FMT_BGRA4444_PRE: return 2;
    default: return 4;
    }
}

static uint8_t *decode_internal(const uint8_t *data, size_t size,
                                WPDPixelFormat format, int *width, int *height,
                                int *frames, size_t *row_bytes) {
    uint8_t    *out  = NULL;
    size_t      used = 0, row = 0;
    int         count   = 0, ret;
    WPDFrame    frame   = WPD_FRAME_INIT;
    WPDDecoder *decoder = wpd_decoder_create();

    if (!decoder)
        return NULL;
    if (wpd_decoder_set_output_format(decoder, format) != WPD_OK ||
        wpd_decoder_open(decoder, data, size) != WPD_OK) {
        wpd_decoder_free(decoder);
        failures++;
        return NULL;
    }
    while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
        uint8_t *grown;

        row   = (size_t)frame.width * packed_bpp(frame.format);
        grown = realloc(out, used + row * (size_t)frame.height);
        if (!grown)
            break;
        out = grown;
        for (int y = 0; y < frame.height; y++)
            memcpy(out + used + row * (size_t)y,
                   frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                   row);
        used += row * (size_t)frame.height;
        *width  = frame.width;
        *height = frame.height;
        count++;
    }
    if (ret < 0) {
        fprintf(stderr, "decode failed: %s\n", wpd_decoder_error(decoder));
        failures++;
    }
    wpd_decoder_free(decoder);
    *frames    = count;
    *row_bytes = row;
    return out;
}

static void test_file_metadata(const char *path, int expect,
                               size_t expect_size) {
    size_t       size;
    uint8_t     *data    = read_file(path, &size);
    uint8_t     *want    = NULL;
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDDecoder  *decoder = wpd_decoder_create();

    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        CHECK(data && decoder);
        return;
    }

    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    CHECK(info.metadata == expect);

    CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
    for (int bit = 1; bit <= WPD_METADATA_XMP; bit <<= 1) {
        const uint8_t *found;
        size_t         n;

        CHECK(wpd_decoder_metadata(decoder, (WPDMetadata)bit, &found, &n) ==
              WPD_OK);
        if (!(expect & bit)) {
            CHECK(!found && !n);
            continue;
        }
        CHECK(n == expect_size);
        want = malloc(n ? n : 1);
        if (want && n)
            memcpy(want, found, n);
    }

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t offset = 0; offset < size; offset += 337) {
        const size_t n = size - offset < 337 ? size - offset : 337;

        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    for (int bit = 1; bit <= WPD_METADATA_XMP; bit <<= 1) {
        const uint8_t *found;
        size_t         n;

        CHECK(wpd_decoder_metadata(decoder, (WPDMetadata)bit, &found, &n) ==
              WPD_OK);
        if (!(expect & bit)) {
            CHECK(!found && !n);
            continue;
        }
        CHECK(n == expect_size);
        if (want && n == expect_size)
            CHECK(!memcmp(found, want, n));
    }

    free(want);
    wpd_decoder_free(decoder);
    free(data);
}

static void test_output_buffer(const char *path, WPDPixelFormat format) {
    size_t   size, row;
    int      width = 0, height = 0, frames = 0;
    uint8_t *data = read_file(path, &size);
    uint8_t *reference;

    if (!data)
        return;
    reference = decode_internal(
        data, size, format, &width, &height, &frames, &row);
    if (!reference || !frames) {
        free(data);
        free(reference);
        failures++;
        return;
    }

    for (int variant = 0; variant < 3; variant++) {
        const size_t    pad     = variant == 1 ? 37 : 0;
        const size_t    advance = row + pad;
        const int       flip    = variant == 2;
        uint8_t        *buffer  = malloc(advance * (size_t)height);
        WPDOutputBuffer out     = WPD_OUTPUT_BUFFER_INIT;
        WPDDecoder     *decoder = wpd_decoder_create();
        WPDFrame        frame   = WPD_FRAME_INIT;
        int             seen    = 0, ret;

        if (!buffer || !decoder) {
            failures++;
            free(buffer);
            wpd_decoder_free(decoder);
            break;
        }
        out.plane[0].size   = advance * (size_t)height;
        out.plane[0].stride = flip ? -(ptrdiff_t)advance : (ptrdiff_t)advance;
        out.plane[0].data   = flip ? buffer + advance * (size_t)(height - 1)
                                   : buffer;

        CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
        CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
        CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);

        while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
            const uint8_t *want = reference +
                row * (size_t)height * (size_t)seen;

            CHECK(frame.data[0] == out.plane[0].data);
            CHECK(frame.stride[0] == out.plane[0].stride);
            CHECK(frame.width == width && frame.height == height);
            for (int y = 0; y < height; y++) {
                const uint8_t *got = buffer +
                    advance * (size_t)(flip ? height - 1 - y : y);

                if (memcmp(got, want + row * (size_t)y, row)) {
                    fprintf(stderr,
                            "%s: variant %d frame %d row %d differs\n",
                            path,
                            variant,
                            seen,
                            y);
                    failures++;
                    break;
                }
            }
            seen++;
        }
        CHECK(ret == 0);
        CHECK(seen == frames);
        free(buffer);
        wpd_decoder_free(decoder);
    }

    {
        uint8_t        *buffer  = malloc(row * (size_t)height);
        WPDDecoder     *decoder = wpd_decoder_create();
        WPDOutputBuffer out     = WPD_OUTPUT_BUFFER_INIT;
        WPDFrame        frame   = WPD_FRAME_INIT;

        out.plane[0].data   = buffer;
        out.plane[0].size   = row * (size_t)height - 1;
        out.plane[0].stride = (ptrdiff_t)row;

        CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
        CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
        CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
        CHECK(wpd_decoder_next_frame(decoder, &frame) ==
              WPD_ERR_BUFFER_TOO_SMALL);
        CHECK(wpd_decoder_status(decoder) == WPD_ERR_BUFFER_TOO_SMALL);

        out.plane[0].size   = row * (size_t)height;
        out.plane[0].stride = (ptrdiff_t)row - 1;
        CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
        CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
        CHECK(wpd_decoder_next_frame(decoder, &frame) ==
              WPD_ERR_BUFFER_TOO_SMALL);

        free(buffer);
        wpd_decoder_free(decoder);
    }

    free(reference);
    free(data);
}

static void test_output_buffer_incomplete_yuv(const char *path) {
    size_t   size;
    uint8_t *data = read_file(path, &size);
    uint8_t  buffer[4096];

    if (!data)
        return;
    {
        WPDDecoder     *decoder = wpd_decoder_create();
        WPDOutputBuffer out     = WPD_OUTPUT_BUFFER_INIT;
        WPDFrame        frame   = WPD_FRAME_INIT;

        out.plane[0].data   = buffer;
        out.plane[0].size   = sizeof(buffer);
        out.plane[0].stride = 4;

        CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
        CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
        CHECK(wpd_decoder_next_frame(decoder, &frame) ==
              WPD_ERR_BUFFER_TOO_SMALL);
        wpd_decoder_free(decoder);
    }
    free(data);
}

static void test_partial_frame(const char *path, WPDPixelFormat format,
                               size_t chunk, int use_ext) {
    size_t   size, row;
    int      width = 0, height = 0, frames = 0;
    uint8_t *data = read_file(path, &size);
    uint8_t *reference;
    uint8_t *ext       = NULL;
    int      last_rows = 0, rows_at_half = 0, seen = 0;
    size_t   cmp;

    WPDDecoder *decoder;
    WPDFrame    frame = WPD_FRAME_INIT;

    if (!data)
        return;
    reference = decode_internal(
        data, size, format, &width, &height, &frames, &row);
    if (!reference || frames != 1) {
        free(data);
        free(reference);
        failures++;
        return;
    }

    cmp = row;

    decoder = wpd_decoder_create();
    CHECK(decoder != NULL);
    if (!decoder) {
        free(data);
        free(reference);
        return;
    }
    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    if (use_ext) {
        const size_t need = row * (size_t)height;

        ext = malloc(need);
        CHECK(ext != NULL);
        if (ext) {
            const WPDOutputBuffer buffer = {
                .struct_size = sizeof(WPDOutputBuffer),
                .plane[0]    = {
                    .data = ext, .size = need, .stride = (ptrdiff_t)row}};

            CHECK(wpd_decoder_set_output_buffer(decoder, &buffer) == WPD_OK);
        }
    }
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n    = size - offset < chunk ? size - offset : chunk;
        int          rows = -1;

        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;

        CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
        CHECK(rows >= last_rows && rows <= height);
        last_rows = rows;
        if (rows > 0) {
            CHECK(frame.width == width && frame.height == height);
            if (frame.format == WPD_PIX_FMT_YUV420P ||
                frame.format == WPD_PIX_FMT_YUVA420P)
                cmp = (size_t)width;
            for (int y = 0; y < rows; y++)
                if (memcmp(frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                           reference + row * (size_t)y,
                           cmp)) {
                    fprintf(stderr,
                            "%s: partial row %d of %d differs\n",
                            path,
                            y,
                            rows);
                    failures++;
                    break;
                }
        }
        if (offset + n >= size / 2 && !rows_at_half)
            rows_at_half = rows;
    }

    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(seen == 1);

    CHECK(rows_at_half > 0);
    CHECK(rows_at_half < height);

    CHECK(wpd_decoder_partial_frame(decoder, &frame, &last_rows) == WPD_OK);
    CHECK(last_rows == height);
    for (int y = 0; y < height; y++)
        if (memcmp(frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                   reference + row * (size_t)y,
                   cmp)) {
            fprintf(stderr, "%s: final row %d differs\n", path, y);
            failures++;
            break;
        }

    CHECK(wpd_decoder_partial_frame(decoder, &frame, NULL) == WPD_OK);
    CHECK(wpd_decoder_partial_frame(NULL, &frame, NULL) == WPD_ERR_INVALID_ARG);

    wpd_decoder_free(decoder);
    free(ext);
    free(reference);
    free(data);
}

/* Match libwebp: a partial still can expose dimensions from its frame header. */
static void test_info_truncated_still(const char *path) {
    size_t   size;
    uint8_t *data  = read_file(path, &size);
    int      width = 0, height = 0;

    if (!data)
        return;
    {
        WPDImageInfo info = WPD_IMAGE_INFO_INIT;

        CHECK(wpd_get_info(data, size, &info) == WPD_OK);
        width  = info.width;
        height = info.height;
    }
    for (size_t cut = size / 8; cut < size; cut += size / 8) {
        WPDImageInfo info = WPD_IMAGE_INFO_INIT;

        if (wpd_get_info(data, cut, &info) != WPD_OK) {
            fprintf(
                stderr, "%s: get_info refused a %zu byte prefix\n", path, cut);
            failures++;
            continue;
        }
        CHECK(info.width == width && info.height == height);
        CHECK(info.frame_count == 1);
    }
    free(data);
}

static uint32_t rl32(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 |
        (uint32_t)p[3] << 24;
}

static uint8_t *extract_lossless_alpha_still(const uint8_t *data, size_t size,
                                             size_t *out_size) {
    size_t pos = 12;

    while (pos <= size && size - pos >= 8) {
        const size_t payload = rl32(data + pos + 4);
        size_t       total;

        if (payload > SIZE_MAX - 9)
            return NULL;
        total = 8 + payload + (payload & 1);
        if (total > size - pos)
            return NULL;
        if (!memcmp(data + pos, "ANMF", 4)) {
            pos += 8 + 16;
            continue;
        }
        if (!memcmp(data + pos, "VP8L", 4) && payload >= 5 &&
            data[pos + 8] == 0x2f && (rl32(data + pos + 9) >> 28 & 1)) {
            uint8_t *out = malloc(12 + total);

            if (!out)
                return NULL;
            memcpy(out, "RIFF", 4);
            put32(out + 4, (uint32_t)(4 + total));
            memcpy(out + 8, "WEBP", 4);
            memcpy(out + 12, data + pos, total);
            *out_size = 12 + total;
            return out;
        }
        pos += total;
    }
    return NULL;
}

static void check_partial_format_change(const uint8_t *data, size_t size,
                                        size_t chunk, WPDPixelFormat from,
                                        WPDPixelFormat to, int peek_again) {
    size_t      row;
    int         width = 0, height = 0, frames = 0;
    uint8_t    *reference;
    int         switched = 0, seen = 0;
    WPDDecoder *decoder;
    WPDFrame    frame = WPD_FRAME_INIT;

    reference = decode_internal(data, size, to, &width, &height, &frames, &row);
    if (!reference || frames != 1) {
        free(reference);
        failures++;
        return;
    }

    decoder = wpd_decoder_create();
    CHECK(decoder != NULL);
    if (!decoder) {
        free(reference);
        return;
    }
    CHECK(wpd_decoder_set_output_format(decoder, from) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n    = size - offset < chunk ? size - offset : chunk;
        int          rows = 0;
        int          ret;

        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);
        while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) seen++;
        CHECK(ret == 0);
        CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
        if (!switched && rows > 0 && rows < height) {
            CHECK(wpd_decoder_set_output_format(decoder, to) == WPD_OK);
            switched = 1;
            if (peek_again) {
                int again = 0;

                CHECK(wpd_decoder_partial_frame(decoder, &frame, &again) ==
                      WPD_OK);
            }
        }
    }

    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(switched);
    CHECK(seen == 1);
    CHECK(frame.format == to);
    for (int y = 0; y < height; y++)
        CHECK(memcmp(frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                     reference + row * (size_t)y,
                     row) == 0);

    wpd_decoder_free(decoder);
    free(reference);
}

static void check_partial_buffer_change(const uint8_t *data, size_t size,
                                        size_t chunk, WPDPixelFormat format) {
    size_t          row;
    int             width = 0, height = 0, frames = 0;
    uint8_t        *reference;
    uint8_t        *first = NULL, *second = NULL;
    int             swapped = 0, seen = 0, rows = 0;
    WPDDecoder     *decoder;
    WPDFrame        frame = WPD_FRAME_INIT;
    WPDOutputBuffer out   = WPD_OUTPUT_BUFFER_INIT;

    reference = decode_internal(
        data, size, format, &width, &height, &frames, &row);
    if (!reference || frames != 1) {
        free(reference);
        failures++;
        return;
    }

    first   = malloc(row * (size_t)height);
    second  = malloc(row * (size_t)height);
    decoder = wpd_decoder_create();
    CHECK(first && second && decoder);
    if (!first || !second || !decoder) {
        free(first);
        free(second);
        free(reference);
        wpd_decoder_free(decoder);
        return;
    }
    memset(second, 0xa5, row * (size_t)height);

    out.plane[0].data   = first;
    out.plane[0].size   = row * (size_t)height;
    out.plane[0].stride = (ptrdiff_t)row;
    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n = size - offset < chunk ? size - offset : chunk;

        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
        CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
        if (swapped || rows <= 0 || rows >= height)
            continue;

        out.plane[0].data = second;
        CHECK(wpd_decoder_set_output_buffer(decoder, &out) == WPD_OK);
        swapped = 1;
        CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
        CHECK(frame.data[0] == second);
        for (int y = 0; y < rows; y++)
            if (memcmp(second + row * (size_t)y,
                       reference + row * (size_t)y,
                       row)) {
                fprintf(
                    stderr, "swapped buffer row %d of %d differs\n", y, rows);
                failures++;
                break;
            }
    }

    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(swapped);
    CHECK(seen == 1);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
    CHECK(rows == height);
    CHECK(frame.data[0] == second);
    for (int y = 0; y < height; y++)
        if (memcmp(
                second + row * (size_t)y, reference + row * (size_t)y, row)) {
            fprintf(stderr, "swapped buffer final row %d differs\n", y);
            failures++;
            break;
        }

    wpd_decoder_free(decoder);
    free(first);
    free(second);
    free(reference);
}

static void test_partial_buffer_change(const char *path, size_t chunk,
                                       WPDPixelFormat format, int lift_still) {
    size_t   size, still_size;
    uint8_t *data = read_file(path, &size);
    uint8_t *still;

    if (!data)
        return;
    if (!lift_still) {
        check_partial_buffer_change(data, size, chunk, format);
        free(data);
        return;
    }
    still = extract_lossless_alpha_still(data, size, &still_size);
    free(data);
    CHECK(still != NULL);
    if (!still)
        return;
    check_partial_buffer_change(still, still_size, chunk, format);
    free(still);
}

/* Scaling must preserve decoded pixels for repeated exports and animation. */
static void check_scale_keeps_source(const uint8_t *data, size_t size,
                                     WPDPixelFormat other, int width,
                                     int height) {
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDDecoder       *decoder = wpd_decoder_create();
    WPDFrame          frame   = WPD_FRAME_INIT;
    uint8_t          *first   = NULL;
    size_t            row     = 0;
    int               rows = 0, seen = 0;

    CHECK(decoder != NULL);
    if (!decoder)
        return;
    options.use_scaling   = 1;
    options.scaled_width  = width;
    options.scaled_height = height;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_append(decoder, data, size) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(seen == 1);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
    CHECK(rows == height && frame.width == width && frame.height == height);
    if (rows == height) {
        row   = (size_t)width * 4;
        first = malloc(row * (size_t)height);
        CHECK(first != NULL);
    }
    if (first)
        for (int y = 0; y < height; y++)
            memcpy(first + row * (size_t)y,
                   frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                   row);

    CHECK(wpd_decoder_set_output_format(decoder, other) == WPD_OK);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA) == WPD_OK);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
    if (first)
        for (int y = 0; y < height; y++)
            CHECK(memcmp(first + row * (size_t)y,
                         frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                         row) == 0);

    free(first);
    wpd_decoder_free(decoder);
}

static void test_scale_keeps_source(const char *path, WPDPixelFormat other,
                                    int width, int height, int lift_still) {
    size_t   size, still_size;
    uint8_t *data = read_file(path, &size);
    uint8_t *still;

    if (!data)
        return;
    if (!lift_still) {
        check_scale_keeps_source(data, size, other, width, height);
        free(data);
        return;
    }
    still = extract_lossless_alpha_still(data, size, &still_size);
    free(data);
    CHECK(still != NULL);
    if (!still)
        return;
    check_scale_keeps_source(still, still_size, other, width, height);
    free(still);
}

static uint8_t *decode_flipped(const uint8_t *data, size_t size,
                               WPDPixelFormat format, int flip, size_t *bytes,
                               size_t *row_bytes, int *height) {
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDDecoder       *decoder = wpd_decoder_create();
    WPDFrame          frame   = WPD_FRAME_INIT;
    uint8_t          *out     = NULL;
    size_t            used    = 0;

    *bytes     = 0;
    *row_bytes = 0;
    *height    = 0;
    CHECK(decoder != NULL);
    if (!decoder)
        return NULL;
    options.flip = flip;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) {
        const size_t row   = (size_t)frame.width * packed_bpp(frame.format);
        uint8_t     *grown = realloc(out, used + row * (size_t)frame.height);

        if (!grown)
            break;
        out = grown;
        for (int y = 0; y < frame.height; y++)
            memcpy(out + used + row * (size_t)y,
                   frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                   row);
        used += row * (size_t)frame.height;
        *row_bytes = row;
        *height    = frame.height;
    }
    wpd_decoder_free(decoder);
    *bytes = used;
    return out;
}

static void test_flip_reverses_rows(const char *path, WPDPixelFormat format) {
    size_t   size;
    uint8_t *data = read_file(path, &size);
    uint8_t *plain, *flipped;
    size_t   plain_bytes, flipped_bytes, row, flipped_row;
    int      height, flipped_height;

    if (!data)
        return;
    plain = decode_flipped(data, size, format, 0, &plain_bytes, &row, &height);
    flipped = decode_flipped(
        data, size, format, 1, &flipped_bytes, &flipped_row, &flipped_height);
    free(data);
    CHECK(plain != NULL && flipped != NULL);
    CHECK(plain_bytes == flipped_bytes && plain_bytes > 0);
    CHECK(row == flipped_row && height == flipped_height && height > 0);
    if (plain && flipped && plain_bytes == flipped_bytes && row && height > 0) {
        const size_t picture = row * (size_t)height;

        for (size_t at = 0; at + picture <= plain_bytes; at += picture)
            for (int y = 0; y < height; y++)
                CHECK(memcmp(plain + at + row * (size_t)y,
                             flipped + at + row * (size_t)(height - 1 - y),
                             row) == 0);
    }
    free(plain);
    free(flipped);
}

static void test_partial_format_change(const char *path, size_t chunk,
                                       WPDPixelFormat from, WPDPixelFormat to,
                                       int peek_again) {
    size_t   size;
    uint8_t *data = read_file(path, &size);

    if (!data)
        return;
    check_partial_format_change(data, size, chunk, from, to, peek_again);
    free(data);
}

static void test_partial_format_change_alpha(const char *path, size_t chunk) {
    size_t   size, still_size;
    uint8_t *data = read_file(path, &size);
    uint8_t *still;

    if (!data)
        return;
    still = extract_lossless_alpha_still(data, size, &still_size);
    free(data);
    CHECK(still != NULL);
    if (!still)
        return;
    check_partial_format_change(still,
                                still_size,
                                chunk,
                                WPD_PIX_FMT_RGBA_PRE,
                                WPD_PIX_FMT_BGRA_PRE,
                                1);
    check_partial_format_change(
        still, still_size, chunk, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_RGBA, 1);
    check_partial_format_change(
        still, still_size, chunk, WPD_PIX_FMT_BGRA, WPD_PIX_FMT_ARGB_PRE, 1);
    free(still);
}

static void test_stream(const char *path, WPDPixelFormat format, size_t chunk,
                        int expect_progressive) {
    size_t   size, row;
    int      width = 0, height = 0, frames = 0;
    uint8_t *data = read_file(path, &size);
    uint8_t *reference;
    int      seen = 0, seen_before_last_append = 0, seen_at_half = -1, ret = 0;

    WPDDecoder *decoder;
    WPDFrame    frame = WPD_FRAME_INIT;

    if (!data)
        return;
    reference = decode_internal(
        data, size, format, &width, &height, &frames, &row);
    if (!reference || !frames) {
        free(data);
        free(reference);
        failures++;
        return;
    }

    decoder = wpd_decoder_create();
    CHECK(decoder != NULL);
    if (!decoder) {
        free(data);
        free(reference);
        return;
    }
    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);

    CHECK(wpd_decoder_next_frame(decoder, &frame) == 0);
    CHECK(wpd_decoder_get_info(decoder, &(WPDImageInfo)WPD_IMAGE_INFO_INIT) ==
          WPD_ERR_TRUNCATED);

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n = size - offset < chunk ? size - offset : chunk;

        if (offset + n >= size)
            seen_before_last_append = seen;
        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);

        while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
            const uint8_t *want = reference +
                row * (size_t)height * (size_t)seen;

            CHECK(frame.width == width && frame.height == height);
            if (seen < frames) {
                for (int y = 0; y < height; y++)
                    if (memcmp(frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                               want + row * (size_t)y,
                               row)) {
                        fprintf(stderr,
                                "%s: streamed frame %d row %d differs\n",
                                path,
                                seen,
                                y);
                        failures++;
                        break;
                    }
            }
            seen++;
        }
        CHECK(ret == 0);
        if (seen_at_half < 0 && offset + n >= size / 2)
            seen_at_half = seen;
    }

    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) seen++;
    CHECK(ret == 0);
    CHECK(seen == frames);

    /* Streaming animation must expose frames before end of input. */
    if (expect_progressive) {
        CHECK(seen_before_last_append > 0);
        CHECK(seen_at_half >= frames / 3);
        CHECK(seen_at_half < frames);
    }

    CHECK(wpd_decoder_get_info(decoder, &(WPDImageInfo)WPD_IMAGE_INFO_INIT) ==
          WPD_OK);
    CHECK(wpd_decoder_append(decoder, data, 1) == WPD_ERR_INVALID_ARG);

    wpd_decoder_free(decoder);
    free(reference);
    free(data);
}

static void test_stream_errors(const char *path) {
    size_t      size;
    uint8_t    *data    = read_file(path, &size);
    WPDDecoder *decoder = wpd_decoder_create();
    WPDFrame    frame   = WPD_FRAME_INIT;

    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        failures++;
        return;
    }

    CHECK(wpd_decoder_append(decoder, data, size) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_open(decoder, data, size) == WPD_OK);
    CHECK(wpd_decoder_append(decoder, data, size) == WPD_ERR_INVALID_ARG);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_append(decoder, data, size / 2) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) >= 0);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_ERR_TRUNCATED);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_append(decoder, (const uint8_t *)"RIF", 3) == WPD_OK);
    CHECK(wpd_decoder_append(decoder, (const uint8_t *)"XnotWEBPxxxx", 12) ==
          WPD_ERR_NOT_WEBP);

    wpd_decoder_free(decoder);
    free(data);
}

static void test_file_info(const char *path, int width, int height,
                           int has_alpha, int is_animation, int frame_count,
                           WPDCoding coding) {
    size_t       size;
    uint8_t     *data = read_file(path, &size);
    WPDImageInfo info = WPD_IMAGE_INFO_INIT;

    if (!data)
        return;
    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    CHECK(info.width == width);
    CHECK(info.height == height);
    CHECK(info.has_alpha == has_alpha);
    CHECK(info.is_animation == is_animation);
    CHECK(info.frame_count == frame_count);
    CHECK(info.coding == coding);
    free(data);
}

static const uint8_t *find_chunk(const uint8_t *data, size_t size,
                                 const char tag[4], size_t *payload_size,
                                 size_t *chunk_size) {
    size_t pos = 12;

    while (pos <= size && size - pos >= 8) {
        size_t payload = rl32(data + pos + 4);
        size_t total;

        if (payload > SIZE_MAX - 9)
            return NULL;
        total = 8 + payload + (payload & 1);
        if (total > size - pos)
            return NULL;
        if (!memcmp(data + pos, tag, 4)) {
            if (payload_size)
                *payload_size = payload;
            if (chunk_size)
                *chunk_size = total;
            return data + pos;
        }
        pos += total;
    }
    return NULL;
}

static int frame_equal(const WPDFrame *a, const WPDFrame *b) {
    if (a->width != b->width || a->height != b->height ||
        a->format != b->format)
        return 0;
    if (a->format == WPD_PIX_FMT_YUV420P || a->format == WPD_PIX_FMT_YUVA420P) {
        int planes = a->format == WPD_PIX_FMT_YUVA420P ? 4 : 3;

        for (int p = 0; p < planes; p++) {
            int shift  = p == 1 || p == 2;
            int width  = (a->width + shift) >> shift;
            int height = (a->height + shift) >> shift;

            for (int y = 0; y < height; y++)
                if (memcmp(a->data[p] + (ptrdiff_t)y * a->stride[p],
                           b->data[p] + (ptrdiff_t)y * b->stride[p],
                           (size_t)width))
                    return 0;
        }
        return 1;
    }
    {
        size_t row = (size_t)a->width * packed_bpp(a->format);

        for (int y = 0; y < a->height; y++)
            if (memcmp(a->data[0] + (ptrdiff_t)y * a->stride[0],
                       b->data[0] + (ptrdiff_t)y * b->stride[0],
                       row))
                return 0;
    }
    return 1;
}

static void check_partial_matches_whole(const uint8_t *data, size_t size,
                                        WPDPixelFormat format, size_t chunk,
                                        const WPDDecoderOptions *options,
                                        int                      use_ext) {
    WPDDecoder     *decoder   = wpd_decoder_create();
    WPDFrame        reference = WPD_FRAME_INIT;
    WPDFrame        frame     = WPD_FRAME_INIT;
    WPDOutputBuffer buffer    = WPD_OUTPUT_BUFFER_INIT;
    uint8_t        *ext[4]    = {NULL, NULL, NULL, NULL};
    int             last_rows = 0, seen = 0, planar, planes;

    CHECK(decoder != NULL);
    if (!decoder)
        return;
    CHECK(wpd_decode(data, size, format, options, &reference) == WPD_OK);
    if (!reference.data[0]) {
        wpd_decoder_free(decoder);
        return;
    }
    planar = reference.format == WPD_PIX_FMT_YUV420P ||
        reference.format == WPD_PIX_FMT_YUVA420P;
    planes = reference.format == WPD_PIX_FMT_YUVA420P ? 4 : planar ? 3 : 1;

    if (use_ext) {
        for (int p = 0; p < planes; p++) {
            const int    shift = p == 1 || p == 2;
            const size_t w     = planar
                ? (size_t)((reference.width + shift) >> shift)
                : (size_t)reference.width * packed_bpp(reference.format);
            const size_t h     = (size_t)((reference.height + shift) >> shift);

            ext[p] = malloc(w * h);
            CHECK(ext[p] != NULL);
            if (!ext[p])
                goto done;
            buffer.plane[p].data   = ext[p];
            buffer.plane[p].size   = w * h;
            buffer.plane[p].stride = (ptrdiff_t)w;
        }
        CHECK(wpd_decoder_set_output_buffer(decoder, &buffer) == WPD_OK);
    }
    if (options)
        CHECK(wpd_decoder_set_options(decoder, options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);

    for (size_t offset = 0; offset < size; offset += chunk) {
        const size_t n    = size - offset < chunk ? size - offset : chunk;
        int          rows = -1;

        CHECK(wpd_decoder_append(decoder, data + offset, n) == WPD_OK);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
        CHECK(wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK);
        CHECK(rows >= last_rows && rows <= reference.height);
        last_rows = rows;
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) seen++;
    CHECK(seen == 1);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, &last_rows) == WPD_OK);
    CHECK(last_rows == reference.height);
    CHECK(frame_equal(&frame, &reference));

done:
    for (int p = 0; p < 4; p++) free(ext[p]);
    wpd_decoder_free(decoder);
    wpd_frame_free(&reference);
}

static void test_partial_matches_whole(const char *path, WPDPixelFormat format,
                                       size_t                   chunk,
                                       const WPDDecoderOptions *options,
                                       int                      use_ext) {
    size_t   size;
    uint8_t *data = read_file(path, &size);

    if (!data)
        return;
    check_partial_matches_whole(data, size, format, chunk, options, use_ext);
    free(data);
}

static void test_structs_and_limits(void) {
    uint8_t      file[30];
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDFrame     frame   = WPD_FRAME_INIT;
    WPDDecoder  *decoder = wpd_decoder_create();

    memcpy(file, "RIFF", 4);
    put32(file + 4, 22);
    memcpy(file + 8, "WEBPVP8X", 8);
    put32(file + 16, 10);
    memset(file + 20, 0, 10);
    put24(file + 24, 65535);
    put24(file + 27, 65535);
    CHECK(wpd_get_info(file, sizeof(file), &info) == WPD_ERR_TOO_LARGE);

    info.struct_size = 0;
    CHECK(wpd_get_info(file, sizeof(file), &info) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_next_frame(decoder, &frame) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    frame.struct_size = 0;
    CHECK(wpd_decoder_next_frame(decoder, &frame) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_partial_frame(decoder, &frame, NULL) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_options(
              decoder, &(WPDDecoderOptions){.struct_size = sizeof(size_t)}) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_options(decoder, NULL) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_status(decoder) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_options(NULL, NULL) == WPD_ERR_INVALID_ARG);
    wpd_decoder_free(decoder);
}

static void test_borrowed_and_update(const uint8_t *data, size_t size,
                                     const WPDFrame *reference) {
    WPDDecoder *decoder = wpd_decoder_create();
    WPDFrame    frame   = WPD_FRAME_INIT;
    uint8_t    *owned   = NULL;
    int         ret     = 0;

    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) == 1);
    CHECK(frame_equal(&frame, reference));

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t n = 1; n < size && ret == 0;) {
        uint8_t *next = malloc(n);

        CHECK(next != NULL);
        if (!next)
            break;
        memcpy(next, data, n);
        CHECK(wpd_decoder_update(decoder, next, n) == WPD_OK);
        free(owned);
        owned = next;
        ret   = wpd_decoder_next_frame(decoder, &frame);
        n     = n > size / 2 ? size : n * 2;
    }
    if (!ret) {
        uint8_t *next = malloc(size);

        CHECK(next != NULL);
        if (next)
            memcpy(next, data, size);
        CHECK(next && wpd_decoder_update(decoder, next, size) == WPD_OK);
        free(owned);
        owned = next;
        ret   = wpd_decoder_next_frame(decoder, &frame);
    }
    if (!ret) {
        CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
        ret = wpd_decoder_next_frame(decoder, &frame);
    }
    CHECK(ret == 1);
    CHECK(frame_equal(&frame, reference));
    CHECK(wpd_decoder_append(decoder, data, 1) == WPD_ERR_INVALID_ARG);
    wpd_decoder_free(decoder);
    free(owned);

    /* A rejected update must not retain caller memory. */
    decoder = wpd_decoder_create();
    CHECK(decoder != NULL);
    if (!decoder)
        return;
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    {
        uint8_t *bad = malloc(size);

        CHECK(bad != NULL);
        if (bad) {
            memcpy(bad, data, size);
            memset(bad, 'X', 12 < size ? 12 : size);
            CHECK(wpd_decoder_update(decoder, bad, size) < 0);
            free(bad);
            CHECK(wpd_decoder_end_of_stream(decoder) < 0);
            CHECK(wpd_decoder_next_frame(decoder, &frame) <= 0);
        }
    }
    wpd_decoder_free(decoder);
}

static void test_transforms(const uint8_t *data, size_t size,
                            const WPDFrame *reference, int lossless) {
    WPDDecoderOptions options  = WPD_DECODER_OPTIONS_INIT;
    WPDFrame          frame    = WPD_FRAME_INIT;
    const int         origin_x = lossless ? 3 : 2;
    const int         origin_y = lossless ? 5 : 4;

    options.use_cropping = 1;
    options.crop_left    = 3;
    options.crop_top     = 5;
    options.crop_width   = 31;
    options.crop_height  = 29;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) == WPD_OK);
    CHECK(frame.width == 31 && frame.height == 29);
    for (int y = 0; y < frame.height; y++)
        CHECK(!memcmp(frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                      reference->data[0] +
                          (ptrdiff_t)(y + origin_y) * reference->stride[0] +
                          origin_x * 4,
                      (size_t)frame.width * 4));
    wpd_frame_free(&frame);

    options      = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.flip = 1;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) == WPD_OK);
    for (int y = 0; y < frame.height; y++)
        CHECK(!memcmp(
            frame.data[0] + (ptrdiff_t)y * frame.stride[0],
            reference->data[0] +
                (ptrdiff_t)(frame.height - 1 - y) * reference->stride[0],
            (size_t)frame.width * 4));
    wpd_frame_free(&frame);

    options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.use_scaling   = 1;
    options.scaled_width  = 37;
    options.scaled_height = 31;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) == WPD_OK);
    CHECK(frame.width == 37 && frame.height == 31);
    wpd_frame_free(&frame);

    /* libwebp scales lossy chroma instead of fancy-upsampling it. */
    if (lossless) {
        options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
        options.use_scaling   = 1;
        options.scaled_width  = reference->width;
        options.scaled_height = reference->height;
        CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) ==
              WPD_OK);
        CHECK(frame.width == reference->width &&
              frame.height == reference->height);
        for (int y = 0; y < frame.height; y++)
            CHECK(!memcmp(
                frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                reference->data[0] + (ptrdiff_t)y * reference->stride[0],
                (size_t)frame.width * 4));
        wpd_frame_free(&frame);
    }

    /* An inferred dimension is rounded up, as libwebp does. */
    options               = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    options.use_scaling   = 1;
    options.scaled_height = 48;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) == WPD_OK);
    CHECK(frame.height == 48);
    CHECK(frame.width ==
          (int)(((int64_t)reference->width * 48 + reference->height - 1) /
                reference->height));
    wpd_frame_free(&frame);
}

/* libwebp premultiplies rgbA4444 in the packed 4-bit domain, after the
   truncation, not in 8-bit before it. */
static void premultiply_4444(uint8_t *pair) {
    const unsigned rg   = pair[0];
    const unsigned ba   = pair[1];
    const unsigned a    = ba & 0x0f;
    const unsigned mult = a * 0x1111u;
    const unsigned r    = (((rg & 0xf0) | rg >> 4) * mult) >> 16;
    const unsigned g    = (((rg & 0x0f) | (rg << 4 & 0xf0)) * mult) >> 16;
    const unsigned b    = (((ba & 0xf0) | ba >> 4) * mult) >> 16;

    pair[0] = (uint8_t)((r & 0xf0) | (g >> 4 & 0x0f));
    pair[1] = (uint8_t)((b & 0xf0) | a);
}

static void test_16bit(const uint8_t *data, size_t size,
                       const WPDFrame *reference) {
    WPDFrame rgb565      = WPD_FRAME_INIT;
    WPDFrame rgba4444    = WPD_FRAME_INIT;
    WPDFrame rgba4444pre = WPD_FRAME_INIT;

    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGB565, NULL, &rgb565) == WPD_OK);
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA4444, NULL, &rgba4444) ==
          WPD_OK);
    CHECK(
        wpd_decode(data, size, WPD_PIX_FMT_RGBA4444_PRE, NULL, &rgba4444pre) ==
        WPD_OK);
    for (int y = 0; y < reference->height; y++) {
        const uint8_t *src = reference->data[0] +
            (ptrdiff_t)y * reference->stride[0];
        const uint8_t *p565  = rgb565.data[0] + (ptrdiff_t)y * rgb565.stride[0];
        const uint8_t *p4444 = rgba4444.data[0] +
            (ptrdiff_t)y * rgba4444.stride[0];
        const uint8_t *ppre = rgba4444pre.data[0] +
            (ptrdiff_t)y * rgba4444pre.stride[0];

        for (int x = 0; x < reference->width; x++) {
            uint8_t want[2];

            CHECK(p565[2 * x] == ((src[4 * x] & 0xf8) | src[4 * x + 1] >> 5));
            CHECK(p565[2 * x + 1] ==
                  ((src[4 * x + 1] << 3 & 0xe0) | src[4 * x + 2] >> 3));
            CHECK(p4444[2 * x] == ((src[4 * x] & 0xf0) | src[4 * x + 1] >> 4));
            CHECK(p4444[2 * x + 1] ==
                  ((src[4 * x + 2] & 0xf0) | src[4 * x + 3] >> 4));

            want[0] = p4444[2 * x];
            want[1] = p4444[2 * x + 1];
            premultiply_4444(want);
            CHECK(ppre[2 * x] == want[0]);
            CHECK(ppre[2 * x + 1] == want[1]);
        }
    }
    wpd_frame_free(&rgb565);
    wpd_frame_free(&rgba4444);
    wpd_frame_free(&rgba4444pre);
}

/* BGR 16-bit output differs from RGB only in byte order. */
static void test_16bit_swapped(const uint8_t *data, size_t size) {
    static const struct {
        WPDPixelFormat stock, swapped;
    } pairs[] = {
        {WPD_PIX_FMT_RGB565, WPD_PIX_FMT_BGR565},
        {WPD_PIX_FMT_RGBA4444, WPD_PIX_FMT_BGRA4444},
        {WPD_PIX_FMT_RGBA4444_PRE, WPD_PIX_FMT_BGRA4444_PRE},
    };

    for (size_t i = 0; i < sizeof(pairs) / sizeof(*pairs); i++) {
        WPDFrame stock = WPD_FRAME_INIT, swapped = WPD_FRAME_INIT;

        CHECK(wpd_decode(data, size, pairs[i].stock, NULL, &stock) == WPD_OK);
        CHECK(wpd_decode(data, size, pairs[i].swapped, NULL, &swapped) ==
              WPD_OK);
        CHECK(swapped.format == pairs[i].swapped);
        CHECK(swapped.width == stock.width && swapped.height == stock.height);
        for (int y = 0; y < stock.height; y++) {
            const uint8_t *a = stock.data[0] + (ptrdiff_t)y * stock.stride[0];
            const uint8_t *b = swapped.data[0] +
                (ptrdiff_t)y * swapped.stride[0];

            for (int x = 0; x < stock.width; x++) {
                CHECK(b[2 * x] == a[2 * x + 1]);
                CHECK(b[2 * x + 1] == a[2 * x]);
            }
        }
        wpd_frame_free(&stock);
        wpd_frame_free(&swapped);
    }
}

/* Floating-point reference for libwebp's lossless RGB-to-YUV conversion. */
static double gamma_to_linear_ref(int v) {
    const int scale = (1 << 12) - 1;

    return floor(pow(v / 255.0, 0.80) * scale + 0.5);
}

static int linear_to_gamma_ref(double linear, int shift) {
    const double value = linear * (1 << shift);
    const int    pos   = (int)(value / (1 << 9));
    const double x     = value - pos * (double)(1 << 9);
    const double scale = 128.0 / ((1 << 12) - 1);
    const double v0    = floor(255.0 * pow(scale * pos, 1.0 / 0.80) + 0.5);
    const double v1 = floor(255.0 * pow(scale * (pos + 1), 1.0 / 0.80) + 0.5);

    return (int)((v1 * x + v0 * (512 - x) + 64) / 128);
}

static int clip_uv_ref(long uv) {
    uv = (uv + (1 << 17) + (128L << 18)) >> 18;
    return uv < 0 ? 0 : uv > 255 ? 255 : (int)uv;
}

static void test_lossless_yuv_reference(const uint8_t *data, size_t size,
                                        int has_alpha) {
    WPDFrame argb       = WPD_FRAME_INIT;
    WPDFrame yuv        = WPD_FRAME_INIT;
    int      mismatches = 0;

    CHECK(wpd_decode(data, size, WPD_PIX_FMT_ARGB, NULL, &argb) == WPD_OK);
    CHECK(wpd_decode(data,
                     size,
                     has_alpha ? WPD_PIX_FMT_YUVA420P : WPD_PIX_FMT_YUV420P,
                     NULL,
                     &yuv) == WPD_OK);
    if (!argb.data[0] || !yuv.data[0])
        return;

    for (int y = 0; y < argb.height; y += 2) {
        const int      y1  = y + 1 < argb.height ? y + 1 : y;
        const uint8_t *top = argb.data[0] + (ptrdiff_t)y * argb.stride[0];
        const uint8_t *bot = argb.data[0] + (ptrdiff_t)y1 * argb.stride[0];

        for (int x = 0; x < argb.width; x += 2) {
            const int      x1   = x + 1 < argb.width ? x + 1 : x;
            const uint8_t *p[4] = {
                top + 4 * x, top + 4 * x1, bot + 4 * x, bot + 4 * x1};
            const int last_col = x1 == x, last_row = y1 == y;
            const int weight[4] = {1 + last_col + 2 * last_row * (1 + last_col),
                                   last_col ? 0 : 1 + 2 * last_row,
                                   last_row ? 0 : 1 + last_col,
                                   (last_col || last_row) ? 0 : 1};
            long      total_a   = 0;
            double    lin[3]    = {0, 0, 0};
            int       rgb[3];
            int       expect_u, expect_v;

            for (int k = 0; k < 4; k++) total_a += (long)weight[k] * p[k][0];
            for (int c = 0; c < 3; c++) {
                double sum = 0;

                if (has_alpha && total_a != 0 && total_a != 4 * 255) {
                    for (int k = 0; k < 4; k++)
                        sum += weight[k] * (double)p[k][0] *
                            gamma_to_linear_ref(p[k][c + 1]);
                    sum = floor(sum * (double)((1u << 19) / (unsigned)total_a) /
                                (double)(1 << 17));
                    lin[c] = linear_to_gamma_ref(sum, 0);
                } else {
                    for (int k = 0; k < 4; k++)
                        sum += weight[k] * gamma_to_linear_ref(p[k][c + 1]);
                    lin[c] = linear_to_gamma_ref(sum, 0);
                }
            }
            for (int c = 0; c < 3; c++) rgb[c] = (int)lin[c];
            expect_u = clip_uv_ref(-9719L * rgb[0] - 19081L * rgb[1] +
                                   28800L * rgb[2]);
            expect_v = clip_uv_ref(28800L * rgb[0] - 24116L * rgb[1] -
                                   4684L * rgb[2]);
            if (yuv.data[1][(ptrdiff_t)(y >> 1) * yuv.stride[1] + (x >> 1)] !=
                    expect_u ||
                yuv.data[2][(ptrdiff_t)(y >> 1) * yuv.stride[2] + (x >> 1)] !=
                    expect_v)
                mismatches++;
        }
    }
    CHECK(mismatches == 0);
    wpd_frame_free(&argb);
    wpd_frame_free(&yuv);
}

static void test_planar(const uint8_t *data, size_t size) {
    WPDFrame        reference = WPD_FRAME_INIT;
    WPDFrame        frame     = WPD_FRAME_INIT;
    WPDOutputBuffer output    = WPD_OUTPUT_BUFFER_INIT;
    uint8_t        *planes[4] = {NULL};

    CHECK(wpd_decode(data, size, WPD_PIX_FMT_YUVA420P, NULL, &reference) ==
          WPD_OK);
    for (int p = 0; p < 4; p++) {
        int shift  = p == 1 || p == 2;
        int width  = (reference.width + shift) >> shift;
        int height = (reference.height + shift) >> shift;

        planes[p] = malloc((size_t)width * height);
        CHECK(planes[p] != NULL);
        output.plane[p].data   = planes[p];
        output.plane[p].size   = (size_t)width * height;
        output.plane[p].stride = width;
    }
    CHECK(wpd_decode_into(
              data, size, WPD_PIX_FMT_YUVA420P, NULL, &output, &frame) ==
          WPD_OK);
    CHECK(frame_equal(&frame, &reference));
    for (int p = 0; p < 4; p++) free(planes[p]);
    wpd_frame_free(&reference);
}

static void test_raw(const uint8_t *data, size_t size, const char tag[4],
                     const WPDFrame *reference, int ignore_alpha) {
    size_t         payload_size;
    const uint8_t *chunk = find_chunk(data, size, tag, &payload_size, NULL);
    WPDFrame       frame = WPD_FRAME_INIT;
    WPDImageInfo   info  = WPD_IMAGE_INFO_INIT;

    CHECK(chunk != NULL);
    if (!chunk)
        return;
    CHECK(wpd_get_info(chunk + 8, payload_size, &info) == WPD_OK);
    CHECK(info.width == reference->width && info.height == reference->height);
    CHECK(wpd_decode(chunk + 8, payload_size, WPD_PIX_FMT_RGBA, NULL, &frame) ==
          WPD_OK);
    if (ignore_alpha) {
        for (int y = 0; y < frame.height; y++) {
            const uint8_t *got = frame.data[0] + (ptrdiff_t)y * frame.stride[0];
            const uint8_t *want = reference->data[0] +
                (ptrdiff_t)y * reference->stride[0];

            for (int x = 0; x < frame.width; x++)
                CHECK(!memcmp(got + 4 * x, want + 4 * x, 3));
        }
    } else {
        CHECK(frame_equal(&frame, reference));
    }
    wpd_frame_free(&frame);
}

static void test_alph_vp8_raw(const uint8_t *data, size_t size,
                              const WPDFrame *reference) {
    size_t         alpha_chunk_size, vp8_chunk_size;
    const uint8_t *alpha = find_chunk(
        data, size, "ALPH", NULL, &alpha_chunk_size);
    const uint8_t *vp8 = find_chunk(data, size, "VP8 ", NULL, &vp8_chunk_size);
    uint8_t       *raw;
    WPDFrame       frame = WPD_FRAME_INIT;

    CHECK(alpha && vp8);
    if (!alpha || !vp8)
        return;
    raw = malloc(alpha_chunk_size + vp8_chunk_size);
    CHECK(raw != NULL);
    if (!raw)
        return;
    memcpy(raw, alpha, alpha_chunk_size);
    memcpy(raw + alpha_chunk_size, vp8, vp8_chunk_size);
    CHECK(wpd_decode(raw,
                     alpha_chunk_size + vp8_chunk_size,
                     WPD_PIX_FMT_RGBA,
                     NULL,
                     &frame) == WPD_OK);
    CHECK(frame_equal(&frame, reference));
    wpd_frame_free(&frame);

    raw[8] |= 3;
    CHECK(wpd_decode(raw,
                     alpha_chunk_size + vp8_chunk_size,
                     WPD_PIX_FMT_RGBA,
                     NULL,
                     &frame) == WPD_ERR_UNSUPPORTED);
    free(raw);
}

static void test_raw_stream(const uint8_t *data, size_t size, const char tag[4],
                            int alpha_raw) {
    size_t         payload_size, alpha_chunk_size = 0, vp8_chunk_size = 0;
    const uint8_t *chunk    = find_chunk(data, size, tag, &payload_size, NULL);
    const uint8_t *raw      = chunk ? chunk + 8 : NULL;
    uint8_t       *joined   = NULL;
    size_t         raw_size = payload_size;
    WPDFrame       frame = WPD_FRAME_INIT, whole = WPD_FRAME_INIT;

    CHECK(chunk != NULL);
    if (!chunk)
        return;

    if (alpha_raw) {
        const uint8_t *alpha = find_chunk(
            data, size, "ALPH", NULL, &alpha_chunk_size);
        const uint8_t *vp8 = find_chunk(
            data, size, "VP8 ", NULL, &vp8_chunk_size);

        CHECK(alpha && vp8);
        if (!alpha || !vp8)
            return;
        raw_size = alpha_chunk_size + vp8_chunk_size;
        joined   = malloc(raw_size);
        CHECK(joined != NULL);
        if (!joined)
            return;
        memcpy(joined, alpha, alpha_chunk_size);
        memcpy(joined + alpha_chunk_size, vp8, vp8_chunk_size);
        raw = joined;
    }

    CHECK(wpd_decode(raw, raw_size, WPD_PIX_FMT_RGBA, NULL, &whole) == WPD_OK);

    for (size_t chunk_bytes = 1; chunk_bytes <= 16; chunk_bytes *= 4) {
        WPDDecoder *decoder = wpd_decoder_create();
        int         failed  = 0;

        CHECK(decoder != NULL);
        if (!decoder)
            break;
        CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA) ==
              WPD_OK);
        CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
        for (size_t off = 0; off < raw_size && !failed; off += chunk_bytes) {
            size_t n = raw_size - off < chunk_bytes ? raw_size - off
                                                    : chunk_bytes;

            if (wpd_decoder_append(decoder, raw + off, n) != WPD_OK) {
                CHECK(!"raw stream rejected a partial prefix");
                failed = 1;
            }
        }
        if (!failed) {
            CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
            CHECK(wpd_decoder_next_frame(decoder, &frame) == 1);
            CHECK(frame_equal(&frame, &whole));
        }
        wpd_decoder_free(decoder);
    }

    {
        WPDDecoder *decoder = wpd_decoder_create();
        const int   bounded = alpha_raw || !memcmp(tag, "VP8 ", 4);

        CHECK(decoder != NULL);
        if (decoder) {
            CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
            CHECK(wpd_decoder_append(decoder, raw, 10) == WPD_OK);
            CHECK(wpd_decoder_next_frame(decoder, &frame) == 0);
            if (bounded) {
                CHECK(wpd_decoder_end_of_stream(decoder) == WPD_ERR_TRUNCATED);
            } else {
                CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
                CHECK(wpd_decoder_next_frame(decoder, &frame) < 0);
            }
            wpd_decoder_free(decoder);
        }
    }

    if (!alpha_raw && !memcmp(tag, "VP8 ", 4)) {
        uint8_t    *bad     = malloc(raw_size);
        WPDDecoder *decoder = wpd_decoder_create();
        uint32_t    bits;

        CHECK(bad && decoder);
        if (bad && decoder) {
            memcpy(bad, raw, raw_size);
            bits = (uint32_t)bad[0] | (uint32_t)bad[1] << 8 |
                (uint32_t)bad[2] << 16;
            bits   = (bits & 31) | (uint32_t)((raw_size + 4096) << 5);
            bad[0] = bits & 0xff;
            bad[1] = (bits >> 8) & 0xff;
            bad[2] = (bits >> 16) & 0xff;
            CHECK(wpd_decoder_open(decoder, bad, raw_size) ==
                  WPD_ERR_BITSTREAM);
            wpd_decoder_free(decoder);

            decoder = wpd_decoder_create();
            CHECK(decoder != NULL);
            if (decoder) {
                CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
                CHECK(wpd_decoder_append(decoder, bad, raw_size) == WPD_OK);
                CHECK(wpd_decoder_end_of_stream(decoder) == WPD_ERR_TRUNCATED);
            }
        }
        wpd_decoder_free(decoder);
        free(bad);
    }

    wpd_frame_free(&frame);
    wpd_frame_free(&whole);
    free(joined);
}

static void test_decode_options(const uint8_t *data, size_t size) {
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDFrame          frame   = WPD_FRAME_INIT;

    options.bypass_filtering    = 1;
    options.no_fancy_upsampling = 1;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, &options, &frame) == WPD_OK);
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_BGRA, NULL, &frame) == WPD_OK);
    CHECK(frame.format == WPD_PIX_FMT_BGRA);
    wpd_frame_free(&frame);
}

static void test_replacement_api_file(const char *path, const char tag[4],
                                      int alpha_raw) {
    size_t   size;
    uint8_t *data      = read_file(path, &size);
    WPDFrame reference = WPD_FRAME_INIT;
    CHECK(data != NULL);
    if (!data)
        return;
    CHECK(wpd_decode(data, size, WPD_PIX_FMT_RGBA, NULL, &reference) == WPD_OK);
    test_borrowed_and_update(data, size, &reference);
    test_transforms(data, size, &reference, !memcmp(tag, "VP8L", 4));
    test_16bit(data, size, &reference);
    test_16bit_swapped(data, size);
    test_planar(data, size);
    if (!memcmp(tag, "VP8L", 4)) {
        test_lossless_yuv_reference(data, size, 0);
        test_lossless_yuv_reference(data, size, 1);
    }
    test_raw(data, size, tag, &reference, alpha_raw);
    test_raw_stream(data, size, tag, 0);
    if (alpha_raw) {
        test_alph_vp8_raw(data, size, &reference);
        test_raw_stream(data, size, tag, 1);
    }
    test_decode_options(data, size);
    wpd_frame_free(&reference);
    free(data);
}

static uint8_t premultiply_8(unsigned value, unsigned alpha) {
    return alpha == 0xff ? (uint8_t)value
                         : (uint8_t)((value * (alpha * 32897u)) >> 23);
}

static void check_animation_format_switch(const uint8_t *data, size_t size,
                                          WPDPixelFormat from,
                                          WPDPixelFormat to, int at) {
    WPDDecoder *ref = wpd_decoder_create(), *mix = wpd_decoder_create();
    WPDFrame    a = WPD_FRAME_INIT, b = WPD_FRAME_INIT;
    int         frames = 0;

    CHECK(ref && mix);
    if (!ref || !mix) {
        wpd_decoder_free(ref);
        wpd_decoder_free(mix);
        return;
    }
    CHECK(wpd_decoder_set_output_format(ref, to) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(mix, from) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(ref, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(mix, data, size) == WPD_OK);

    while (wpd_decoder_next_frame(ref, &a) == 1 &&
           wpd_decoder_next_frame(mix, &b) == 1) {
        frames++;
        if (frames == at) {
            CHECK(wpd_decoder_set_output_format(mix, to) == WPD_OK);
            continue;
        }
        if (frames < at)
            continue;
        CHECK(a.width == b.width && a.height == b.height);
        for (int y = 0; y < a.height; y++) {
            const uint8_t *p = a.data[0] + (ptrdiff_t)y * a.stride[0];
            const uint8_t *q = b.data[0] + (ptrdiff_t)y * b.stride[0];

            for (int x = 0; x < a.width * 4; x += 4) {
                const int alpha = p[x + 3];

                CHECK(q[x + 3] == alpha);
                for (int c = 0; c < 3; c++)
                    CHECK(abs((int)p[x + c] - (int)q[x + c]) * alpha / 255 <=
                          2);
            }
        }
    }
    CHECK(frames > at);
    wpd_decoder_free(ref);
    wpd_decoder_free(mix);
}

static void test_animation_format_switch(const char *path) {
    size_t   size;
    uint8_t *data = read_file(path, &size);

    CHECK(data != NULL);
    if (!data)
        return;
    for (int at = 1; at <= 3; at++) {
        check_animation_format_switch(
            data, size, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_RGBA, at);
        check_animation_format_switch(
            data, size, WPD_PIX_FMT_RGBA, WPD_PIX_FMT_RGBA_PRE, at);
        check_animation_format_switch(
            data, size, WPD_PIX_FMT_BGRA_PRE, WPD_PIX_FMT_RGBA, at);
    }
    free(data);
}

static void test_scaled_premultiply_identity(const char *path) {
    size_t            size;
    uint8_t          *data    = read_file(path, &size);
    WPDDecoder       *plain   = wpd_decoder_create();
    WPDDecoder       *scaled  = wpd_decoder_create();
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDImageInfo      info    = WPD_IMAGE_INFO_INIT;
    WPDFrame          a = WPD_FRAME_INIT, b = WPD_FRAME_INIT;
    int               frames = 0;

    CHECK(data && plain && scaled);
    if (!data || !plain || !scaled) {
        free(data);
        wpd_decoder_free(plain);
        wpd_decoder_free(scaled);
        return;
    }
    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    options.use_scaling   = 1;
    options.scaled_width  = info.width;
    options.scaled_height = info.height;
    CHECK(wpd_decoder_set_options(scaled, &options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(plain, WPD_PIX_FMT_RGBA_PRE) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(scaled, WPD_PIX_FMT_RGBA_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_open_borrowed(plain, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(scaled, data, size) == WPD_OK);

    while (wpd_decoder_next_frame(plain, &a) == 1 &&
           wpd_decoder_next_frame(scaled, &b) == 1) {
        CHECK(frame_equal(&a, &b));
        frames++;
    }
    CHECK(frames > 0);
    wpd_decoder_free(plain);
    wpd_decoder_free(scaled);
    free(data);
}

/* A premultiplied animation canvas must not be alpha-weighted twice. */
static void test_scaled_premultiply(const char *path) {
    size_t      size;
    uint8_t    *data = read_file(path, &size);
    WPDDecoder *pre = wpd_decoder_create(), *straight = wpd_decoder_create();
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDFrame          a = WPD_FRAME_INIT, b = WPD_FRAME_INIT;
    int               frames = 0;

    CHECK(data && pre && straight);
    if (!data || !pre || !straight) {
        free(data);
        wpd_decoder_free(pre);
        wpd_decoder_free(straight);
        return;
    }
    options.use_scaling   = 1;
    options.scaled_width  = 37;
    options.scaled_height = 31;
    CHECK(wpd_decoder_set_options(pre, &options) == WPD_OK);
    CHECK(wpd_decoder_set_options(straight, &options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(pre, WPD_PIX_FMT_RGBA_PRE) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(straight, WPD_PIX_FMT_RGBA) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(pre, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(straight, data, size) == WPD_OK);

    while (wpd_decoder_next_frame(pre, &a) == 1 &&
           wpd_decoder_next_frame(straight, &b) == 1) {
        CHECK(a.width == b.width && a.height == b.height);
        for (int y = 0; y < a.height; y++) {
            const uint8_t *got  = a.data[0] + (ptrdiff_t)y * a.stride[0];
            const uint8_t *want = b.data[0] + (ptrdiff_t)y * b.stride[0];

            for (int x = 0; x < a.width * 4; x += 4) {
                CHECK(got[x + 3] == want[x + 3]);
                for (int c = 0; c < 3; c++) {
                    const int expect = premultiply_8(want[x + c], want[x + 3]);

                    CHECK(abs((int)got[x + c] - expect) <= 2);
                }
            }
        }
        frames++;
    }
    CHECK(frames > 0);
    wpd_decoder_free(pre);
    wpd_decoder_free(straight);
    free(data);
}

static void test_replacement_animation(const char *path) {
    size_t            size;
    uint8_t          *data    = read_file(path, &size);
    WPDDecoder       *decoder = wpd_decoder_create();
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;
    WPDImageInfo      info    = WPD_IMAGE_INFO_INIT;
    WPDFrame          frame   = WPD_FRAME_INIT;
    int               frames  = 0, ret;
    CHECK(data && decoder);
    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        return;
    }
    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    options.use_cropping  = 1;
    options.crop_left     = 3;
    options.crop_top      = 5;
    options.crop_width    = 101;
    options.crop_height   = 99;
    options.use_scaling   = 1;
    options.scaled_width  = 51;
    options.scaled_height = 49;
    options.flip          = 1;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA4444_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);
    while ((ret = wpd_decoder_next_frame(decoder, &frame)) > 0) {
        CHECK(frame.width == 51 && frame.height == 49);
        CHECK(frame.format == WPD_PIX_FMT_RGBA4444_PRE);
        frames++;
    }
    CHECK(ret == 0);
    CHECK(frames == info.frame_count);
    wpd_decoder_free(decoder);
    free(data);
}

static void blend_over_premult(uint8_t *dst, const uint8_t *src, int pixels) {
    for (int x = 0; x < pixels; x++, dst += 4, src += 4) {
        const unsigned scale = 256 - src[0];

        if (src[0] == 255) {
            memcpy(dst, src, 4);
            continue;
        }
        for (int c = 0; c < 4; c++)
            dst[c] = (uint8_t)(src[c] + ((dst[c] * scale) >> 8));
    }
}

static void test_subframe_composite(const char *path) {
    size_t       size;
    uint8_t     *data         = read_file(path, &size);
    WPDDecoder  *whole        = wpd_decoder_create();
    WPDDecoder  *parts        = wpd_decoder_create();
    WPDImageInfo info         = WPD_IMAGE_INFO_INIT;
    uint8_t     *canvas       = NULL;
    int          frames       = 0;
    int          prev_dispose = WPD_DISPOSE_NONE;
    int          prev_x = 0, prev_y = 0, prev_w = 0, prev_h = 0;

    if (!data || !whole || !parts)
        goto done;
    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    CHECK(info.is_animation);
    canvas = calloc((size_t)info.width * info.height, 4);
    CHECK(canvas != NULL);
    if (!canvas)
        goto done;

    CHECK(wpd_decoder_set_output_format(whole, WPD_PIX_FMT_ARGB_PRE) == WPD_OK);
    CHECK(wpd_decoder_set_output_format(parts, WPD_PIX_FMT_ARGB_PRE) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(parts, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(whole, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(parts, data, size) == WPD_OK);

    for (;;) {
        WPDFrame reference = WPD_FRAME_INIT, sub = WPD_FRAME_INIT;
        int      got_reference = wpd_decoder_next_frame(whole, &reference);
        int      got_sub       = wpd_decoder_next_frame(parts, &sub);

        CHECK(got_reference == got_sub);
        if (got_reference <= 0 || got_sub <= 0)
            break;

        if (prev_dispose == WPD_DISPOSE_BACKGROUND)
            for (int y = 0; y < prev_h; y++)
                memset(
                    canvas + ((size_t)(prev_y + y) * info.width + prev_x) * 4,
                    0,
                    (size_t)prev_w * 4);

        CHECK(sub.pos_x == reference.pos_x && sub.pos_y == reference.pos_y);
        CHECK(sub.dispose == reference.dispose && sub.blend == reference.blend);
        CHECK(sub.duration == reference.duration);
        CHECK(sub.timestamp == reference.timestamp);
        CHECK(reference.width == info.width && reference.height == info.height);
        CHECK(sub.pos_x + sub.width <= info.width);
        CHECK(sub.pos_y + sub.height <= info.height);

        for (int y = 0; y < sub.height; y++) {
            uint8_t *row = canvas +
                ((size_t)(sub.pos_y + y) * info.width + sub.pos_x) * 4;
            const uint8_t *src = sub.data[0] + (ptrdiff_t)y * sub.stride[0];

            if (sub.blend == WPD_BLEND_ALPHA)
                blend_over_premult(row, src, sub.width);
            else
                memcpy(row, src, (size_t)sub.width * 4);
        }

        for (int y = 0; y < info.height; y++)
            if (memcmp(canvas + (size_t)y * info.width * 4,
                       reference.data[0] + (ptrdiff_t)y * reference.stride[0],
                       (size_t)info.width * 4)) {
                fprintf(stderr,
                        "%s: frame %d row %d differs from the composited "
                        "canvas\n",
                        path,
                        frames,
                        y);
                failures++;
                goto done;
            }

        prev_dispose = sub.dispose;
        prev_x       = sub.pos_x;
        prev_y       = sub.pos_y;
        prev_w       = sub.width;
        prev_h       = sub.height;
        frames++;
    }
    CHECK(frames == info.frame_count);

done:
    free(canvas);
    wpd_decoder_free(whole);
    wpd_decoder_free(parts);
    free(data);
}

static void test_subframe_4444_premultiply(const char *path) {
    size_t      size;
    uint8_t    *data     = read_file(path, &size);
    WPDDecoder *straight = wpd_decoder_create();
    WPDDecoder *pre      = wpd_decoder_create();
    WPDDecoder *swapped  = wpd_decoder_create();
    int         frames   = 0;

    CHECK(data && straight && pre && swapped);
    if (!data || !straight || !pre || !swapped)
        goto done;

    CHECK(wpd_decoder_set_output_format(straight, WPD_PIX_FMT_RGBA4444) ==
          WPD_OK);
    CHECK(wpd_decoder_set_output_format(pre, WPD_PIX_FMT_RGBA4444_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_set_output_format(swapped, WPD_PIX_FMT_BGRA4444_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(straight, WPD_ANIM_SUBFRAME) ==
          WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(pre, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(swapped, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(straight, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(pre, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(swapped, data, size) == WPD_OK);

    for (;;) {
        WPDFrame  a = WPD_FRAME_INIT, b = WPD_FRAME_INIT, c = WPD_FRAME_INIT;
        const int got_a = wpd_decoder_next_frame(straight, &a);
        const int got_b = wpd_decoder_next_frame(pre, &b);
        const int got_c = wpd_decoder_next_frame(swapped, &c);

        CHECK(got_a == got_b && got_b == got_c);
        if (got_a <= 0 || got_b <= 0 || got_c <= 0)
            break;
        CHECK(b.format == WPD_PIX_FMT_RGBA4444_PRE);
        CHECK(c.format == WPD_PIX_FMT_BGRA4444_PRE);
        CHECK(a.width == b.width && a.height == b.height);
        CHECK(b.width == c.width && b.height == c.height);
        CHECK(a.pos_x == b.pos_x && a.pos_y == b.pos_y);

        for (int y = 0; y < a.height; y++) {
            const uint8_t *pa = a.data[0] + (ptrdiff_t)y * a.stride[0];
            const uint8_t *pb = b.data[0] + (ptrdiff_t)y * b.stride[0];
            const uint8_t *pc = c.data[0] + (ptrdiff_t)y * c.stride[0];

            for (int x = 0; x < a.width; x++) {
                uint8_t want[2];

                want[0] = pa[2 * x];
                want[1] = pa[2 * x + 1];
                premultiply_4444(want);
                CHECK(pb[2 * x] == want[0] && pb[2 * x + 1] == want[1]);
                CHECK(pc[2 * x] == want[1] && pc[2 * x + 1] == want[0]);
            }
        }
        frames++;
    }
    CHECK(frames > 0);

done:
    wpd_decoder_free(straight);
    wpd_decoder_free(pre);
    wpd_decoder_free(swapped);
    free(data);
}

/* libwebp's composited canvas is already premultiplied before packing. */
static void test_composited_4444_premultiply(const char *path) {
    size_t      size;
    uint8_t    *data   = read_file(path, &size);
    WPDDecoder *canvas = wpd_decoder_create();
    WPDDecoder *packed = wpd_decoder_create();
    int         frames = 0;

    CHECK(data && canvas && packed);
    if (!data || !canvas || !packed)
        goto done;

    CHECK(wpd_decoder_set_output_format(canvas, WPD_PIX_FMT_ARGB_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_set_output_format(packed, WPD_PIX_FMT_RGBA4444_PRE) ==
          WPD_OK);
    CHECK(wpd_decoder_open_borrowed(canvas, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(packed, data, size) == WPD_OK);

    for (;;) {
        WPDFrame  a = WPD_FRAME_INIT, b = WPD_FRAME_INIT;
        const int got_a = wpd_decoder_next_frame(canvas, &a);
        const int got_b = wpd_decoder_next_frame(packed, &b);

        CHECK(got_a == got_b);
        if (got_a <= 0 || got_b <= 0)
            break;
        CHECK(a.width == b.width && a.height == b.height);

        for (int y = 0; y < a.height; y++) {
            const uint8_t *pa = a.data[0] + (ptrdiff_t)y * a.stride[0];
            const uint8_t *pb = b.data[0] + (ptrdiff_t)y * b.stride[0];

            for (int x = 0; x < a.width; x++) {
                const uint8_t want[2] = {
                    (uint8_t)((pa[4 * x + 1] & 0xf0) | pa[4 * x + 2] >> 4),
                    (uint8_t)((pa[4 * x + 3] & 0xf0) | pa[4 * x] >> 4),
                };

                CHECK(pb[2 * x] == want[0] && pb[2 * x + 1] == want[1]);
            }
        }
        frames++;
    }
    CHECK(frames > 0);

done:
    wpd_decoder_free(canvas);
    wpd_decoder_free(packed);
    free(data);
}

static void test_subframe_rejects_transforms(void) {
    WPDDecoder       *decoder = wpd_decoder_create();
    WPDDecoderOptions options = WPD_DECODER_OPTIONS_INIT;

    CHECK(decoder != NULL);
    if (!decoder)
        return;

    options.use_scaling   = 1;
    options.scaled_width  = 8;
    options.scaled_height = 8;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_COMPOSITED) ==
          WPD_OK);

    options = (WPDDecoderOptions)WPD_DECODER_OPTIONS_INIT;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) == WPD_OK);

    options.use_cropping = 1;
    options.crop_width = options.crop_height = 4;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_ERR_INVALID_ARG);
    options.use_cropping = 0;
    options.flip         = 1;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_ERR_INVALID_ARG);
    options.flip = 0;
    CHECK(wpd_decoder_set_options(decoder, &options) == WPD_OK);

    CHECK(wpd_decoder_set_animation_mode(decoder, 7) == WPD_ERR_INVALID_ARG);
    wpd_decoder_free(decoder);
}

static void test_rewind(const char *path, WPDPixelFormat format, int mode) {
    size_t       size;
    uint8_t     *data    = read_file(path, &size);
    WPDDecoder  *decoder = wpd_decoder_create();
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    uint8_t    **pass    = NULL;
    size_t      *bytes   = NULL;
    int          frames = 0, pass_frames = 0;

    if (!data || !decoder)
        goto done;
    CHECK(wpd_get_info(data, size, &info) == WPD_OK);
    pass  = calloc((size_t)info.frame_count, sizeof(*pass));
    bytes = calloc((size_t)info.frame_count, sizeof(*bytes));
    CHECK(pass && bytes);
    if (!pass || !bytes)
        goto done;

    CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, mode) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);

    for (int loop = 0; loop < 3; loop++) {
        WPDFrame frame = WPD_FRAME_INIT;

        if (loop)
            CHECK(wpd_decoder_rewind(decoder) == WPD_OK);
        pass_frames = 0;
        while (wpd_decoder_next_frame(decoder, &frame) > 0) {
            const size_t row = (size_t)frame.width *
                (size_t)packed_bpp(frame.format);
            uint8_t *flat;

            if (pass_frames >= info.frame_count) {
                failures++;
                break;
            }
            flat = malloc(row * (size_t)frame.height);
            CHECK(flat != NULL);
            if (!flat)
                break;
            for (int y = 0; y < frame.height; y++)
                memcpy(flat + (size_t)y * row,
                       frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                       row);
            if (!loop) {
                pass[pass_frames]  = flat;
                bytes[pass_frames] = row * (size_t)frame.height;
            } else {
                CHECK(bytes[pass_frames] == row * (size_t)frame.height);
                CHECK(!memcmp(pass[pass_frames], flat, bytes[pass_frames]));
                free(flat);
            }
            pass_frames++;
        }
        if (!loop)
            frames = pass_frames;
        CHECK(pass_frames == frames);
    }
    CHECK(frames == info.frame_count);

done:
    for (int i = 0; pass && i < info.frame_count; i++) free(pass[i]);
    free(pass);
    free(bytes);
    wpd_decoder_free(decoder);
    free(data);
}

/* WPDDecoderOptions as it shipped before n_threads was added, so that a v1
   caller can be spelled out here exactly as one really looks: the size it
   passes in struct_size is sizeof this, tail padding included. */
typedef struct WPDDecoderOptionsV1 {
    size_t struct_size;
    int    bypass_filtering;
    int    no_fancy_upsampling;
    int    use_cropping;
    int    crop_left;
    int    crop_top;
    int    crop_width;
    int    crop_height;
    int    use_scaling;
    int    scaled_width;
    int    scaled_height;
    int    flip;
} WPDDecoderOptionsV1;

/* Output must not depend on how many threads produced it, and a caller built
   against the options struct as it was before n_threads existed must still get
   the same pixels it always did. */
static void test_threads_match(const char *path, WPDPixelFormat format) {
    static const int counts[] = {0, 1, 2, 3, 5, 8, 16};
    size_t           size;
    uint8_t         *data        = read_file(path, &size);
    uint8_t         *first       = NULL;
    size_t           first_bytes = 0;

    if (!data)
        return;

    for (size_t i = 0; i < sizeof(counts) / sizeof(counts[0]) + 1; i++) {
        WPDDecoderOptions        options = WPD_DECODER_OPTIONS_INIT;
        WPDDecoderOptionsV1      v1;
        const WPDDecoderOptions *pass    = &options;
        WPDDecoder              *decoder = wpd_decoder_create();
        WPDFrame                 frame   = WPD_FRAME_INIT;
        uint8_t                 *flat    = NULL;
        size_t                   used    = 0;

        if (!decoder)
            break;
        if (i < sizeof(counts) / sizeof(counts[0])) {
            options.n_threads = counts[i];
        } else {
            /* The tail padding is dirty on purpose: it is not the caller's to
               zero, and it is where a field appended to the old struct would
               land, so reading n_threads out of it has to be impossible. */
            memset(&v1, 0xFF, sizeof(v1));
            memset(&v1, 0, offsetof(WPDDecoderOptionsV1, flip) + sizeof(int));
            v1.struct_size = sizeof(v1);
            pass           = (const WPDDecoderOptions *)&v1;
        }
        CHECK(wpd_decoder_set_options(decoder, pass) == WPD_OK);
        CHECK(wpd_decoder_set_output_format(decoder, format) == WPD_OK);
        CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);

        while (wpd_decoder_next_frame(decoder, &frame) > 0) {
            const size_t row = (size_t)frame.width *
                (size_t)packed_bpp(frame.format);
            const size_t plane = row * (size_t)frame.height;
            uint8_t     *grown = realloc(flat, used + plane);

            CHECK(grown != NULL);
            if (!grown)
                break;
            flat = grown;
            for (int y = 0; y < frame.height; y++)
                memcpy(flat + used + (size_t)y * row,
                       frame.data[0] + (ptrdiff_t)y * frame.stride[0],
                       row);
            used += plane;
        }
        CHECK(used > 0);
        if (!first) {
            first       = flat;
            first_bytes = used;
        } else {
            CHECK(used == first_bytes);
            CHECK(used == first_bytes && !memcmp(first, flat, used));
            free(flat);
        }
        wpd_decoder_free(decoder);
    }
    free(first);
    free(data);
}

static void test_rewind_errors(const char *path) {
    size_t      size;
    uint8_t    *data    = read_file(path, &size);
    WPDDecoder *decoder = wpd_decoder_create();
    WPDFrame    frame   = WPD_FRAME_INIT;

    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        return;
    }
    CHECK(wpd_decoder_rewind(NULL) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_rewind(decoder) == WPD_ERR_INVALID_ARG);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t off = 0; off < size; off += 4096) {
        const size_t n = size - off < 4096 ? size - off : 4096;

        CHECK(wpd_decoder_append(decoder, data + off, n) == WPD_OK);
        while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_rewind(decoder) == WPD_ERR_UNSUPPORTED);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_update(decoder, data, size) == WPD_OK);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) continue;
    CHECK(wpd_decoder_rewind(decoder) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);

    wpd_decoder_free(decoder);
    free(data);
}

static void test_frame_table(const char *path) {
    size_t       size;
    uint8_t     *data    = read_file(path, &size);
    WPDDecoder  *decoder = wpd_decoder_create();
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDFrameInfo entry   = WPD_FRAME_INFO_INIT;
    WPDFrame     frame   = WPD_FRAME_INIT;
    int          index   = 0;
    union {
        WPDFrameInfo info;
        struct {
            uint8_t  fields[sizeof(size_t) + 9 * sizeof(int)];
            uint32_t canary;
        } guarded;
    } exact;

    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        return;
    }
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    memset(&exact, 0xff, sizeof(exact));
    exact.info.struct_size = sizeof(exact.guarded.fields);
    exact.guarded.canary   = 0x12345678;
    CHECK(wpd_decoder_frame_info(decoder, 0, &exact.info) == WPD_OK);
    CHECK(exact.guarded.canary == 0x12345678);
    CHECK(wpd_decoder_frame_info(decoder, -1, &entry) == WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_frame_info(decoder, 0, NULL) == WPD_ERR_INVALID_ARG);

    while (wpd_decoder_next_frame(decoder, &frame) > 0) {
        CHECK(wpd_decoder_frame_info(decoder, index, &entry) == WPD_OK);
        CHECK(entry.pos_x == frame.pos_x && entry.pos_y == frame.pos_y);
        CHECK(entry.duration == frame.duration);
        CHECK(entry.dispose == frame.dispose && entry.blend == frame.blend);
        CHECK(entry.has_alpha == frame.has_alpha);
        CHECK(entry.complete == 1);
        if (info.is_animation)
            CHECK(entry.width == frame.width && entry.height == frame.height);
        else
            CHECK(entry.width == info.width && entry.height == info.height);
        index++;
    }
    CHECK(index == info.frame_count);
    CHECK(wpd_decoder_frame_info(decoder, index, &entry) ==
          WPD_ERR_INVALID_ARG);
    wpd_decoder_free(decoder);
    free(data);
}

static int frame_info_equal(const WPDFrameInfo *a, const WPDFrameInfo *b) {
    return a->pos_x == b->pos_x && a->pos_y == b->pos_y &&
        a->width == b->width && a->height == b->height &&
        a->duration == b->duration && a->dispose == b->dispose &&
        a->blend == b->blend && a->has_alpha == b->has_alpha &&
        a->complete == b->complete;
}

static void test_frame_table_stream(const char *path, size_t chunk,
                                    int use_append) {
    size_t       size;
    uint8_t     *data    = read_file(path, &size);
    WPDDecoder  *decoder = wpd_decoder_create();
    WPDDecoder  *whole   = wpd_decoder_create();
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    int          seen = 0, saw_incomplete = 0;

    if (!data || !decoder || !whole) {
        free(data);
        wpd_decoder_free(decoder);
        wpd_decoder_free(whole);
        return;
    }
    CHECK(wpd_decoder_open_borrowed(whole, data, size) == WPD_OK);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t off = 0; off < size; off += chunk) {
        const size_t n         = size - off < chunk ? size - off : chunk;
        int          available = 0;

        if (use_append)
            CHECK(wpd_decoder_append(decoder, data + off, n) == WPD_OK);
        else
            CHECK(wpd_decoder_update(decoder, data, off + n) == WPD_OK);
        for (;;) {
            WPDFrameInfo entry = WPD_FRAME_INFO_INIT;
            WPDFrameInfo want  = WPD_FRAME_INFO_INIT;

            if (wpd_decoder_frame_info(decoder, available, &entry) != WPD_OK)
                break;
            saw_incomplete |= !entry.complete;
            if (entry.complete) {
                CHECK(wpd_decoder_frame_info(whole, available, &want) ==
                      WPD_OK);
                CHECK(frame_info_equal(&entry, &want));
            }
            available++;
        }
        CHECK(available >= seen);
        seen = available;
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(seen == info.frame_count);
    CHECK(saw_incomplete);
    wpd_decoder_free(decoder);
    wpd_decoder_free(whole);
    free(data);
}

static void test_anmf_subchunk_scan(int pad_chunks, const int *kinds,
                                    int nb_kinds) {
    uint8_t     *file   = malloc(ANMF_SUBCHUNKS_SIZE(pad_chunks, nb_kinds));
    WPDDecoder  *stream = wpd_decoder_create();
    WPDImageInfo info   = WPD_IMAGE_INFO_INIT;
    int          alph   = 0;
    size_t       size;

    CHECK(file && stream);
    if (!file || !stream) {
        free(file);
        wpd_decoder_free(stream);
        return;
    }
    size = make_anmf_subchunks(file, pad_chunks, kinds, nb_kinds);
    CHECK(size <= ANMF_SUBCHUNKS_SIZE(pad_chunks, nb_kinds));
    CHECK(wpd_decoder_open_stream(stream) == WPD_OK);

    for (size_t prefix = 1; prefix <= size; prefix++) {
        WPDDecoder *fresh = wpd_decoder_create();

        CHECK(wpd_decoder_update(stream, file, prefix) == WPD_OK);
        CHECK(fresh != NULL);
        if (!fresh)
            break;
        CHECK(wpd_decoder_open_stream(fresh) == WPD_OK);
        CHECK(wpd_decoder_update(fresh, file, prefix) == WPD_OK);
        for (int i = 0;; i++) {
            WPDFrameInfo    a  = WPD_FRAME_INFO_INIT;
            WPDFrameInfo    b  = WPD_FRAME_INFO_INIT;
            const WPDStatus sa = wpd_decoder_frame_info(stream, i, &a);
            const WPDStatus sb = wpd_decoder_frame_info(fresh, i, &b);

            CHECK(sa == sb);
            if (sa != WPD_OK || sb != WPD_OK)
                break;
            CHECK(frame_info_equal(&a, &b));
        }
        wpd_decoder_free(fresh);
    }

    CHECK(wpd_decoder_end_of_stream(stream) == WPD_OK);
    CHECK(wpd_decoder_get_info(stream, &info) == WPD_OK);
    CHECK(info.frame_count == nb_kinds);
    for (int i = 0; i < nb_kinds; i++) {
        WPDFrameInfo entry = WPD_FRAME_INFO_INIT;

        CHECK(wpd_decoder_frame_info(stream, i, &entry) == WPD_OK);
        CHECK(entry.complete == 1);
        CHECK(entry.has_alpha == (kinds[i] != ANMF_SUB_VP8L_OPAQUE));
        alph |= kinds[i] == ANMF_SUB_ALPH;
    }
    if (!alph)
        for (int i = 0; i < nb_kinds; i++) {
            WPDFrameInfo entry = WPD_FRAME_INFO_INIT;
            WPDFrame     frame = WPD_FRAME_INIT;

            CHECK(wpd_decoder_frame_info(stream, i, &entry) == WPD_OK);
            CHECK(wpd_decoder_next_frame(stream, &frame) > 0);
            CHECK(frame.has_alpha == entry.has_alpha);
        }
    wpd_decoder_free(stream);
    free(file);
}

static void test_anmf_subchunk_scan_stress(void) {
    static const int kind       = ANMF_SUB_VP8L_ALPHA;
    const int        pad_chunks = 80000;
    uint8_t         *file       = malloc(ANMF_SUBCHUNKS_SIZE(pad_chunks, 1));
    WPDDecoder      *decoder    = wpd_decoder_create();
    WPDFrameInfo     entry      = WPD_FRAME_INFO_INIT;
    WPDFrame         frame      = WPD_FRAME_INIT;
    size_t           size, fed = 0;

    CHECK(file && decoder);
    if (!file || !decoder) {
        free(file);
        wpd_decoder_free(decoder);
        return;
    }
    size = make_anmf_subchunks(file, pad_chunks, &kind, 1);
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    while (fed < size && wpd_decoder_append(decoder, file + fed, 1) == WPD_OK)
        fed++;
    CHECK(fed == size);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 1);
    CHECK(entry.has_alpha == 1);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);
    CHECK(frame.has_alpha == entry.has_alpha);
    wpd_decoder_free(decoder);
    free(file);
}

/* An odd-sized ANMF stays partial until its uncounted pad byte arrives. */
static void test_frame_table_odd_anmf(void) {
    uint8_t      file[128];
    const size_t size    = make_odd_anmf(file);
    WPDDecoder  *decoder = wpd_decoder_create();
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDFrame     frame   = WPD_FRAME_INIT;
    int          frames  = 0;

    CHECK(decoder != NULL);
    if (!decoder)
        return;

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    for (size_t prefix = 1; prefix <= size; prefix++) {
        WPDFrameInfo entry = WPD_FRAME_INFO_INIT;

        CHECK(wpd_decoder_update(decoder, file, prefix) == WPD_OK);
        CHECK(wpd_decoder_frame_info(decoder, 1, &entry) != WPD_OK);
        if (wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK)
            CHECK(entry.complete == (prefix >= size));
    }
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(info.frame_count == 1);
    while (wpd_decoder_next_frame(decoder, &frame) > 0) frames++;
    CHECK(frames == 1);
    wpd_decoder_free(decoder);
}

static void test_frame_table_declared_alpha(void) {
    uint8_t      file[64];
    uint8_t      image[8];
    size_t       size    = make_vp8x(file, 8, 8, 0x10);
    WPDDecoder  *decoder = wpd_decoder_create();
    WPDImageInfo info    = WPD_IMAGE_INFO_INIT;
    WPDFrameInfo entry   = WPD_FRAME_INFO_INIT;
    WPDFrame     frame   = WPD_FRAME_INIT;

    CHECK(decoder != NULL);
    if (!decoder)
        return;
    size += put_chunk(
        file + size, "VP8L", image, make_vp8l_blank(image, 8, 8, 0));
    put32(file + 4, (uint32_t)size - 8);

    CHECK(wpd_decoder_open(decoder, file, size) == WPD_OK);
    CHECK(wpd_decoder_get_info(decoder, &info) == WPD_OK);
    CHECK(info.has_alpha == 1);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.has_alpha == 0);
    CHECK(entry.width == 8 && entry.height == 8);
    CHECK(entry.complete == 1);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);
    CHECK(frame.has_alpha == entry.has_alpha);
    wpd_decoder_free(decoder);
}

static void test_frame_table_still_completion(void) {
    uint8_t      raw[10], file[80], metadata[8] = {0};
    const size_t raw_size = make_vp8l_blank(raw, 8, 8, 0);
    WPDDecoder  *decoder  = wpd_decoder_create();
    WPDFrameInfo entry    = WPD_FRAME_INFO_INIT;
    WPDFrame     frame    = WPD_FRAME_INIT;
    size_t       image_end, size;

    CHECK(decoder != NULL);
    if (!decoder)
        return;

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_update(decoder, raw, 5) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 0);
    CHECK(wpd_decoder_update(decoder, raw, raw_size) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 0);
    CHECK(wpd_decoder_next_frame(decoder, &frame) == 0);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 1);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);

    memset(raw, 0, sizeof(raw));
    raw[0] = 0x10;
    raw[3] = 0x9d;
    raw[4] = 0x01;
    raw[5] = 0x2a;
    raw[6] = raw[8] = 8;
    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_update(decoder, raw, sizeof(raw)) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 0);
    CHECK(wpd_decoder_end_of_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 1);

    make_vp8l_blank(raw, 8, 8, 0);
    size = make_vp8x(file, 8, 8, 0x08);
    size += put_chunk(file + size, "VP8L", raw, raw_size);
    image_end = size;
    size += put_chunk(file + size, "EXIF", metadata, sizeof(metadata));
    put32(file + 4, (uint32_t)size - 8);

    CHECK(wpd_decoder_open_stream(decoder) == WPD_OK);
    CHECK(wpd_decoder_update(decoder, file, image_end) == WPD_OK);
    CHECK(wpd_decoder_frame_info(decoder, 0, &entry) == WPD_OK);
    CHECK(entry.complete == 1);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);
    wpd_decoder_free(decoder);
}

static void test_animation_mode_switch(const char *path) {
    size_t      size;
    uint8_t    *data    = read_file(path, &size);
    WPDDecoder *decoder = wpd_decoder_create();
    WPDFrame    frame   = WPD_FRAME_INIT;

    if (!data || !decoder) {
        free(data);
        wpd_decoder_free(decoder);
        return;
    }
    CHECK(wpd_decoder_open_borrowed(decoder, data, size) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_COMPOSITED) ==
          WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) ==
          WPD_ERR_INVALID_ARG);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_COMPOSITED) ==
          WPD_OK);
    CHECK(wpd_decoder_rewind(decoder) == WPD_OK);
    CHECK(wpd_decoder_set_animation_mode(decoder, WPD_ANIM_SUBFRAME) == WPD_OK);
    CHECK(wpd_decoder_next_frame(decoder, &frame) > 0);
    wpd_decoder_free(decoder);
    free(data);
}

int main(int argc, char **argv) {
    test_version();
    test_status_strings();
    test_get_info();
    test_log_callback();
    test_decoder_errors();
    test_metadata();
    test_structs_and_limits();
    test_subframe_rejects_transforms();
    test_frame_table_odd_anmf();
    test_frame_table_declared_alpha();
    test_frame_table_still_completion();
    {
        static const int mixed[]        = {ANMF_SUB_VP8L_ALPHA,
                                           ANMF_SUB_VP8L_OPAQUE,
                                           ANMF_SUB_ALPH,
                                           ANMF_SUB_VP8L_ALPHA};
        static const int opaque_first[] = {ANMF_SUB_VP8L_OPAQUE,
                                           ANMF_SUB_VP8L_ALPHA};
        static const int lone[]         = {ANMF_SUB_VP8L_ALPHA};

        test_anmf_subchunk_scan(64, mixed, 4);
        test_anmf_subchunk_scan(64, opaque_first, 2);
        test_anmf_subchunk_scan(0, opaque_first, 2);
        test_anmf_subchunk_scan(1, lone, 1);
    }
    test_anmf_subchunk_scan_stress();

    if (argc > 1) {
        const char *dir = argv[1];
        char        path[4096];

        wpd_set_log_callback(NULL, NULL);

        snprintf(path, sizeof(path), "%s/lossless.webp", dir);
        test_file_info(path, 576, 576, 0, 0, 1, WPD_CODING_LOSSLESS);
        test_flip_reverses_rows(path, WPD_PIX_FMT_RGBA);
        test_replacement_api_file(path, "VP8L", 0);
        test_output_buffer(path, WPD_PIX_FMT_NONE);
        test_output_buffer(path, WPD_PIX_FMT_RGBA);
        test_output_buffer(path, WPD_PIX_FMT_BGR);
        test_rewind(path, WPD_PIX_FMT_RGBA, WPD_ANIM_COMPOSITED);
        test_frame_table(path);

        snprintf(path, sizeof(path), "%s/a_lossy.webp", dir);
        test_file_info(path, 600, 600, 1, 0, 1, WPD_CODING_LOSSY);
        test_replacement_api_file(path, "VP8 ", 1);
        test_output_buffer(path, WPD_PIX_FMT_BGRA);
        test_output_buffer(path, WPD_PIX_FMT_ARGB_PRE);
        test_output_buffer_incomplete_yuv(path);
        test_rewind(path, WPD_PIX_FMT_BGRA, WPD_ANIM_COMPOSITED);
        test_frame_table(path);

        snprintf(path, sizeof(path), "%s/anim_yuva.webp", dir);
        test_file_info(path, 422, 480, 1, 1, 14, WPD_CODING_UNKNOWN);
        test_flip_reverses_rows(path, WPD_PIX_FMT_ARGB_PRE);
        test_flip_reverses_rows(path, WPD_PIX_FMT_RGBA);
        test_replacement_animation(path);
        test_subframe_composite(path);
        test_subframe_4444_premultiply(path);
        test_composited_4444_premultiply(path);
        test_rewind(path, WPD_PIX_FMT_BGRA, WPD_ANIM_COMPOSITED);
        test_rewind(path, WPD_PIX_FMT_ARGB_PRE, WPD_ANIM_SUBFRAME);
        test_rewind_errors(path);
        test_animation_mode_switch(path);
        test_frame_table(path);
        test_frame_table_stream(path, 997, 0);
        test_frame_table_stream(path, 251, 1);
        test_scaled_premultiply(path);
        test_scaled_premultiply_identity(path);
        test_animation_format_switch(path);
        test_output_buffer(path, WPD_PIX_FMT_RGBA);
        test_output_buffer(path, WPD_PIX_FMT_ARGB);
        test_stream(path, WPD_PIX_FMT_RGBA, 1024, 1);
        test_stream(path, WPD_PIX_FMT_ARGB, 64, 1);
        test_stream(path, WPD_PIX_FMT_ARGB, 1, 1);
        test_stream_errors(path);

        snprintf(path, sizeof(path), "%s/lossless.webp", dir);
        test_stream(path, WPD_PIX_FMT_RGBA, 4096, 0);
        snprintf(path, sizeof(path), "%s/lossy.webp", dir);
        test_replacement_api_file(path, "VP8 ", 0);
        snprintf(path, sizeof(path), "%s/a_lossy.webp", dir);
        test_stream(path, WPD_PIX_FMT_BGRA, 512, 0);
        test_stream(path, WPD_PIX_FMT_NONE, 512, 0);
        snprintf(path, sizeof(path), "%s/mixed_codecs.webp", dir);
        test_stream(path, WPD_PIX_FMT_ARGB, 256, 1);
        test_scaled_premultiply(path);
        test_scaled_premultiply_identity(path);
        test_animation_format_switch(path);
        test_subframe_composite(path);
        test_subframe_4444_premultiply(path);
        test_composited_4444_premultiply(path);
        test_frame_table(path);
        snprintf(path, sizeof(path), "%s/transparent_over.webp", dir);
        test_scaled_premultiply(path);
        test_scaled_premultiply_identity(path);
        test_animation_format_switch(path);
        test_subframe_composite(path);
        test_subframe_4444_premultiply(path);
        test_composited_4444_premultiply(path);
        snprintf(path, sizeof(path), "%s/kitchen_sink.webp", dir);
        test_scaled_premultiply(path);
        test_scaled_premultiply_identity(path);
        test_animation_format_switch(path);
        test_subframe_composite(path);
        test_subframe_4444_premultiply(path);
        test_composited_4444_premultiply(path);
        test_frame_table(path);
        test_rewind(path, WPD_PIX_FMT_RGBA, WPD_ANIM_SUBFRAME);
        test_threads_match(path, WPD_PIX_FMT_RGBA);

        {
            snprintf(path, sizeof(path), "%s/lossy.webp", dir);
            test_threads_match(path, WPD_PIX_FMT_RGBA);
            test_threads_match(path, WPD_PIX_FMT_RGB565);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_BGR, WPD_PIX_FMT_RGBA, 0);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_BGR, WPD_PIX_FMT_RGBA, 1);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_BGRA_PRE, 1);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_RGBA, 1);
            test_partial_format_change(
                path, 337, WPD_PIX_FMT_ARGB_PRE, WPD_PIX_FMT_BGRA, 1);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_BGR, 4096, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 2048, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 3079, 1);
            test_partial_frame(path, WPD_PIX_FMT_RGB565, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGB565, 2053, 1);
            test_partial_buffer_change(path, 1021, WPD_PIX_FMT_RGBA, 0);
            test_partial_buffer_change(path, 2048, WPD_PIX_FMT_BGR, 0);

            {
                WPDDecoderOptions simple = WPD_DECODER_OPTIONS_INIT;

                simple.no_fancy_upsampling = 1;
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_RGBA, 1021, &simple, 0);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_RGBA, 1021, &simple, 1);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_RGBA4444_PRE, 2053, &simple, 0);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_NONE, 2048, &simple, 1);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_RGBA, 1021, NULL, 1);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_NONE, 1021, NULL, 1);
                test_partial_matches_whole(
                    path, WPD_PIX_FMT_YUVA420P, 337, NULL, 1);
            }

            snprintf(path, sizeof(path), "%s/simplelf-lossy.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_ARGB_PRE, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 2053, 0);

            snprintf(path, sizeof(path), "%s/a_lossy.webp", dir);
            test_threads_match(path, WPD_PIX_FMT_RGBA_PRE);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 337, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 337, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA4444, 337, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA4444_PRE, 211, 0);

            snprintf(path, sizeof(path), "%s/odd_a_lossy.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_BGRA, 211, 0);

            snprintf(path, sizeof(path), "%s/a_lossy.webp", dir);
            test_file_metadata(path, WPD_METADATA_EXIF, 68);

            snprintf(path, sizeof(path), "%s/lossless.webp", dir);
            test_file_metadata(path, 0, 0);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_BGR, WPD_PIX_FMT_RGBA, 0);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_BGR, WPD_PIX_FMT_RGBA, 1);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_BGRA_PRE, 1);
            test_partial_format_change(
                path, 1021, WPD_PIX_FMT_RGBA_PRE, WPD_PIX_FMT_RGBA, 1);
            test_partial_format_change(
                path, 337, WPD_PIX_FMT_ARGB_PRE, WPD_PIX_FMT_BGRA, 1);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 4096, 0);
            test_partial_frame(path, WPD_PIX_FMT_BGR, 2048, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 3079, 1);
            test_partial_frame(path, WPD_PIX_FMT_RGBA4444, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA4444_PRE, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_RGBA4444_PRE, 2053, 1);
            test_partial_matches_whole(
                path, WPD_PIX_FMT_RGBA4444_PRE, 1021, NULL, 0);
            test_partial_matches_whole(
                path, WPD_PIX_FMT_YUV420P, 1021, NULL, 0);
            test_partial_matches_whole(
                path, WPD_PIX_FMT_YUVA420P, 337, NULL, 1);

            snprintf(path, sizeof(path), "%s/palette2bpp_rgb.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_ARGB_PRE, 331, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 211, 0);
            test_partial_matches_whole(path, WPD_PIX_FMT_YUV420P, 211, NULL, 0);

            snprintf(path, sizeof(path), "%s/palette_rgb.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_BGRA, 53, 0);

            snprintf(path, sizeof(path), "%s/predict_topright.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_RGBA, 1021, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 337, 0);
            test_partial_matches_whole(path, WPD_PIX_FMT_RGBA, 1021, NULL, 0);
            test_partial_matches_whole(path, WPD_PIX_FMT_RGBA, 337, NULL, 1);
            test_partial_matches_whole(
                path, WPD_PIX_FMT_YUVA420P, 1021, NULL, 0);

            snprintf(
                path, sizeof(path), "%s/transforms_before_palette.webp", dir);
            test_partial_frame(path, WPD_PIX_FMT_ARGB, 13, 0);
            test_partial_frame(path, WPD_PIX_FMT_NONE, 71, 0);
            test_partial_matches_whole(path, WPD_PIX_FMT_RGBA, 13, NULL, 0);
            test_partial_matches_whole(path, WPD_PIX_FMT_RGBA, 71, NULL, 1);

            snprintf(path, sizeof(path), "%s/lossy.webp", dir);
            test_info_truncated_still(path);
            snprintf(path, sizeof(path), "%s/lossless.webp", dir);
            test_info_truncated_still(path);

            snprintf(path, sizeof(path), "%s/anim_rgb.webp", dir);
            test_partial_buffer_change(path, 61, WPD_PIX_FMT_RGBA_PRE, 1);
            test_partial_format_change_alpha(path, 61);
            test_partial_format_change_alpha(path, 256);
            snprintf(path, sizeof(path), "%s/dispose_bg_fullframe.webp", dir);
            test_scale_keeps_source(path, WPD_PIX_FMT_BGRA, 51, 37, 1);
            test_scale_keeps_source(path, WPD_PIX_FMT_ARGB_PRE, 300, 220, 1);
            snprintf(path, sizeof(path), "%s/overlap_exact.webp", dir);
            test_partial_format_change_alpha(path, 8);
            test_partial_format_change_alpha(path, 41);

            snprintf(path, sizeof(path), "%s/a_lossy.webp", dir);
            test_scale_keeps_source(path, WPD_PIX_FMT_YUVA420P, 300, 300, 0);
            test_scale_keeps_source(path, WPD_PIX_FMT_BGR, 700, 200, 0);

            {
                static const char *const anims[] = {
                    "dispose_bg_blend",
                    "dispose_bg_fullframe",
                    "dispose_bg_noblend",
                    "dispose_none_blend",
                    "dispose_none_noblend",
                    "durations",
                    "edge_frames",
                    "keyframe_midstream",
                    "odd_canvas",
                    "odd_frames",
                    "overlap_bottom",
                    "overlap_contains",
                    "overlap_corner",
                    "overlap_disjoint",
                    "overlap_exact",
                    "overlap_inside",
                    "overlap_left",
                    "overlap_odd",
                    "overlap_right",
                    "overlap_single",
                    "overlap_top",
                    "anim_rgb",
                    "anim_yuv",
                };

                for (size_t i = 0; i < sizeof(anims) / sizeof(*anims); i++) {
                    snprintf(path, sizeof(path), "%s/%s.webp", dir, anims[i]);
                    test_subframe_composite(path);
                    test_subframe_4444_premultiply(path);
                    test_composited_4444_premultiply(path);
                    test_frame_table(path);
                }
            }
        }
    }

    if (failures)
        fprintf(stderr, "%d check(s) failed\n", failures);
    return failures ? 1 : 0;
}
