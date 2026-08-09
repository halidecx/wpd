
#include "wpd.h"

#include "vp8.h"
#include "vp8l_dsp.h"
#include "wpd_codec.h"

#include <stdio.h>
#include <stdlib.h>

#define VP8X_FLAG_ANIMATION 0x02
#define VP8X_FLAG_ALPHA 0x10

#define ANMF_FLAG_DISPOSE (1 << 0)
#define ANMF_FLAG_NO_BLEND (1 << 1)

#define NUM_CODE_LENGTH_CODES 19
#define HUFFMAN_CODES_PER_META_CODE 5
#define NUM_LITERAL_CODES 256
#define NUM_LENGTH_CODES 24
#define NUM_DISTANCE_CODES 40
#define NUM_SHORT_DISTANCES 120
#define MAX_HUFFMAN_CODE_LENGTH 15

#define WPD_FILE_PADDING 64

#define MKTAG(a, b, c, d)                                       \
    ((uint32_t)(a) | (uint32_t)(b) << 8 | (uint32_t)(c) << 16 | \
     (uint32_t)(d) << 24)

static const uint16_t alphabet_sizes[HUFFMAN_CODES_PER_META_CODE] = {
    NUM_LITERAL_CODES + NUM_LENGTH_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_DISTANCE_CODES};

static const uint8_t code_length_code_order[NUM_CODE_LENGTH_CODES] = {
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15};

static const int8_t lz77_distance_offsets[NUM_SHORT_DISTANCES][2] = {
    {0, 1},  {1, 0},  {1, 1},  {-1, 1}, {0, 2},  {2, 0},  {1, 2},  {-1, 2},
    {2, 1},  {-2, 1}, {2, 2},  {-2, 2}, {0, 3},  {3, 0},  {1, 3},  {-1, 3},
    {3, 1},  {-3, 1}, {2, 3},  {-2, 3}, {3, 2},  {-3, 2}, {0, 4},  {4, 0},
    {1, 4},  {-1, 4}, {4, 1},  {-4, 1}, {3, 3},  {-3, 3}, {2, 4},  {-2, 4},
    {4, 2},  {-4, 2}, {0, 5},  {3, 4},  {-3, 4}, {4, 3},  {-4, 3}, {5, 0},
    {1, 5},  {-1, 5}, {5, 1},  {-5, 1}, {2, 5},  {-2, 5}, {5, 2},  {-5, 2},
    {4, 4},  {-4, 4}, {3, 5},  {-3, 5}, {5, 3},  {-5, 3}, {0, 6},  {6, 0},
    {1, 6},  {-1, 6}, {6, 1},  {-6, 1}, {2, 6},  {-2, 6}, {6, 2},  {-6, 2},
    {4, 5},  {-4, 5}, {5, 4},  {-5, 4}, {3, 6},  {-3, 6}, {6, 3},  {-6, 3},
    {0, 7},  {7, 0},  {1, 7},  {-1, 7}, {5, 5},  {-5, 5}, {7, 1},  {-7, 1},
    {4, 6},  {-4, 6}, {6, 4},  {-6, 4}, {2, 7},  {-2, 7}, {7, 2},  {-7, 2},
    {3, 7},  {-3, 7}, {7, 3},  {-7, 3}, {5, 6},  {-5, 6}, {6, 5},  {-6, 5},
    {8, 0},  {4, 7},  {-4, 7}, {7, 4},  {-7, 4}, {8, 1},  {8, 2},  {6, 6},
    {-6, 6}, {8, 3},  {5, 7},  {-5, 7}, {7, 5},  {-7, 5}, {8, 4},  {6, 7},
    {-6, 7}, {7, 6},  {-7, 6}, {8, 5},  {7, 7},  {-7, 7}, {8, 6},  {8, 7}};

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

#define SCALEBITS 10
#define ONE_HALF (1 << (SCALEBITS - 1))
#define FIX(x) ((int)((x) * (1 << SCALEBITS) + 0.5))

#define YUV_TO_RGB1_CCIR(cb1, cr1)                            \
    do {                                                      \
        cb    = (cb1) - 128;                                  \
        cr    = (cr1) - 128;                                  \
        r_add = FIX(1.40200 * 255.0 / 224.0) * cr + ONE_HALF; \
        g_add = -FIX(0.34414 * 255.0 / 224.0) * cb -          \
            FIX(0.71414 * 255.0 / 224.0) * cr + ONE_HALF;     \
        b_add = FIX(1.77200 * 255.0 / 224.0) * cb + ONE_HALF; \
    } while (0)

#define YUV_TO_RGB2_CCIR(r, g, b, y1)                 \
    do {                                              \
        y = ((y1) - 16) * FIX(255.0 / 219.0);         \
        r = wpd_clip_uint8((y + r_add) >> SCALEBITS); \
        g = wpd_clip_uint8((y + g_add) >> SCALEBITS); \
        b = wpd_clip_uint8((y + b_add) >> SCALEBITS); \
    } while (0)

#define RGB_TO_Y_CCIR(r, g, b)                                                \
    ((FIX(0.29900 * 219.0 / 255.0) * (r) +                                    \
      FIX(0.58700 * 219.0 / 255.0) * (g) +                                    \
      FIX(0.11400 * 219.0 / 255.0) * (b) + (ONE_HALF + (16 << SCALEBITS))) >> \
     SCALEBITS)

#define RGB_TO_U_CCIR(r1, g1, b1, shift)                                       \
    (((-FIX(0.16874 * 224.0 / 255.0) * r1 -                                    \
       FIX(0.33126 * 224.0 / 255.0) * g1 + FIX(0.50000 * 224.0 / 255.0) * b1 + \
       (ONE_HALF << shift) - 1) >>                                             \
      (SCALEBITS + shift)) +                                                   \
     128)

#define RGB_TO_V_CCIR(r1, g1, b1, shift)                                       \
    (((FIX(0.50000 * 224.0 / 255.0) * r1 - FIX(0.41869 * 224.0 / 255.0) * g1 - \
       FIX(0.08131 * 224.0 / 255.0) * b1 + (ONE_HALF << shift) - 1) >>         \
      (SCALEBITS + shift)) +                                                   \
     128)

static wpd_always_inline uint32_t rb32(const uint8_t *p) {
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 |
        p[3];
}

static wpd_always_inline void wb32(uint8_t *p, uint32_t v) {
    p[0] = v >> 24;
    p[1] = v >> 16;
    p[2] = v >> 8;
    p[3] = v;
}

static wpd_always_inline void copy32(uint8_t *dst, const uint8_t *src) {
    memcpy(dst, src, 4);
}

static wpd_always_inline int u8_to_s8(uint8_t v) { return (int8_t)v; }

#define CEIL_RSHIFT(v, s) (-((-(v)) >> (s)))

#define BR_MAX_BITS 24
#define BR_LBITS 64
#define BR_WBITS 32

static const uint32_t br_bit_mask[BR_MAX_BITS + 1] = {
    0,        0x000001, 0x000003, 0x000007, 0x00000f, 0x00001f, 0x00003f,
    0x00007f, 0x0000ff, 0x0001ff, 0x0003ff, 0x0007ff, 0x000fff, 0x001fff,
    0x003fff, 0x007fff, 0x00ffff, 0x01ffff, 0x03ffff, 0x07ffff, 0x0fffff,
    0x1fffff, 0x3fffff, 0x7fffff, 0xffffff};

typedef struct LEBitReader {
    uint64_t       val;
    const uint8_t *buf;
    size_t         len;
    size_t         pos;
    int            bit_pos;
    int            eos;
} LEBitReader;

/* VP8L is LSB-first; refills leave 32 usable bits in a 64-bit window. */
static void br_init(LEBitReader *br, const uint8_t *buf, size_t size) {
    size_t   prefetch = WPD_MIN(size, sizeof(br->val));
    uint64_t value    = 0;

    br->buf     = buf;
    br->len     = size;
    br->bit_pos = 0;
    br->eos     = 0;

    for (size_t i = 0; i < prefetch; i++) value |= (uint64_t)buf[i] << (8 * i);
    br->val = value;
    br->pos = prefetch;
}

static wpd_always_inline int br_is_eos(const LEBitReader *br) {
    return br->eos || (br->pos == br->len && br->bit_pos > BR_LBITS);
}

static void br_set_eos(LEBitReader *br) {
    /* Reset bit_pos so later prefetch shifts remain defined. */
    br->eos     = 1;
    br->bit_pos = 0;
}

static void br_shift_bytes(LEBitReader *br) {
    while (br->bit_pos >= 8 && br->pos < br->len) {
        br->val >>= 8;
        br->val |= (uint64_t)br->buf[br->pos] << (BR_LBITS - 8);
        br->pos++;
        br->bit_pos -= 8;
    }
    if (br_is_eos(br))
        br_set_eos(br);
}

static wpd_always_inline uint32_t br_prefetch(const LEBitReader *br) {
    return (uint32_t)(br->val >> (br->bit_pos & (BR_LBITS - 1)));
}

static wpd_always_inline void br_set_bit_pos(LEBitReader *br, int pos) {
    br->bit_pos = pos;
}

static void br_do_fill(LEBitReader *br) {
    if (br->pos + sizeof(br->val) < br->len) {
        br->val >>= BR_WBITS;
        br->bit_pos -= BR_WBITS;
        br->val |= (uint64_t)WPD_RL32(br->buf + br->pos)
            << (BR_LBITS - BR_WBITS);
        br->pos += BR_WBITS / 8;
        return;
    }
    br_shift_bytes(br);
}

static wpd_always_inline void br_fill(LEBitReader *br) {
    if (br->bit_pos >= BR_WBITS)
        br_do_fill(br);
}

static wpd_always_inline unsigned br_bits(LEBitReader *br, int n) {
    if (!br->eos && n <= BR_MAX_BITS) {
        const uint32_t v = br_prefetch(br) & br_bit_mask[n];
        br->bit_pos += n;
        br_shift_bytes(br);
        return v;
    }
    br_set_eos(br);
    return 0;
}

static wpd_always_inline unsigned br_bit(LEBitReader *br) {
    return br_bits(br, 1);
}

typedef struct WebPImage {
    uint8_t       *data[4];
    uint8_t       *alloc[4];
    size_t         alloc_size[4];
    int            linesize[4];
    int            width, height;
    WPDPixelFormat format;
} WebPImage;

static void image_free(WebPImage *img) {
    for (int p = 0; p < 4; p++) wpd_free(img->alloc[p]);
    memset(img, 0, sizeof(*img));
}

static uint8_t *image_alloc_plane(WebPImage *img, int p, size_t size) {
    if (img->alloc[p] && img->alloc_size[p] >= size) {
        memset(img->alloc[p], 0, size);
        return img->alloc[p];
    }
    wpd_free(img->alloc[p]);
    img->alloc[p]      = wpd_mallocz(size);
    img->alloc_size[p] = img->alloc[p] ? size : 0;
    return img->alloc[p];
}

static int image_alloc_argb(WebPImage *img, int w, int h) {
    const size_t size = (size_t)w * 4 * h + WPD_FILE_PADDING;

    for (int p = 1; p < 4; p++) {
        wpd_free(img->alloc[p]);
        img->alloc[p]      = NULL;
        img->alloc_size[p] = 0;
        img->data[p]       = NULL;
        img->linesize[p]   = 0;
    }
    img->linesize[0] = w * 4;
    img->data[0]     = image_alloc_plane(img, 0, size);
    if (!img->data[0])
        return WPD_ERROR(ENOMEM);
    img->width  = w;
    img->height = h;
    img->format = WPD_PIX_FMT_ARGB;
    return 0;
}

static int image_alloc_yuva(WebPImage *img, int w, int h) {
    for (int p = 0; p < 4; p++) {
        int pw           = (p == 1 || p == 2) ? (w + 1) / 2 : w;
        int ph           = (p == 1 || p == 2) ? (h + 1) / 2 : h;
        img->linesize[p] = pw;
        img->data[p]     = image_alloc_plane(
            img, p, (size_t)pw * ph + WPD_FILE_PADDING);
        if (!img->data[p]) {
            image_free(img);
            return WPD_ERROR(ENOMEM);
        }
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

#define HUFF_TABLE_BITS 8
#define HUFF_TABLE_MASK ((1 << HUFF_TABLE_BITS) - 1)

typedef struct HuffCode {
    uint8_t  bits;
    uint16_t value;
} HuffCode;

typedef struct HuffReader {
    HuffCode *table;
} HuffReader;

/* An 8-bit root table sends longer canonical codes to secondary tables. */
static void huff_reader_free(HuffReader *r) { wpd_freep(&r->table); }

static wpd_always_inline uint32_t huff_next_key(uint32_t key, int len) {
    uint32_t step = 1u << (len - 1);

    while (key & step) step >>= 1;
    return step ? (key & (step - 1)) + step : key;
}

static wpd_always_inline void huff_replicate(HuffCode *table, int step, int end,
                                             HuffCode code) {
    do {
        end -= step;
        table[end] = code;
    } while (end > 0);
}

static int huff_next_table_bits(const int *count, int len, int root_bits) {
    int left = 1 << (len - root_bits);

    while (len < MAX_HUFFMAN_CODE_LENGTH) {
        left -= count[len];
        if (left <= 0)
            break;
        len++;
        left <<= 1;
    }
    return len - root_bits;
}

static int huff_build_table(HuffCode *root_table, int root_bits,
                            const uint8_t *code_lengths, int code_lengths_size,
                            uint16_t *sorted) {
    /* The first call sizes the tables; the second call fills them. */
    HuffCode *table                              = root_table;
    int       total_size                         = 1 << root_bits;
    int       count[MAX_HUFFMAN_CODE_LENGTH + 1] = {0};
    int       offset[MAX_HUFFMAN_CODE_LENGTH + 1];
    int       len, symbol, step;

    for (symbol = 0; symbol < code_lengths_size; symbol++) {
        if (code_lengths[symbol] > MAX_HUFFMAN_CODE_LENGTH)
            return 0;
        count[code_lengths[symbol]]++;
    }
    if (count[0] == code_lengths_size)
        return 0;

    offset[1] = 0;
    for (len = 1; len < MAX_HUFFMAN_CODE_LENGTH; len++) {
        if (count[len] > (1 << len))
            return 0;
        offset[len + 1] = offset[len] + count[len];
    }

    for (symbol = 0; symbol < code_lengths_size; symbol++) {
        const int len_sym = code_lengths[symbol];
        if (len_sym > 0) {
            if (sorted) {
                if (offset[len_sym] >= code_lengths_size)
                    return 0;
                sorted[offset[len_sym]++] = symbol;
            } else {
                offset[len_sym]++;
            }
        }
    }

    if (offset[MAX_HUFFMAN_CODE_LENGTH] == 1) {
        if (sorted) {
            HuffCode code;
            code.bits  = 0;
            code.value = sorted[0];
            huff_replicate(table, 1, total_size, code);
        }
        return total_size;
    }

    {
        uint32_t low        = 0xFFFFFFFFu;
        uint32_t mask       = total_size - 1;
        uint32_t key        = 0;
        int      num_nodes  = 1;
        int      num_open   = 1;
        int      table_bits = root_bits;
        int      table_size = 1 << table_bits;

        symbol = 0;
        for (len = 1, step = 2; len <= root_bits; len++, step <<= 1) {
            num_open <<= 1;
            num_nodes += num_open;
            num_open -= count[len];
            if (num_open < 0)
                return 0;
            if (!root_table)
                continue;
            for (; count[len] > 0; count[len]--) {
                HuffCode code;
                code.bits  = len;
                code.value = sorted[symbol++];
                huff_replicate(&table[key], step, table_size, code);
                key = huff_next_key(key, len);
            }
        }

        for (len = root_bits + 1, step = 2; len <= MAX_HUFFMAN_CODE_LENGTH;
             len++, step <<= 1) {
            num_open <<= 1;
            num_nodes += num_open;
            num_open -= count[len];
            if (num_open < 0)
                return 0;
            for (; count[len] > 0; count[len]--) {
                HuffCode code;
                if ((key & mask) != low) {
                    if (root_table)
                        table += table_size;
                    table_bits = huff_next_table_bits(count, len, root_bits);
                    table_size = 1 << table_bits;
                    total_size += table_size;
                    low = key & mask;
                    if (root_table) {
                        root_table[low].bits  = table_bits + root_bits;
                        root_table[low].value = (table - root_table) - low;
                    }
                }
                if (root_table) {
                    code.bits  = len - root_bits;
                    code.value = sorted[symbol++];
                    huff_replicate(
                        &table[key >> root_bits], step, table_size, code);
                }
                key = huff_next_key(key, len);
            }
        }

        if (num_nodes != 2 * offset[MAX_HUFFMAN_CODE_LENGTH] - 1)
            return 0;
    }

    return total_size;
}

static int huff_reader_build(HuffReader *r, const uint8_t *code_lengths,
                             int alphabet_size, uint16_t *sorted) {
    int total_size;

    huff_reader_free(r);

    total_size = huff_build_table(
        NULL, HUFF_TABLE_BITS, code_lengths, alphabet_size, NULL);
    if (total_size == 0)
        return WPD_ERROR_INVALID_DATA;

    r->table = malloc((size_t)total_size * sizeof(*r->table));
    if (!r->table)
        return WPD_ERROR(ENOMEM);

    huff_build_table(
        r->table, HUFF_TABLE_BITS, code_lengths, alphabet_size, sorted);
    return 0;
}

static wpd_always_inline int huff_read_symbol(const HuffCode *table,
                                              LEBitReader    *br) {
    uint32_t val = br_prefetch(br);
    int      nbits;

    table += val & HUFF_TABLE_MASK;
    nbits = table->bits - HUFF_TABLE_BITS;
    if (nbits > 0) {
        br_set_bit_pos(br, br->bit_pos + HUFF_TABLE_BITS);
        val = br_prefetch(br);
        table += table->value;
        table += val & ((1u << nbits) - 1);
    }
    br_set_bit_pos(br, br->bit_pos + table->bits);
    return table->value;
}

typedef struct HTreeGroup {
    HuffReader trees[HUFFMAN_CODES_PER_META_CODE];
    int        trivial_literal;
    uint8_t    literal[4];
} HTreeGroup;

typedef struct ImageContext {
    enum ImageRole role;
    WebPImage     *frame;
    WebPImage      storage;
    int            color_cache_bits;
    uint32_t      *color_cache;
    int            nb_huffman_groups;
    HTreeGroup    *huffman_groups;
    int            size_reduction;
    int            is_alpha_primary;
} ImageContext;

struct WPDDecoder {
    WpdCodecContext codec;
    VP8Context      vp8;
    int             vp8_initialized;
    WPDLosslessDSP  ldsp;

    uint8_t *file;
    size_t   file_size;
    size_t   pos, end;
    int      animation;
    int      still_done;
    int      frame_index;
    int      canvas_width, canvas_height;

    int                   has_alpha;
    enum AlphaCompression alpha_compression;
    enum AlphaFilter      alpha_filter;
    const uint8_t        *alpha_data;
    int                   alpha_data_size;
    uint8_t              *alpha_plane;
    size_t                alpha_plane_size;

    LEBitReader        gb;
    uint8_t           *alpha_dst;
    int                alpha_dst_stride;
    int                alpha_dst_used;
    int                width, height;
    int                lossless_has_alpha;
    int                nb_transforms;
    enum TransformType transforms[4];
    int                reduced_width;
    int                nb_huffman_groups;
    ImageContext       image[IMAGE_ROLE_NB];

    WebPImage argb;
    WebPImage alpha_argb;
    WebPImage subframe;

    WebPImage canvas;
    int       anmf_flags, pos_x, pos_y;
    int       frame_has_alpha, key_frame;
    int       prev_anmf_flags, prev_width, prev_height, prev_pos_x, prev_pos_y;
    int       prev_key_frame;
    uint8_t   clear_argb[4];
    uint8_t   clear_yuva[4];

    int      anim_loop_count, anim_frame_count;
    uint32_t anim_background_argb;
    int      frame_duration;
    int64_t  frame_timestamp;

    char error[128];
};

static void image_ctx_free(ImageContext *img) {
    wpd_free(img->color_cache);
    if (img->role != IMAGE_ROLE_ARGB)
        image_free(&img->storage);
    if (img->huffman_groups) {
        for (int i = 0; i < img->nb_huffman_groups; i++)
            for (int j = 0; j < HUFFMAN_CODES_PER_META_CODE; j++)
                huff_reader_free(&img->huffman_groups[i].trees[j]);
        wpd_free(img->huffman_groups);
    }
    memset(img, 0, sizeof(*img));
}

static void read_huffman_code_simple(WPDDecoder *s, uint8_t *code_lengths,
                                     int alphabet_size) {
    int nb_symbols = br_bit(&s->gb) + 1;
    int symbol;

    symbol = br_bit(&s->gb) ? br_bits(&s->gb, 8) : br_bit(&s->gb);
    if (symbol < alphabet_size)
        code_lengths[symbol] = 1;

    if (nb_symbols == 2) {
        symbol = br_bits(&s->gb, 8);
        if (symbol < alphabet_size)
            code_lengths[symbol] = 1;
    }
}

static int read_huffman_code_normal(WPDDecoder *s, uint8_t *code_lengths,
                                    int alphabet_size) {
    /* Code lengths are 3 bits wide, so this table never needs a second level. */
    HuffCode code_len_table[1 << HUFF_TABLE_BITS];
    uint16_t sorted[NUM_CODE_LENGTH_CODES];
    uint8_t  code_length_code_lengths[NUM_CODE_LENGTH_CODES] = {0};
    int      symbol, max_symbol, prev_code_len, ret;
    int      num_codes = 4 + br_bits(&s->gb, 4);

    for (int i = 0; i < num_codes; i++)
        code_length_code_lengths[code_length_code_order[i]] = br_bits(&s->gb,
                                                                      3);

    if (br_bit(&s->gb)) {
        int bits   = 2 + 2 * br_bits(&s->gb, 3);
        max_symbol = 2 + br_bits(&s->gb, bits);
        if (max_symbol > alphabet_size) {
            wpd_log(NULL,
                    WPD_LOG_ERROR,
                    "max symbol %d > alphabet size %d\n",
                    max_symbol,
                    alphabet_size);
            return WPD_ERROR_INVALID_DATA;
        }
    } else {
        max_symbol = alphabet_size;
    }

    if (huff_build_table(NULL,
                         HUFF_TABLE_BITS,
                         code_length_code_lengths,
                         NUM_CODE_LENGTH_CODES,
                         NULL) != 1 << HUFF_TABLE_BITS)
        return WPD_ERROR_INVALID_DATA;
    huff_build_table(code_len_table,
                     HUFF_TABLE_BITS,
                     code_length_code_lengths,
                     NUM_CODE_LENGTH_CODES,
                     sorted);

    prev_code_len = 8;
    symbol        = 0;
    while (symbol < alphabet_size) {
        int code_len;

        if (!max_symbol--)
            break;
        if (br_is_eos(&s->gb))
            break;
        br_fill(&s->gb);
        code_len = huff_read_symbol(code_len_table, &s->gb);
        if (code_len < 16) {
            code_lengths[symbol++] = code_len;
            if (code_len)
                prev_code_len = code_len;
        } else {
            int repeat = 0, length = 0;
            switch (code_len) {
            default: ret = WPD_ERROR_INVALID_DATA; goto finish;
            case 16:
                repeat = 3 + br_bits(&s->gb, 2);
                length = prev_code_len;
                break;
            case 17: repeat = 3 + br_bits(&s->gb, 3); break;
            case 18: repeat = 11 + br_bits(&s->gb, 7); break;
            }
            if (symbol + repeat > alphabet_size) {
                wpd_log(NULL,
                        WPD_LOG_ERROR,
                        "invalid symbol %d + repeat %d > alphabet size %d\n",
                        symbol,
                        repeat,
                        alphabet_size);
                ret = WPD_ERROR_INVALID_DATA;
                goto finish;
            }
            while (repeat-- > 0) code_lengths[symbol++] = length;
        }
    }

    ret = 0;

finish:
    return ret;
}

static int decode_entropy_coded_image(WPDDecoder *s, enum ImageRole role, int w,
                                      int h);

#define PARSE_BLOCK_SIZE(w, h)                                    \
    do {                                                          \
        block_bits = br_bits(&s->gb, 3) + 2;                      \
        blocks_w   = ((w) + (1 << block_bits) - 1) >> block_bits; \
        blocks_h   = ((h) + (1 << block_bits) - 1) >> block_bits; \
    } while (0)

static int decode_entropy_image(WPDDecoder *s) {
    ImageContext *img;
    int           ret, block_bits, blocks_w, blocks_h, x, y, max;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(s, IMAGE_ROLE_ENTROPY, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    img                 = &s->image[IMAGE_ROLE_ENTROPY];
    img->size_reduction = block_bits;

    max = 0;
    for (y = 0; y < img->frame->height; y++) {
        for (x = 0; x < img->frame->width; x++) {
            int p0 = GET_PIXEL_COMP(img->frame, x, y, 1);
            int p1 = GET_PIXEL_COMP(img->frame, x, y, 2);
            int p  = p0 << 8 | p1;
            max    = WPD_MAX(max, p);
        }
    }
    s->nb_huffman_groups = max + 1;

    return 0;
}

static int parse_transform_predictor(WPDDecoder *s) {
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(
        s, IMAGE_ROLE_PREDICTOR, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    s->image[IMAGE_ROLE_PREDICTOR].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color(WPDDecoder *s) {
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(s->reduced_width, s->height);

    ret = decode_entropy_coded_image(
        s, IMAGE_ROLE_COLOR_TRANSFORM, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    s->image[IMAGE_ROLE_COLOR_TRANSFORM].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color_indexing(WPDDecoder *s) {
    ImageContext *img;
    int           width_bits, index_size, ret, x;
    uint8_t      *ct;

    index_size = br_bits(&s->gb, 8) + 1;

    if (index_size <= 2)
        width_bits = 3;
    else if (index_size <= 4)
        width_bits = 2;
    else if (index_size <= 16)
        width_bits = 1;
    else
        width_bits = 0;

    ret = decode_entropy_coded_image(
        s, IMAGE_ROLE_COLOR_INDEXING, index_size, 1);
    if (ret < 0)
        return ret;

    img                 = &s->image[IMAGE_ROLE_COLOR_INDEXING];
    img->size_reduction = width_bits;
    if (width_bits > 0)
        s->reduced_width = (s->width + ((1 << width_bits) - 1)) >> width_bits;

    ct = img->frame->data[0] + 4;
    for (x = 4; x < img->frame->width * 4; x++, ct++) ct[0] += ct[-4];

    return 0;
}

static HTreeGroup *get_huffman_group(WPDDecoder *s, ImageContext *img, int x,
                                     int y) {
    ImageContext *gimg  = &s->image[IMAGE_ROLE_ENTROPY];
    int           group = 0;

    if (gimg->size_reduction > 0) {
        int group_x = x >> gimg->size_reduction;
        int group_y = y >> gimg->size_reduction;
        int g0      = GET_PIXEL_COMP(gimg->frame, group_x, group_y, 1);
        int g1      = GET_PIXEL_COMP(gimg->frame, group_x, group_y, 2);
        group       = g0 << 8 | g1;
    }

    return &img->huffman_groups[group];
}

static wpd_always_inline void color_cache_put(ImageContext *img, uint32_t c) {
    uint32_t cache_idx = (0x1E35A7BD * c) >> (32 - img->color_cache_bits);
    img->color_cache[cache_idx] = c;
}

static wpd_always_inline const uint8_t *color_cache_fill(ImageContext  *img,
                                                         const uint8_t *from,
                                                         const uint8_t *to) {
    for (; from < to; from += 4) color_cache_put(img, rb32(from));
    return from;
}

static wpd_always_inline void copy_block32(uint8_t *dst, int dist, int length) {
    const uint8_t *src = dst - 4 * (ptrdiff_t)dist;
    const size_t   n   = (size_t)length * 4;
    size_t         i;

    if (dist >= length) {
        memcpy(dst, src, n);
    } else if (dist <= 2) {
        uint64_t pattern;

        if (dist == 1) {
            uint32_t v;
            memcpy(&v, src, 4);
            pattern = (uint64_t)v << 32 | v;
        } else {
            memcpy(&pattern, src, 8);
        }
        for (i = 0; i + 8 <= n; i += 8) memcpy(dst + i, &pattern, 8);
        if (i < n)
            memcpy(dst + i, &pattern, 4);
    } else {
        for (i = 0; i < n; i += 4) memcpy(dst + i, src + i, 4);
    }
}

static int decode_entropy_coded_image(WPDDecoder *s, enum ImageRole role, int w,
                                      int h) {
    ImageContext *img;
    HTreeGroup   *hg;
    uint8_t      *code_lengths;
    uint16_t     *sorted;
    int           i, j, ret, x, y, width, max_alphabet_size;

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
            wpd_log(NULL,
                    WPD_LOG_ERROR,
                    "invalid color cache bits: %d\n",
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
                                      sizeof(*img->huffman_groups));
    if (!img->huffman_groups)
        return WPD_ERROR(ENOMEM);

    max_alphabet_size = alphabet_sizes[HUFF_IDX_GREEN];
    if (img->color_cache_bits > 0)
        max_alphabet_size += 1 << img->color_cache_bits;
    sorted = malloc((size_t)max_alphabet_size * (sizeof(uint16_t) + 1));
    if (!sorted)
        return WPD_ERROR(ENOMEM);
    code_lengths = (uint8_t *)(sorted + max_alphabet_size);

    for (i = 0; i < img->nb_huffman_groups; i++) {
        hg = &img->huffman_groups[i];
        for (j = 0; j < HUFFMAN_CODES_PER_META_CODE; j++) {
            int alphabet_size = alphabet_sizes[j];
            if (!j && img->color_cache_bits > 0)
                alphabet_size += 1 << img->color_cache_bits;

            memset(code_lengths, 0, alphabet_size);
            if (br_bit(&s->gb))
                read_huffman_code_simple(s, code_lengths, alphabet_size);
            else
                ret = read_huffman_code_normal(s, code_lengths, alphabet_size);
            if (ret >= 0)
                ret = huff_reader_build(
                    &hg->trees[j], code_lengths, alphabet_size, sorted);
            if (ret < 0) {
                free(sorted);
                return ret;
            }
        }

        hg->trivial_literal = hg->trees[HUFF_IDX_RED].table[0].bits == 0 &&
            hg->trees[HUFF_IDX_BLUE].table[0].bits == 0 &&
            hg->trees[HUFF_IDX_ALPHA].table[0].bits == 0;
        if (hg->trivial_literal) {
            hg->literal[0] = hg->trees[HUFF_IDX_ALPHA].table[0].value;
            hg->literal[1] = hg->trees[HUFF_IDX_RED].table[0].value;
            hg->literal[3] = hg->trees[HUFF_IDX_BLUE].table[0].value;
        }
    }
    free(sorted);

    width = img->frame->width;
    if (role == IMAGE_ROLE_ARGB && s->reduced_width < width) {
        /* Decode packed palette rows contiguously; expansion re-strides. */
        width                   = s->reduced_width;
        img->frame->linesize[0] = width * 4;
    }

    {
        const int      multi_group = img->nb_huffman_groups > 1;
        const int      huff_bits   = multi_group
            ? s->image[IMAGE_ROLE_ENTROPY].size_reduction
            : 0;
        const int      huff_mask   = huff_bits ? (1 << huff_bits) - 1 : ~0;
        uint8_t       *base        = img->frame->data[0];
        const size_t   total       = (size_t)width * img->frame->height;
        const uint8_t *cached      = base;
        size_t         pos         = 0;

        hg = &img->huffman_groups[0];
        x  = 0;
        y  = 0;
        while (pos < total) {
            uint8_t *p = base + 4 * pos;
            int      v;

            if (br_is_eos(&s->gb))
                return WPD_ERROR_INVALID_DATA;

            if ((x & huff_mask) == 0)
                hg = get_huffman_group(s, img, x, y);
            br_fill(&s->gb);
            v = huff_read_symbol(hg->trees[HUFF_IDX_GREEN].table, &s->gb);
            if (v < NUM_LITERAL_CODES) {
                if (hg->trivial_literal) {
                    copy32(p, hg->literal);
                    p[2] = v;
                } else {
                    int r = huff_read_symbol(hg->trees[HUFF_IDX_RED].table,
                                             &s->gb);
                    int b, a;
                    br_fill(&s->gb);
                    b    = huff_read_symbol(hg->trees[HUFF_IDX_BLUE].table,
                                            &s->gb);
                    a    = huff_read_symbol(hg->trees[HUFF_IDX_ALPHA].table,
                                            &s->gb);
                    p[0] = a;
                    p[1] = r;
                    p[2] = v;
                    p[3] = b;
                }
                pos++;
                if (++x == width) {
                    x = 0;
                    y++;
                    if (img->color_cache_bits)
                        cached = color_cache_fill(img, cached, base + 4 * pos);
                }
            } else if (v < NUM_LITERAL_CODES + NUM_LENGTH_CODES) {
                int prefix_code, length, distance;

                prefix_code = v - NUM_LITERAL_CODES;
                if (prefix_code < 4) {
                    length = prefix_code + 1;
                } else {
                    int extra_bits = (prefix_code - 2) >> 1;
                    int offset     = (2 + (prefix_code & 1)) << extra_bits;
                    length         = offset + br_bits(&s->gb, extra_bits) + 1;
                }
                prefix_code = huff_read_symbol(hg->trees[HUFF_IDX_DIST].table,
                                               &s->gb);
                br_fill(&s->gb);
                if (prefix_code > 39) {
                    wpd_log(NULL,
                            WPD_LOG_ERROR,
                            "distance prefix code too large: %d\n",
                            prefix_code);
                    return WPD_ERROR_INVALID_DATA;
                }
                if (prefix_code < 4) {
                    distance = prefix_code + 1;
                } else {
                    int extra_bits = (prefix_code - 2) >> 1;
                    int offset     = (2 + (prefix_code & 1)) << extra_bits;
                    distance       = offset + br_bits(&s->gb, extra_bits) + 1;
                }

                if (distance <= NUM_SHORT_DISTANCES) {
                    int xi   = lz77_distance_offsets[distance - 1][0];
                    int yi   = lz77_distance_offsets[distance - 1][1];
                    distance = WPD_MAX(1, xi + yi * width);
                } else {
                    distance -= NUM_SHORT_DISTANCES;
                }

                if ((size_t)distance > pos || (size_t)length > total - pos)
                    return WPD_ERROR_INVALID_DATA;

                copy_block32(p, distance, length);
                pos += length;
                x += length;
                while (x >= width) {
                    x -= width;
                    y++;
                }
                if (multi_group && (x & huff_mask))
                    hg = get_huffman_group(s, img, x, y);
                if (img->color_cache_bits)
                    cached = color_cache_fill(img, cached, base + 4 * pos);
            } else {
                int cache_idx = v - (NUM_LITERAL_CODES + NUM_LENGTH_CODES);

                if (!img->color_cache_bits) {
                    wpd_log(NULL, WPD_LOG_ERROR, "color cache not found\n");
                    return WPD_ERROR_INVALID_DATA;
                }
                if (cache_idx >= 1 << img->color_cache_bits) {
                    wpd_log(NULL,
                            WPD_LOG_ERROR,
                            "color cache index out-of-bounds\n");
                    return WPD_ERROR_INVALID_DATA;
                }
                cached = color_cache_fill(img, cached, p);
                wb32(p, img->color_cache[cache_idx]);
                pos++;
                if (++x == width) {
                    x = 0;
                    y++;
                }
            }
        }
    }

    return 0;
}

static int apply_predictor_transform(WPDDecoder *s) {
    ImageContext              *img       = &s->image[IMAGE_ROLE_ARGB];
    ImageContext              *pimg      = &s->image[IMAGE_ROLE_PREDICTOR];
    pred_add_func const *const pred_add  = s->ldsp.pred_add;
    const int                  width     = s->reduced_width;
    const int                  height    = img->frame->height;
    const int                  stride    = img->frame->linesize[0] / 4;
    const int                  tile_bits = pimg->size_reduction;
    const int                  tile_size = 1 << tile_bits;
    const int                  tile_mask = tile_size - 1;
    uint32_t                  *row       = (uint32_t *)img->frame->data[0];
    int                        y;

    if (width <= 0 || height <= 0)
        return 0;

    pred_add[0](row, NULL, 1, row);
    if (width > 1)
        pred_add[1](row + 1, NULL, width - 1, row + 1);

    for (y = 1, row += stride; y < height; y++, row += stride) {
        const uint32_t *upper = row - stride;
        const uint8_t  *modes = pimg->frame->data[0] +
            (y >> tile_bits) * pimg->frame->linesize[0];
        int x = 1;

        pred_add[2](row, upper, 1, row);

        while (x < width) {
            const unsigned m     = modes[(x >> tile_bits) * 4 + 2];
            int            x_end = (x & ~tile_mask) + tile_size;

            if (m > 13) {
                wpd_log(NULL, WPD_LOG_ERROR, "invalid predictor mode: %u\n", m);
                return WPD_ERROR_INVALID_DATA;
            }
            if (x_end > width)
                x_end = width;
            pred_add[m](row + x, upper + x, x_end - x, row + x);
            x = x_end;
        }
    }
    return 0;
}

static wpd_always_inline uint8_t color_transform_delta(uint8_t color_pred,
                                                       uint8_t color) {
    return u8_to_s8(color_pred) * u8_to_s8(color) >> 5;
}

static int apply_color_transform(WPDDecoder *s) {
    ImageContext *img       = &s->image[IMAGE_ROLE_ARGB];
    ImageContext *cimg      = &s->image[IMAGE_ROLE_COLOR_TRANSFORM];
    const int     width     = s->reduced_width;
    const int     height    = img->frame->height;
    const int     tile_bits = cimg->size_reduction;
    const int     tile_size = 1 << tile_bits;
    const int     tile_mask = tile_size - 1;
    int           y;

    for (y = 0; y < height; y++) {
        const uint8_t *mult_row = cimg->frame->data[0] +
            (y >> tile_bits) * cimg->frame->linesize[0];
        uint8_t *p = GET_PIXEL(img->frame, 0, y);
        int      x = 0;

        while (x < width) {
            const uint8_t *cp            = mult_row + (x >> tile_bits) * 4;
            const uint8_t  green_to_red  = cp[3];
            const uint8_t  green_to_blue = cp[2];
            const uint8_t  red_to_blue   = cp[1];
            int            x_end         = (x & ~tile_mask) + tile_size;

            if (x_end > width)
                x_end = width;
            for (; x < x_end; x++, p += 4) {
                p[1] += color_transform_delta(green_to_red, p[2]);
                p[3] += color_transform_delta(green_to_blue, p[2]) +
                    color_transform_delta(red_to_blue, p[1]);
            }
        }
    }
    return 0;
}

static int apply_subtract_green_transform(WPDDecoder *s) {
    ImageContext *img    = &s->image[IMAGE_ROLE_ARGB];
    const int     width  = s->reduced_width;
    const int     height = img->frame->height;
    int           x, y;

    for (y = 0; y < height; y++) {
        uint8_t *p = GET_PIXEL(img->frame, 0, y);
        for (x = 0; x < width; x++, p += 4) {
            p[1] += p[2];
            p[3] += p[2];
        }
    }
    return 0;
}

static int apply_color_indexing_transform_alpha(WPDDecoder *s) {
    ImageContext *img    = &s->image[IMAGE_ROLE_ARGB];
    ImageContext *pal    = &s->image[IMAGE_ROLE_COLOR_INDEXING];
    const int     width  = img->frame->width;
    const int     height = img->frame->height;
    const int     pal_w  = pal->frame->width;
    uint8_t       palette[256];
    int           i, x, y;

    for (i = 0; i < pal_w; i++)
        palette[i] = GET_PIXEL_COMP(pal->frame, i, 0, 2);
    memset(palette + pal_w, 0, sizeof(palette) - pal_w);

    for (y = 0; y < height; y++) {
        const uint8_t *src = GET_PIXEL(img->frame, 0, y) + 2;
        uint8_t       *dst = s->alpha_dst + (size_t)s->alpha_dst_stride * y;

        if (pal->size_reduction > 0) {
            const int      pixel_bits      = 8 >> pal->size_reduction;
            const int      pixels_per_byte = 1 << pal->size_reduction;
            const unsigned bit_mask        = (1u << pixel_bits) - 1;

            for (x = 0; x + pixels_per_byte <= width; x += pixels_per_byte) {
                unsigned packed = *src;
                src += 4;
                for (i = 0; i < pixels_per_byte; i++) {
                    *dst++ = palette[packed & bit_mask];
                    packed >>= pixel_bits;
                }
            }
            if (x < width) {
                unsigned packed = *src;
                for (; x < width; x++) {
                    *dst++ = palette[packed & bit_mask];
                    packed >>= pixel_bits;
                }
            }
        } else {
            for (x = 0; x < width; x++, src += 4) *dst++ = palette[*src];
        }
    }

    img->frame->linesize[0] = width * 4;
    s->alpha_dst_used       = 1;
    s->reduced_width        = s->width;
    return 0;
}

static wpd_always_inline void expand_palette_rows(uint8_t *base, int dst_stride,
                                                  int src_stride, int width,
                                                  int            height,
                                                  const uint8_t *expand,
                                                  int            ppb) {
    const int group_bytes = ppb * 4;
    const int full        = width / ppb;
    const int tail        = width - full * ppb;

    /* Bottom-up, right-to-left: dst_stride >= src_stride, so every write
       lands at or past the not-yet-read indices it derives from. */
    for (int y = height - 1; y >= 0; y--) {
        uint8_t       *dst = base + (size_t)y * dst_stride;
        const uint8_t *src = base + (size_t)y * src_stride;

        if (tail)
            memcpy(dst + 4 * (full * ppb),
                   expand + (size_t)src[4 * full + 2] * group_bytes,
                   (size_t)tail * 4);
        for (int b = full - 1; b >= 0; b--)
            memcpy(dst + 4 * (b * ppb),
                   expand + (size_t)src[4 * b + 2] * group_bytes,
                   (size_t)group_bytes);
    }
}

static int apply_color_indexing_transform(WPDDecoder *s) {
    ImageContext *img;
    ImageContext *pal;
    int           i, x, y;
    uint8_t      *p;

    img = &s->image[IMAGE_ROLE_ARGB];
    pal = &s->image[IMAGE_ROLE_COLOR_INDEXING];

    if (pal->size_reduction > 0) {
        const int      pixel_bits      = 8 >> pal->size_reduction;
        const int      pixels_per_byte = 1 << pal->size_reduction;
        const unsigned bit_mask        = (1u << pixel_bits) - 1;
        const int      width           = img->frame->width;
        const int      pal_size        = pal->frame->width * 4;
        uint8_t        palette[256 * 4];
        uint8_t        expand[256 * 8 * 4];
        uint8_t       *base       = GET_PIXEL(img->frame, 0, 0);
        const int      src_stride = img->frame->linesize[0];
        const int      dst_stride = width * 4;
        const int      height     = img->frame->height;

        memcpy(palette, GET_PIXEL(pal->frame, 0, 0), pal_size);
        memset(palette + pal_size, 0, sizeof(palette) - pal_size);

        for (i = 0; i < 256; i++) {
            unsigned packed = (unsigned)i;
            uint8_t *entry  = expand + (size_t)i * pixels_per_byte * 4;

            for (x = 0; x < pixels_per_byte; x++) {
                copy32(entry + x * 4, &palette[(packed & bit_mask) * 4]);
                packed >>= pixel_bits;
            }
        }

        switch (pixels_per_byte) {
        case 2:
            expand_palette_rows(
                base, dst_stride, src_stride, width, height, expand, 2);
            break;
        case 4:
            expand_palette_rows(
                base, dst_stride, src_stride, width, height, expand, 4);
            break;
        default:
            expand_palette_rows(
                base, dst_stride, src_stride, width, height, expand, 8);
            break;
        }
        img->frame->linesize[0] = dst_stride;
        s->reduced_width        = s->width;
        return 0;
    }

    if (img->frame->height * img->frame->width > 300) {
        uint32_t       palette[256];
        const int      size     = pal->frame->width * 4;
        const int      w        = img->frame->width;
        const int      h        = img->frame->height;
        const int      linesize = img->frame->linesize[0];
        uint8_t *const base     = GET_PIXEL(img->frame, 0, 0);

        memcpy(palette, GET_PIXEL(pal->frame, 0, 0), size);
        memset((uint8_t *)palette + size, 0, sizeof(palette) - size);
        for (y = 0; y < h; y++) {
            uint8_t *row = base + (size_t)y * linesize;

            s->ldsp.map_color32(row, row, palette, w);
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

static void update_canvas_size(WPDDecoder *s, int w, int h) {
    if (s->width && s->width != w)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Width mismatch. %d != %d\n", s->width, w);
    s->width = w;
    if (s->height && s->height != h)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Height mismatch. %d != %d\n", s->height, h);
    s->height = h;
}

static int vp8_lossless_decode_frame(WPDDecoder *s, WebPImage *out,
                                     const uint8_t *data_start,
                                     unsigned int   data_size,
                                     int            is_alpha_chunk) {
    int      w, h, ret, i;
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

    s->nb_transforms = 0;
    s->reduced_width = s->width;
    used             = 0;
    while (br_bit(&s->gb)) {
        enum TransformType transform = br_bits(&s->gb, 2);
        if (used & (1 << transform)) {
            wpd_log(NULL,
                    WPD_LOG_ERROR,
                    "Transform %d used more than once\n",
                    transform);
            ret = WPD_ERROR_INVALID_DATA;
            goto free_and_return;
        }
        used |= (1 << transform);
        s->transforms[s->nb_transforms++] = transform;
        ret                               = 0;
        switch (transform) {
        case PREDICTOR_TRANSFORM: ret = parse_transform_predictor(s); break;
        case COLOR_TRANSFORM: ret = parse_transform_color(s); break;
        case COLOR_INDEXING_TRANSFORM:
            ret = parse_transform_color_indexing(s);
            break;
        case SUBTRACT_GREEN: break;
        }
        if (ret < 0)
            goto free_and_return;
    }

    s->image[IMAGE_ROLE_ARGB].frame = out;
    if (is_alpha_chunk)
        s->image[IMAGE_ROLE_ARGB].is_alpha_primary = 1;
    ret = decode_entropy_coded_image(s, IMAGE_ROLE_ARGB, w, h);
    if (ret < 0)
        goto free_and_return;

    for (i = s->nb_transforms - 1; i >= 0; i--) {
        switch (s->transforms[i]) {
        case PREDICTOR_TRANSFORM: ret = apply_predictor_transform(s); break;
        case COLOR_TRANSFORM: ret = apply_color_transform(s); break;
        case SUBTRACT_GREEN: ret = apply_subtract_green_transform(s); break;
        case COLOR_INDEXING_TRANSFORM:
            if (s->alpha_dst && s->nb_transforms == 1)
                ret = apply_color_indexing_transform_alpha(s);
            else
                ret = apply_color_indexing_transform(s);
            break;
        }
        if (ret < 0)
            goto free_and_return;
    }

    ret = 0;

free_and_return:
    out->linesize[0] = out->width * 4;
    for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&s->image[i]);

    return ret;
}

static void alpha_inverse_prediction(WebPImage *frame, enum AlphaFilter m) {
    int      x, y, ls;
    uint8_t *dec;

    ls = frame->linesize[3];

    dec = frame->data[3] + 1;
    for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - 1);

    dec = frame->data[3] + ls;
    for (y = 1; y < frame->height; y++, dec += ls) *dec += *(dec - ls);

    switch (m) {
    case ALPHA_FILTER_HORIZONTAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - 1);
        }
        break;
    case ALPHA_FILTER_VERTICAL:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++) *dec += *(dec - ls);
        }
        break;
    case ALPHA_FILTER_GRADIENT:
        for (y = 1; y < frame->height; y++) {
            dec = frame->data[3] + y * ls + 1;
            for (x = 1; x < frame->width; x++, dec++)
                dec[0] += wpd_clip_uint8(*(dec - 1) + *(dec - ls) -
                                         *(dec - ls - 1));
        }
        break;
    case ALPHA_FILTER_NONE: break;
    }
}

static int vp8_lossy_decode_alpha(WPDDecoder *s, WebPImage *p,
                                  const uint8_t *data_start,
                                  unsigned int   data_size) {
    int y, ret;

    if (s->alpha_compression == ALPHA_COMPRESSION_NONE) {
        const uint8_t *src  = data_start;
        size_t         left = data_size;

        for (y = 0; y < s->height; y++) {
            size_t n = WPD_MIN((size_t)s->width, left);
            memcpy(p->data[3] + p->linesize[3] * y, src, n);
            src += n;
            left -= n;
        }
    } else if (s->alpha_compression == ALPHA_COMPRESSION_VP8L) {
        s->alpha_dst        = p->data[3];
        s->alpha_dst_stride = p->linesize[3];
        s->alpha_dst_used   = 0;

        ret = vp8_lossless_decode_frame(
            s, &s->alpha_argb, data_start, data_size, 1);
        s->alpha_dst = NULL;
        if (ret < 0) {
            image_free(&s->alpha_argb);
            return ret;
        }

        if (!s->alpha_dst_used)
            for (y = 0; y < s->height; y++)
                s->ldsp.extract_green(p->data[3] + p->linesize[3] * y,
                                      GET_PIXEL(&s->alpha_argb, 0, y),
                                      s->width);
        image_free(&s->alpha_argb);
    }

    if (s->alpha_filter)
        alpha_inverse_prediction(p, s->alpha_filter);

    return 0;
}

static int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                                  const uint8_t *data_start,
                                  unsigned int   data_size) {
    WpdPacket packet;
    WpdFrame  decoded;
    int       ret;

    if (!s->vp8_initialized) {
        s->codec.priv_data = &s->vp8;
        ret                = vp8_decode_init(&s->codec);
        if (ret < 0)
            return ret;
        s->vp8_initialized = 1;
    }

    packet.data = data_start;
    packet.size = data_size;
    ret         = vp8_decode_frame(&s->codec, &decoded, &packet);
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
            s->alpha_plane      = plane;
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

static int image_nb_components(const WebPImage *img) {
    switch (img->format) {
    case WPD_PIX_FMT_YUV420P: return 3;
    case WPD_PIX_FMT_YUVA420P: return 4;
    default: return 4;
    }
}

typedef struct SubRect {
    int x, y, w, h;
} SubRect;

static void blend_argb_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] + (r.y + y) * src->linesize[0] +
            r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (s->pos_y + r.y + y) * dst->linesize[0] + (s->pos_x + r.x) * 4;

        s->ldsp.blend_row_argb(dst_argb, src_argb, r.w);
    }
}

static void copy_argb_region(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] + (r.y + y) * src->linesize[0] +
            r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (s->pos_y + r.y + y) * dst->linesize[0] + (s->pos_x + r.x) * 4;

        memcpy(dst_argb, src_argb, (size_t)r.w * 4);
    }
}

static void blend_yuva_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    int base_x = s->pos_x + r.x, base_y = s->pos_y + r.y;

    for (int y = 0; y < CEIL_RSHIFT(r.h, 1); y++) {
        int            tile_h = WPD_MIN(r.h - y * 2, 2);
        const uint8_t *src_u  = src->data[1] +
            ((r.y >> 1) + y) * src->linesize[1] + (r.x >> 1);
        const uint8_t *src_v = src->data[2] +
            ((r.y >> 1) + y) * src->linesize[2] + (r.x >> 1);
        uint8_t *dst_u = dst->data[1] + ((base_y >> 1) + y) * dst->linesize[1] +
            (base_x >> 1);
        uint8_t *dst_v = dst->data[2] + ((base_y >> 1) + y) * dst->linesize[2] +
            (base_x >> 1);
        for (int x = 0; x < CEIL_RSHIFT(r.w, 1); x++) {
            int tile_w    = WPD_MIN(r.w - x * 2, 2);
            int src_alpha = 0;
            int dst_alpha = 0;
            for (int yy = 0; yy < tile_h; yy++) {
                for (int xx = 0; xx < tile_w; xx++) {
                    src_alpha +=
                        src->data[3][(r.y + y * 2 + yy) * src->linesize[3] +
                                     (r.x + x * 2 + xx)];
                    dst_alpha +=
                        dst->data[3][(base_y + y * 2 + yy) * dst->linesize[3] +
                                     (base_x + x * 2 + xx)];
                }
            }
            int shift = (tile_h == 2) + (tile_w == 2);
            src_alpha = CEIL_RSHIFT(src_alpha, shift);
            dst_alpha = CEIL_RSHIFT(dst_alpha, shift);

            if (src_alpha == 255) {
                *dst_u = *src_u;
                *dst_v = *src_v;
            } else if (src_alpha == 0) {
            } else {
                int tmp_alpha   = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale       = (1UL << 24) / blend_alpha;
                *dst_u          = (((uint32_t)(*src_u * src_alpha +
                                               *dst_u * tmp_alpha)) *
                                   scale) >>
                    24;
                *dst_v = (((uint32_t)(*src_v * src_alpha +
                                      *dst_v * tmp_alpha)) *
                          scale) >>
                    24;
            }
            src_u += 1;
            src_v += 1;
            dst_u += 1;
            dst_v += 1;
        }
    }

    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_y = src->data[0] + (r.y + y) * src->linesize[0] +
            r.x;
        const uint8_t *src_a = src->data[3] + (r.y + y) * src->linesize[3] +
            r.x;
        uint8_t *dst_y = dst->data[0] + (base_y + y) * dst->linesize[0] +
            base_x;
        uint8_t *dst_a = dst->data[3] + (base_y + y) * dst->linesize[3] +
            base_x;
        for (int x = 0; x < r.w; x++) {
            int src_alpha = *src_a;
            int dst_alpha = *dst_a;

            if (src_alpha == 255) {
                *dst_y = *src_y;
                *dst_a = 255;
            } else if (src_alpha == 0) {
            } else {
                int tmp_alpha   = (dst_alpha * (256 - src_alpha)) >> 8;
                int blend_alpha = src_alpha + tmp_alpha;
                int scale       = (1UL << 24) / blend_alpha;
                *dst_y          = (((uint32_t)(*src_y * src_alpha +
                                               *dst_y * tmp_alpha)) *
                                   scale) >>
                    24;
                *dst_a = blend_alpha;
            }
            src_y += 1;
            src_a += 1;
            dst_y += 1;
            dst_a += 1;
        }
    }
}

static void copy_yuva_region(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, SubRect r) {
    int nb_components = image_nb_components(src);
    int base_x = s->pos_x + r.x, base_y = s->pos_y + r.y;

    for (int comp = 0; comp < nb_components; comp++) {
        int            shift = (comp == 1 || comp == 2) ? 1 : 0;
        const uint8_t *src_p = src->data[comp] +
            (r.y >> shift) * src->linesize[comp] + (r.x >> shift);
        uint8_t *dst_p = dst->data[comp] +
            (base_y >> shift) * dst->linesize[comp] + (base_x >> shift);

        for (int y = 0; y < CEIL_RSHIFT(r.h, shift); y++) {
            memcpy(dst_p, src_p, CEIL_RSHIFT(r.w, shift));
            src_p += src->linesize[comp];
            dst_p += dst->linesize[comp];
        }
    }

    if (nb_components < 4) {
        uint8_t *dst_a = dst->data[3] + base_y * dst->linesize[3] + base_x;

        for (int y = 0; y < r.h; y++) {
            memset(dst_a, 255, r.w);
            dst_a += dst->linesize[3];
        }
    }
}

static wpd_always_inline void webp_yuva2argb(uint8_t *out, int Y, int U, int V,
                                             int A) {
    uint8_t r, g, b;
    int     y, cb, cr;
    int     r_add, g_add, b_add;

    YUV_TO_RGB1_CCIR(U, V);
    YUV_TO_RGB2_CCIR(r, g, b, Y);

    out[0] = wpd_clip_uint8(A);
    out[1] = wpd_clip_uint8(r);
    out[2] = wpd_clip_uint8(g);
    out[3] = wpd_clip_uint8(b);
}

static void copy_yuva2argb(WebPImage *dst, const WebPImage *src, int pos_x,
                           int pos_y, SubRect r) {
    int alpha = image_nb_components(src) > 3;

    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_y = src->data[0] + (r.y + y) * src->linesize[0] +
            r.x;
        const uint8_t *src_u = src->data[1] +
            ((r.y + y) >> 1) * src->linesize[1] + (r.x >> 1);
        const uint8_t *src_v = src->data[2] +
            ((r.y + y) >> 1) * src->linesize[2] + (r.x >> 1);
        const uint8_t *src_a    = NULL;
        uint8_t       *dst_argb = dst->data[0] +
            (pos_y + r.y + y) * dst->linesize[0] + (pos_x + r.x) * 4;
        if (alpha)
            src_a = src->data[3] + (r.y + y) * src->linesize[3] + r.x;

        for (int x = r.x; x < r.x + r.w; x++) {
            webp_yuva2argb(
                dst_argb, *src_y, *src_u, *src_v, (alpha ? *src_a : 255));
            src_y += 1;
            src_u += x & 1;
            src_v += x & 1;
            if (alpha)
                src_a += 1;
            dst_argb += 4;
        }
    }
}

static void blend_yuva2argb(WebPImage *dst, const WebPImage *src, int pos_x,
                            int pos_y, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_y = src->data[0] + (r.y + y) * src->linesize[0] +
            r.x;
        const uint8_t *src_u = src->data[1] +
            ((r.y + y) >> 1) * src->linesize[1] + (r.x >> 1);
        const uint8_t *src_v = src->data[2] +
            ((r.y + y) >> 1) * src->linesize[2] + (r.x >> 1);
        const uint8_t *src_a = src->data[3] + (r.y + y) * src->linesize[3] +
            r.x;
        uint8_t *dst_argb = dst->data[0] +
            (pos_y + r.y + y) * dst->linesize[0] + (pos_x + r.x) * 4;

        for (int x = r.x; x < r.x + r.w; x++) {
            int src_alpha = *src_a;
            int dst_alpha = dst_argb[0];

            if (src_alpha == 255) {
                webp_yuva2argb(dst_argb, *src_y, *src_u, *src_v, src_alpha);
            } else if (src_alpha == 0) {
            } else {
                uint8_t tmp[4];
                int     tmp_alpha   = (dst_alpha * (256 - src_alpha)) >> 8;
                int     blend_alpha = src_alpha + tmp_alpha;
                int     scale       = (1UL << 24) / blend_alpha;

                webp_yuva2argb(tmp, *src_y, *src_u, *src_v, src_alpha);

                dst_argb[0] = blend_alpha;
                dst_argb[1] = (((uint32_t)(tmp[1] * src_alpha +
                                           dst_argb[1] * tmp_alpha)) *
                               scale) >>
                    24;
                dst_argb[2] = (((uint32_t)(tmp[2] * src_alpha +
                                           dst_argb[2] * tmp_alpha)) *
                               scale) >>
                    24;
                dst_argb[3] = (((uint32_t)(tmp[3] * src_alpha +
                                           dst_argb[3] * tmp_alpha)) *
                               scale) >>
                    24;
            }

            src_y += 1;
            src_u += x & 1;
            src_v += x & 1;
            src_a += 1;
            dst_argb += 4;
        }
    }
}

static void composite_region(WPDDecoder *s, const WebPImage *frame, SubRect r,
                             int blend) {
    WebPImage *canvas = &s->canvas;

    if (r.w <= 0 || r.h <= 0)
        return;

    if (canvas->format != WPD_PIX_FMT_ARGB) {
        if (blend)
            blend_yuva_region(s, canvas, frame, r);
        else
            copy_yuva_region(s, canvas, frame, r);
    } else if (canvas->format == frame->format) {
        if (blend)
            blend_argb_region(s, canvas, frame, r);
        else
            copy_argb_region(s, canvas, frame, r);
    } else {
        if (blend)
            blend_yuva2argb(canvas, frame, s->pos_x, s->pos_y, r);
        else
            copy_yuva2argb(canvas, frame, s->pos_x, s->pos_y, r);
    }
}

// libwebp overwrites the frame rect and alpha-blends only where the prev
// canvas can be non-transparent, blending elsewhere would round down
static void composite_subframe(WPDDecoder *s, const WebPImage *frame) {
    SubRect full = {0, 0, frame->width, frame->height};
    SubRect keep = {0, 0, 0, 0};

    // frames w no alpha plane cannot blend
    if (!s->key_frame && !(s->anmf_flags & ANMF_FLAG_NO_BLEND) &&
        frame->format != WPD_PIX_FMT_YUV420P) {
        if (!(s->prev_anmf_flags & ANMF_FLAG_DISPOSE)) {
            composite_region(s, frame, full, 1);
            return;
        }
        keep.x = WPD_MAX(s->pos_x, s->prev_pos_x) - s->pos_x;
        keep.y = WPD_MAX(s->pos_y, s->prev_pos_y) - s->pos_y;
        keep.w = WPD_MIN(s->pos_x + frame->width,
                         s->prev_pos_x + s->prev_width) -
            s->pos_x - keep.x;
        keep.h = WPD_MIN(s->pos_y + frame->height,
                         s->prev_pos_y + s->prev_height) -
            s->pos_y - keep.y;
        if (keep.w <= 0 || keep.h <= 0) {
            composite_region(s, frame, full, 1);
            return;
        }
        if (s->canvas.format != WPD_PIX_FMT_ARGB) {
            keep.w &= ~1;
            keep.h &= ~1;
            if (!keep.w || !keep.h) {
                composite_region(s, frame, full, 1);
                return;
            }
        }

        SubRect top    = {0, 0, full.w, keep.y};
        SubRect bottom = {0, keep.y + keep.h, full.w, full.h - keep.y - keep.h};
        SubRect left   = {0, keep.y, keep.x, keep.h};
        SubRect right  = {
            keep.x + keep.w, keep.y, full.w - keep.x - keep.w, keep.h};

        composite_region(s, frame, top, 1);
        composite_region(s, frame, bottom, 1);
        composite_region(s, frame, left, 1);
        composite_region(s, frame, right, 1);
        composite_region(s, frame, keep, 0);
        return;
    }

    composite_region(s, frame, full, 0);
}

static void clear_canvas_rect(WPDDecoder *s, int pos_x, int pos_y, int width,
                              int height) {
    WebPImage *canvas = &s->canvas;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        uint8_t *const base     = canvas->data[0];
        const int      linesize = canvas->linesize[0];
        uint32_t       bg;

        memcpy(&bg, s->clear_argb, 4);
        for (int y = 0; y < height; y++) {
            uint32_t *dst = (uint32_t *)(base +
                                         (size_t)(pos_y + y) * linesize) +
                pos_x;

            for (int x = 0; x < width; x++) dst[x] = bg;
        }
    } else {
        for (int comp = 0; comp < 4; comp++) {
            int      shift = (comp == 1 || comp == 2) ? 1 : 0;
            uint8_t *dst   = canvas->data[comp] +
                (pos_y >> shift) * canvas->linesize[comp] + (pos_x >> shift);
            for (int y = 0; y < CEIL_RSHIFT(height, shift); y++) {
                memset(dst, s->clear_yuva[comp], CEIL_RSHIFT(width, shift));
                dst += canvas->linesize[comp];
            }
        }
    }
}

static int allocate_canvas(WPDDecoder *s, WPDPixelFormat format) {
    int ret;

    if (format == WPD_PIX_FMT_ARGB)
        ret = image_alloc_argb(&s->canvas, s->canvas_width, s->canvas_height);
    else
        ret = image_alloc_yuva(&s->canvas, s->canvas_width, s->canvas_height);
    return ret;
}

static int is_full_frame(const WPDDecoder *s, int width, int height) {
    return width == s->canvas_width && height == s->canvas_height;
}

static int is_key_frame(const WPDDecoder *s, const WebPImage *frame) {
    if (s->frame_index == 0)
        return 1;
    if ((!s->frame_has_alpha || (s->anmf_flags & ANMF_FLAG_NO_BLEND)) &&
        s->pos_x == 0 && s->pos_y == 0 &&
        is_full_frame(s, frame->width, frame->height))
        return 1;
    return (s->prev_anmf_flags & ANMF_FLAG_DISPOSE) &&
        (is_full_frame(s, s->prev_width, s->prev_height) || s->prev_key_frame);
}

static int prepare_canvas(WPDDecoder *s, const WebPImage *frame,
                          WPDPixelFormat format) {
    int covers_canvas = s->pos_x == 0 && s->pos_y == 0 &&
        is_full_frame(s, frame->width, frame->height);
    int ret;

    if (s->key_frame && s->canvas.data[0] && s->canvas.format != format)
        image_free(&s->canvas);

    if (!s->canvas.data[0]) {
        ret = allocate_canvas(s, format);
        if (ret < 0)
            return ret;
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else if (s->key_frame) {
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else {
        if (format == WPD_PIX_FMT_ARGB &&
            s->canvas.format == WPD_PIX_FMT_YUVA420P) {
            WebPImage yuva_canvas = s->canvas;
            SubRect canvas_rect = {0, 0, yuva_canvas.width, yuva_canvas.height};
            memset(&s->canvas, 0, sizeof(s->canvas));
            ret = allocate_canvas(s, WPD_PIX_FMT_ARGB);
            if (ret < 0) {
                image_free(&yuva_canvas);
                return ret;
            }
            copy_yuva2argb(&s->canvas, &yuva_canvas, 0, 0, canvas_rect);
            image_free(&yuva_canvas);
        }
        if (s->prev_anmf_flags & ANMF_FLAG_DISPOSE)
            clear_canvas_rect(
                s, s->prev_pos_x, s->prev_pos_y, s->prev_width, s->prev_height);
    }

    return 0;
}

static int decode_anmf(WPDDecoder *s, const uint8_t *data, size_t size) {
    const uint8_t   *p = data, *end = data + size;
    const WebPImage *sub = NULL;
    int              declared_width, declared_height;
    int              ret;

    if (size < 16)
        return WPD_ERROR_INVALID_DATA;

    s->pos_x          = WPD_RL24(p) * 2;
    s->pos_y          = WPD_RL24(p + 3) * 2;
    declared_width    = WPD_RL24(p + 6) + 1;
    declared_height   = WPD_RL24(p + 9) + 1;
    s->frame_duration = WPD_RL24(p + 12);
    s->anmf_flags     = p[15];
    p += 16;

    if (s->pos_x + declared_width > s->canvas_width ||
        s->pos_y + declared_height > s->canvas_height) {
        wpd_log(NULL,
                WPD_LOG_ERROR,
                "Frame (%dx%d at pos %dx%d) does not fit into canvas (%dx%d)\n",
                declared_width,
                declared_height,
                s->pos_x,
                s->pos_y,
                s->canvas_width,
                s->canvas_height);
        return WPD_ERROR_INVALID_DATA;
    }

    s->has_alpha = 0;
    s->width     = 0;
    s->height    = 0;

    while (end - p >= 8) {
        uint32_t chunk_type   = WPD_RL32(p);
        uint32_t payload_size = WPD_RL32(p + 4);
        uint32_t padded_size;

        if (payload_size == UINT32_MAX)
            return WPD_ERROR_INVALID_DATA;
        padded_size = payload_size + (payload_size & 1);
        p += 8;

        if ((size_t)(end - p) < padded_size) {
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
            int compression = alpha_header & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL,
                        WPD_LOG_WARNING,
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
            sub                = &s->subframe;
            s->frame_has_alpha = s->has_alpha;
            break;
        case MKTAG('V', 'P', '8', 'L'):
            if (sub)
                break;
            ret = vp8_lossless_decode_frame(s, &s->argb, p, payload_size, 0);
            if (ret < 0)
                return ret;
            sub                = &s->argb;
            s->frame_has_alpha = s->lossless_has_alpha;
            break;
        default: break;
        }
        p += padded_size;
    }

    if (!sub) {
        wpd_log(NULL, WPD_LOG_ERROR, "image data not found\n");
        return WPD_ERROR_INVALID_DATA;
    }

    if (sub->width != declared_width || sub->height != declared_height)
        wpd_log(NULL,
                WPD_LOG_WARNING,
                "ANMF declares %dx%d but the image is %dx%d\n",
                declared_width,
                declared_height,
                sub->width,
                sub->height);

    if (s->pos_x + sub->width > s->canvas_width ||
        s->pos_y + sub->height > s->canvas_height) {
        wpd_log(NULL,
                WPD_LOG_ERROR,
                "Frame (%dx%d at pos %dx%d) does not fit into canvas (%dx%d)\n",
                sub->width,
                sub->height,
                s->pos_x,
                s->pos_y,
                s->canvas_width,
                s->canvas_height);
        return WPD_ERROR_INVALID_DATA;
    }

    s->key_frame = is_key_frame(s, sub);
    ret          = prepare_canvas(s,
                                  sub,
                                  sub->format == WPD_PIX_FMT_ARGB
                                      ? WPD_PIX_FMT_ARGB
                                      : WPD_PIX_FMT_YUVA420P);
    if (ret < 0)
        return ret;

    composite_subframe(s, sub);

    s->frame_timestamp += s->frame_duration;
    s->prev_anmf_flags = s->anmf_flags;
    s->prev_width      = sub->width;
    s->prev_height     = sub->height;
    s->prev_pos_x      = s->pos_x;
    s->prev_pos_y      = s->pos_y;
    s->prev_key_frame  = s->key_frame;
    s->frame_index++;

    return 0;
}

static void export_frame(const WPDDecoder *s, const WebPImage *img,
                         WPDFrame *frame) {
    memset(frame, 0, sizeof(*frame));
    for (int p = 0; p < 4; p++) {
        frame->data[p]   = img->data[p];
        frame->stride[p] = img->linesize[p];
    }
    frame->width     = img->width;
    frame->height    = img->height;
    frame->format    = img->format;
    frame->duration  = s->frame_duration;
    frame->timestamp = s->frame_timestamp - s->frame_duration;
}

static int set_error(WPDDecoder *decoder, const char *message, int code) {
    snprintf(decoder->error, sizeof(decoder->error), "%s (%d)", message, code);
    return -1;
}

WPDDecoder *wpd_decoder_create(void) {
    WPDDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    wpd_init_cpu();
    wpd_vp8l_dsp_init(&decoder->ldsp);
    return decoder;
}

static void scan_still_size(WPDDecoder *s, uint32_t tag, const uint8_t *p,
                            uint32_t size) {
    if (tag == MKTAG('V', 'P', '8', 'L')) {
        if (size >= 5 && p[0] == 0x2f) {
            uint32_t bits = WPD_RL32(p + 1);

            s->canvas_width  = (bits & 0x3fff) + 1;
            s->canvas_height = ((bits >> 14) & 0x3fff) + 1;
        }
    } else if (size >= 10 && p[3] == 0x9d && p[4] == 0x01 && p[5] == 0x2a) {
        s->canvas_width  = WPD_RL16(p + 6) & 0x3fff;
        s->canvas_height = WPD_RL16(p + 8) & 0x3fff;
    }
}

static void scan_headers(WPDDecoder *s) {
    size_t pos    = 12;
    int    images = 0;

    while (pos + 8 <= s->end) {
        const uint8_t *chunk = s->file + pos;
        uint32_t       tag   = WPD_RL32(chunk);
        uint32_t       size  = WPD_RL32(chunk + 4);
        uint32_t       padded_size;

        if (size == UINT32_MAX)
            break;
        padded_size = size + (size & 1);
        if (s->end - (pos + 8) < padded_size)
            break;

        switch (tag) {
        case MKTAG('V', 'P', '8', 'X'):
            if (size >= 10) {
                s->canvas_width  = WPD_RL24(chunk + 12) + 1;
                s->canvas_height = WPD_RL24(chunk + 15) + 1;
            }
            break;
        case MKTAG('A', 'N', 'I', 'M'):
            s->animation = 1;
            if (size >= 6) {
                s->anim_background_argb = WPD_RL32(chunk + 8);
                s->anim_loop_count      = WPD_RL16(chunk + 12);
            }
            break;
        case MKTAG('A', 'N', 'M', 'F'): s->anim_frame_count++; break;
        case MKTAG('V', 'P', '8', ' '):
        case MKTAG('V', 'P', '8', 'L'):
            if (!images++ && !s->canvas_width)
                scan_still_size(s, tag, chunk + 8, size);
            break;
        default: break;
        }
        pos += 8 + padded_size;
    }

    if (!s->animation)
        s->anim_frame_count = images ? 1 : 0;
}

int wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data, size_t size) {
    uint32_t riff_size;

    if (!decoder || !data)
        return -1;

    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    memset(&decoder->subframe, 0, sizeof(decoder->subframe));
    decoder->animation    = 0;
    decoder->still_done   = 0;
    decoder->frame_index  = 0;
    decoder->canvas_width = decoder->canvas_height = 0;
    decoder->width = decoder->height = 0;
    decoder->has_alpha               = 0;
    decoder->frame_has_alpha         = 0;
    decoder->key_frame = decoder->prev_key_frame = 0;
    decoder->prev_anmf_flags = decoder->anmf_flags = 0;
    decoder->prev_width = decoder->prev_height = 0;
    decoder->prev_pos_x = decoder->prev_pos_y = 0;
    decoder->anim_loop_count = decoder->anim_frame_count = 0;
    decoder->anim_background_argb                        = 0;
    decoder->frame_duration                              = 0;
    decoder->frame_timestamp                             = 0;
    memset(decoder->clear_argb, 0, sizeof(decoder->clear_argb));
    decoder->clear_yuva[0] = RGB_TO_Y_CCIR(0, 0, 0);
    decoder->clear_yuva[1] = RGB_TO_U_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[2] = RGB_TO_V_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[3] = 0;
    decoder->error[0]      = 0;

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

    riff_size    = WPD_RL32(decoder->file + 4);
    decoder->pos = 12;
    decoder->end = WPD_MIN((size_t)riff_size + 8, size);
    scan_headers(decoder);
    return 0;
}

int wpd_decoder_anim_info(const WPDDecoder *decoder, WPDAnimInfo *info) {
    if (!decoder || !info || !decoder->file)
        return -1;

    info->canvas_width    = decoder->canvas_width;
    info->canvas_height   = decoder->canvas_height;
    info->frame_count     = decoder->anim_frame_count;
    info->loop_count      = decoder->anim_loop_count;
    info->background_argb = decoder->anim_background_argb;
    info->is_animation    = decoder->animation;
    return 0;
}

int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame) {
    if (!decoder || !frame)
        return -1;
    if (!decoder->file)
        return set_error(decoder, "no file opened", WPD_ERROR_INVALID_DATA);

    while (decoder->pos + 8 <= decoder->end) {
        const uint8_t *chunk      = decoder->file + decoder->pos;
        uint32_t       chunk_type = WPD_RL32(chunk);
        uint32_t       size       = WPD_RL32(chunk + 4);
        uint32_t       padded_size;
        const uint8_t *payload = chunk + 8;
        int            ret;

        if (size == UINT32_MAX)
            return set_error(
                decoder, "invalid chunk size", WPD_ERROR_INVALID_DATA);
        padded_size = size + (size & 1);

        if (decoder->end - (decoder->pos + 8) < padded_size) {
            break;
        }
        decoder->pos += 8 + padded_size;

        switch (chunk_type) {
        case MKTAG('A', 'L', 'P', 'H'): {
            int alpha_header, filter_m, compression;

            if (size == 0)
                return set_error(decoder,
                                 "invalid ALPHA chunk size",
                                 WPD_ERROR_INVALID_DATA);
            alpha_header             = payload[0];
            decoder->alpha_data      = payload + 1;
            decoder->alpha_data_size = size - 1;

            filter_m    = (alpha_header >> 2) & 0x03;
            compression = alpha_header & 0x03;

            if (compression > ALPHA_COMPRESSION_VP8L) {
                wpd_log(NULL,
                        WPD_LOG_WARNING,
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
            ret                              = vp8_lossy_decode_frame(
                decoder, &decoder->subframe, payload, size);
            if (ret < 0)
                return set_error(decoder, "VP8 decode failed", ret);
            decoder->still_done = 1;
            export_frame(decoder, &decoder->subframe, frame);
            return 1;
        case MKTAG('V', 'P', '8', 'L'):
            if (decoder->animation || decoder->still_done)
                break;
            decoder->width = decoder->height = 0;
            ret                              = vp8_lossless_decode_frame(
                decoder, &decoder->argb, payload, size, 0);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
            decoder->still_done = 1;
            export_frame(decoder, &decoder->argb, frame);
            return 1;
        case MKTAG('A', 'N', 'M', 'F'):
            if (!decoder->animation || !decoder->canvas_width ||
                !decoder->canvas_height)
                return set_error(decoder,
                                 "ANMF chunk without animation header",
                                 WPD_ERROR_INVALID_DATA);
            ret = decode_anmf(decoder, payload, size);
            if (ret < 0)
                return set_error(decoder, "animation frame decode failed", ret);
            export_frame(decoder, &decoder->canvas, frame);
            return 1;
        default: break;
        }
    }

    return 0;
}

const char *wpd_decoder_error(const WPDDecoder *decoder) {
    return decoder && decoder->error[0] ? decoder->error
                                        : "unknown decoder error";
}

void wpd_decoder_free(WPDDecoder *decoder) {
    if (!decoder)
        return;
    if (decoder->vp8_initialized)
        vp8_decode_free(&decoder->codec);
    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    image_free(&decoder->alpha_argb);
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&decoder->image[i]);
    free(decoder->alpha_plane);
    free(decoder->file);
    free(decoder);
}
