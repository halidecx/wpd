
#include "wpd.h"

#include "rescaler.h"
#include "vp8.h"
#include "vp8l_dsp.h"
#include "wpd_codec.h"
#include "yuvdsp.h"

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

#define VP8L_NEED_MORE 1
#define VP8L_TAIL_MARGIN 64

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

/* Points a reader that has not overrun at a longer, possibly moved, copy of
   the same bytes. Everything it tracks is an offset, so nothing else moves. */
static void br_extend(LEBitReader *br, const uint8_t *buf, size_t size) {
    br->buf = buf;
    br->len = size;
}

typedef struct HeaderScan {
    size_t    pos;
    uint64_t  riff_end;
    size_t    end;
    int       width, height;
    int       has_alpha;
    int       animation;
    int       images;
    int       vp8x;
    int       frame_count;
    int       loop_count;
    uint32_t  background_argb;
    WPDCoding coding;
    int       truncated;
    int       raw_kind;
    size_t    raw_image_offset;
    size_t    raw_image_size;
    size_t    raw_alpha_offset;
    size_t    raw_alpha_size;
} HeaderScan;

typedef struct WebPImage {
    /* Set when the colour channels already carry alpha, as the animation
       canvas does for a premultiplied output format. */
    int            premultiplied;
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

static int image_alloc_packed(WebPImage *img, int w, int h, int bpp,
                              WPDPixelFormat format) {
    size_t row, size;

    if (w <= 0 || h <= 0 || bpp <= 0 || (size_t)w > SIZE_MAX / (size_t)bpp)
        return WPD_ERROR_TOO_LARGE;
    row = (size_t)w * (size_t)bpp;
    if ((size_t)h > (SIZE_MAX - WPD_FILE_PADDING) / row)
        return WPD_ERROR_TOO_LARGE;
    size = row * (size_t)h + WPD_FILE_PADDING;
    if (row > INT_MAX)
        return WPD_ERROR_TOO_LARGE;

    for (int p = 1; p < 4; p++) {
        wpd_free(img->alloc[p]);
        img->alloc[p]      = NULL;
        img->alloc_size[p] = 0;
        img->data[p]       = NULL;
        img->linesize[p]   = 0;
    }
    img->linesize[0] = (int)row;
    img->data[0]     = image_alloc_plane(img, 0, size);
    if (!img->data[0])
        return WPD_ERROR(ENOMEM);
    img->width  = w;
    img->height = h;
    img->format = format;
    return 0;
}

static int image_alloc_argb(WebPImage *img, int w, int h) {
    return image_alloc_packed(img, w, h, 4, WPD_PIX_FMT_ARGB);
}

static int image_alloc_yuva(WebPImage *img, int w, int h) {
    if (w <= 0 || h <= 0)
        return WPD_ERROR_TOO_LARGE;
    for (int p = 0; p < 4; p++) {
        int    pw = (p == 1 || p == 2) ? (w + 1) / 2 : w;
        int    ph = (p == 1 || p == 2) ? (h + 1) / 2 : h;
        size_t size;

        if ((size_t)ph > (SIZE_MAX - WPD_FILE_PADDING) / (size_t)pw) {
            image_free(img);
            return WPD_ERROR_TOO_LARGE;
        }
        size             = (size_t)pw * (size_t)ph + WPD_FILE_PADDING;
        img->linesize[p] = pw;
        img->data[p]     = image_alloc_plane(img, p, size);
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
    WPDYUVDSP       ydsp;
    WPDPixelFormat  out_format;
    int             premultiply;

    const uint8_t *file;
    uint8_t       *file_alloc;
    size_t         file_size;
    size_t         discarded;
    size_t         pos, end;
    HeaderScan     scan;
    int            animation;
    int            still_done;
    int            vp8_active;
    int            still_lossy;
    int            alpha_pending;
    int            converted_rows;
    WPDPixelFormat converted_format;
    int            vp8l_active;
    int            still_lossless;
    size_t         vp8l_next_try;
    size_t         vp8l_pos, vp8l_cached;
    int            vp8l_x, vp8l_y, vp8l_hg;
    int            vp8l_rows_done, vp8l_rows_out, vp8l_peeked;
    int            frame_index;
    int            canvas_width, canvas_height;

    int                   has_alpha;
    enum AlphaCompression alpha_compression;
    enum AlphaFilter      alpha_filter;
    /* An offset, not a pointer: appending to a stream can move 'file'. */
    size_t   alpha_data_offset;
    int      alpha_data_size;
    uint8_t *alpha_plane;
    size_t   alpha_plane_size;

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

    WebPImage  argb;
    WebPImage  lossless_out;
    WebPImage *lossless_frame;
    uint8_t   *lossless_top;
    size_t     lossless_top_size;
    WebPImage  alpha_argb;
    WebPImage  subframe;
    WebPImage  converted;
    WebPImage  output;

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

    int       info_has_alpha;
    WPDCoding info_coding;

    size_t file_capacity;
    int    opened;
    int    streaming;
    int    eos;
    int    headers_valid;
    int    truncated;
    int    borrowed;
    int    input_mode;

    WPDStatus status;
    char      error[128];
};

#define WPD_FIELD_END(type, field) \
    (offsetof(type, field) + sizeof(((type *)0)->field))

static int frame_valid(const WPDFrame *frame) {
    return frame && frame->struct_size >= WPD_FIELD_END(WPDFrame, timestamp);
}

static void frame_clear(WPDFrame *frame) {
    const size_t struct_size = frame->struct_size;

    memset((uint8_t *)frame + sizeof(frame->struct_size),
           0,
           WPD_FIELD_END(WPDFrame, timestamp) - sizeof(frame->struct_size));
    frame->struct_size = struct_size;
}

static int info_valid(const WPDImageInfo *info) {
    return info && info->struct_size >= WPD_FIELD_END(WPDImageInfo, coding);
}

static void info_clear(WPDImageInfo *info) {
    const size_t struct_size = info->struct_size;

    memset((uint8_t *)info + sizeof(info->struct_size),
           0,
           WPD_FIELD_END(WPDImageInfo, coding) - sizeof(info->struct_size));
    info->struct_size = struct_size;
}

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

static wpd_always_inline int read_entropy_image_header(WPDDecoder    *s,
                                                       enum ImageRole role,
                                                       int w, int h) {
    ImageContext *img;
    HTreeGroup   *hg;
    uint8_t      *code_lengths;
    uint16_t     *sorted;
    int           i, j, ret, max_alphabet_size;

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

    return 0;
}

static wpd_always_inline int decode_entropy_pixels(WPDDecoder    *s,
                                                   enum ImageRole role,
                                                   const int      resumable) {
    ImageContext *img = &s->image[role];
    HTreeGroup   *hg;
    int           x, y, width;

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
        LEBitReader    snap;
        int            near = 0;

        hg = &img->huffman_groups[0];
        x  = 0;
        y  = 0;
        if (resumable) {
            pos    = s->vp8l_pos;
            x      = s->vp8l_x;
            y      = s->vp8l_y;
            cached = base + s->vp8l_cached;
            hg     = &img->huffman_groups[s->vp8l_hg];
        }
        while (pos < total) {
            uint8_t *p = base + 4 * pos;
            int      v;

            if (!resumable && br_is_eos(&s->gb))
                return WPD_ERROR_INVALID_DATA;
            /* One pixel reads at most 108 bits, which draws the reader no
               more than 20 bytes further in, so the margin leaves the loop
               nothing to save or check until the end really is in sight. */
            if (resumable) {
                near = s->gb.len - s->gb.pos <= VP8L_TAIL_MARGIN;
                if (near)
                    snap = s->gb;
            }

            if ((x & huff_mask) == 0)
                hg = get_huffman_group(s, img, x, y);
            br_fill(&s->gb);
            v = huff_read_symbol(hg->trees[HUFF_IDX_GREEN].table, &s->gb);
            if (v < NUM_LITERAL_CODES) {
                if (hg->trivial_literal) {
                    if (resumable && near && br_is_eos(&s->gb))
                        goto suspend;
                    copy32(p, hg->literal);
                    p[2] = v;
                } else {
                    int r = huff_read_symbol(hg->trees[HUFF_IDX_RED].table,
                                             &s->gb);
                    int b, a;
                    br_fill(&s->gb);
                    b = huff_read_symbol(hg->trees[HUFF_IDX_BLUE].table,
                                         &s->gb);
                    a = huff_read_symbol(hg->trees[HUFF_IDX_ALPHA].table,
                                         &s->gb);
                    if (resumable && near && br_is_eos(&s->gb))
                        goto suspend;
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

                if (resumable && near && br_is_eos(&s->gb))
                    goto suspend;

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

                if (resumable && near && br_is_eos(&s->gb))
                    goto suspend;
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
        if (resumable)
            s->vp8l_rows_done = y;
        return 0;

    suspend:
        s->gb             = snap;
        s->vp8l_pos       = pos;
        s->vp8l_x         = x;
        s->vp8l_y         = y;
        s->vp8l_cached    = (size_t)(cached - base);
        s->vp8l_hg        = (int)(hg - img->huffman_groups);
        s->vp8l_rows_done = y;
        return VP8L_NEED_MORE;
    }
}

static wpd_noclone int vp8l_resume_argb_pixels(WPDDecoder *s) {
    return decode_entropy_pixels(s, IMAGE_ROLE_ARGB, 1);
}

static int decode_entropy_coded_image(WPDDecoder *s, enum ImageRole role, int w,
                                      int h) {
    int ret = read_entropy_image_header(s, role, w, h);

    if (ret < 0)
        return ret;
    return decode_entropy_pixels(s, role, 0);
}

static wpd_always_inline int predictor_transform_rows(WPDDecoder *s,
                                                      uint32_t   *rows,
                                                      int         stride,
                                                      uint32_t *upper0, int y0,
                                                      int y1) {
    ImageContext              *pimg      = &s->image[IMAGE_ROLE_PREDICTOR];
    pred_add_func const *const pred_add  = s->ldsp.pred_add;
    const int                  width     = s->reduced_width;
    const int                  tile_bits = pimg->size_reduction;
    const int                  tile_size = 1 << tile_bits;
    const int                  tile_mask = tile_size - 1;
    uint32_t                  *upper     = upper0;
    uint32_t                  *row       = rows;
    int                        y         = y0;

    if (width <= 0 || y1 <= y0)
        return 0;

    if (!y0) {
        pred_add[0](row, NULL, 1, row);
        if (width > 1)
            pred_add[1](row + 1, NULL, width - 1, row + 1);
        upper = row;
        row += stride;
        y = 1;
    }

    for (; y < y1; y++, upper = row, row += stride) {
        const uint8_t *modes = pimg->frame->data[0] +
            (y >> tile_bits) * pimg->frame->linesize[0];
        int x = 1;

        pred_add[2](row, upper, 1, row);
        /* The top-right of the last pixel in a row is the leftmost pixel of
           that same row, which falls out of the layout only while the row
           above is physically adjacent. */
        if (upper + width != row)
            upper[width] = row[0];

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

static int apply_predictor_transform(WPDDecoder *s) {
    ImageContext *img = &s->image[IMAGE_ROLE_ARGB];

    return predictor_transform_rows(s,
                                    (uint32_t *)img->frame->data[0],
                                    img->frame->linesize[0] / 4,
                                    NULL,
                                    0,
                                    img->frame->height);
}

static wpd_always_inline uint8_t color_transform_delta(uint8_t color_pred,
                                                       uint8_t color) {
    return u8_to_s8(color_pred) * u8_to_s8(color) >> 5;
}

static wpd_always_inline int color_transform_rows(WPDDecoder *s, uint8_t *rows,
                                                  int stride, int y0, int y1) {
    ImageContext *cimg      = &s->image[IMAGE_ROLE_COLOR_TRANSFORM];
    const int     width     = s->reduced_width;
    const int     tile_bits = cimg->size_reduction;
    const int     tile_size = 1 << tile_bits;
    const int     tile_mask = tile_size - 1;
    int           y;

    for (y = y0; y < y1; y++, rows += stride) {
        const uint8_t *mult_row = cimg->frame->data[0] +
            (y >> tile_bits) * cimg->frame->linesize[0];
        uint8_t *p = rows;
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

static int apply_color_transform(WPDDecoder *s) {
    ImageContext *img = &s->image[IMAGE_ROLE_ARGB];

    return color_transform_rows(
        s, img->frame->data[0], img->frame->linesize[0], 0, img->frame->height);
}

static wpd_always_inline int subtract_green_rows(WPDDecoder *s, uint8_t *rows,
                                                 int stride, int y0, int y1) {
    const int width = s->reduced_width;
    int       x, y;

    for (y = y0; y < y1; y++, rows += stride) {
        uint8_t *p = rows;
        for (x = 0; x < width; x++, p += 4) {
            p[1] += p[2];
            p[3] += p[2];
        }
    }
    return 0;
}

static int apply_subtract_green_transform(WPDDecoder *s) {
    ImageContext *img = &s->image[IMAGE_ROLE_ARGB];

    return subtract_green_rows(
        s, img->frame->data[0], img->frame->linesize[0], 0, img->frame->height);
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

static wpd_always_inline int color_indexing_rows(WPDDecoder *s, uint8_t *base,
                                                 int dst_stride, int src_stride,
                                                 int height, int big) {
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
        return 0;
    }

    if (big) {
        uint32_t  palette[256];
        const int size = pal->frame->width * 4;
        const int w    = img->frame->width;

        memcpy(palette, GET_PIXEL(pal->frame, 0, 0), size);
        memset((uint8_t *)palette + size, 0, sizeof(palette) - size);
        for (y = 0; y < height; y++) {
            uint8_t *row = base + (size_t)y * dst_stride;

            s->ldsp.map_color32(row, row, palette, w);
        }
    } else {
        for (y = 0; y < height; y++) {
            for (x = 0; x < img->frame->width; x++) {
                p = base + (size_t)y * dst_stride + 4 * x;
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

static int apply_color_indexing_transform(WPDDecoder *s) {
    ImageContext *img    = &s->image[IMAGE_ROLE_ARGB];
    ImageContext *pal    = &s->image[IMAGE_ROLE_COLOR_INDEXING];
    const int     width  = img->frame->width;
    const int     height = img->frame->height;
    int           ret;

    ret = color_indexing_rows(s,
                              GET_PIXEL(img->frame, 0, 0),
                              width * 4,
                              img->frame->linesize[0],
                              height,
                              height * width > 300);
    if (ret < 0)
        return ret;
    if (pal->size_reduction > 0) {
        img->frame->linesize[0] = width * 4;
        s->reduced_width        = s->width;
    }
    return 0;
}

static size_t file_buffered(const WPDDecoder *decoder) {
    return decoder->file_size - decoder->discarded;
}

static const uint8_t *file_at(const WPDDecoder *decoder, size_t offset) {
    return decoder->file + (offset - decoder->discarded);
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

static wpd_always_inline int vp8l_read_frame_header(
    WPDDecoder *s, WebPImage *out, const uint8_t *data_start,
    unsigned int data_size, int is_alpha_chunk, int *w_out, int *h_out) {
    int      w, h, ret;
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
            return WPD_ERROR_INVALID_DATA;
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
            return ret;
    }

    s->image[IMAGE_ROLE_ARGB].frame = out;
    if (is_alpha_chunk)
        s->image[IMAGE_ROLE_ARGB].is_alpha_primary = 1;
    *w_out = w;
    *h_out = h;
    return read_entropy_image_header(s, IMAGE_ROLE_ARGB, w, h);
}

static wpd_noclone int vp8l_apply_transforms(WPDDecoder *s) {
    int i, ret;

    for (i = s->nb_transforms - 1; i >= 0; i--) {
        ret = 0;
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
            return ret;
    }
    return 0;
}

static int vp8_lossless_decode_frame(WPDDecoder *s, WebPImage *out,
                                     const uint8_t *data_start,
                                     unsigned int   data_size,
                                     int            is_alpha_chunk) {
    int w, h, ret, i;

    ret = vp8l_read_frame_header(
        s, out, data_start, data_size, is_alpha_chunk, &w, &h);
    if (ret < 0)
        goto free_and_return;

    ret = decode_entropy_pixels(s, IMAGE_ROLE_ARGB, 0);
    if (ret < 0)
        goto free_and_return;

    ret = vp8l_apply_transforms(s);

free_and_return:
    out->linesize[0] = out->width * 4;
    for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&s->image[i]);

    return ret;
}

#define VP8L_ROW_BATCH 16

static int vp8l_transform_rows(WPDDecoder *s, int y0, int y1) {
    ImageContext *img        = &s->image[IMAGE_ROLE_ARGB];
    WebPImage    *dst        = &s->lossless_out;
    const int     stride     = dst->linesize[0];
    const int     src_stride = img->frame->linesize[0];
    const int     packed     = s->reduced_width;
    const size_t  packed_row = (size_t)packed * 4;
    uint8_t      *rows       = dst->data[0] + (size_t)y0 * stride;
    int           i, ret = 0;

    for (i = 0; i < y1 - y0; i++)
        memcpy(rows + (size_t)i * stride,
               img->frame->data[0] + (size_t)(y0 + i) * src_stride,
               packed_row);

    for (i = s->nb_transforms - 1; i >= 0 && ret >= 0; i--) {
        switch (s->transforms[i]) {
        case PREDICTOR_TRANSFORM:
            ret = predictor_transform_rows(
                s,
                (uint32_t *)rows,
                stride / 4,
                y0 ? (uint32_t *)s->lossless_top : NULL,
                y0,
                y1);
            if (ret >= 0)
                memcpy(s->lossless_top,
                       rows + (size_t)(y1 - 1 - y0) * stride,
                       (size_t)s->reduced_width * 4);
            break;
        case COLOR_TRANSFORM:
            ret = color_transform_rows(s, rows, stride, y0, y1);
            break;
        case SUBTRACT_GREEN:
            ret = subtract_green_rows(s, rows, stride, y0, y1);
            break;
        case COLOR_INDEXING_TRANSFORM:
            ret = color_indexing_rows(s,
                                      rows,
                                      stride,
                                      stride,
                                      y1 - y0,
                                      dst->height * dst->width > 300);
            s->reduced_width = s->width;
            break;
        }
    }
    s->reduced_width = packed;
    return ret;
}

static int vp8l_still_alloc(WPDDecoder *s) {
    const size_t top = ((size_t)s->width + 1) * 4;
    int          ret;

    ret = image_alloc_argb(&s->lossless_out, s->width, s->height);
    if (ret < 0)
        return ret;
    if (s->lossless_top_size < top) {
        uint8_t *buf = realloc(s->lossless_top, top);

        if (!buf)
            return WPD_ERROR(ENOMEM);
        s->lossless_top      = buf;
        s->lossless_top_size = top;
    }
    return 0;
}

/* Decodes as much of a still lossless image as the buffered bytes allow.
   Returns 1 once the whole image is out, 0 while more input is needed. */
static int vp8l_still_step(WPDDecoder *s, const uint8_t *payload,
                           unsigned avail, unsigned size, int complete) {
    int rows, ret, i, w, h;

    if (!s->vp8l_active) {
        const size_t first = WPD_MAX((size_t)16, (size_t)size / 16);

        if (avail < first || (!complete && avail < s->vp8l_next_try))
            return 0;
        for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&s->image[i]);
        s->width = s->height = 0;
        ret = vp8l_read_frame_header(s, &s->argb, payload, avail, 0, &w, &h);
        if (ret >= 0 && br_is_eos(&s->gb))
            ret = WPD_ERROR_INVALID_DATA;
        if (ret < 0) {
            for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&s->image[i]);
            if (complete)
                return ret;
            s->vp8l_next_try = 2 * (size_t)avail;
            return 0;
        }
        s->vp8l_pos    = 0;
        s->vp8l_cached = 0;
        s->vp8l_x = s->vp8l_y = s->vp8l_hg = 0;
        s->vp8l_rows_done = s->vp8l_rows_out = 0;
        s->vp8l_peeked                       = 0;
        s->vp8l_active                       = 1;
        s->still_lossless                    = 1;
        s->lossless_frame                    = &s->argb;
    } else {
        br_extend(&s->gb, payload, avail);
    }

    ret = vp8l_resume_argb_pixels(s);
    if (ret < 0)
        return ret;
    if (ret == VP8L_NEED_MORE && complete)
        return WPD_ERROR_INVALID_DATA;

    rows = s->vp8l_rows_done;
    if (ret)
        rows -= rows % VP8L_ROW_BATCH;
    if (s->vp8l_peeked && rows > s->vp8l_rows_out) {
        int done = vp8l_transform_rows(s, s->vp8l_rows_out, rows);

        if (done < 0)
            return done;
        s->vp8l_rows_out = rows;
    }
    if (ret)
        return 0;

    /* Nobody looked, so the image can be transformed where it lies. */
    if (!s->vp8l_peeked)
        ret = vp8l_apply_transforms(s);
    s->argb.linesize[0] = s->argb.width * 4;
    for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&s->image[i]);
    s->vp8l_active = 0;
    return ret < 0 ? ret : 1;
}

/* Switches the in-progress image over to handing rows out as they finish,
   which needs somewhere to put them: backward references keep reading the
   untransformed pixels for as long as the image is being decoded. */
static int vp8l_still_peek(WPDDecoder *s) {
    int ret, rows;

    if (!s->vp8l_peeked) {
        ret = vp8l_still_alloc(s);
        if (ret < 0)
            return ret;
        s->vp8l_peeked    = 1;
        s->lossless_frame = &s->lossless_out;
    }
    rows = s->vp8l_rows_done - s->vp8l_rows_done % VP8L_ROW_BATCH;
    if (rows > s->vp8l_rows_out) {
        ret = vp8l_transform_rows(s, s->vp8l_rows_out, rows);
        if (ret < 0)
            return ret;
        s->vp8l_rows_out = rows;
    }
    return 0;
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

static void vp8_lossy_export_planes(const WPDDecoder *s, WebPImage *out,
                                    const WpdFrame *decoded) {
    memset(out, 0, sizeof(*out));
    out->width  = s->width;
    out->height = s->height;
    out->format = WPD_PIX_FMT_YUV420P;
    for (int plane = 0; plane < 3; plane++) {
        out->data[plane]     = decoded->data[plane];
        out->linesize[plane] = decoded->linesize[plane];
    }
    if (s->has_alpha) {
        out->data[3]     = s->alpha_plane;
        out->linesize[3] = s->width;
        out->format      = WPD_PIX_FMT_YUVA420P;
    }
}

static int vp8_lossy_alpha_plane(WPDDecoder *s, WebPImage *out) {
    const size_t alpha_size = (size_t)s->width * s->height;
    int          ret;

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
    ret              = vp8_lossy_decode_alpha(
        s, out, file_at(s, s->alpha_data_offset), s->alpha_data_size);
    s->alpha_pending = 0;
    return ret;
}

static int vp8_lossy_init(WPDDecoder *s) {
    int ret;

    if (s->vp8_initialized)
        return 0;
    s->codec.priv_data = &s->vp8;
    ret                = vp8_decode_init(&s->codec);
    if (ret < 0)
        return ret;
    s->vp8_initialized = 1;
    return 0;
}

/* Returns 1 when the frame is complete, 0 when more of the chunk is needed. */
static int vp8_lossy_step(WPDDecoder *s, WebPImage *out,
                          const uint8_t *data_start, unsigned int avail,
                          unsigned int data_size) {
    WpdFrame decoded;
    int      ret;

    if ((ret = vp8_lossy_init(s)) < 0)
        return ret;

    if (!s->vp8_active) {
        ret = vp8_decode_frame_init(
            &s->codec, data_start, (int)avail, (int)data_size);
        if (ret < 0)
            return ret;
        if (ret)
            return 0;

        update_canvas_size(s, s->codec.width, s->codec.height);
        vp8_lossy_export_planes(s, out, &s->vp8.frame);
        if (s->has_alpha && (ret = vp8_lossy_alpha_plane(s, out)) < 0)
            return ret;
        s->still_lossy = !s->animation;
        s->vp8_active  = 1;
    } else {
        vp8_decode_extend(&s->codec, data_start, (int)avail);
    }

    ret = vp8_decode_rows(&s->codec, &decoded);
    if (ret < 0)
        return ret;
    vp8_lossy_export_planes(s, out, &decoded);
    if (ret)
        return 0;

    s->vp8_active = 0;
    return 1;
}

static int vp8_lossy_decode_frame(WPDDecoder *s, WebPImage *out,
                                  const uint8_t *data_start,
                                  unsigned int   data_size) {
    WpdPacket packet;
    WpdFrame  decoded;
    int       ret;

    if ((ret = vp8_lossy_init(s)) < 0)
        return ret;

    packet.data = data_start;
    packet.size = data_size;
    ret         = vp8_decode_frame(&s->codec, &decoded, &packet);
    if (ret < 0)
        return ret;

    update_canvas_size(s, s->codec.width, s->codec.height);
    vp8_lossy_export_planes(s, out, &decoded);
    if (s->has_alpha && (ret = vp8_lossy_alpha_plane(s, out)) < 0)
        return ret;
    s->still_lossy = !s->animation;
    return 0;
}

static int image_nb_components(const WebPImage *img) {
    switch (img->format) {
    case WPD_PIX_FMT_YUV420P: return 3;
    case WPD_PIX_FMT_YUVA420P: return 4;
    default: return 1;
    }
}

typedef struct SubRect {
    int x, y, w, h;
} SubRect;

static int format_is_packed(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_ARGB;
}

static int format_bpp(WPDPixelFormat format) {
    return format == WPD_PIX_FMT_RGB || format == WPD_PIX_FMT_BGR ? 3 : 4;
}

static int format_is_premultiplied(WPDPixelFormat format) {
    return format == WPD_PIX_FMT_ARGB_PRE || format == WPD_PIX_FMT_RGBA_PRE ||
        format == WPD_PIX_FMT_BGRA_PRE;
}

static int format_valid(WPDPixelFormat format) {
    return format >= WPD_PIX_FMT_YUV420P && format <= WPD_PIX_FMT_BGRA_PRE;
}

static pack_row_func format_packer(const WPDDecoder *s, WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGBA:
    case WPD_PIX_FMT_RGBA_PRE: return s->ydsp.pack_rgba;
    case WPD_PIX_FMT_BGRA:
    case WPD_PIX_FMT_BGRA_PRE: return s->ydsp.pack_bgra;
    case WPD_PIX_FMT_RGB: return s->ydsp.pack_rgb;
    case WPD_PIX_FMT_BGR: return s->ydsp.pack_bgr;
    default: return NULL;
    }
}

/* The byte layouts the upsampler can emit without a second pass. */
static int format_layout(WPDPixelFormat format) {
    switch (format) {
    case WPD_PIX_FMT_RGBA:
    case WPD_PIX_FMT_RGBA_PRE: return WPD_LAYOUT_RGBA;
    case WPD_PIX_FMT_BGRA:
    case WPD_PIX_FMT_BGRA_PRE: return WPD_LAYOUT_BGRA;
    case WPD_PIX_FMT_RGB: return WPD_LAYOUT_RGB;
    case WPD_PIX_FMT_BGR: return WPD_LAYOUT_BGR;
    default: return WPD_LAYOUT_ARGB;
    }
}

static void blend_argb_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(s->pos_y + r.y + y) * dst->linesize[0] +
            (s->pos_x + r.x) * 4;

        if (s->premultiply)
            s->ldsp.blend_row_argb_premult(dst_argb, src_argb, r.w);
        else
            s->ldsp.blend_row_argb(dst_argb, src_argb, r.w);
    }
}

static void copy_argb_region(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, SubRect r) {
    for (int y = 0; y < r.h; y++) {
        const uint8_t *src_argb = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x * 4;
        uint8_t *dst_argb = dst->data[0] +
            (ptrdiff_t)(s->pos_y + r.y + y) * dst->linesize[0] +
            (s->pos_x + r.x) * 4;

        memcpy(dst_argb, src_argb, (size_t)r.w * 4);
    }
}

static void blend_yuva_region(WPDDecoder *s, WebPImage *dst,
                              const WebPImage *src, SubRect r) {
    int base_x = s->pos_x + r.x, base_y = s->pos_y + r.y;

    for (int y = 0; y < CEIL_RSHIFT(r.h, 1); y++) {
        int            tile_h = WPD_MIN(r.h - y * 2, 2);
        const uint8_t *src_u  = src->data[1] +
            (ptrdiff_t)((r.y >> 1) + y) * src->linesize[1] + (r.x >> 1);
        const uint8_t *src_v = src->data[2] +
            (ptrdiff_t)((r.y >> 1) + y) * src->linesize[2] + (r.x >> 1);
        uint8_t *dst_u = dst->data[1] +
            (ptrdiff_t)((base_y >> 1) + y) * dst->linesize[1] + (base_x >> 1);
        uint8_t *dst_v = dst->data[2] +
            (ptrdiff_t)((base_y >> 1) + y) * dst->linesize[2] + (base_x >> 1);
        for (int x = 0; x < CEIL_RSHIFT(r.w, 1); x++) {
            int tile_w    = WPD_MIN(r.w - x * 2, 2);
            int src_alpha = 0;
            int dst_alpha = 0;
            for (int yy = 0; yy < tile_h; yy++) {
                for (int xx = 0; xx < tile_w; xx++) {
                    src_alpha += src->data[3][(ptrdiff_t)(r.y + y * 2 + yy) *
                                                  src->linesize[3] +
                                              (r.x + x * 2 + xx)];
                    dst_alpha += dst->data[3][(ptrdiff_t)(base_y + y * 2 + yy) *
                                                  dst->linesize[3] +
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
        const uint8_t *src_y = src->data[0] +
            (ptrdiff_t)(r.y + y) * src->linesize[0] + r.x;
        const uint8_t *src_a = src->data[3] +
            (ptrdiff_t)(r.y + y) * src->linesize[3] + r.x;
        uint8_t *dst_y = dst->data[0] +
            (ptrdiff_t)(base_y + y) * dst->linesize[0] + base_x;
        uint8_t *dst_a = dst->data[3] +
            (ptrdiff_t)(base_y + y) * dst->linesize[3] + base_x;
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
            (ptrdiff_t)(r.y >> shift) * src->linesize[comp] + (r.x >> shift);
        uint8_t *dst_p = dst->data[comp] +
            (ptrdiff_t)(base_y >> shift) * dst->linesize[comp] +
            (base_x >> shift);

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

static int convert_to_packed(WPDDecoder *s, WebPImage *dst,
                             const WebPImage *src, WPDPixelFormat format) {
    const int layout = format_layout(format);
    int       ret;

    ret = image_alloc_packed(
        dst, src->width, src->height, format_bpp(format), format);

    if (ret < 0)
        return ret;
    wpd_yuv420_to_packed(&s->ydsp,
                         layout,
                         dst->data[0],
                         dst->linesize[0],
                         src->data[0],
                         src->linesize[0],
                         src->data[1],
                         src->data[2],
                         src->linesize[1],
                         src->data[3],
                         src->linesize[3],
                         src->width,
                         src->height);
    return 0;
}

static int convert_to_argb(WPDDecoder *s, WebPImage *dst,
                           const WebPImage *src) {
    return convert_to_packed(s, dst, src, WPD_PIX_FMT_ARGB);
}

static int convert_argb_to_yuva(WPDDecoder *s, WebPImage *dst,
                                const WebPImage *src, int want_alpha,
                                int row_start, int row_end) {
    int ret;

    if (!row_start &&
        (ret = image_alloc_yuva(dst, src->width, src->height)) < 0)
        return ret;
    wpd_argb_to_yuva(&s->ydsp,
                     dst->data[0],
                     dst->linesize[0],
                     dst->data[1],
                     dst->data[2],
                     dst->linesize[1],
                     want_alpha ? dst->data[3] : NULL,
                     dst->linesize[3],
                     src->data[0],
                     src->linesize[0],
                     src->width,
                     row_start,
                     row_end);
    if (!want_alpha)
        for (int y = row_start; y < row_end; y++)
            memset(dst->data[3] + (ptrdiff_t)y * dst->linesize[3],
                   255,
                   (size_t)src->width);
    return 0;
}

static int ensure_yuva_rows(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                            int want_alpha, int row_start, int row_end) {
    int ret;

    if (src->format == WPD_PIX_FMT_ARGB)
        return convert_argb_to_yuva(
            s, dst, src, want_alpha, row_start, row_end);
    if (!row_start &&
        (ret = image_alloc_yuva(dst, src->width, src->height)) < 0)
        return ret;
    for (int p = 0; p < 4; p++) {
        const int shift = p == 1 || p == 2;
        const int w     = CEIL_RSHIFT(src->width, shift);
        const int h     = CEIL_RSHIFT(row_end, shift);

        for (int y = row_start >> shift; y < h; y++) {
            uint8_t *out = dst->data[p] + (ptrdiff_t)y * dst->linesize[p];

            if (p == 3 && src->format == WPD_PIX_FMT_YUV420P)
                memset(out, 255, (size_t)w);
            else
                memcpy(out,
                       src->data[p] + (ptrdiff_t)y * src->linesize[p],
                       (size_t)w);
        }
    }
    return 0;
}

static int ensure_yuva(WPDDecoder *s, WebPImage *dst, const WebPImage *src,
                       int want_alpha) {
    return ensure_yuva_rows(s, dst, src, want_alpha, 0, src->height);
}

static void composite_region(WPDDecoder *s, const WebPImage *frame, SubRect r,
                             int blend) {
    WebPImage *canvas = &s->canvas;

    if (r.w <= 0 || r.h <= 0)
        return;

    if (canvas->format == WPD_PIX_FMT_ARGB) {
        if (blend)
            blend_argb_region(s, canvas, frame, r);
        else
            copy_argb_region(s, canvas, frame, r);
    } else {
        if (blend)
            blend_yuva_region(s, canvas, frame, r);
        else
            copy_yuva_region(s, canvas, frame, r);
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
                (ptrdiff_t)(pos_y >> shift) * canvas->linesize[comp] +
                (pos_x >> shift);
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

/* The canvas holds whichever alpha convention the output format asked for when
   its pixels were composited, and the caller may change that format between
   frames. Bring what is already there into the convention the next frame will
   be blended in, so the two are never mixed. */
static void reconcile_canvas_alpha(WPDDecoder *s) {
    if (s->canvas.data[0] && s->canvas.format == WPD_PIX_FMT_ARGB &&
        s->canvas.premultiplied != s->premultiply)
        for (int y = 0; y < s->canvas.height; y++) {
            uint8_t *row = s->canvas.data[0] +
                (ptrdiff_t)y * s->canvas.linesize[0];

            if (s->premultiply)
                s->ydsp.premultiply_row(row, 1, s->canvas.width);
            else
                wpd_premultiply_argb_row(row, s->canvas.width, 1);
        }
    s->canvas.premultiplied = s->premultiply;
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
        s->canvas.premultiplied = s->premultiply;
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else if (s->key_frame) {
        if (!covers_canvas)
            clear_canvas_rect(s, 0, 0, s->canvas.width, s->canvas.height);
    } else {
        if (format == WPD_PIX_FMT_ARGB &&
            s->canvas.format == WPD_PIX_FMT_YUVA420P) {
            WebPImage yuva_canvas = s->canvas;
            memset(&s->canvas, 0, sizeof(s->canvas));
            ret = convert_to_argb(s, &s->canvas, &yuva_canvas);
            image_free(&yuva_canvas);
            if (ret < 0)
                return ret;
        }
        if (s->prev_anmf_flags & ANMF_FLAG_DISPOSE)
            clear_canvas_rect(
                s, s->prev_pos_x, s->prev_pos_y, s->prev_width, s->prev_height);
    }

    reconcile_canvas_alpha(s);
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
            int alpha_header     = p[0];
            s->alpha_data_offset = s->discarded + (size_t)(p + 1 - s->file);
            s->alpha_data_size   = payload_size - 1;

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

    WPDPixelFormat target = WPD_PIX_FMT_YUVA420P;
    if (sub->format == WPD_PIX_FMT_ARGB || format_is_packed(s->out_format) ||
        (!s->key_frame && s->canvas.data[0] &&
         s->canvas.format == WPD_PIX_FMT_ARGB))
        target = WPD_PIX_FMT_ARGB;

    if (target == WPD_PIX_FMT_ARGB && sub->format != WPD_PIX_FMT_ARGB) {
        ret = convert_to_argb(s, &s->converted, sub);
        if (ret < 0)
            return ret;
        sub = &s->converted;
    }

    /* libwebp premultiplies each frame before compositing it, which is not
       the same as premultiplying the finished canvas. */
    if (s->premultiply) {
        WebPImage *frame = sub == &s->converted ? &s->converted : &s->argb;

        for (int y = 0; y < frame->height; y++)
            s->ydsp.premultiply_row(
                frame->data[0] + (size_t)y * frame->linesize[0],
                1,
                frame->width);
    }

    ret = prepare_canvas(s, sub, target);
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
                         WPDPixelFormat format, WPDFrame *frame) {
    int planes = format == WPD_PIX_FMT_YUVA420P ? 4
        : format == WPD_PIX_FMT_YUV420P         ? 3
                                                : 1;

    frame_clear(frame);
    for (int p = 0; p < planes; p++) {
        frame->data[p]   = img->data[p];
        frame->stride[p] = img->linesize[p];
    }
    frame->width     = img->width;
    frame->height    = img->height;
    frame->format    = format;
    frame->duration  = s->frame_duration;
    frame->timestamp = s->frame_timestamp - s->frame_duration;
}

static int export_packed(WPDDecoder *s, WebPImage *img, WPDFrame *frame) {
    const WPDPixelFormat format = s->out_format;
    WebPImage            view;
    WebPImage           *planar;
    pack_row_func        pack;
    int                  ret;

    if (format == WPD_PIX_FMT_YUV420P || format == WPD_PIX_FMT_YUVA420P) {
        if ((img->format == WPD_PIX_FMT_YUV420P &&
             format == WPD_PIX_FMT_YUV420P) ||
            (img->format == WPD_PIX_FMT_YUVA420P)) {
            planar = img;
        } else {
            ret = ensure_yuva(
                s, &s->output, img, format == WPD_PIX_FMT_YUVA420P);
            if (ret < 0)
                return ret;
            planar = &s->output;
        }
        export_frame(s, planar, format, frame);
        return 0;
    }
    if (!format_is_packed(format)) {
        export_frame(s, img, img->format, frame);
        return 0;
    }
    if (!format_is_packed(img->format)) {
        ret = convert_to_packed(s, &s->output, img, format);
        if (ret < 0)
            return ret;
        img = &s->output;
    } else if (img->format != format) {
        pack = format_packer(s, format);
        if (!pack) {
            if (format != WPD_PIX_FMT_ARGB_PRE ||
                img->format != WPD_PIX_FMT_ARGB)
                return WPD_ERR_UNSUPPORTED;
            if (s->animation) {
                view        = *img;
                view.format = format;
                img         = &view;
            } else {
                ret = image_alloc_packed(
                    &s->output, img->width, img->height, 4, format);
                if (ret < 0)
                    return ret;
                for (int y = 0; y < img->height; y++)
                    memcpy(s->output.data[0] +
                               (ptrdiff_t)y * s->output.linesize[0],
                           img->data[0] + (ptrdiff_t)y * img->linesize[0],
                           (size_t)img->width * 4);
                img = &s->output;
            }
        } else {
            ret = image_alloc_packed(&s->output,
                                     img->width,
                                     img->height,
                                     format_bpp(format),
                                     format);
            if (ret < 0)
                return ret;
            for (int y = 0; y < img->height; y++)
                pack(s->output.data[0] + (ptrdiff_t)y * s->output.linesize[0],
                     img->data[0] + (ptrdiff_t)y * img->linesize[0],
                     img->width);
            img = &s->output;
        }
    }
    if (s->premultiply && !s->animation)
        for (int y = 0; y < img->height; y++)
            s->ydsp.premultiply_row(
                img->data[0] + (ptrdiff_t)y * img->linesize[0],
                format_layout(img->format) == WPD_LAYOUT_ARGB,
                img->width);
    export_frame(s, img, format, frame);
    return 0;
}

/* Converts and hands out rows [0, upto) of the still lossy frame, converting
   each row exactly once however many times it is asked for. */
static int export_still_packed(WPDDecoder *s, WPDFrame *frame, int upto) {
    const WPDPixelFormat format = s->out_format;
    const WebPImage     *src    = &s->subframe;
    WebPImage           *dst    = &s->converted;
    const int first = s->converted_format == format ? s->converted_rows : 0;
    int       converted_from = first;
    int       ret;

    if (upto < s->converted_rows)
        upto = s->converted_rows;

    if (!first) {
        ret = image_alloc_packed(
            dst, src->width, src->height, format_bpp(format), format);
        if (ret < 0)
            return ret;
    }

    if (upto > first) {
        converted_from = wpd_yuv420_to_packed_rows(&s->ydsp,
                                                   format_layout(format),
                                                   dst->data[0],
                                                   dst->linesize[0],
                                                   src->data[0],
                                                   src->linesize[0],
                                                   src->data[1],
                                                   src->data[2],
                                                   src->linesize[1],
                                                   src->data[3],
                                                   src->linesize[3],
                                                   src->width,
                                                   src->height,
                                                   first,
                                                   upto);
    }
    if (s->premultiply)
        for (int y = converted_from; y < upto; y++)
            s->ydsp.premultiply_row(dst->data[0] + (size_t)y * dst->linesize[0],
                                    format_layout(format) == WPD_LAYOUT_ARGB,
                                    dst->width);

    s->converted_rows   = upto;
    s->converted_format = format;
    export_frame(s, dst, format, frame);
    return 0;
}

/* Hands out rows [0, upto) of the still lossless frame, premultiplying and
   packing each row exactly once however many times it is asked for. */
static int export_still_lossless(WPDDecoder *s, WPDFrame *frame, int upto) {
    const WPDPixelFormat format = s->out_format;
    WebPImage           *img    = s->lossless_frame;
    const int     first = s->converted_format == format ? s->converted_rows : 0;
    pack_row_func pack;
    int           ret;

    if (upto < s->converted_rows)
        upto = s->converted_rows;

    if (format == WPD_PIX_FMT_YUV420P || format == WPD_PIX_FMT_YUVA420P) {
        ret = ensure_yuva_rows(
            s, &s->output, img, format == WPD_PIX_FMT_YUVA420P, first, upto);
        if (ret < 0)
            return ret;
        export_frame(s, &s->output, format, frame);
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (!format_is_packed(format)) {
        export_frame(s, img, img->format, frame);
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    pack = format_packer(s, format);
    if (!s->premultiply && (!pack || img->format == format)) {
        export_frame(s, img, format, frame);
        s->converted_rows   = upto;
        s->converted_format = format;
        return 0;
    }

    if (!first) {
        ret = image_alloc_packed(
            &s->output, img->width, img->height, format_bpp(format), format);
        if (ret < 0)
            return ret;
    }
    for (int y = first; y < upto; y++) {
        uint8_t *dst = s->output.data[0] + (size_t)y * s->output.linesize[0];
        const uint8_t *src = img->data[0] + (size_t)y * img->linesize[0];

        if (pack)
            pack(dst, src, img->width);
        else
            memcpy(dst, src, (size_t)img->width * 4);
        if (s->premultiply)
            s->ydsp.premultiply_row(
                dst, format_layout(format) == WPD_LAYOUT_ARGB, img->width);
    }
    export_frame(s, &s->output, format, frame);
    s->converted_rows   = upto;
    s->converted_format = format;
    return 0;
}

const char *wpd_status_string(WPDStatus status) {
    switch (status) {
    case WPD_OK: return "success";
    case WPD_ERR_INVALID_ARG: return "invalid argument";
    case WPD_ERR_NOT_WEBP: return "not a WebP file";
    case WPD_ERR_BITSTREAM: return "invalid bitstream";
    case WPD_ERR_TRUNCATED: return "truncated file";
    case WPD_ERR_UNSUPPORTED: return "unsupported feature";
    case WPD_ERR_NO_MEMORY: return "out of memory";
    case WPD_ERR_TOO_LARGE: return "image too large";
    }
    return "unknown error";
}

/* Internal failures are either a WPDStatus or a negated errno. */
static WPDStatus status_from_internal(int code) {
    switch (code) {
    case 0: return WPD_OK;
    case WPD_ERROR_INVALID_DATA: return WPD_ERR_BITSTREAM;
    case WPD_ERROR(ENOMEM): return WPD_ERR_NO_MEMORY;
    case WPD_ERROR_TOO_LARGE: return WPD_ERR_TOO_LARGE;
    case WPD_ERROR(EINVAL): return WPD_ERR_INVALID_ARG;
    default: break;
    }
    if (code <= WPD_ERR_INVALID_ARG && code >= WPD_ERR_TOO_LARGE)
        return (WPDStatus)code;
    return WPD_ERR_BITSTREAM;
}

static WPDStatus set_error(WPDDecoder *decoder, const char *message, int code) {
    decoder->status = status_from_internal(code);
    snprintf(decoder->error,
             sizeof(decoder->error),
             "%s (%s)",
             message,
             wpd_status_string(decoder->status));
    return decoder->status;
}

WPDDecoder *wpd_decoder_create(void) {
    WPDDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder)
        return NULL;
    wpd_init_cpu();
    wpd_vp8l_dsp_init(&decoder->ldsp);
    wpd_yuv_dsp_init(&decoder->ydsp);
    decoder->out_format = WPD_PIX_FMT_NONE;
    return decoder;
}

WPDStatus wpd_decoder_set_output_format(WPDDecoder    *decoder,
                                        WPDPixelFormat format) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (format != WPD_PIX_FMT_NONE && !format_valid(format))
        return set_error(decoder, "invalid output format", WPD_ERR_INVALID_ARG);
    decoder->out_format  = format;
    decoder->premultiply = format_is_premultiplied(format);
    return WPD_OK;
}

static void scan_still_header(HeaderScan *hs, uint32_t tag, const uint8_t *p,
                              size_t avail, size_t size) {
    if (tag == MKTAG('V', 'P', '8', 'L')) {
        hs->coding = WPD_CODING_LOSSLESS;
        if (avail >= 5 && p[0] == 0x2f) {
            uint32_t bits = WPD_RL32(p + 1);

            if (bits >> 29)
                return;
            hs->width  = (bits & 0x3fff) + 1;
            hs->height = ((bits >> 14) & 0x3fff) + 1;
            hs->has_alpha |= (bits >> 28) & 1;
        }
    } else {
        hs->coding = WPD_CODING_LOSSY;
        if (avail >= 10 && size >= 10 && p[3] == 0x9d && p[4] == 0x01 &&
            p[5] == 0x2a) {
            uint32_t bits = WPD_RL24(p);

            if ((bits & 1) || ((bits >> 1) & 7) > 3 || !(bits & 0x10) ||
                (bits >> 5) > size - 10)
                return;
            hs->width  = WPD_RL16(p + 6) & 0x3fff;
            hs->height = WPD_RL16(p + 8) & 0x3fff;
        }
    }
}

static WPDStatus scan_raw_headers(HeaderScan *hs, const uint8_t *data,
                                  size_t size, int partial) {
    uint32_t tag;

    hs->truncated = 0;
    if (!size)
        return WPD_ERR_TRUNCATED;
    if (data[0] == 0x2f) {
        hs->raw_kind         = 1;
        hs->raw_image_offset = 0;
        hs->raw_image_size   = size;
        if (size < 5)
            return WPD_ERR_TRUNCATED;
        scan_still_header(hs, MKTAG('V', 'P', '8', 'L'), data, size, size);
    } else if (size >= 6 && data[3] == 0x9d && data[4] == 0x01 &&
               data[5] == 0x2a) {
        /* A bare stream declares no payload length, so until the caller says
           the stream has ended the keyframe header's own first partition is
           the only length to measure it against. */
        size_t payload;

        hs->raw_kind         = 2;
        hs->raw_image_offset = 0;
        hs->raw_image_size   = size;
        if (size < 10)
            return WPD_ERR_TRUNCATED;
        payload = 10 + (size_t)(WPD_RL24(data) >> 5);
        if (!partial || payload < size)
            payload = size;
        scan_still_header(hs, MKTAG('V', 'P', '8', ' '), data, size, payload);
        if (hs->width && payload > size)
            hs->truncated = 1;
    } else if (size >= 4 && WPD_RL32(data) == MKTAG('A', 'L', 'P', 'H')) {
        uint32_t alpha_size, image_size;
        uint64_t padded;
        size_t   image_header, have;

        hs->raw_kind = 3;
        if (size < 8)
            return WPD_ERR_TRUNCATED;
        alpha_size = WPD_RL32(data + 4);
        if (alpha_size == UINT32_MAX)
            return WPD_ERR_BITSTREAM;
        padded = (uint64_t)alpha_size + (alpha_size & 1);
        if (padded > (uint64_t)(size - 8) || size - 8 - padded < 8)
            return WPD_ERR_TRUNCATED;
        image_header = 8 + (size_t)padded;
        tag          = WPD_RL32(data + image_header);
        if (tag != MKTAG('V', 'P', '8', ' '))
            return WPD_ERR_BITSTREAM;
        image_size = WPD_RL32(data + image_header + 4);
        have       = image_size;
        if ((size_t)image_size > size - image_header - 8) {
            hs->truncated = 1;
            if (!partial)
                return WPD_ERR_TRUNCATED;
            have = size - image_header - 8;
        }
        hs->raw_alpha_offset = 8;
        hs->raw_alpha_size   = alpha_size;
        hs->raw_image_offset = image_header + 8;
        hs->raw_image_size   = have;
        hs->has_alpha        = 1;
        if (have < 10)
            return WPD_ERR_TRUNCATED;
        scan_still_header(
            hs, tag, data + hs->raw_image_offset, have, image_size);
    } else {
        return size < 12 && partial ? WPD_ERR_TRUNCATED : WPD_ERR_NOT_WEBP;
    }
    hs->frame_count = 1;
    hs->images      = 1;
    hs->end         = size;
    return hs->width && hs->height ? WPD_OK : WPD_ERR_BITSTREAM;
}

/* Walks the chunk list without decoding anything, so it is safe to run on the
   caller's memory before the file is copied. Resumes from where it stopped
   last time, so feeding a stream one piece at a time stays linear; 'base' is
   the stream offset the buffer now starts at, once earlier bytes have been
   dropped. */
static WPDStatus scan_headers(HeaderScan *hs, const uint8_t *data, size_t base,
                              size_t size, int partial) {
    int partial_still = 0;

    hs->truncated = 0;

    if (!hs->pos) {
        if (size < 12 && size >= 4 &&
            WPD_RL32(data) == MKTAG('R', 'I', 'F', 'F'))
            return WPD_ERR_TRUNCATED;
        if (size < 12 || WPD_RL32(data) != MKTAG('R', 'I', 'F', 'F') ||
            WPD_RL32(data + 8) != MKTAG('W', 'E', 'B', 'P'))
            return scan_raw_headers(hs, data, size, partial);
        hs->riff_end = (uint64_t)WPD_RL32(data + 4) + 8;
        hs->pos      = 12;
    }

    hs->end = size;
    if (hs->riff_end < (uint64_t)size)
        hs->end = (size_t)hs->riff_end;
    else if (hs->riff_end > (uint64_t)size)
        hs->truncated = 1;

    while (hs->pos + 8 <= hs->end) {
        const uint8_t *chunk = data + (hs->pos - base);
        uint32_t       tag   = WPD_RL32(chunk);
        uint32_t       size_ = WPD_RL32(chunk + 4);
        uint32_t       padded_size;

        if (size_ == UINT32_MAX) {
            hs->truncated = 1;
            break;
        }
        padded_size = size_ + (size_ & 1);
        if (hs->end - (hs->pos + 8) < padded_size) {
            hs->truncated = 1;
            if (partial && !hs->images &&
                (tag == MKTAG('V', 'P', '8', ' ') ||
                 tag == MKTAG('V', 'P', '8', 'L'))) {
                const int width = hs->width, height = hs->height;

                partial_still = 1;
                scan_still_header(
                    hs, tag, chunk + 8, hs->end - (hs->pos + 8), size_);
                if (hs->vp8x && width && height) {
                    hs->width  = width;
                    hs->height = height;
                }
            }
            break;
        }

        switch (tag) {
        case MKTAG('V', 'P', '8', 'X'):
            hs->vp8x = 1;
            if (size_ >= 10) {
                hs->has_alpha |= (chunk[8] & VP8X_FLAG_ALPHA) != 0;
                hs->width  = WPD_RL24(chunk + 12) + 1;
                hs->height = WPD_RL24(chunk + 15) + 1;
                if ((uint64_t)hs->width * (uint64_t)hs->height >= 1ULL << 32)
                    return WPD_ERR_TOO_LARGE;
            }
            break;
        case MKTAG('A', 'L', 'P', 'H'): hs->has_alpha = 1; break;
        case MKTAG('A', 'N', 'I', 'M'):
            hs->animation = 1;
            if (size_ >= 6) {
                hs->background_argb = WPD_RL32(chunk + 8);
                hs->loop_count      = WPD_RL16(chunk + 12);
            }
            break;
        case MKTAG('A', 'N', 'M', 'F'): hs->frame_count++; break;
        case MKTAG('V', 'P', '8', ' '):
        case MKTAG('V', 'P', '8', 'L'):
            if (!hs->images++) {
                int width = hs->width, height = hs->height;

                scan_still_header(hs, tag, chunk + 8, size_, size_);
                if (hs->vp8x && width && height) {
                    hs->width  = width;
                    hs->height = height;
                }
            }
            break;
        default: break;
        }
        hs->pos += 8 + padded_size;
    }

    /* An animation may mix lossy and lossless frames, which libwebp reports as
       an undefined coding; only the first still's coding is meaningful. */
    if (hs->animation)
        hs->coding = WPD_CODING_UNKNOWN;
    else
        hs->frame_count = hs->images || partial_still ? 1 : 0;

    if (!hs->width || !hs->height)
        return hs->truncated ? WPD_ERR_TRUNCATED : WPD_ERR_BITSTREAM;
    return WPD_OK;
}

static void info_from_scan(WPDImageInfo *info, const HeaderScan *hs) {
    info->width           = hs->width;
    info->height          = hs->height;
    info->has_alpha       = hs->has_alpha;
    info->is_animation    = hs->animation;
    info->frame_count     = hs->frame_count;
    info->loop_count      = hs->loop_count;
    info->background_argb = hs->background_argb;
    info->coding          = hs->coding;
}

WPDStatus wpd_get_info(const uint8_t *data, size_t size, WPDImageInfo *info) {
    HeaderScan hs;
    WPDStatus  status;

    if (!data || !info_valid(info))
        return WPD_ERR_INVALID_ARG;

    info_clear(info);
    memset(&hs, 0, sizeof(hs));
    status = scan_headers(&hs, data, 0, size, 1);
    if (status != WPD_OK)
        return status;
    info_from_scan(info, &hs);
    return WPD_OK;
}

/* Clears everything derived from a file but keeps the input allocation, which
   a stream grows across many calls. */
static void decoder_reset(WPDDecoder *decoder) {
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&decoder->image[i]);
    image_free(&decoder->canvas);
    image_free(&decoder->argb);
    image_free(&decoder->lossless_out);
    image_free(&decoder->converted);
    image_free(&decoder->output);
    memset(&decoder->subframe, 0, sizeof(decoder->subframe));
    decoder->file_size = 0;
    decoder->discarded = 0;
    decoder->file      = decoder->file_alloc;
    memset(&decoder->scan, 0, sizeof(decoder->scan));
    decoder->pos = decoder->end = 0;
    decoder->opened             = 0;
    decoder->streaming          = 0;
    decoder->eos                = 0;
    decoder->headers_valid      = 0;
    decoder->truncated          = 0;
    decoder->borrowed           = 0;
    decoder->input_mode         = 0;
    decoder->animation          = 0;
    decoder->still_done         = 0;
    decoder->vp8_active         = 0;
    decoder->still_lossy        = 0;
    decoder->alpha_pending      = 0;
    decoder->converted_rows     = 0;
    decoder->converted_format   = WPD_PIX_FMT_NONE;
    decoder->vp8l_active        = 0;
    decoder->still_lossless     = 0;
    decoder->vp8l_next_try      = 0;
    decoder->vp8l_peeked        = 0;
    decoder->lossless_frame     = NULL;
    decoder->frame_index        = 0;
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
    decoder->clear_yuva[0]  = RGB_TO_Y_CCIR(0, 0, 0);
    decoder->clear_yuva[1]  = RGB_TO_U_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[2]  = RGB_TO_V_CCIR(0, 0, 0, 0);
    decoder->clear_yuva[3]  = 0;
    decoder->info_has_alpha = 0;
    decoder->info_coding    = WPD_CODING_UNKNOWN;
    decoder->status         = WPD_OK;
    decoder->error[0]       = 0;
}

/* Drops input the decoder can no longer look at. The chunk at 'pos' is kept
   whole: a VP8 chunk decoded row by row keeps range coders pointing into it
   until the frame is done, and those are rebased on the next step. */
static void file_compact(WPDDecoder *decoder) {
    size_t keep = decoder->pos;

    if (decoder->alpha_pending && decoder->alpha_data_offset < keep)
        keep = decoder->alpha_data_offset;
    if (keep < decoder->discarded || keep - decoder->discarded < 1 << 16)
        return;

    memmove(decoder->file_alloc,
            file_at(decoder, keep),
            decoder->file_size - keep + WPD_FILE_PADDING);
    decoder->file      = decoder->file_alloc;
    decoder->discarded = keep;
}

static WPDStatus file_reserve(WPDDecoder *decoder, size_t size) {
    const size_t buffered = file_buffered(decoder);
    const size_t needed   = buffered + size + WPD_FILE_PADDING;
    size_t       capacity;
    uint8_t     *grown;

    if (size > (size_t)INT_MAX - WPD_FILE_PADDING ||
        buffered > (size_t)INT_MAX - WPD_FILE_PADDING - size)
        return WPD_ERR_TOO_LARGE;
    if (decoder->file_capacity >= needed)
        return WPD_OK;

    capacity = decoder->file_capacity ? decoder->file_capacity : 1 << 16;
    while (capacity < needed) capacity *= 2;
    grown = realloc(decoder->file_alloc, capacity);
    if (!grown)
        return WPD_ERR_NO_MEMORY;
    decoder->file_alloc    = grown;
    decoder->file          = grown;
    decoder->file_capacity = capacity;
    return WPD_OK;
}

static WPDStatus rescan_headers(WPDDecoder *decoder) {
    const HeaderScan *hs     = &decoder->scan;
    WPDStatus         status = scan_headers(&decoder->scan,
                                            decoder->file,
                                            decoder->discarded,
                                            decoder->file_size,
                                            decoder->streaming);

    if (status != WPD_OK)
        return status;

    decoder->end                  = hs->end;
    decoder->canvas_width         = hs->width;
    decoder->canvas_height        = hs->height;
    decoder->animation            = hs->animation;
    decoder->anim_frame_count     = hs->frame_count;
    decoder->anim_loop_count      = hs->loop_count;
    decoder->anim_background_argb = hs->background_argb;
    decoder->info_has_alpha       = hs->has_alpha;
    decoder->info_coding          = hs->coding;
    decoder->truncated            = hs->truncated;
    if (!decoder->headers_valid) {
        decoder->pos           = hs->raw_kind ? 0 : 12;
        decoder->headers_valid = 1;
    }
    return WPD_OK;
}

/* No more input is coming, so a chunk list that stops short of what it
   promised, or that never carried an image, cannot be completed. */
static WPDStatus check_final_headers(WPDDecoder *decoder, const char *message) {
    const HeaderScan *hs = &decoder->scan;

    if (hs->truncated)
        return set_error(decoder, message, WPD_ERR_TRUNCATED);
    if (!hs->images && !hs->frame_count)
        return set_error(decoder, "no image data found", WPD_ERR_BITSTREAM);
    return WPD_OK;
}

WPDStatus wpd_decoder_open(WPDDecoder *decoder, const uint8_t *data,
                           size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);

    decoder_reset(decoder);

    status = file_reserve(decoder, size);
    if (status != WPD_OK)
        return set_error(decoder, "cannot buffer input", status);
    memcpy(decoder->file_alloc, data, size);
    memset(decoder->file_alloc + size, 0, WPD_FILE_PADDING);
    decoder->file      = decoder->file_alloc;
    decoder->file_size = size;
    decoder->discarded = 0;

    status = rescan_headers(decoder);
    if (status != WPD_OK) {
        decoder->file_size = 0;
        return set_error(decoder, "cannot read headers", status);
    }
    status = check_final_headers(decoder, "file ends inside a chunk");
    if (status != WPD_OK) {
        decoder->file_size     = 0;
        decoder->headers_valid = 0;
        return status;
    }
    decoder->opened = 1;
    decoder->eos    = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_open_borrowed(WPDDecoder *decoder, const uint8_t *data,
                                    size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);

    decoder_reset(decoder);
    decoder->file      = data;
    decoder->file_size = size;
    decoder->borrowed  = 1;

    status = rescan_headers(decoder);
    if (status != WPD_OK)
        status = set_error(decoder, "cannot read headers", status);
    else
        status = check_final_headers(decoder, "file ends inside a chunk");
    if (status != WPD_OK) {
        decoder->file          = decoder->file_alloc;
        decoder->file_size     = 0;
        decoder->borrowed      = 0;
        decoder->headers_valid = 0;
        return status;
    }
    decoder->opened = 1;
    decoder->eos    = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_open_stream(WPDDecoder *decoder) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    decoder_reset(decoder);
    decoder->opened    = 1;
    decoder->streaming = 1;
    return WPD_OK;
}

WPDStatus wpd_decoder_append(WPDDecoder *decoder, const uint8_t *data,
                             size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);
    if (!decoder->streaming || decoder->eos)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);
    if (!size)
        return WPD_OK;
    if (decoder->input_mode == 2)
        return set_error(
            decoder, "cannot mix append and update", WPD_ERR_INVALID_ARG);
    decoder->input_mode = 1;

    file_compact(decoder);
    status = file_reserve(decoder, size);
    if (status != WPD_OK)
        return set_error(decoder, "cannot buffer input", status);
    memcpy(decoder->file_alloc + file_buffered(decoder), data, size);
    decoder->file_size += size;
    memset(decoder->file_alloc + file_buffered(decoder), 0, WPD_FILE_PADDING);

    status = rescan_headers(decoder);
    /* Headers that are merely incomplete are the normal state of a stream. */
    if (status == WPD_ERR_TRUNCATED)
        return WPD_OK;
    if (status != WPD_OK)
        return set_error(decoder, "cannot read headers", status);
    return WPD_OK;
}

WPDStatus wpd_decoder_update(WPDDecoder *decoder, const uint8_t *data,
                             size_t size) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!data)
        return set_error(decoder, "invalid input data", WPD_ERR_INVALID_ARG);
    if (!decoder->streaming || decoder->eos)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);
    if (decoder->input_mode == 1)
        return set_error(
            decoder, "cannot mix append and update", WPD_ERR_INVALID_ARG);
    if (size < decoder->file_size)
        return set_error(decoder, "stream buffer shrank", WPD_ERR_INVALID_ARG);

    decoder->input_mode = 2;
    decoder->borrowed   = 1;
    decoder->file       = data;
    decoder->file_size  = size;
    decoder->discarded  = 0;

    status = rescan_headers(decoder);
    if (status == WPD_ERR_TRUNCATED)
        return WPD_OK;
    if (status != WPD_OK) {
        decoder->file          = decoder->file_alloc;
        decoder->file_size     = 0;
        decoder->borrowed      = 0;
        decoder->headers_valid = 0;
        return set_error(decoder, "cannot read headers", status);
    }
    return WPD_OK;
}

WPDStatus wpd_decoder_end_of_stream(WPDDecoder *decoder) {
    WPDStatus status;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!decoder->streaming)
        return set_error(decoder, "not an open stream", WPD_ERR_INVALID_ARG);

    decoder->eos = 1;
    status       = rescan_headers(decoder);
    if (status != WPD_OK)
        return set_error(decoder, "cannot read headers", status);
    return check_final_headers(decoder, "stream ended early");
}

WPDStatus wpd_decoder_get_info(const WPDDecoder *decoder, WPDImageInfo *info) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!info_valid(info) || !decoder->opened)
        return set_error((WPDDecoder *)decoder,
                         "invalid decoder state",
                         WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid)
        return set_error(
            (WPDDecoder *)decoder, "headers incomplete", WPD_ERR_TRUNCATED);

    info_clear(info);
    info->width           = decoder->canvas_width;
    info->height          = decoder->canvas_height;
    info->has_alpha       = decoder->info_has_alpha;
    info->is_animation    = decoder->animation;
    info->frame_count     = decoder->anim_frame_count;
    info->loop_count      = decoder->anim_loop_count;
    info->background_argb = decoder->anim_background_argb;
    info->coding          = decoder->info_coding;
    return WPD_OK;
}

static int still_lossy_pending(const WPDDecoder *decoder, uint32_t chunk_type) {
    return chunk_type == MKTAG('V', 'P', '8', ' ') && !decoder->animation &&
        !decoder->still_done;
}

static int still_lossless_pending(const WPDDecoder *decoder,
                                  uint32_t          chunk_type) {
    return chunk_type == MKTAG('V', 'P', '8', 'L') && !decoder->animation &&
        !decoder->still_done;
}

static int emit_still_lossless(WPDDecoder *decoder, WPDFrame *frame) {
    int ret;

    decoder->still_done = 1;
    ret                 = export_still_lossless(
        decoder, frame, decoder->lossless_frame->height);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int emit_still_lossy(WPDDecoder *decoder, WPDFrame *frame) {
    int ret;

    decoder->still_done = 1;
    if (format_is_packed(decoder->out_format))
        ret = export_still_packed(decoder, frame, decoder->subframe.height);
    else
        ret = export_packed(decoder, &decoder->subframe, frame);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

static int decode_raw(WPDDecoder *decoder, WPDFrame *frame) {
    const HeaderScan *hs   = &decoder->scan;
    const uint8_t    *data = file_at(decoder, hs->raw_image_offset);
    int               ret;

    if (!decoder->eos)
        return 0;
    if (hs->truncated)
        return set_error(decoder, "raw image is truncated", WPD_ERR_TRUNCATED);
    if (hs->raw_image_size > INT_MAX)
        return set_error(decoder, "raw image is too large", WPD_ERR_TOO_LARGE);

    decoder->width = decoder->height = 0;
    if (hs->raw_kind == 1) {
        ret = vp8_lossless_decode_frame(
            decoder, &decoder->argb, data, (unsigned)hs->raw_image_size, 0);
        if (ret < 0)
            return set_error(decoder, "VP8L decode failed", ret);
        decoder->still_done     = 1;
        decoder->still_lossless = 1;
        decoder->lossless_frame = &decoder->argb;
        decoder->converted_rows = decoder->argb.height;
        ret                     = export_packed(decoder, &decoder->argb, frame);
    } else {
        if (hs->raw_kind == 3) {
            const uint8_t *alpha = file_at(decoder, hs->raw_alpha_offset);
            int            header;

            if (!hs->raw_alpha_size)
                return set_error(
                    decoder, "invalid ALPHA chunk", WPD_ERR_BITSTREAM);
            header = alpha[0];
            if ((header & 3) > ALPHA_COMPRESSION_VP8L)
                return set_error(decoder,
                                 "unsupported ALPHA compression",
                                 WPD_ERR_UNSUPPORTED);
            decoder->has_alpha         = 1;
            decoder->alpha_compression = header & 3;
            decoder->alpha_filter      = header >> 2 & 3;
            decoder->alpha_data_offset = hs->raw_alpha_offset + 1;
            decoder->alpha_data_size   = (int)hs->raw_alpha_size - 1;
        }
        ret = vp8_lossy_decode_frame(
            decoder, &decoder->subframe, data, (unsigned)hs->raw_image_size);
        if (ret < 0)
            return set_error(decoder, "VP8 decode failed", ret);
        decoder->still_done = 1;
        ret                 = export_packed(decoder, &decoder->subframe, frame);
    }
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    return 1;
}

int wpd_decoder_next_frame(WPDDecoder *decoder, WPDFrame *frame) {
    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_valid(frame))
        return set_error(decoder, "invalid frame", WPD_ERR_INVALID_ARG);
    if (!decoder->opened)
        return set_error(decoder, "no file opened", WPD_ERR_INVALID_ARG);
    if (!decoder->headers_valid) {
        if (!decoder->eos)
            return 0; /* the headers have not arrived yet */
        return set_error(decoder, "no image data found", WPD_ERR_TRUNCATED);
    }
    if (decoder->scan.raw_kind)
        return decoder->still_done ? 0 : decode_raw(decoder, frame);

    while (decoder->pos + 8 <= decoder->end) {
        const size_t   chunk_pos  = decoder->pos;
        const uint8_t *chunk      = file_at(decoder, chunk_pos);
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
            if (!decoder->eos) {
                if (still_lossy_pending(decoder, chunk_type)) {
                    ret = vp8_lossy_step(
                        decoder,
                        &decoder->subframe,
                        payload,
                        (unsigned)(decoder->end - (decoder->pos + 8)),
                        size);
                    if (ret < 0)
                        return set_error(decoder, "VP8 decode failed", ret);
                    if (ret)
                        return emit_still_lossy(decoder, frame);
                } else if (still_lossless_pending(decoder, chunk_type)) {
                    ret = vp8l_still_step(
                        decoder,
                        payload,
                        (unsigned)(decoder->end - (decoder->pos + 8)),
                        size,
                        0);
                    if (ret < 0)
                        return set_error(decoder, "VP8L decode failed", ret);
                    if (ret)
                        return emit_still_lossless(decoder, frame);
                }
                return 0; /* the rest of this chunk has not arrived yet */
            }
            return set_error(decoder,
                             "chunk runs past the end of the file",
                             WPD_ERR_TRUNCATED);
        }
        decoder->pos += 8 + padded_size;

        switch (chunk_type) {
        case MKTAG('A', 'L', 'P', 'H'): {
            int alpha_header, filter_m, compression;

            if (size == 0)
                return set_error(decoder,
                                 "invalid ALPHA chunk size",
                                 WPD_ERROR_INVALID_DATA);
            alpha_header               = payload[0];
            decoder->alpha_data_offset = chunk_pos + 9;
            decoder->alpha_pending     = 1;
            decoder->alpha_data_size   = size - 1;

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
            if (decoder->vp8_active) {
                ret = vp8_lossy_step(
                    decoder, &decoder->subframe, payload, size, size);
                if (ret == 0)
                    ret = WPD_ERROR_INVALID_DATA;
            } else {
                decoder->width = decoder->height = 0;
                ret                              = vp8_lossy_decode_frame(
                    decoder, &decoder->subframe, payload, size);
            }
            if (ret < 0)
                return set_error(decoder, "VP8 decode failed", ret);
            return emit_still_lossy(decoder, frame);
        case MKTAG('V', 'P', '8', 'L'):
            if (decoder->animation || decoder->still_done)
                break;
            if (decoder->vp8l_active) {
                ret = vp8l_still_step(decoder, payload, size, size, 1);
                if (ret == 0)
                    ret = WPD_ERROR_INVALID_DATA;
                if (ret < 0)
                    return set_error(decoder, "VP8L decode failed", ret);
                return emit_still_lossless(decoder, frame);
            }
            decoder->width = decoder->height = 0;
            ret                              = vp8_lossless_decode_frame(
                decoder, &decoder->argb, payload, size, 0);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
            decoder->still_done = 1;
            ret                 = export_packed(decoder, &decoder->argb, frame);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            decoder->still_lossless = 1;
            decoder->lossless_frame = &decoder->argb;
            decoder->converted_rows = decoder->argb.height;
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
            ret = export_packed(decoder, &decoder->canvas, frame);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            return 1;
        default: break;
        }
    }

    return 0;
}

WPDStatus wpd_decoder_partial_frame(WPDDecoder *decoder, WPDFrame *frame,
                                    int *rows_valid) {
    int rows, ret;

    if (!decoder)
        return WPD_ERR_INVALID_ARG;
    if (!frame_valid(frame))
        return set_error(decoder, "invalid frame", WPD_ERR_INVALID_ARG);
    if (!decoder->opened)
        return set_error(decoder, "no file opened", WPD_ERR_INVALID_ARG);

    frame_clear(frame);
    if (rows_valid)
        *rows_valid = 0;

    if (decoder->still_lossless) {
        if (decoder->vp8l_active) {
            ret = vp8l_still_peek(decoder);
            if (ret < 0)
                return set_error(decoder, "VP8L decode failed", ret);
        }
        ret = export_still_lossless(decoder,
                                    frame,
                                    decoder->vp8l_active
                                        ? decoder->vp8l_rows_out
                                        : decoder->lossless_frame->height);
        if (ret < 0)
            return set_error(decoder, "cannot output frame", ret);
        if (rows_valid)
            *rows_valid = decoder->converted_rows;
        return WPD_OK;
    }
    if (!decoder->still_lossy)
        return WPD_OK;

    rows = decoder->vp8_active ? vp8_rows_finalized(&decoder->codec)
                               : decoder->subframe.height;

    if (!format_is_packed(decoder->out_format)) {
        const WPDPixelFormat format = decoder->out_format == WPD_PIX_FMT_NONE
            ? decoder->subframe.format
            : decoder->out_format;
        const WPDPixelFormat have   = decoder->subframe.format;
        const WebPImage     *plane  = &decoder->subframe;
        const int            first  = decoder->converted_format == format
            ? decoder->converted_rows
            : 0;

        if (rows < first)
            rows = first;
        if (have != WPD_PIX_FMT_YUVA420P && format != have) {
            ret = ensure_yuva_rows(decoder,
                                   &decoder->output,
                                   &decoder->subframe,
                                   format == WPD_PIX_FMT_YUVA420P,
                                   first,
                                   rows);
            if (ret < 0)
                return set_error(decoder, "cannot output frame", ret);
            plane = &decoder->output;
        }
        export_frame(decoder, plane, format, frame);
        decoder->converted_rows   = rows;
        decoder->converted_format = format;
        if (rows_valid)
            *rows_valid = rows;
        return WPD_OK;
    }

    /* The fancy upsampler pairs a row with the one below it, so the last
       finished row cannot be converted until the row after it exists. */
    if (rows && rows < decoder->subframe.height)
        rows--;

    ret = export_still_packed(decoder, frame, rows);
    if (ret < 0)
        return set_error(decoder, "cannot output frame", ret);
    if (rows_valid)
        *rows_valid = decoder->converted_rows;
    return WPD_OK;
}

WPDStatus wpd_decoder_status(const WPDDecoder *decoder) {
    return decoder ? decoder->status : WPD_ERR_INVALID_ARG;
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
    image_free(&decoder->converted);
    image_free(&decoder->output);
    image_free(&decoder->alpha_argb);
    image_free(&decoder->lossless_out);
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&decoder->image[i]);
    free(decoder->lossless_top);
    free(decoder->alpha_plane);
    free(decoder->file_alloc);
    free(decoder);
}
