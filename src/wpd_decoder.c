/*
 * WebP decoder: RIFF container, lossless (VP8L), alpha and animation
 * support ported from FFmpeg's libavcodec/webp.c; lossy bitstreams are
 * handled by the VP8 decoder in vp8.c.
 */

#include "wpd.h"

#include "wpd_codec.h"
#include "vp8.h"

#include <stdio.h>
#include <stdlib.h>

#define VP8X_FLAG_ANIMATION             0x02
#define VP8X_FLAG_ALPHA                 0x10

#define ANMF_FLAG_DISPOSE               (1 << 0)
#define ANMF_FLAG_NO_BLEND              (1 << 1)

#define NUM_CODE_LENGTH_CODES           19
#define HUFFMAN_CODES_PER_META_CODE     5
#define NUM_LITERAL_CODES               256
#define NUM_LENGTH_CODES                24
#define NUM_DISTANCE_CODES              40
#define NUM_SHORT_DISTANCES             120
#define MAX_HUFFMAN_CODE_LENGTH         15

#define WPD_FILE_PADDING                64

#define MKTAG(a, b, c, d) ((uint32_t)(a) | (uint32_t)(b) << 8 | \
                           (uint32_t)(c) << 16 | (uint32_t)(d) << 24)

static const uint16_t alphabet_sizes[HUFFMAN_CODES_PER_META_CODE] = {
    NUM_LITERAL_CODES + NUM_LENGTH_CODES,
    NUM_LITERAL_CODES, NUM_LITERAL_CODES, NUM_LITERAL_CODES,
    NUM_DISTANCE_CODES
};

static const uint8_t code_length_code_order[NUM_CODE_LENGTH_CODES] = {
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
};

static const int8_t lz77_distance_offsets[NUM_SHORT_DISTANCES][2] = {
    {  0, 1 }, {  1, 0 }, {  1, 1 }, { -1, 1 }, {  0, 2 }, {  2, 0 }, {  1, 2 }, { -1, 2 },
    {  2, 1 }, { -2, 1 }, {  2, 2 }, { -2, 2 }, {  0, 3 }, {  3, 0 }, {  1, 3 }, { -1, 3 },
    {  3, 1 }, { -3, 1 }, {  2, 3 }, { -2, 3 }, {  3, 2 }, { -3, 2 }, {  0, 4 }, {  4, 0 },
    {  1, 4 }, { -1, 4 }, {  4, 1 }, { -4, 1 }, {  3, 3 }, { -3, 3 }, {  2, 4 }, { -2, 4 },
    {  4, 2 }, { -4, 2 }, {  0, 5 }, {  3, 4 }, { -3, 4 }, {  4, 3 }, { -4, 3 }, {  5, 0 },
    {  1, 5 }, { -1, 5 }, {  5, 1 }, { -5, 1 }, {  2, 5 }, { -2, 5 }, {  5, 2 }, { -5, 2 },
    {  4, 4 }, { -4, 4 }, {  3, 5 }, { -3, 5 }, {  5, 3 }, { -5, 3 }, {  0, 6 }, {  6, 0 },
    {  1, 6 }, { -1, 6 }, {  6, 1 }, { -6, 1 }, {  2, 6 }, { -2, 6 }, {  6, 2 }, { -6, 2 },
    {  4, 5 }, { -4, 5 }, {  5, 4 }, { -5, 4 }, {  3, 6 }, { -3, 6 }, {  6, 3 }, { -6, 3 },
    {  0, 7 }, {  7, 0 }, {  1, 7 }, { -1, 7 }, {  5, 5 }, { -5, 5 }, {  7, 1 }, { -7, 1 },
    {  4, 6 }, { -4, 6 }, {  6, 4 }, { -6, 4 }, {  2, 7 }, { -2, 7 }, {  7, 2 }, { -7, 2 },
    {  3, 7 }, { -3, 7 }, {  7, 3 }, { -7, 3 }, {  5, 6 }, { -5, 6 }, {  6, 5 }, { -6, 5 },
    {  8, 0 }, {  4, 7 }, { -4, 7 }, {  7, 4 }, { -7, 4 }, {  8, 1 }, {  8, 2 }, {  6, 6 },
    { -6, 6 }, {  8, 3 }, {  5, 7 }, { -5, 7 }, {  7, 5 }, { -7, 5 }, {  8, 4 }, {  6, 7 },
    { -6, 7 }, {  7, 6 }, { -7, 6 }, {  8, 5 }, {  7, 7 }, { -7, 7 }, {  8, 6 }, {  8, 7 }
};

enum AlphaCompression {
    ALPHA_COMPRESSION_NONE,
    ALPHA_COMPRESSION_VP8L,
};

enum AlphaFilter {
    ALPHA_FILTER_NONE,
    ALPHA_FILTER_HORIZONTAL,
    ALPHA_FILTER_VERTICAL,
    ALPHA_FILTER_GRADIENT,
};

enum TransformType {
    PREDICTOR_TRANSFORM      = 0,
    COLOR_TRANSFORM          = 1,
    SUBTRACT_GREEN           = 2,
    COLOR_INDEXING_TRANSFORM = 3,
};

enum PredictionMode {
    PRED_MODE_BLACK,
    PRED_MODE_L,
    PRED_MODE_T,
    PRED_MODE_TR,
    PRED_MODE_TL,
    PRED_MODE_AVG_T_AVG_L_TR,
    PRED_MODE_AVG_L_TL,
    PRED_MODE_AVG_L_T,
    PRED_MODE_AVG_TL_T,
    PRED_MODE_AVG_T_TR,
    PRED_MODE_AVG_AVG_L_TL_AVG_T_TR,
    PRED_MODE_SELECT,
    PRED_MODE_ADD_SUBTRACT_FULL,
    PRED_MODE_ADD_SUBTRACT_HALF,
};

enum HuffmanIndex {
    HUFF_IDX_GREEN = 0,
    HUFF_IDX_RED   = 1,
    HUFF_IDX_BLUE  = 2,
    HUFF_IDX_ALPHA = 3,
    HUFF_IDX_DIST  = 4
};

enum ImageRole {
    IMAGE_ROLE_ARGB,
    IMAGE_ROLE_ENTROPY,
    IMAGE_ROLE_PREDICTOR,
    IMAGE_ROLE_COLOR_TRANSFORM,
    IMAGE_ROLE_COLOR_INDEXING,
    IMAGE_ROLE_NB,
};

/* Fixed-point YUV<->RGB (CCIR range), as in FFmpeg's colorspace.h. */
#define SCALEBITS 10
#define ONE_HALF  (1 << (SCALEBITS - 1))
#define FIX(x)    ((int)((x) * (1 << SCALEBITS) + 0.5))

#define YUV_TO_RGB1_CCIR(cb1, cr1) do {                                     \
    cb = (cb1) - 128;                                                       \
    cr = (cr1) - 128;                                                       \
    r_add = FIX(1.40200*255.0/224.0) * cr + ONE_HALF;                       \
    g_add = - FIX(0.34414*255.0/224.0) * cb - FIX(0.71414*255.0/224.0) * cr \
            + ONE_HALF;                                                     \
    b_add = FIX(1.77200*255.0/224.0) * cb + ONE_HALF;                       \
} while (0)

#define YUV_TO_RGB2_CCIR(r, g, b, y1) do {                                  \
    y = ((y1) - 16) * FIX(255.0/219.0);                                     \
    r = cm[(y + r_add) >> SCALEBITS];                                       \
    g = cm[(y + g_add) >> SCALEBITS];                                       \
    b = cm[(y + b_add) >> SCALEBITS];                                       \
} while (0)

#define RGB_TO_Y_CCIR(r, g, b) \
    ((FIX(0.29900*219.0/255.0) * (r) + FIX(0.58700*219.0/255.0) * (g) + \
      FIX(0.11400*219.0/255.0) * (b) + (ONE_HALF + (16 << SCALEBITS))) >> SCALEBITS)

#define RGB_TO_U_CCIR(r1, g1, b1, shift) \
    (((- FIX(0.16874*224.0/255.0) * r1 - FIX(0.33126*224.0/255.0) * g1 + \
       FIX(0.50000*224.0/255.0) * b1 + (ONE_HALF << shift) - 1) >> (SCALEBITS + shift)) + 128)

#define RGB_TO_V_CCIR(r1, g1, b1, shift) \
    (((FIX(0.50000*224.0/255.0) * r1 - FIX(0.41869*224.0/255.0) * g1 - \
       FIX(0.08131*224.0/255.0) * b1 + (ONE_HALF << shift) - 1) >> (SCALEBITS + shift)) + 128)

static wpd_always_inline uint32_t rb32(const uint8_t *p)
{
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 |
           (uint32_t)p[2] << 8 | p[3];
}

static wpd_always_inline void wb32(uint8_t *p, uint32_t v)
{
    p[0] = v >> 24; p[1] = v >> 16; p[2] = v >> 8; p[3] = v;
}

static wpd_always_inline void copy32(uint8_t *dst, const uint8_t *src)
{
    memcpy(dst, src, 4);
}

static wpd_always_inline int u8_to_s8(uint8_t v)
{
    return (int8_t)v;
}

#define CEIL_RSHIFT(v, s) (-((-(v)) >> (s)))

/*
 * Least-significant-bit-first bit reader, the bit order used by the VP8L
 * bitstream. Reads past the end return zero bits and drive bits_left
 * negative, which callers check.
 */
typedef struct LEBitReader {
    const uint8_t *buf;
    size_t size_bits;
    size_t index;
} LEBitReader;

static void br_init(LEBitReader *br, const uint8_t *buf, size_t size)
{
    br->buf = buf;
    br->size_bits = size * 8;
    br->index = 0;
}

static wpd_always_inline unsigned br_bit(LEBitReader *br)
{
    unsigned bit = 0;
    if (br->index < br->size_bits)
        bit = br->buf[br->index >> 3] >> (br->index & 7) & 1;
    br->index++;
    return bit;
}

static wpd_always_inline unsigned br_bits(LEBitReader *br, int n)
{
    unsigned v = 0;
    for (int i = 0; i < n; i++)
        v |= br_bit(br) << i;
    return v;
}

static wpd_always_inline void br_skip(LEBitReader *br, int n)
{
    br->index += n;
}

static wpd_always_inline ptrdiff_t br_bits_left(const LEBitReader *br)
{
    return (ptrdiff_t)br->size_bits - (ptrdiff_t)br->index;
}

/*
 * A decoded image. ARGB images use a single packed plane in data[0];
 * planar images use Y/U/V/A in data[0..3]. Planes in alloc[] are owned,
 * others (the lossy path borrows the VP8 decoder's planes) are not.
 */
typedef struct WebPImage {
    uint8_t *data[4];
    uint8_t *alloc[4];
    int linesize[4];
    int width, height;
    WPDPixelFormat format;
} WebPImage;

static void image_free(WebPImage *img)
{
    for (int p = 0; p < 4; p++)
        wpd_free(img->alloc[p]);
    memset(img, 0, sizeof(*img));
}

static int image_alloc_argb(WebPImage *img, int w, int h)
{
    image_free(img);
    img->linesize[0] = w * 4;
    img->alloc[0] = wpd_mallocz((size_t)img->linesize[0] * h + WPD_FILE_PADDING);
    if (!img->alloc[0])
        return WPD_ERROR(ENOMEM);
    img->data[0] = img->alloc[0];
    img->width  = w;
    img->height = h;
    img->format = WPD_PIX_FMT_ARGB;
    return 0;
}

static int image_alloc_yuva(WebPImage *img, int w, int h)
{
    image_free(img);
    for (int p = 0; p < 4; p++) {
        int pw = (p == 1 || p == 2) ? (w + 1) / 2 : w;
        int ph = (p == 1 || p == 2) ? (h + 1) / 2 : h;
        img->linesize[p] = pw;
        img->alloc[p] = wpd_mallocz((size_t)pw * ph + WPD_FILE_PADDING);
        if (!img->alloc[p]) {
            image_free(img);
            return WPD_ERROR(ENOMEM);
        }
        img->data[p] = img->alloc[p];
    }
    img->width  = w;
    img->height = h;
    img->format = WPD_PIX_FMT_YUVA420P;
    return 0;
}

#define GET_PIXEL(img, x, y) \
    ((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x))

#define GET_PIXEL_COMP(img, x, y, c) \
    (*((img)->data[0] + (y) * (img)->linesize[0] + 4 * (x) + (c)))

/*
 * Canonical Huffman reader. Codes are read bit by bit, first bit is the
 * most significant bit of the code, as in DEFLATE.
 */
typedef struct HuffReader {
    int simple;
    int nb_symbols;
    uint16_t simple_symbols[2];
    uint16_t *syms;                                 /* sorted by (length, symbol) */
    int first_code[MAX_HUFFMAN_CODE_LENGTH + 1];
    int sym_offset[MAX_HUFFMAN_CODE_LENGTH + 1];
    int count[MAX_HUFFMAN_CODE_LENGTH + 1];
} HuffReader;

static void huff_reader_free(HuffReader *r)
{
    wpd_freep(&r->syms);
}

static int huff_reader_build_canonical(HuffReader *r, const uint8_t *code_lengths,
                                       int alphabet_size)
{
    unsigned nb_codes = 0;
    int code = 0, offset = 0;

    memset(r->count, 0, sizeof(r->count));
    for (int sym = 0; sym < alphabet_size; sym++)
        if (code_lengths[sym])
            r->count[code_lengths[sym]]++;
    for (int len = 1; len <= MAX_HUFFMAN_CODE_LENGTH; len++)
        nb_codes += r->count[len];

    if (nb_codes == 0)
        return WPD_ERROR_INVALID_DATA;

    if (nb_codes == 1) {
        for (int sym = 0; sym < alphabet_size; sym++) {
            if (code_lengths[sym]) {
                r->nb_symbols = 1;
                r->simple = 1;
                r->simple_symbols[0] = sym;
                return 0;
            }
        }
    }

    r->syms = malloc(nb_codes * sizeof(*r->syms));
    if (!r->syms)
        return WPD_ERROR(ENOMEM);

    for (int len = 1; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        r->first_code[len] = code;
        r->sym_offset[len] = offset;
        code = (code + r->count[len]) << 1;
        offset += r->count[len];
    }

    memset(r->count, 0, sizeof(r->count));
    for (int sym = 0; sym < alphabet_size; sym++) {
        int len = code_lengths[sym];
        if (len)
            r->syms[r->sym_offset[len] + r->count[len]++] = sym;
    }

    r->simple = 0;
    return 0;
}

static int huff_reader_get_symbol(HuffReader *r, LEBitReader *br)
{
    int code = 0;

    if (r->simple) {
        if (r->nb_symbols == 1)
            return r->simple_symbols[0];
        return r->simple_symbols[br_bit(br)];
    }
    for (int len = 1; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        int idx;
        code = code << 1 | br_bit(br);
        idx = code - r->first_code[len];
        if (idx >= 0 && idx < r->count[len])
            return r->syms[r->sym_offset[len] + idx];
    }
    return WPD_ERROR_INVALID_DATA;
}

typedef struct ImageContext {
    enum ImageRole role;
    WebPImage *frame;                   /* target image */
    WebPImage storage;                  /* backing store for non-primary roles */
    int color_cache_bits;
    uint32_t *color_cache;
    int nb_huffman_groups;
    HuffReader *huffman_groups;
    int size_reduction;
    int is_alpha_primary;
} ImageContext;

struct WPDDecoder {
    WpdCodecContext codec;
    VP8Context vp8;
    int vp8_initialized;

    uint8_t *file;                      /* padded copy of the input */
    size_t file_size;
    size_t pos, end;                    /* chunk walk over the RIFF payload */
    int animation;
    int still_done;
    int frame_index;
    int canvas_width, canvas_height;

    /* alpha chunk state, for the following lossy bitstream */
    int has_alpha;
    enum AlphaCompression alpha_compression;
    enum AlphaFilter alpha_filter;
    const uint8_t *alpha_data;
    int alpha_data_size;
    uint8_t *alpha_plane;
    size_t alpha_plane_size;

    /* lossless decoder state */
    LEBitReader gb;
    int width, height;                  /* dimensions of the current subimage */
    int lossless_has_alpha;
    int nb_transforms;
    enum TransformType transforms[4];
    int reduced_width;
    int nb_huffman_groups;
    ImageContext image[IMAGE_ROLE_NB];

    WebPImage argb;                     /* lossless output image */
    WebPImage alpha_argb;               /* lossless-coded alpha channel */
    WebPImage subframe;                 /* current subframe (may borrow planes) */

    /* animation state */
    WebPImage canvas;
    int anmf_flags, pos_x, pos_y;
    int prev_anmf_flags, prev_width, prev_height, prev_pos_x, prev_pos_y;
    uint8_t background_argb[4];
    uint8_t background_yuva[4];

    char error[128];
};

static void image_ctx_free(ImageContext *img)
{
    wpd_free(img->color_cache);
    if (img->role != IMAGE_ROLE_ARGB)
        image_free(&img->storage);
    if (img->huffman_groups) {
        for (int i = 0; i < img->nb_huffman_groups; i++)
            for (int j = 0; j < HUFFMAN_CODES_PER_META_CODE; j++)
                huff_reader_free(&img->huffman_groups[i * HUFFMAN_CODES_PER_META_CODE + j]);
        wpd_free(img->huffman_groups);
    }
    memset(img, 0, sizeof(*img));
}

static void read_huffman_code_simple(WPDDecoder *s, HuffReader *hc)
{
    hc->nb_symbols = br_bit(&s->gb) + 1;

    if (br_bit(&s->gb))
        hc->simple_symbols[0] = br_bits(&s->gb, 8);
    else
        hc->simple_symbols[0] = br_bit(&s->gb);

    if (hc->nb_symbols == 2)
        hc->simple_symbols[1] = br_bits(&s->gb, 8);

    hc->simple = 1;
}

static int read_huffman_code_normal(WPDDecoder *s, HuffReader *hc,
                                    int alphabet_size)
{
    HuffReader code_len_hc = { 0 };
    uint8_t *code_lengths;
    uint8_t code_length_code_lengths[NUM_CODE_LENGTH_CODES] = { 0 };
    int symbol, max_symbol, prev_code_len, ret;
    int num_codes = 4 + br_bits(&s->gb, 4);

    for (int i = 0; i < num_codes; i++)
        code_length_code_lengths[code_length_code_order[i]] = br_bits(&s->gb, 3);

    if (br_bit(&s->gb)) {
        int bits   = 2 + 2 * br_bits(&s->gb, 3);
        max_symbol = 2 + br_bits(&s->gb, bits);
        if (max_symbol > alphabet_size) {
            wpd_log(NULL, WPD_LOG_ERROR, "max symbol %d > alphabet size %d\n",
                    max_symbol, alphabet_size);
            return WPD_ERROR_INVALID_DATA;
        }
    } else {
        max_symbol = alphabet_size;
    }

    ret = huff_reader_build_canonical(&code_len_hc, code_length_code_lengths,
                                      NUM_CODE_LENGTH_CODES);
    if (ret < 0)
        return ret;

    code_lengths = calloc(alphabet_size, 1);
    if (!code_lengths) {
        ret = WPD_ERROR(ENOMEM);
        goto finish;
    }

    prev_code_len = 8;
    symbol        = 0;
    while (symbol < alphabet_size) {
        int code_len;

        if (!max_symbol--)
            break;
        code_len = huff_reader_get_symbol(&code_len_hc, &s->gb);
        if (code_len < 0) {
            ret = WPD_ERROR_INVALID_DATA;
            goto finish;
        }
        if (code_len < 16) {
            /* Code length code [0..15] indicates literal code lengths. */
            code_lengths[symbol++] = code_len;
            if (code_len)
                prev_code_len = code_len;
        } else {
            int repeat = 0, length = 0;
            switch (code_len) {
            default:
                ret = WPD_ERROR_INVALID_DATA;
                goto finish;
            case 16:
                /* Code 16 repeats the previous non-zero value [3..6] times. */
                repeat = 3 + br_bits(&s->gb, 2);
                length = prev_code_len;
                break;
            case 17:
                /* Code 17 emits a streak of zeros [3..10]. */
                repeat = 3 + br_bits(&s->gb, 3);
                break;
            case 18:
                /* Code 18 emits a streak of zeros of length [11..138]. */
                repeat = 11 + br_bits(&s->gb, 7);
                break;
            }
            if (symbol + repeat > alphabet_size) {
                wpd_log(NULL, WPD_LOG_ERROR,
                        "invalid symbol %d + repeat %d > alphabet size %d\n",
                        symbol, repeat, alphabet_size);
                ret = WPD_ERROR_INVALID_DATA;
                goto finish;
            }
            while (repeat-- > 0)
                code_lengths[symbol++] = length;
        }
    }

    ret = huff_reader_build_canonical(hc, code_lengths, symbol);

finish:
    huff_reader_free(&code_len_hc);
    free(code_lengths);
    return ret;
}

static int decode_entropy_coded_image(WPDDecoder *s, enum ImageRole role,
                                      int w, int h);

#define PARSE_BLOCK_SIZE(w, h) do {                                     \
    block_bits = br_bits(&s->gb, 3) + 2;                                \
    blocks_w   = ((w) + (1 << block_bits) - 1) >> block_bits;           \
    blocks_h   = ((h) + (1 << block_bits) - 1) >> block_bits;           \
} while (0)

static int decode_entropy_image(WPDDecoder *s)
{
    ImageContext *img;
    int ret, block_bits, blocks_w, blocks_h, x, y, max;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(s, IMAGE_ROLE_ENTROPY, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    img = &s->image[IMAGE_ROLE_ENTROPY];
    img->size_reduction = block_bits;

    /* the number of huffman groups is determined by the maximum group number
     * coded in the entropy image */
    max = 0;
    for (y = 0; y < img->frame->height; y++) {
        for (x = 0; x < img->frame->width; x++) {
            int p0 = GET_PIXEL_COMP(img->frame, x, y, 1);
            int p1 = GET_PIXEL_COMP(img->frame, x, y, 2);
            int p  = p0 << 8 | p1;
            max = WPD_MAX(max, p);
        }
    }
    s->nb_huffman_groups = max + 1;

    return 0;
}

static int parse_transform_predictor(WPDDecoder *s)
{
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(s, IMAGE_ROLE_PREDICTOR, blocks_w,
                                     blocks_h);
    if (ret < 0)
        return ret;

    s->image[IMAGE_ROLE_PREDICTOR].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color(WPDDecoder *s)
{
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(s, IMAGE_ROLE_COLOR_TRANSFORM, blocks_w,
                                     blocks_h);
    if (ret < 0)
        return ret;

    s->image[IMAGE_ROLE_COLOR_TRANSFORM].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color_indexing(WPDDecoder *s)
{
    ImageContext *img;
    int width_bits, index_size, ret, x;
    uint8_t *ct;

    index_size = br_bits(&s->gb, 8) + 1;

    if (index_size <= 2)
        width_bits = 3;
    else if (index_size <= 4)
        width_bits = 2;
    else if (index_size <= 16)
        width_bits = 1;
    else
        width_bits = 0;

    ret = decode_entropy_coded_image(s, IMAGE_ROLE_COLOR_INDEXING,
                                     index_size, 1);
    if (ret < 0)
        return ret;

    img = &s->image[IMAGE_ROLE_COLOR_INDEXING];
    img->size_reduction = width_bits;
    if (width_bits > 0)
        s->reduced_width = (s->width + ((1 << width_bits) - 1)) >> width_bits;

    /* color index values are delta-coded */
    ct = img->frame->data[0] + 4;
    for (x = 4; x < img->frame->width * 4; x++, ct++)
        ct[0] += ct[-4];

    return 0;
}

static HuffReader *get_huffman_group(WPDDecoder *s, ImageContext *img,
                                     int x, int y)
{
    ImageContext *gimg = &s->image[IMAGE_ROLE_ENTROPY];
    int group = 0;

    if (gimg->size_reduction > 0) {
        int group_x = x >> gimg->size_reduction;
        int group_y = y >> gimg->size_reduction;
        int g0      = GET_PIXEL_COMP(gimg->frame, group_x, group_y, 1);
        int g1      = GET_PIXEL_COMP(gimg->frame, group_x, group_y, 2);
        group       = g0 << 8 | g1;
    }

    return &img->huffman_groups[group * HUFFMAN_CODES_PER_META_CODE];
}

static wpd_always_inline void color_cache_put(ImageContext *img, uint32_t c)
{
    uint32_t cache_idx = (0x1E35A7BD * c) >> (32 - img->color_cache_bits);
    img->color_cache[cache_idx] = c;
}

static int decode_entropy_coded_image(WPDDecoder *s, enum ImageRole role,
                                      int w, int h)
{
    ImageContext *img;
    HuffReader *hg;
    int i, j, ret, x, y, width;

    img       = &s->image[role];
    img->role = role;

    if (!img->frame)
        img->frame = &img->storage;

    ret = image_alloc_argb(img->frame, w, h);
    if (ret < 0)
        return ret;

    if (br_bit(&s->gb)) {
        img->color_cache_bits = br_bits(&s->gb, 4);
        if (img->color_cache_bits < 1 || img->color_cache_bits > 11) {
            wpd_log(NULL, WPD_LOG_ERROR, "invalid color cache bits: %d\n",
                    img->color_cache_bits);
            return WPD_ERROR_INVALID_DATA;
        }
        img->color_cache = wpd_mallocz(((size_t)1 << img->color_cache_bits) *
                                       sizeof(*img->color_cache));
        if (!img->color_cache)
            return WPD_ERROR(ENOMEM);
    } else {
        img->color_cache_bits = 0;
    }

    img->nb_huffman_groups = 1;
    if (role == IMAGE_ROLE_ARGB && br_bit(&s->gb)) {
        ret = decode_entropy_image(s);
        if (ret < 0)
            return ret;
        img->nb_huffman_groups = s->nb_huffman_groups;
    }
    img->huffman_groups = wpd_mallocz((size_t)img->nb_huffman_groups *
                                      HUFFMAN_CODES_PER_META_CODE *
                                      sizeof(*img->huffman_groups));
    if (!img->huffman_groups)
        return WPD_ERROR(ENOMEM);

    for (i = 0; i < img->nb_huffman_groups; i++) {
        hg = &img->huffman_groups[i * HUFFMAN_CODES_PER_META_CODE];
        for (j = 0; j < HUFFMAN_CODES_PER_META_CODE; j++) {
            int alphabet_size = alphabet_sizes[j];
            if (!j && img->color_cache_bits > 0)
                alphabet_size += 1 << img->color_cache_bits;

            if (br_bit(&s->gb)) {
                read_huffman_code_simple(s, &hg[j]);
            } else {
                ret = read_huffman_code_normal(s, &hg[j], alphabet_size);
                if (ret < 0)
                    return ret;
            }
        }
    }

    width = img->frame->width;
    if (role == IMAGE_ROLE_ARGB)
        width = s->reduced_width;

    x = 0; y = 0;
    while (y < img->frame->height) {
        int v;

        if (br_bits_left(&s->gb) < 0)
            return WPD_ERROR_INVALID_DATA;

        hg = get_huffman_group(s, img, x, y);
        v = huff_reader_get_symbol(&hg[HUFF_IDX_GREEN], &s->gb);
        if (v < 0)
            return WPD_ERROR_INVALID_DATA;
        if (v < NUM_LITERAL_CODES) {
            /* literal pixel values */
            uint8_t *p = GET_PIXEL(img->frame, x, y);
            int r, b, a;
            p[2] = v;
            r = huff_reader_get_symbol(&hg[HUFF_IDX_RED],   &s->gb);
            b = huff_reader_get_symbol(&hg[HUFF_IDX_BLUE],  &s->gb);
            a = huff_reader_get_symbol(&hg[HUFF_IDX_ALPHA], &s->gb);
            if (r < 0 || b < 0 || a < 0)
                return WPD_ERROR_INVALID_DATA;
            p[1] = r;
            p[3] = b;
            p[0] = a;
            if (img->color_cache_bits)
                color_cache_put(img, rb32(p));
            x++;
            if (x == width) {
                x = 0;
                y++;
            }
        } else if (v < NUM_LITERAL_CODES + NUM_LENGTH_CODES) {
            /* LZ77 backwards mapping */
            int prefix_code, length, distance, ref_x, ref_y;

            /* parse length and distance */
            prefix_code = v - NUM_LITERAL_CODES;
            if (prefix_code < 4) {
                length = prefix_code + 1;
            } else {
                int extra_bits = (prefix_code - 2) >> 1;
                int offset     = (2 + (prefix_code & 1)) << extra_bits;
                length = offset + br_bits(&s->gb, extra_bits) + 1;
            }
            prefix_code = huff_reader_get_symbol(&hg[HUFF_IDX_DIST], &s->gb);
            if (prefix_code < 0 || prefix_code > 39) {
                wpd_log(NULL, WPD_LOG_ERROR,
                        "distance prefix code too large: %d\n", prefix_code);
                return WPD_ERROR_INVALID_DATA;
            }
            if (prefix_code < 4) {
                distance = prefix_code + 1;
            } else {
                int extra_bits = (prefix_code - 2) >> 1;
                int offset     = (2 + (prefix_code & 1)) << extra_bits;
                distance = offset + br_bits(&s->gb, extra_bits) + 1;
            }

            /* find reference location */
            if (distance <= NUM_SHORT_DISTANCES) {
                int xi = lz77_distance_offsets[distance - 1][0];
                int yi = lz77_distance_offsets[distance - 1][1];
                distance = WPD_MAX(1, xi + yi * width);
            } else {
                distance -= NUM_SHORT_DISTANCES;
            }
            ref_x = x;
            ref_y = y;
            if (distance <= x) {
                ref_x -= distance;
                distance = 0;
            } else {
                ref_x = 0;
                distance -= x;
            }
            while (distance >= width) {
                ref_y--;
                distance -= width;
            }
            if (distance > 0) {
                ref_x = width - distance;
                ref_y--;
            }
            ref_x = WPD_MAX(0, ref_x);
            ref_y = WPD_MAX(0, ref_y);

            if (ref_y == y && ref_x >= x)
                return WPD_ERROR_INVALID_DATA;

            /* copy pixels
             * source and dest regions can overlap and wrap lines, so just
             * copy per-pixel */
            for (i = 0; i < length; i++) {
                uint8_t *p_ref = GET_PIXEL(img->frame, ref_x, ref_y);
                uint8_t *p     = GET_PIXEL(img->frame,     x,     y);

                copy32(p, p_ref);
                if (img->color_cache_bits)
                    color_cache_put(img, rb32(p));
                x++;
                ref_x++;
                if (x == width) {
                    x = 0;
                    y++;
                }
                if (ref_x == width) {
                    ref_x = 0;
                    ref_y++;
                }
                if (y == img->frame->height || ref_y == img->frame->height)
                    break;
            }
        } else {
            /* read from color cache */
            uint8_t *p = GET_PIXEL(img->frame, x, y);
            int cache_idx = v - (NUM_LITERAL_CODES + NUM_LENGTH_CODES);

            if (!img->color_cache_bits) {
                wpd_log(NULL, WPD_LOG_ERROR, "color cache not found\n");
                return WPD_ERROR_INVALID_DATA;
            }
            if (cache_idx >= 1 << img->color_cache_bits) {
                wpd_log(NULL, WPD_LOG_ERROR,
                        "color cache index out-of-bounds\n");
                return WPD_ERROR_INVALID_DATA;
            }
            wb32(p, img->color_cache[cache_idx]);
            x++;
            if (x == width) {
                x = 0;
                y++;
            }
        }
    }

    return 0;
}

/* PRED_MODE_BLACK */
static void inv_predict_0(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    wb32(p, 0xFF000000);
}

/* PRED_MODE_L */
static void inv_predict_1(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    copy32(p, p_l);
}

/* PRED_MODE_T */
static void inv_predict_2(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    copy32(p, p_t);
}

/* PRED_MODE_TR */
static void inv_predict_3(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    copy32(p, p_tr);
}

/* PRED_MODE_TL */
static void inv_predict_4(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    copy32(p, p_tl);
}

/* PRED_MODE_AVG_T_AVG_L_TR */
static void inv_predict_5(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (p_t[0] + ((p_l[0] + p_tr[0]) >> 1)) >> 1;
    p[1] = (p_t[1] + ((p_l[1] + p_tr[1]) >> 1)) >> 1;
    p[2] = (p_t[2] + ((p_l[2] + p_tr[2]) >> 1)) >> 1;
    p[3] = (p_t[3] + ((p_l[3] + p_tr[3]) >> 1)) >> 1;
}

/* PRED_MODE_AVG_L_TL */
static void inv_predict_6(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (p_l[0] + p_tl[0]) >> 1;
    p[1] = (p_l[1] + p_tl[1]) >> 1;
    p[2] = (p_l[2] + p_tl[2]) >> 1;
    p[3] = (p_l[3] + p_tl[3]) >> 1;
}

/* PRED_MODE_AVG_L_T */
static void inv_predict_7(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (p_l[0] + p_t[0]) >> 1;
    p[1] = (p_l[1] + p_t[1]) >> 1;
    p[2] = (p_l[2] + p_t[2]) >> 1;
    p[3] = (p_l[3] + p_t[3]) >> 1;
}

/* PRED_MODE_AVG_TL_T */
static void inv_predict_8(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (p_tl[0] + p_t[0]) >> 1;
    p[1] = (p_tl[1] + p_t[1]) >> 1;
    p[2] = (p_tl[2] + p_t[2]) >> 1;
    p[3] = (p_tl[3] + p_t[3]) >> 1;
}

/* PRED_MODE_AVG_T_TR */
static void inv_predict_9(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                          const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (p_t[0] + p_tr[0]) >> 1;
    p[1] = (p_t[1] + p_tr[1]) >> 1;
    p[2] = (p_t[2] + p_tr[2]) >> 1;
    p[3] = (p_t[3] + p_tr[3]) >> 1;
}

/* PRED_MODE_AVG_AVG_L_TL_AVG_T_TR */
static void inv_predict_10(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                           const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = (((p_l[0] + p_tl[0]) >> 1) + ((p_t[0] + p_tr[0]) >> 1)) >> 1;
    p[1] = (((p_l[1] + p_tl[1]) >> 1) + ((p_t[1] + p_tr[1]) >> 1)) >> 1;
    p[2] = (((p_l[2] + p_tl[2]) >> 1) + ((p_t[2] + p_tr[2]) >> 1)) >> 1;
    p[3] = (((p_l[3] + p_tl[3]) >> 1) + ((p_t[3] + p_tr[3]) >> 1)) >> 1;
}

/* PRED_MODE_SELECT */
static void inv_predict_11(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                           const uint8_t *p_t, const uint8_t *p_tr)
{
    int diff = (WPD_ABS(p_l[0] - p_tl[0]) - WPD_ABS(p_t[0] - p_tl[0])) +
               (WPD_ABS(p_l[1] - p_tl[1]) - WPD_ABS(p_t[1] - p_tl[1])) +
               (WPD_ABS(p_l[2] - p_tl[2]) - WPD_ABS(p_t[2] - p_tl[2])) +
               (WPD_ABS(p_l[3] - p_tl[3]) - WPD_ABS(p_t[3] - p_tl[3]));
    if (diff <= 0)
        copy32(p, p_t);
    else
        copy32(p, p_l);
}

/* PRED_MODE_ADD_SUBTRACT_FULL */
static void inv_predict_12(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                           const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = wpd_clip_uint8(p_l[0] + p_t[0] - p_tl[0]);
    p[1] = wpd_clip_uint8(p_l[1] + p_t[1] - p_tl[1]);
    p[2] = wpd_clip_uint8(p_l[2] + p_t[2] - p_tl[2]);
    p[3] = wpd_clip_uint8(p_l[3] + p_t[3] - p_tl[3]);
}

static wpd_always_inline uint8_t clamp_add_subtract_half(int a, int b, int c)
{
    int d = (a + b) >> 1;
    return wpd_clip_uint8(d + (d - c) / 2);
}

/* PRED_MODE_ADD_SUBTRACT_HALF */
static void inv_predict_13(uint8_t *p, const uint8_t *p_l, const uint8_t *p_tl,
                           const uint8_t *p_t, const uint8_t *p_tr)
{
    p[0] = clamp_add_subtract_half(p_l[0], p_t[0], p_tl[0]);
    p[1] = clamp_add_subtract_half(p_l[1], p_t[1], p_tl[1]);
    p[2] = clamp_add_subtract_half(p_l[2], p_t[2], p_tl[2]);
    p[3] = clamp_add_subtract_half(p_l[3], p_t[3], p_tl[3]);
}

typedef void (*inv_predict_func)(uint8_t *p, const uint8_t *p_l,
                                 const uint8_t *p_tl, const uint8_t *p_t,
                                 const uint8_t *p_tr);

static const inv_predict_func inverse_predict[14] = {
    inv_predict_0,  inv_predict_1,  inv_predict_2,  inv_predict_3,
    inv_predict_4,  inv_predict_5,  inv_predict_6,  inv_predict_7,
    inv_predict_8,  inv_predict_9,  inv_predict_10, inv_predict_11,
    inv_predict_12, inv_predict_13,
};

static void inverse_prediction(WebPImage *frame, enum PredictionMode m, int x, int y)
{
    uint8_t *dec, *p_l, *p_tl, *p_t, *p_tr;
    uint8_t p[4];

    dec  = GET_PIXEL(frame, x,     y);
    p_l  = GET_PIXEL(frame, x - 1, y);
    p_tl = GET_PIXEL(frame, x - 1, y - 1);
    p_t  = GET_PIXEL(frame, x,     y - 1);
    if (x == frame->width - 1)
        p_tr = GET_PIXEL(frame, 0, y);
    else
        p_tr = GET_PIXEL(frame, x + 1, y - 1);

    inverse_predict[m](p, p_l, p_tl, p_t, p_tr);

    dec[0] += p[0];
    dec[1] += p[1];
    dec[2] += p[2];
    dec[3] += p[3];
}

static int apply_predictor_transform(WPDDecoder *s)
{
    ImageContext *img  = &s->image[IMAGE_ROLE_ARGB];
    ImageContext *pimg = &s->image[IMAGE_ROLE_PREDICTOR];
    int x, y;

    for (y = 0; y < img->frame->height; y++) {
        for (x = 0; x < s->reduced_width; x++) {
            int tx = x >> pimg->size_reduction;
            int ty = y >> pimg->size_reduction;
            enum PredictionMode m = GET_PIXEL_COMP(pimg->frame, tx, ty, 2);

            if (x == 0) {
                if (y == 0)
                    m = PRED_MODE_BLACK;
                else
                    m = PRED_MODE_T;
            } else if (y == 0)
                m = PRED_MODE_L;

            if (m > 13) {
                wpd_log(NULL, WPD_LOG_ERROR, "invalid predictor mode: %d\n", m);
                return WPD_ERROR_INVALID_DATA;
            }
            inverse_prediction(img->frame, m, x, y);
        }
    }
    return 0;
}

static wpd_always_inline uint8_t color_transform_delta(uint8_t color_pred,
                                                       uint8_t color)
{
    return u8_to_s8(color_pred) * u8_to_s8(color) >> 5;
}

static int apply_color_transform(WPDDecoder *s)
{
    ImageContext *img, *cimg;
    int x, y, cx, cy;
    uint8_t *p, *cp;

    img  = &s->image[IMAGE_ROLE_ARGB];
    cimg = &s->image[IMAGE_ROLE_COLOR_TRANSFORM];

    for (y = 0; y < img->frame->height; y++) {
        for (x = 0; x < s->reduced_width; x++) {
            cx = x >> cimg->size_reduction;
            cy = y >> cimg->size_reduction;
            cp = GET_PIXEL(cimg->frame, cx, cy);
            p  = GET_PIXEL(img->frame,   x,  y);

            p[1] += color_transform_delta(cp[3], p[2]);
            p[3] += color_transform_delta(cp[2], p[2]) +
                    color_transform_delta(cp[1], p[1]);
        }
    }
    return 0;
}

static int apply_subtract_green_transform(WPDDecoder *s)
{
    int x, y;
    ImageContext *img = &s->image[IMAGE_ROLE_ARGB];

    for (y = 0; y < img->frame->height; y++) {
        for (x = 0; x < s->reduced_width; x++) {
            uint8_t *p = GET_PIXEL(img->frame, x, y);
            p[1] += p[2];
            p[3] += p[2];
        }
    }
    return 0;
}

static int apply_color_indexing_transform(WPDDecoder *s)
{
    ImageContext *img;
    ImageContext *pal;
    int i, x, y;
    uint8_t *p;

    img = &s->image[IMAGE_ROLE_ARGB];
    pal = &s->image[IMAGE_ROLE_COLOR_INDEXING];

    if (pal->size_reduction > 0) { // undo pixel packing
        LEBitReader gb_g;
        uint8_t *line;
        int pixel_bits = 8 >> pal->size_reduction;

        line = malloc(img->frame->linesize[0] + WPD_FILE_PADDING);
        if (!line)
            return WPD_ERROR(ENOMEM);

        for (y = 0; y < img->frame->height; y++) {
            p = GET_PIXEL(img->frame, 0, y);
            memcpy(line, p, img->frame->linesize[0]);
            br_init(&gb_g, line, img->frame->linesize[0]);
            br_skip(&gb_g, 16);
            i = 0;
            for (x = 0; x < img->frame->width; x++) {
                p    = GET_PIXEL(img->frame, x, y);
                p[2] = br_bits(&gb_g, pixel_bits);
                i++;
                if (i == 1 << pal->size_reduction) {
                    br_skip(&gb_g, 24);
                    i = 0;
                }
            }
        }
        free(line);
        s->reduced_width = s->width; // we are back to full size
    }

    // switch to local palette if it's worth initializing it
    if (img->frame->height * img->frame->width > 300) {
        uint8_t palette[256 * 4];
        const int size = pal->frame->width * 4;
        memcpy(palette, GET_PIXEL(pal->frame, 0, 0), size);   // copy palette
        // set extra entries to transparent black
        memset(palette + size, 0, 256 * 4 - size);
        for (y = 0; y < img->frame->height; y++) {
            for (x = 0; x < img->frame->width; x++) {
                p = GET_PIXEL(img->frame, x, y);
                i = p[2];
                copy32(p, &palette[i * 4]);
            }
        }
    } else {
        for (y = 0; y < img->frame->height; y++) {
            for (x = 0; x < img->frame->width; x++) {
                p = GET_PIXEL(img->frame, x, y);
                i = p[2];
                if (i >= pal->frame->width) {
                    wb32(p, 0x00000000);
                } else {
                    const uint8_t *pi = GET_PIXEL(pal->frame, i, 0);
                    copy32(p, pi);
                }
            }
        }
    }

    return 0;
}

static void update_canvas_size(WPDDecoder *s, int w, int h)
{
    if (s->width && s->width != w)
        wpd_log(NULL, WPD_LOG_WARNING, "Width mismatch. %d != %d\n",
                s->width, w);
    s->width = w;
    if (s->height && s->height != h)
        wpd_log(NULL, WPD_LOG_WARNING, "Height mismatch. %d != %d\n",
                s->height, h);
    s->height = h;
}

static int vp8_lossless_decode_frame(WPDDecoder *s, WebPImage *out,
                                     const uint8_t *data_start,
                                     unsigned int data_size, int is_alpha_chunk)
{
    int w, h, ret, i;
    unsigned used;

    br_init(&s->gb, data_start, data_size);

    if (!is_alpha_chunk) {
        if (br_bits(&s->gb, 8) != 0x2F) {
            wpd_log(NULL, WPD_LOG_ERROR, "Invalid WebP Lossless signature\n");
            return WPD_ERROR_INVALID_DATA;
        }

        w = br_bits(&s->gb, 14) + 1;
        h = br_bits(&s->gb, 14) + 1;

        update_canvas_size(s, w, h);

        ret = wpd_check_image_size(s->width, s->height);
        if (ret < 0)
            return ret;

        s->lossless_has_alpha = br_bit(&s->gb);

        if (br_bits(&s->gb, 3) != 0x0) {
            wpd_log(NULL, WPD_LOG_ERROR, "Invalid WebP Lossless version\n");
            return WPD_ERROR_INVALID_DATA;
        }
    } else {
        if (!s->width || !s->height)
            return WPD_ERROR_INVALID_DATA;
        w = s->width;
        h = s->height;
    }

    /* parse transformations */
    s->nb_transforms = 0;
    s->reduced_width = s->width;
    used = 0;
    while (br_bit(&s->gb)) {
        enum TransformType transform = br_bits(&s->gb, 2);
        if (used & (1 << transform)) {
            wpd_log(NULL, WPD_LOG_ERROR, "Transform %d used more than once\n",
                    transform);
            ret = WPD_ERROR_INVALID_DATA;
            goto free_and_return;
        }
        used |= (1 << transform);
        s->transforms[s->nb_transforms++] = transform;
        ret = 0;
        switch (transform) {
        case PREDICTOR_TRANSFORM:
            ret = parse_transform_predictor(s);
            break;
        case COLOR_TRANSFORM:
            ret = parse_transform_color(s);
            break;
        case COLOR_INDEXING_TRANSFORM:
            ret = parse_transform_color_indexing(s);
            break;
        case SUBTRACT_GREEN:
            break;
        }
        if (ret < 0)
            goto free_and_return;
    }

    /* decode primary image */
    s->image[IMAGE_ROLE_ARGB].frame = out;
    if (is_alpha_chunk)
        s->image[IMAGE_ROLE_ARGB].is_alpha_primary = 1;
    ret = decode_entropy_coded_image(s, IMAGE_ROLE_ARGB, w, h);
    if (ret < 0)
        goto free_and_return;

    /* apply transformations */
    for (i = s->nb_transforms - 1; i >= 0; i--) {
        switch (s->transforms[i]) {
        case PREDICTOR_TRANSFORM:
            ret = apply_predictor_transform(s);
            break;
        case COLOR_TRANSFORM:
            ret = apply_color_transform(s);
            break;
        case SUBTRACT_GREEN:
            ret = apply_subtract_green_transform(s);
            break;
        case COLOR_INDEXING_TRANSFORM:
            ret = apply_color_indexing_transform(s);
            break;
        }
        if (ret < 0)
            goto free_and_return;
    }

    ret = 0;

free_and_return:
    for (i = 0; i < IMAGE_ROLE_NB; i++)
        image_ctx_free(&s->image[i]);

    return ret;
}

static void alpha_inverse_prediction(WebPImage *frame, enum AlphaFilter m)
{
    int x, y, ls;
    uint8_t *dec;

    ls = frame->linesize[3];

    /* filter first row using horizontal filter */
    dec = frame->data[3] + 1;
    for (x = 1; x < frame->width; x++, dec++)
        *dec += *(dec - 1);

    /* filter first column using vertical filter */
    dec = frame->data[3] + ls;
    for (y = 1; y < frame->height; y++, dec += ls)
        *dec += *(dec - ls);

    /* filter the rest using the specified filter */
    switch (m) {
    case ALPHA_FILTER_HORIZONTAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++)
                *dec += *(dec - 1);
        }
        break;
    case ALPHA_FILTER_VERTICAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++)
                *dec += *(dec - ls);
        }
        break;
    case ALPHA_FILTER_GRADIENT:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++)
                dec[0] += wpd_clip_uint8(*(dec - 1) + *(dec - ls) - *(dec - ls - 1));
        }
        break;
    case ALPHA_FILTER_NONE:
        break;
    }
}

static int vp8_lossy_decode_alpha(WPDDecoder *s, WebPImage *p,
                                  const uint8_t *data_start,
                                  unsigned int data_size)
{
    int x, y, ret;

    if (s->alpha_compression == ALPHA_COMPRESSION_NONE) {
        const uint8_t *src = data_start;
        size_t left = data_size;

        for (y = 0; y < s->height; y++) {
            size_t n = WPD_MIN((size_t)s->width, left);
            memcpy(p->data[3] + p->linesize[3] * y, src, n);
            src += n;
            left -= n;
        }
    } else if (s->alpha_compression == ALPHA_COMPRESSION_VP8L) {
        uint8_t *ap, *pp;

        ret = vp8_lossless_decode_frame(s, &s->alpha_argb, data_start,
                                        data_size, 1);
        if (ret < 0) {
            image_free(&s->alpha_argb);
            return ret;
        }

        /* copy green component of alpha image to alpha plane of primary image */
        for (y = 0; y < s->height; y++) {
            ap = GET_PIXEL(&s->alpha_argb, 0, y) + 2;
            pp = p->data[3] + p->linesize[3] * y;
            for (x = 0; x < s->width; x++) {
                *pp = *ap;
                pp++;
                ap += 4;
            }
        }
        image_free(&s->alpha_argb);
    }

    /* apply alpha filtering */
    if (s->alpha_filter)
        alpha_inverse_prediction(p, s->alpha_filter);

    return 0;
}

/*
 * Decode a lossy (VP8) bitstream into out. The Y/U/V planes are borrowed
 * from the VP8 decoder and stay valid until its next invocation; the
 * alpha plane, if any, lives in s->alpha_plane.
 */
static int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                                  const uint8_t *data_start,
                                  unsigned int data_size)
{
    WpdPacket packet;
    WpdFrame decoded;
    int ret;

    if (!s->vp8_initialized) {
        s->codec.priv_data = &s->vp8;
        ret = vp8_decode_init(&s->codec);
        if (ret < 0)
            return ret;
        s->vp8_initialized = 1;
    }

    packet.data = data_start;
    packet.size = data_size;
    ret = vp8_decode_frame(&s->codec, &decoded, &packet);
    if (ret < 0)
        return ret;

    update_canvas_size(s, s->codec.width, s->codec.height);

    memset(out, 0, sizeof(*out));
    out->width  = s->width;
    out->height = s->height;
    out->format = WPD_PIX_FMT_YUV420P;
    for (int plane = 0; plane < 3; plane++) {
        out->data[plane]     = decoded.data[plane];
        out->linesize[plane] = decoded.linesize[plane];
    }

    if (s->has_alpha) {
        size_t alpha_size = (size_t)s->width * s->height;
        if (s->alpha_plane_size < alpha_size) {
            uint8_t *plane = realloc(s->alpha_plane, alpha_size);
            if (!plane)
                return WPD_ERROR(ENOMEM);
            s->alpha_plane = plane;
            s->alpha_plane_size = alpha_size;
        }
        memset(s->alpha_plane, 0, alpha_size);
        out->data[3]     = s->alpha_plane;
        out->linesize[3] = s->width;
        out->format      = WPD_PIX_FMT_YUVA420P;
        ret = vp8_lossy_decode_alpha(s, out, s->alpha_data, s->alpha_data_size);
        if (ret < 0)
            return ret;
    }
    return 0;
}

/*
 * Animation compositing, ported from FFmpeg's webp_anim decoder. The
 * canvas is either YUVA420P (lossy subframes) or ARGB (lossless ones),
 * upgraded from YUVA to ARGB when a lossless frame follows lossy ones.
 */

static int image_nb_components(const WebPImage *img)
{
    switch (img->format) {
    case WPD_PIX_FMT_YUV420P:  return 3;
    case WPD_PIX_FMT_YUVA420P: return 4;
    default:                   return 4;
    }
}

/*
 * Blend src (foreground) into dst (background), in ARGB format.
 * pos_x, pos_y is the position in dst.
 */
static void blend_alpha_argb(WebPImage *dst, const WebPImage *src,
                             int pos_x, int pos_y)
{
    for (int y = 0; y < src->height; y++) {
        const uint8_t *src_argb = src->data[0] +          y  * src->linesize[0];
        uint8_t       *dst_argb = dst->data[0] + (pos_y + y) * dst->linesize[0] + pos_x * 4;
        for (int x = 0; x < src->width; x++) {
            int src_alpha = src_argb[0];
            int dst_alpha = dst_argb[0];

            if (src_alpha == 255) {
                memcpy(dst_argb, src_argb, 4);
            } else if (src_alpha == 0) {
                // no-op
            } else {
                int tmp_alpha = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale = (1UL << 24) / blend_alpha;

                dst_argb[0] = blend_alpha;
                dst_argb[1] = (((uint32_t)(src_argb[1] * src_alpha + dst_argb[1] * tmp_alpha)) * scale) >> 24;
                dst_argb[2] = (((uint32_t)(src_argb[2] * src_alpha + dst_argb[2] * tmp_alpha)) * scale) >> 24;
                dst_argb[3] = (((uint32_t)(src_argb[3] * src_alpha + dst_argb[3] * tmp_alpha)) * scale) >> 24;
            }
            src_argb += 4;
            dst_argb += 4;
        }
    }
}

/*
 * Blend src (foreground) into dst (background), in YUVA format.
 * pos_x, pos_y is the position in dst.
 */
static void blend_alpha_yuva(WebPImage *dst, const WebPImage *src,
                             int pos_x, int pos_y)
{
    // blend U & V planes first, because the later step may modify alpha plane
    for (int y = 0; y < CEIL_RSHIFT(src->height, 1); y++) {
        int tile_h = WPD_MIN(src->height - y * 2, 2);
        const uint8_t *src_u = src->data[1] +                 y  * src->linesize[1];
        const uint8_t *src_v = src->data[2] +                 y  * src->linesize[2];
        uint8_t       *dst_u = dst->data[1] + ((pos_y >> 1) + y) * dst->linesize[1] + (pos_x >> 1);
        uint8_t       *dst_v = dst->data[2] + ((pos_y >> 1) + y) * dst->linesize[2] + (pos_x >> 1);
        for (int x = 0; x < CEIL_RSHIFT(src->width, 1); x++) {
            int tile_w = WPD_MIN(src->width - x * 2, 2);
            // calculate the average alpha of the tile
            int src_alpha = 0;
            int dst_alpha = 0;
            for (int yy = 0; yy < tile_h; yy++) {
                for (int xx = 0; xx < tile_w; xx++) {
                    src_alpha += src->data[3][(y * 2 + yy) * src->linesize[3] +
                                              (x * 2 + xx)];
                    dst_alpha += dst->data[3][(((pos_y >> 1) + y) * 2 + yy) * dst->linesize[3] +
                                              (((pos_x >> 1) + x) * 2 + xx)];
                }
            }
            int shift = (tile_h == 2) + (tile_w == 2);
            src_alpha = CEIL_RSHIFT(src_alpha, shift);
            dst_alpha = CEIL_RSHIFT(dst_alpha, shift);

            if (src_alpha == 255) {
                *dst_u = *src_u;
                *dst_v = *src_v;
            } else if (src_alpha == 0) {
                // no-op
            } else {
                int tmp_alpha = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale = (1UL << 24) / blend_alpha;
                *dst_u = (((uint32_t)(*src_u * src_alpha + *dst_u * tmp_alpha)) * scale) >> 24;
                *dst_v = (((uint32_t)(*src_v * src_alpha + *dst_v * tmp_alpha)) * scale) >> 24;
            }
            src_u += 1;
            src_v += 1;
            dst_u += 1;
            dst_v += 1;
        }
    }

    // blend Y & A planes
    for (int y = 0; y < src->height; y++) {
        const uint8_t *src_y = src->data[0] +          y  * src->linesize[0];
        const uint8_t *src_a = src->data[3] +          y  * src->linesize[3];
        uint8_t       *dst_y = dst->data[0] + (pos_y + y) * dst->linesize[0] + pos_x;
        uint8_t       *dst_a = dst->data[3] + (pos_y + y) * dst->linesize[3] + pos_x;
        for (int x = 0; x < src->width; x++) {
            int src_alpha = *src_a;
            int dst_alpha = *dst_a;

            if (src_alpha == 255) {
                *dst_y = *src_y;
                *dst_a = 255;
            } else if (src_alpha == 0) {
                // no-op
            } else {
                int tmp_alpha = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale = (1UL << 24) / blend_alpha;
                *dst_y = (((uint32_t)(*src_y * src_alpha + *dst_y * tmp_alpha)) * scale) >> 24;
                *dst_a = blend_alpha;
            }
            src_y += 1;
            src_a += 1;
            dst_y += 1;
            dst_a += 1;
        }
    }
}

static wpd_always_inline void webp_yuva2argb(uint8_t *out, int Y, int U, int V, int A)
{
    // variables used in macros
    const uint8_t *cm = wpd_crop_table + WPD_MAX_NEG_CROP;
    uint8_t r, g, b;
    int y, cb, cr;
    int r_add, g_add, b_add;

    YUV_TO_RGB1_CCIR(U, V);
    YUV_TO_RGB2_CCIR(r, g, b, Y);

    out[0] = wpd_clip_uint8(A);
    out[1] = wpd_clip_uint8(r);
    out[2] = wpd_clip_uint8(g);
    out[3] = wpd_clip_uint8(b);
}

static void copy_yuva2argb(WebPImage *dst, const WebPImage *src,
                           int pos_x, int pos_y)
{
    int alpha = image_nb_components(src) > 3;

    for (int y = 0; y < src->height; y++) {
        const uint8_t *src_y = src->data[0] +  y       * src->linesize[0];
        const uint8_t *src_u = src->data[1] + (y >> 1) * src->linesize[1];
        const uint8_t *src_v = src->data[2] + (y >> 1) * src->linesize[2];
        const uint8_t *src_a = NULL;
        uint8_t       *dst_argb = dst->data[0] + (pos_y + y) * dst->linesize[0] + pos_x * 4;
        if (alpha)
            src_a = src->data[3] + y * src->linesize[3];

        for (int x = 0; x < src->width; x++) {
            webp_yuva2argb(dst_argb, *src_y, *src_u, *src_v, (alpha ? *src_a : 255));
            src_y += 1;
            src_u += x & 1;
            src_v += x & 1;
            if (alpha)
                src_a += 1;
            dst_argb += 4;
        }
    }
}

static void blend_yuva2argb(WebPImage *dst, const WebPImage *src,
                            int pos_x, int pos_y)
{
    for (int y = 0; y < src->height; y++) {
        const uint8_t *src_y = src->data[0] +  y       * src->linesize[0];
        const uint8_t *src_u = src->data[1] + (y >> 1) * src->linesize[1];
        const uint8_t *src_v = src->data[2] + (y >> 1) * src->linesize[2];
        const uint8_t *src_a = src->data[3] +  y       * src->linesize[3];
        uint8_t       *dst_argb = dst->data[0] + (pos_y + y) * dst->linesize[0] + pos_x * 4;

        for (int x = 0; x < src->width; x++) {
            int src_alpha = *src_a;
            int dst_alpha = dst_argb[0];

            if (src_alpha == 255) {
                webp_yuva2argb(dst_argb, *src_y, *src_u, *src_v, src_alpha);
            } else if (src_alpha == 0) {
                // no-op
            } else {
                uint8_t tmp[4];
                int tmp_alpha = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale = (1UL << 24) / blend_alpha;

                webp_yuva2argb(tmp, *src_y, *src_u, *src_v, src_alpha);

                dst_argb[0] = blend_alpha;
                dst_argb[1] = (((uint32_t)(tmp[1] * src_alpha + dst_argb[1] * tmp_alpha)) * scale) >> 24;
                dst_argb[2] = (((uint32_t)(tmp[2] * src_alpha + dst_argb[2] * tmp_alpha)) * scale) >> 24;
                dst_argb[3] = (((uint32_t)(tmp[3] * src_alpha + dst_argb[3] * tmp_alpha)) * scale) >> 24;
            }

            src_y += 1;
            src_u += x & 1;
            src_v += x & 1;
            src_a += 1;
            dst_argb += 4;
        }
    }
}

static void blend_subframe_into_canvas(WPDDecoder *s, const WebPImage *frame)
{
    WebPImage *canvas = &s->canvas;

    if ((s->anmf_flags & ANMF_FLAG_NO_BLEND)
        || frame->format == WPD_PIX_FMT_YUV420P) {
        // do not blend, overwrite

        if (canvas->format == WPD_PIX_FMT_ARGB) {
            if (canvas->format == frame->format) {
                const uint8_t *src = frame->data[0];
                uint8_t       *dst = canvas->data[0] +
                                     s->pos_y * canvas->linesize[0] +
                                     s->pos_x * 4;
                for (int y = 0; y < frame->height; y++) {
                    memcpy(dst, src, (size_t)frame->width * 4);
                    src += frame->linesize[0];
                    dst += canvas->linesize[0];
                }
            } else {
                copy_yuva2argb(canvas, frame, s->pos_x, s->pos_y);
            }
        } else /* canvas->format == WPD_PIX_FMT_YUVA420P */ {
            int nb_components = image_nb_components(frame);

            for (int comp = 0; comp < nb_components; comp++) {
                int plane = comp;
                int shift = (comp == 1 || comp == 2) ? 1 : 0;
                const uint8_t *src = frame->data[plane];
                uint8_t       *dst = canvas->data[plane] +
                                     (s->pos_y >> shift) * canvas->linesize[plane] +
                                     (s->pos_x >> shift);
                for (int y = 0; y < CEIL_RSHIFT(frame->height, shift); y++) {
                    memcpy(dst, src, CEIL_RSHIFT(frame->width, shift));
                    src += frame->linesize[plane];
                    dst += canvas->linesize[plane];
                }
            }

            if (nb_components < 4) {
                // frame does not have alpha, set alpha to 255
                uint8_t *dst = canvas->data[3] + s->pos_y * canvas->linesize[3] + s->pos_x;
                for (int y = 0; y < frame->height; y++) {
                    memset(dst, 255, frame->width);
                    dst += canvas->linesize[3];
                }
            }
        }
    } else {
        // alpha blending

        if (canvas->format == WPD_PIX_FMT_ARGB) {
            if (canvas->format == frame->format) {
                blend_alpha_argb(canvas, frame, s->pos_x, s->pos_y);
            } else {
                blend_yuva2argb(canvas, frame, s->pos_x, s->pos_y);
            }
        } else /* canvas->format == WPD_PIX_FMT_YUVA420P */ {
            blend_alpha_yuva(canvas, frame, s->pos_x, s->pos_y);
        }
    }
}

/*
 * Fill a rectangle on the canvas with the background color (transparent
 * black, matching FFmpeg's default of not using the ANIM chunk color).
 */
static void fill_canvas_rect(WPDDecoder *s, int pos_x, int pos_y,
                             int width, int height)
{
    WebPImage *canvas = &s->canvas;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        const uint8_t *bg = s->background_argb;
        for (int y = 0; y < height; y++) {
            uint8_t *dst = canvas->data[0] + (pos_y + y) * canvas->linesize[0] + pos_x * 4;
            for (int x = 0; x < width; x++, dst += 4)
                copy32(dst, bg);
        }
    } else /* canvas->format == WPD_PIX_FMT_YUVA420P */ {
        for (int comp = 0; comp < 4; comp++) {
            int shift = (comp == 1 || comp == 2) ? 1 : 0;
            uint8_t *dst = canvas->data[comp] + (pos_y >> shift) * canvas->linesize[comp] + (pos_x >> shift);
            for (int y = 0; y < CEIL_RSHIFT(height, shift); y++) {
                memset(dst, s->background_yuva[comp], CEIL_RSHIFT(width, shift));
                dst += canvas->linesize[comp];
            }
        }
    }
}

static int allocate_canvas(WPDDecoder *s, WPDPixelFormat format)
{
    int ret;

    if (format == WPD_PIX_FMT_ARGB)
        ret = image_alloc_argb(&s->canvas, s->canvas_width, s->canvas_height);
    else
        ret = image_alloc_yuva(&s->canvas, s->canvas_width, s->canvas_height);
    return ret;
}

static int prepare_canvas(WPDDecoder *s, int key_frame, WPDPixelFormat format)
{
    int ret;

    /*
     * Clear the canvas on keyframes and frames that overwrite the entire
     * canvas.
     */
    if (key_frame ||
        ((s->anmf_flags & ANMF_FLAG_NO_BLEND) &&
         (s->pos_x == 0) && (s->pos_x + s->width == s->canvas_width) &&
         (s->pos_y == 0) && (s->pos_y + s->height == s->canvas_height)))
        image_free(&s->canvas);

    if (!s->canvas.data[0]) {
        ret = allocate_canvas(s, format);
        if (ret < 0)
            return ret;
        fill_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else {
        if (format == WPD_PIX_FMT_ARGB && s->canvas.format == WPD_PIX_FMT_YUVA420P) {
            /*
             * If we have a lossless frame following a lossy frame, we
             * upgrade the canvas to ARGB, but we don't convert the canvas
             * back to YUVA if there is a lossy frame following a lossless
             * frame.
             */
            WebPImage yuva_canvas = s->canvas;
            memset(&s->canvas, 0, sizeof(s->canvas));
            ret = allocate_canvas(s, WPD_PIX_FMT_ARGB);
            if (ret < 0) {
                image_free(&yuva_canvas);
                return ret;
            }
            copy_yuva2argb(&s->canvas, &yuva_canvas, 0, 0);
            image_free(&yuva_canvas);
        }
        /* Dispose of previous frame if needed. */
        if (s->prev_anmf_flags & ANMF_FLAG_DISPOSE)
            fill_canvas_rect(s, s->prev_pos_x, s->prev_pos_y,
                             s->prev_width, s->prev_height);
    }

    return 0;
}

static int decode_anmf(WPDDecoder *s, const uint8_t *data, size_t size)
{
    const uint8_t *p = data, *end = data + size;
    const WebPImage *sub = NULL;
    int key_frame = s->frame_index == 0;
    int ret;

    if (size < 16)
        return WPD_ERROR_INVALID_DATA;

    s->pos_x      = WPD_RL24(p)     * 2;
    s->pos_y      = WPD_RL24(p + 3) * 2;
    /* Frame dimensions are taken from the decoded bitstream below;
     * duration is irrelevant for raw decoding. */
    s->anmf_flags = p[15];
    p += 16;

    /* Reset alpha and dimensions from previous frame. */
    s->has_alpha = 0;
    s->width     = 0;
    s->height    = 0;

    /* Parse ANMF subchunks. */
    while (end - p > 8) {
        uint32_t chunk_type = WPD_RL32(p);
        uint32_t payload_size = WPD_RL32(p + 4);
        uint32_t padded_size;

        if (payload_size == UINT32_MAX)
            return WPD_ERROR_INVALID_DATA;
        padded_size = payload_size + (payload_size & 1);
        p += 8;

        if ((size_t)(end - p) < padded_size) {
            /* we seem to be running out of data, but it could also be that
             * the bitstream has trailing junk leading to bogus payload_size. */
            break;
        }

        switch (chunk_type) {
        case MKTAG('A', 'L', 'P', 'H'): {
            if (payload_size == 0) {
                wpd_log(NULL, WPD_LOG_ERROR, "invalid ALPHA chunk size\n");
                return WPD_ERROR_INVALID_DATA;
            }
            int alpha_header   = p[0];
            s->alpha_data      = p + 1;
            s->alpha_data_size = payload_size - 1;

            int filter_m    = (alpha_header >> 2) & 0x03;
            int compression =  alpha_header       & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL, WPD_LOG_WARNING,
                        "skipping unsupported ALPHA chunk\n");
            } else {
                s->has_alpha         = 1;
                s->alpha_compression = compression;
                s->alpha_filter      = filter_m;
            }
            break;
        }
        case MKTAG('V', 'P', '8', ' '):
            if (sub)
                break;
            ret = vp8_lossy_decode_frame(s, &s->subframe, p, payload_size);
            if (ret < 0)
                return ret;
            sub = &s->subframe;
            ret = prepare_canvas(s, key_frame, WPD_PIX_FMT_YUVA420P);
            if (ret < 0)
                return ret;
            break;
        case MKTAG('V', 'P', '8', 'L'):
            if (sub)
                break;
            ret = vp8_lossless_decode_frame(s, &s->argb, p, payload_size, 0);
            if (ret < 0)
                return ret;
            sub = &s->argb;
            ret = prepare_canvas(s, key_frame, WPD_PIX_FMT_ARGB);
            if (ret < 0)
                return ret;
            break;
        default:
            break;
        }
        p += padded_size;
    }

    if (!sub) {
        wpd_log(NULL, WPD_LOG_ERROR, "image data not found\n");
        return WPD_ERROR_INVALID_DATA;
    }

    if (s->pos_x + sub->width  > s->canvas_width ||
        s->pos_y + sub->height > s->canvas_height) {
        wpd_log(NULL, WPD_LOG_ERROR,
                "Frame (%dx%d at pos %dx%d) does not fit into canvas (%dx%d)\n",
                sub->width, sub->height, s->pos_x, s->pos_y,
                s->canvas_width, s->canvas_height);
        return WPD_ERROR_INVALID_DATA;
    }

    blend_subframe_into_canvas(s, sub);

    s->prev_anmf_flags = s->anmf_flags;
    s->prev_width      = sub->width;
    s->prev_height     = sub->height;
    s->prev_pos_x      = s->pos_x;
    s->prev_pos_y      = s->pos_y;
    s->frame_index++;

    return 0;
}

static void export_frame(const WebPImage *img, WPDFrame *frame)
{
    memset(frame, 0, sizeof(*frame));
    for (int p = 0; p < 4; p++) {
        frame->data[p]   = img->data[p];
        frame->stride[p] = img->linesize[p];
    }
    frame->width  = img->width;
    frame->height = img->height;
    frame->format = img->format;
}

static int set_error(WPDDecoder *decoder, const char *message, int code)
{
    snprintf(decoder->error, sizeof(decoder->error), "%s (%d)", message, code);
    return -1;
}

WPDDecoder *wpd_decoder_create(void)
{
    WPDDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    wpd_dsp_data_init();
    return decoder;
}

int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data, size_t size)
{
    uint32_t riff_size;

    if (!decoder || !data)
        return -1;

    /* reset per-file state */
    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    memset(&decoder->subframe, 0, sizeof(decoder->subframe));
    decoder->animation = 0;
    decoder->still_done = 0;
    decoder->frame_index = 0;
    decoder->canvas_width = decoder->canvas_height = 0;
    decoder->width = decoder->height = 0;
    decoder->has_alpha = 0;
    decoder->prev_anmf_flags = decoder->anmf_flags = 0;
    decoder->prev_width = decoder->prev_height = 0;
    decoder->prev_pos_x = decoder->prev_pos_y = 0;
    memset(decoder->background_argb, 0, sizeof(decoder->background_argb));
    /* transparent black in YUV (CCIR range) */
    decoder->background_yuva[0] = RGB_TO_Y_CCIR(0, 0, 0);
    decoder->background_yuva[1] = RGB_TO_U_CCIR(0, 0, 0, 0);
    decoder->background_yuva[2] = RGB_TO_V_CCIR(0, 0, 0, 0);
    decoder->background_yuva[3] = 0;
    decoder->error[0] = 0;

    if (size < 12 || size > INT_MAX - WPD_FILE_PADDING)
        return set_error(decoder, "not a WebP file", WPD_ERROR_INVALID_DATA);

    free(decoder->file);
    decoder->file = malloc(size + WPD_FILE_PADDING);
    if (!decoder->file) {
        decoder->file_size = 0;
        return set_error(decoder, "out of memory", WPD_ERROR(ENOMEM));
    }
    memcpy(decoder->file, data, size);
    memset(decoder->file + size, 0, WPD_FILE_PADDING);
    decoder->file_size = size;

    if (WPD_RL32(decoder->file) != MKTAG('R', 'I', 'F', 'F') ||
        WPD_RL32(decoder->file + 8) != MKTAG('W', 'E', 'B', 'P'))
        return set_error(decoder, "not a WebP file", WPD_ERROR_INVALID_DATA);

    riff_size = WPD_RL32(decoder->file + 4);
    decoder->pos = 12;
    decoder->end = WPD_MIN((size_t)riff_size + 8, size);
    return 0;
}

int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame)
{
    if (!decoder || !frame)
        return -1;
    if (!decoder->file)
        return set_error(decoder, "no file opened", WPD_ERROR_INVALID_DATA);

    while (decoder->pos + 8 <= decoder->end) {
        const uint8_t *chunk = decoder->file + decoder->pos;
        uint32_t chunk_type = WPD_RL32(chunk);
        uint32_t size = WPD_RL32(chunk + 4);
        uint32_t padded_size;
        const uint8_t *payload = chunk + 8;
        int ret;

        if (size == UINT32_MAX)
            return set_error(decoder, "invalid chunk size", WPD_ERROR_INVALID_DATA);
        padded_size = size + (size & 1);

        if (decoder->end - (decoder->pos + 8) < padded_size) {
            /* truncated file or trailing junk; stop like FFmpeg does */
            break;
        }
        decoder->pos += 8 + padded_size;

        switch (chunk_type) {
        case MKTAG('V', 'P', '8', 'X'):
            decoder->canvas_width  = WPD_RL24(payload + 4) + 1;
            decoder->canvas_height = WPD_RL24(payload + 7) + 1;
            break;
        case MKTAG('A', 'N', 'I', 'M'):
            /* Background color intentionally ignored: FFmpeg's demuxer only
             * forwards it with -usebgcolor, which the tests do not use. */
            decoder->animation = 1;
            break;
        case MKTAG('A', 'L', 'P', 'H'): {
            int alpha_header, filter_m, compression;

            if (size == 0)
                return set_error(decoder, "invalid ALPHA chunk size",
                                 WPD_ERROR_INVALID_DATA);
            alpha_header = payload[0];
            decoder->alpha_data      = payload + 1;
            decoder->alpha_data_size = size - 1;

            filter_m    = (alpha_header >> 2) & 0x03;
            compression =  alpha_header       & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL, WPD_LOG_WARNING,
                        "skipping unsupported ALPHA chunk\n");
            } else {
                decoder->has_alpha         = 1;
                decoder->alpha_compression = compression;
                decoder->alpha_filter      = filter_m;
            }
            break;
        }
        case MKTAG('V', 'P', '8', ' '):
            if (decoder->animation || decoder->still_done)
                break;
            decoder->width = decoder->height = 0;
            ret = vp8_lossy_decode_frame(decoder, &decoder->subframe,
                                         payload, size);
            if (ret < 0)
                return set_error(decoder, "VP8 decode failed", ret);
            decoder->still_done = 1;
            export_frame(&decoder->subframe, frame);
            return 1;
        case MKTAG('V', 'P', '8', 'L'):
            if (decoder->animation || decoder->still_done)
                break;
            decoder->width = decoder->height = 0;
            ret = vp8_lossless_decode_frame(decoder, &decoder->argb,
                                            payload, size, 0);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
            decoder->still_done = 1;
            export_frame(&decoder->argb, frame);
            return 1;
        case MKTAG('A', 'N', 'M', 'F'):
            if (!decoder->animation ||
                !decoder->canvas_width || !decoder->canvas_height)
                return set_error(decoder, "ANMF chunk without animation header",
                                 WPD_ERROR_INVALID_DATA);
            ret = decode_anmf(decoder, payload, size);
            if (ret < 0)
                return set_error(decoder, "animation frame decode failed", ret);
            export_frame(&decoder->canvas, frame);
            return 1;
        default:
            break;
        }
    }

    return 0;
}

const char *wpd_decoder_error(const WPDDecoder *decoder)
{
    return decoder && decoder->error[0] ? decoder->error : "unknown decoder error";
}

void wpd_decoder_free(WPDDecoder *decoder)
{
    if (!decoder)
        return;
    if (decoder->vp8_initialized)
        vp8_decode_free(&decoder->codec);
    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    image_free(&decoder->alpha_argb);
    for (int i = 0; i < IMAGE_ROLE_NB; i++)
        image_ctx_free(&decoder->image[i]);
    free(decoder->alpha_plane);
    free(decoder->file);
    free(decoder);
}
