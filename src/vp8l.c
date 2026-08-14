
#include "vp8l.h"

#include "huffman.h"
#include "vp8l_dsp.h"

#define HUFFMAN_CODES_PER_META_CODE 5
#define NUM_LITERAL_CODES 256
#define NUM_LENGTH_CODES 24
#define NUM_DISTANCE_CODES 40
#define NUM_SHORT_DISTANCES 120
#define VP8L_ROW_BATCH 16

enum TransformType {
    PREDICTOR_TRANSFORM      = 0,
    COLOR_TRANSFORM          = 1,
    SUBTRACT_GREEN           = 2,
    COLOR_INDEXING_TRANSFORM = 3,
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
    HuffBlock     *huffman_arena;
    int            size_reduction;
} ImageContext;

struct VP8LContext {
    WPDLosslessDSP ldsp;
    LEBitReader    gb;

    int                width, height;
    int                has_alpha;
    int                reduced_width;
    int                nb_transforms;
    enum TransformType transforms[4];
    int                nb_huffman_groups;
    ImageContext       image[IMAGE_ROLE_NB];

    uint8_t *alpha_dst;
    int      alpha_dst_stride;
    int      alpha_dst_used;

    /* The two pictures a caller decodes into, plus the staging image the
       resumable path transforms finished rows into. */
    WebPImage  argb;
    WebPImage  alpha_argb;
    WebPImage  out;
    WebPImage *frame;
    uint8_t   *top;
    size_t     top_size;

    int    active;
    size_t next_try;
    size_t pos, cached;
    int    x, y, hg;
    int    rows_done, rows_out, peeked;
};

static const uint16_t alphabet_sizes[HUFFMAN_CODES_PER_META_CODE] = {
    NUM_LITERAL_CODES + NUM_LENGTH_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_DISTANCE_CODES};

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

static void image_ctx_free(ImageContext *img) {
    wpd_free(img->color_cache);
    if (img->role != IMAGE_ROLE_ARGB)
        image_free(&img->storage);
    huff_arena_free(&img->huffman_arena);
    wpd_free(img->huffman_groups);
    memset(img, 0, sizeof(*img));
}

/* A picture the caller may read but not free. */
static void image_view(WebPImage *out, const WebPImage *src) {
    if (!out)
        return;
    *out = *src;
    memset(out->alloc, 0, sizeof(out->alloc));
    memset(out->alloc_size, 0, sizeof(out->alloc_size));
}

static void update_canvas_size(VP8LContext *c, int w, int h) {
    if (c->width && c->width != w)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Width mismatch. %d != %d\n", c->width, w);
    c->width = w;
    if (c->height && c->height != h)
        wpd_log(
            NULL, WPD_LOG_WARNING, "Height mismatch. %d != %d\n", c->height, h);
    c->height = h;
}

VP8LContext *vp8l_alloc(void) {
    VP8LContext *c = wpd_mallocz(sizeof(*c));

    if (c)
        wpd_vp8l_dsp_init(&c->ldsp);
    return c;
}

void vp8l_reset(VP8LContext *c) {
    for (int i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&c->image[i]);
    c->active   = 0;
    c->next_try = 0;
    c->peeked   = 0;
    c->frame    = NULL;
    c->width = c->height = 0;
    c->has_alpha         = 0;
}

void vp8l_release(VP8LContext *c) {
    vp8l_reset(c);
    image_free(&c->argb);
    image_free(&c->alpha_argb);
    image_free(&c->out);
}

void vp8l_free(VP8LContext **ctx) {
    VP8LContext *c = *ctx;

    if (!c)
        return;
    vp8l_release(c);
    free(c->top);
    wpd_free(c);
    *ctx = NULL;
}

void vp8l_set_canvas(VP8LContext *c, int width, int height) {
    c->width  = width;
    c->height = height;
}

int vp8l_width(const VP8LContext *c) { return c->width; }
int vp8l_height(const VP8LContext *c) { return c->height; }
int vp8l_has_alpha(const VP8LContext *c) { return c->has_alpha; }
int vp8l_still_active(const VP8LContext *c) { return c->active; }
int vp8l_still_rows_out(const VP8LContext *c) { return c->rows_out; }
int vp8l_alpha_dst_used(const VP8LContext *c) { return c->alpha_dst_used; }

void vp8l_set_alpha_dst(VP8LContext *c, uint8_t *dst, int stride) {
    c->alpha_dst        = dst;
    c->alpha_dst_stride = stride;
    c->alpha_dst_used   = 0;
}

void vp8l_still_frame(const VP8LContext *c, WebPImage *out) {
    if (c->frame)
        image_view(out, c->frame);
}

static int decode_entropy_coded_image(VP8LContext *c, enum ImageRole role,
                                      int w, int h);

#define PARSE_BLOCK_SIZE(w, h)                                    \
    do {                                                          \
        block_bits = br_bits(&c->gb, 3) + 2;                      \
        blocks_w   = ((w) + (1 << block_bits) - 1) >> block_bits; \
        blocks_h   = ((h) + (1 << block_bits) - 1) >> block_bits; \
    } while (0)

static int decode_entropy_image(VP8LContext *c) {
    ImageContext *img;
    int           ret, block_bits, blocks_w, blocks_h, x, y, max;

    PARSE_BLOCK_SIZE(c->reduced_width, c->height);

    ret = decode_entropy_coded_image(c, IMAGE_ROLE_ENTROPY, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    img                 = &c->image[IMAGE_ROLE_ENTROPY];
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
    c->nb_huffman_groups = max + 1;

    return 0;
}

static int parse_transform_predictor(VP8LContext *c) {
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(c->reduced_width, c->height);

    ret = decode_entropy_coded_image(
        c, IMAGE_ROLE_PREDICTOR, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    c->image[IMAGE_ROLE_PREDICTOR].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color(VP8LContext *c) {
    int block_bits, blocks_w, blocks_h, ret;

    PARSE_BLOCK_SIZE(c->reduced_width, c->height);

    ret = decode_entropy_coded_image(
        c, IMAGE_ROLE_COLOR_TRANSFORM, blocks_w, blocks_h);
    if (ret < 0)
        return ret;

    c->image[IMAGE_ROLE_COLOR_TRANSFORM].size_reduction = block_bits;

    return 0;
}

static int parse_transform_color_indexing(VP8LContext *c) {
    ImageContext *img;
    int           width_bits, index_size, ret, x;
    uint8_t      *ct;

    index_size = br_bits(&c->gb, 8) + 1;

    if (index_size <= 2)
        width_bits = 3;
    else if (index_size <= 4)
        width_bits = 2;
    else if (index_size <= 16)
        width_bits = 1;
    else
        width_bits = 0;

    ret = decode_entropy_coded_image(
        c, IMAGE_ROLE_COLOR_INDEXING, index_size, 1);
    if (ret < 0)
        return ret;

    img                 = &c->image[IMAGE_ROLE_COLOR_INDEXING];
    img->size_reduction = width_bits;
    if (width_bits > 0)
        c->reduced_width = (c->width + ((1 << width_bits) - 1)) >> width_bits;

    ct = img->frame->data[0] + 4;
    for (x = 4; x < img->frame->width * 4; x++, ct++) ct[0] += ct[-4];

    return 0;
}

static HTreeGroup *get_huffman_group(VP8LContext *c, ImageContext *img, int x,
                                     int y) {
    ImageContext *gimg  = &c->image[IMAGE_ROLE_ENTROPY];
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

static wpd_always_inline int read_entropy_image_header(VP8LContext   *c,
                                                       enum ImageRole role,
                                                       int w, int h) {
    ImageContext *img;
    HTreeGroup   *hg;
    uint8_t      *code_lengths;
    uint16_t     *sorted;
    int           i, j, ret, max_alphabet_size;

    img       = &c->image[role];
    img->role = role;

    if (!img->frame)
        img->frame = &img->storage;

    ret = image_alloc_argb(img->frame, w, h);
    if (ret < 0)
        return ret;

    if (br_bit(&c->gb)) {
        img->color_cache_bits = br_bits(&c->gb, 4);
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
    if (role == IMAGE_ROLE_ARGB && br_bit(&c->gb)) {
        ret = decode_entropy_image(c);
        if (ret < 0)
            return ret;
        img->nb_huffman_groups = c->nb_huffman_groups;
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
            HuffPlan  plan;
            const int alphabet_size = alphabet_sizes[j] +
                (!j && img->color_cache_bits > 0 ? 1 << img->color_cache_bits
                                                 : 0);

            ret = 0;
            memset(code_lengths, 0, alphabet_size);
            memset(plan.count, 0, sizeof(plan.count));
            if (br_bit(&c->gb))
                read_huffman_code_simple(
                    &c->gb, &plan, code_lengths, alphabet_size);
            else
                ret = read_huffman_code_normal(
                    &c->gb, &plan, code_lengths, alphabet_size);
            if (ret >= 0)
                ret = huff_reader_build(&hg->trees[j],
                                        &img->huffman_arena,
                                        &plan,
                                        code_lengths,
                                        alphabet_size,
                                        sorted);
            if (ret < 0) {
                free(sorted);
                return ret;
            }
        }

        hg->trivial_literal = !hg->trees[HUFF_IDX_RED].mask &&
            !hg->trees[HUFF_IDX_BLUE].mask && !hg->trees[HUFF_IDX_ALPHA].mask;
        if (hg->trivial_literal) {
            hg->literal[0] = hg->trees[HUFF_IDX_ALPHA].table[0] >> 8;
            hg->literal[1] = hg->trees[HUFF_IDX_RED].table[0] >> 8;
            hg->literal[3] = hg->trees[HUFF_IDX_BLUE].table[0] >> 8;
        }
    }
    free(sorted);

    return 0;
}

static wpd_always_inline int decode_entropy_pixels(VP8LContext   *c,
                                                   enum ImageRole role,
                                                   const int      resumable) {
    ImageContext *img = &c->image[role];
    HTreeGroup   *hg;
    int           x, y, width;

    width = img->frame->width;
    if (role == IMAGE_ROLE_ARGB && c->reduced_width < width) {
        /* Decode packed palette rows contiguously; expansion re-strides. */
        width                   = c->reduced_width;
        img->frame->linesize[0] = width * 4;
    }

    {
        const int      multi_group = img->nb_huffman_groups > 1;
        const int      huff_bits   = multi_group
            ? c->image[IMAGE_ROLE_ENTROPY].size_reduction
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
            pos    = c->pos;
            x      = c->x;
            y      = c->y;
            cached = base + c->cached;
            hg     = &img->huffman_groups[c->hg];
        }
        while (pos < total) {
            uint8_t *p = base + 4 * pos;
            int      v;

            if (!resumable && br_is_eos(&c->gb))
                return WPD_ERROR_INVALID_DATA;
            /* One pixel reads at most 108 bits, which draws the reader no
               more than 20 bytes further in, so the margin leaves the loop
               nothing to save or check until the end really is in sight. */
            if (resumable) {
                near = c->gb.len - c->gb.pos <= VP8L_TAIL_MARGIN;
                if (near)
                    snap = c->gb;
            }

            if ((x & huff_mask) == 0)
                hg = get_huffman_group(c, img, x, y);
            br_fill(&c->gb);
            v = huff_read_symbol(&hg->trees[HUFF_IDX_GREEN], &c->gb);
            if (v < NUM_LITERAL_CODES) {
                if (hg->trivial_literal) {
                    if (resumable && near && br_is_eos(&c->gb))
                        goto suspend;
                    copy32(p, hg->literal);
                    p[2] = v;
                } else {
                    int r = huff_read_symbol(&hg->trees[HUFF_IDX_RED], &c->gb);
                    int b, a;
                    br_fill(&c->gb);
                    b = huff_read_symbol(&hg->trees[HUFF_IDX_BLUE], &c->gb);
                    a = huff_read_symbol(&hg->trees[HUFF_IDX_ALPHA], &c->gb);
                    if (resumable && near && br_is_eos(&c->gb))
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
                    length         = offset + br_bits(&c->gb, extra_bits) + 1;
                }
                prefix_code = huff_read_symbol(&hg->trees[HUFF_IDX_DIST],
                                               &c->gb);
                br_fill(&c->gb);
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
                    distance       = offset + br_bits(&c->gb, extra_bits) + 1;
                }

                if (resumable && near && br_is_eos(&c->gb))
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
                    hg = get_huffman_group(c, img, x, y);
                if (img->color_cache_bits)
                    cached = color_cache_fill(img, cached, base + 4 * pos);
            } else {
                int cache_idx = v - (NUM_LITERAL_CODES + NUM_LENGTH_CODES);

                if (resumable && near && br_is_eos(&c->gb))
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
            c->rows_done = y;
        return 0;

    suspend:
        c->gb        = snap;
        c->pos       = pos;
        c->x         = x;
        c->y         = y;
        c->cached    = (size_t)(cached - base);
        c->hg        = (int)(hg - img->huffman_groups);
        c->rows_done = y;
        return VP8L_NEED_MORE;
    }
}

static wpd_noclone int resume_argb_pixels(VP8LContext *c) {
    return decode_entropy_pixels(c, IMAGE_ROLE_ARGB, 1);
}

static int decode_entropy_coded_image(VP8LContext *c, enum ImageRole role,
                                      int w, int h) {
    int ret = read_entropy_image_header(c, role, w, h);

    if (ret < 0)
        return ret;
    return decode_entropy_pixels(c, role, 0);
}

static wpd_always_inline int predictor_transform_rows(VP8LContext *c,
                                                      uint32_t    *rows,
                                                      int          stride,
                                                      uint32_t *upper0, int y0,
                                                      int y1) {
    ImageContext              *pimg      = &c->image[IMAGE_ROLE_PREDICTOR];
    pred_add_func const *const pred_add  = c->ldsp.pred_add;
    const int                  width     = c->reduced_width;
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

static int apply_predictor_transform(VP8LContext *c) {
    ImageContext *img = &c->image[IMAGE_ROLE_ARGB];

    return predictor_transform_rows(c,
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

static wpd_always_inline int color_transform_rows(VP8LContext *c, uint8_t *rows,
                                                  int stride, int y0, int y1) {
    ImageContext *cimg      = &c->image[IMAGE_ROLE_COLOR_TRANSFORM];
    const int     width     = c->reduced_width;
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

static int apply_color_transform(VP8LContext *c) {
    ImageContext *img = &c->image[IMAGE_ROLE_ARGB];

    return color_transform_rows(
        c, img->frame->data[0], img->frame->linesize[0], 0, img->frame->height);
}

static wpd_always_inline int subtract_green_rows(VP8LContext *c, uint8_t *rows,
                                                 int stride, int y0, int y1) {
    const int width = c->reduced_width;
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

static int apply_subtract_green_transform(VP8LContext *c) {
    ImageContext *img = &c->image[IMAGE_ROLE_ARGB];

    return subtract_green_rows(
        c, img->frame->data[0], img->frame->linesize[0], 0, img->frame->height);
}

static int apply_color_indexing_transform_alpha(VP8LContext *c) {
    ImageContext *img    = &c->image[IMAGE_ROLE_ARGB];
    ImageContext *pal    = &c->image[IMAGE_ROLE_COLOR_INDEXING];
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
        uint8_t       *dst = c->alpha_dst + (size_t)c->alpha_dst_stride * y;

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
    c->alpha_dst_used       = 1;
    c->reduced_width        = c->width;
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

static wpd_always_inline int color_indexing_rows(VP8LContext *c, uint8_t *base,
                                                 int dst_stride, int src_stride,
                                                 int height, int big) {
    ImageContext *img;
    ImageContext *pal;
    int           i, x, y;
    uint8_t      *p;

    img = &c->image[IMAGE_ROLE_ARGB];
    pal = &c->image[IMAGE_ROLE_COLOR_INDEXING];

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

            c->ldsp.map_color32(row, row, palette, w);
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

static int apply_color_indexing_transform(VP8LContext *c) {
    ImageContext *img    = &c->image[IMAGE_ROLE_ARGB];
    ImageContext *pal    = &c->image[IMAGE_ROLE_COLOR_INDEXING];
    const int     width  = img->frame->width;
    const int     height = img->frame->height;
    int           ret;

    ret = color_indexing_rows(c,
                              GET_PIXEL(img->frame, 0, 0),
                              width * 4,
                              img->frame->linesize[0],
                              height,
                              height * width > 300);
    if (ret < 0)
        return ret;
    if (pal->size_reduction > 0) {
        img->frame->linesize[0] = width * 4;
        c->reduced_width        = c->width;
    }
    return 0;
}

static wpd_always_inline int vp8l_read_frame_header(
    VP8LContext *c, WebPImage *out, const uint8_t *data_start,
    unsigned int data_size, int is_alpha_chunk, int *w_out, int *h_out) {
    int      w, h, ret;
    unsigned used;

    br_init(&c->gb, data_start, data_size);

    if (!is_alpha_chunk) {
        if (br_bits(&c->gb, 8) != 0x2F) {
            wpd_log(NULL, WPD_LOG_ERROR, "Invalid WebP Lossless signature\n");
            return WPD_ERROR_INVALID_DATA;
        }

        w = br_bits(&c->gb, 14) + 1;
        h = br_bits(&c->gb, 14) + 1;

        update_canvas_size(c, w, h);

        ret = wpd_check_image_size(c->width, c->height);
        if (ret < 0)
            return ret;

        c->has_alpha = br_bit(&c->gb);

        if (br_bits(&c->gb, 3) != 0x0) {
            wpd_log(NULL, WPD_LOG_ERROR, "Invalid WebP Lossless version\n");
            return WPD_ERROR_INVALID_DATA;
        }
    } else {
        if (!c->width || !c->height)
            return WPD_ERROR_INVALID_DATA;
        w = c->width;
        h = c->height;
    }

    c->nb_transforms = 0;
    c->reduced_width = c->width;
    used             = 0;
    while (br_bit(&c->gb)) {
        enum TransformType transform = br_bits(&c->gb, 2);
        if (used & (1 << transform)) {
            wpd_log(NULL,
                    WPD_LOG_ERROR,
                    "Transform %d used more than once\n",
                    transform);
            return WPD_ERROR_INVALID_DATA;
        }
        used |= (1 << transform);
        c->transforms[c->nb_transforms++] = transform;
        ret                               = 0;
        switch (transform) {
        case PREDICTOR_TRANSFORM: ret = parse_transform_predictor(c); break;
        case COLOR_TRANSFORM: ret = parse_transform_color(c); break;
        case COLOR_INDEXING_TRANSFORM:
            ret = parse_transform_color_indexing(c);
            break;
        case SUBTRACT_GREEN: break;
        }
        if (ret < 0)
            return ret;
    }

    c->image[IMAGE_ROLE_ARGB].frame = out;
    *w_out                          = w;
    *h_out                          = h;
    return read_entropy_image_header(c, IMAGE_ROLE_ARGB, w, h);
}

static wpd_noclone int apply_transforms(VP8LContext *c) {
    int i, ret;

    for (i = c->nb_transforms - 1; i >= 0; i--) {
        ret = 0;
        switch (c->transforms[i]) {
        case PREDICTOR_TRANSFORM: ret = apply_predictor_transform(c); break;
        case COLOR_TRANSFORM: ret = apply_color_transform(c); break;
        case SUBTRACT_GREEN: ret = apply_subtract_green_transform(c); break;
        case COLOR_INDEXING_TRANSFORM:
            if (c->alpha_dst && c->nb_transforms == 1)
                ret = apply_color_indexing_transform_alpha(c);
            else
                ret = apply_color_indexing_transform(c);
            break;
        }
        if (ret < 0)
            return ret;
    }
    return 0;
}

int vp8l_decode_frame(VP8LContext *c, enum VP8LTarget target, WebPImage *out,
                      const uint8_t *data_start, unsigned int data_size,
                      int is_alpha_chunk) {
    WebPImage *dst = target == VP8L_TARGET_ALPHA ? &c->alpha_argb : &c->argb;
    int        w, h, ret, i;

    ret = vp8l_read_frame_header(
        c, dst, data_start, data_size, is_alpha_chunk, &w, &h);
    if (ret >= 0)
        ret = decode_entropy_pixels(c, IMAGE_ROLE_ARGB, 0);
    if (ret >= 0)
        ret = apply_transforms(c);

    dst->linesize[0] = dst->width * 4;
    for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&c->image[i]);
    image_view(out, dst);

    return ret;
}

static int transform_rows(VP8LContext *c, int y0, int y1) {
    ImageContext *img        = &c->image[IMAGE_ROLE_ARGB];
    WebPImage    *dst        = &c->out;
    const int     stride     = dst->linesize[0];
    const int     src_stride = img->frame->linesize[0];
    const int     packed     = c->reduced_width;
    const size_t  packed_row = (size_t)packed * 4;
    uint8_t      *rows       = dst->data[0] + (size_t)y0 * stride;
    int           i, ret = 0;

    for (i = 0; i < y1 - y0; i++)
        memcpy(rows + (size_t)i * stride,
               img->frame->data[0] + (size_t)(y0 + i) * src_stride,
               packed_row);

    for (i = c->nb_transforms - 1; i >= 0 && ret >= 0; i--) {
        switch (c->transforms[i]) {
        case PREDICTOR_TRANSFORM:
            ret = predictor_transform_rows(c,
                                           (uint32_t *)rows,
                                           stride / 4,
                                           y0 ? (uint32_t *)c->top : NULL,
                                           y0,
                                           y1);
            if (ret >= 0)
                memcpy(c->top,
                       rows + (size_t)(y1 - 1 - y0) * stride,
                       (size_t)c->reduced_width * 4);
            break;
        case COLOR_TRANSFORM:
            ret = color_transform_rows(c, rows, stride, y0, y1);
            break;
        case SUBTRACT_GREEN:
            ret = subtract_green_rows(c, rows, stride, y0, y1);
            break;
        case COLOR_INDEXING_TRANSFORM:
            ret = color_indexing_rows(c,
                                      rows,
                                      stride,
                                      stride,
                                      y1 - y0,
                                      dst->height * dst->width > 300);
            c->reduced_width = c->width;
            break;
        }
    }
    c->reduced_width = packed;
    return ret;
}

static int still_alloc(VP8LContext *c) {
    const size_t top = ((size_t)c->width + 1) * 4;
    int          ret;

    ret = image_alloc_argb(&c->out, c->width, c->height);
    if (ret < 0)
        return ret;
    if (c->top_size < top) {
        uint8_t *buf = realloc(c->top, top);

        if (!buf)
            return WPD_ERROR(ENOMEM);
        c->top      = buf;
        c->top_size = top;
    }
    return 0;
}

/* Decodes as much of a still lossless image as the buffered bytes allow.
   Returns 1 once the whole image is out, 0 while more input is needed. */
int vp8l_still_step(VP8LContext *c, const uint8_t *payload, unsigned avail,
                    unsigned size, int complete) {
    int rows, ret, i, w, h;

    if (!c->active) {
        const size_t first = WPD_MAX((size_t)16, (size_t)size / 16);

        if (avail < first || (!complete && avail < c->next_try))
            return 0;
        for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&c->image[i]);
        c->width = c->height = 0;
        ret = vp8l_read_frame_header(c, &c->argb, payload, avail, 0, &w, &h);
        if (ret >= 0 && br_is_eos(&c->gb))
            ret = WPD_ERROR_INVALID_DATA;
        if (ret < 0) {
            for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&c->image[i]);
            if (complete)
                return ret;
            c->next_try = 2 * (size_t)avail;
            return 0;
        }
        c->pos    = 0;
        c->cached = 0;
        c->x = c->y = c->hg = 0;
        c->rows_done = c->rows_out = 0;
        c->peeked                  = 0;
        c->active                  = 1;
        c->frame                   = &c->argb;
    } else {
        br_extend(&c->gb, payload, avail);
    }

    ret = resume_argb_pixels(c);
    if (ret < 0)
        return ret;
    if (ret == VP8L_NEED_MORE && complete)
        return WPD_ERROR_INVALID_DATA;

    rows = c->rows_done;
    if (ret)
        rows -= rows % VP8L_ROW_BATCH;
    if (c->peeked && rows > c->rows_out) {
        int done = transform_rows(c, c->rows_out, rows);

        if (done < 0)
            return done;
        c->rows_out = rows;
    }
    if (ret)
        return 0;

    /* Nobody looked, so the image can be transformed where it lies. */
    if (!c->peeked)
        ret = apply_transforms(c);
    c->argb.linesize[0] = c->argb.width * 4;
    for (i = 0; i < IMAGE_ROLE_NB; i++) image_ctx_free(&c->image[i]);
    c->active = 0;
    return ret < 0 ? ret : 1;
}

/* Switches the in-progress image over to handing rows out as they finish,
   which needs somewhere to put them: backward references keep reading the
   untransformed pixels for as long as the image is being decoded. */
int vp8l_still_peek(VP8LContext *c) {
    int ret, rows;

    if (!c->peeked) {
        ret = still_alloc(c);
        if (ret < 0)
            return ret;
        c->peeked = 1;
        c->frame  = &c->out;
    }
    rows = c->rows_done - c->rows_done % VP8L_ROW_BATCH;
    if (rows > c->rows_out) {
        ret = transform_rows(c, c->rows_out, rows);
        if (ret < 0)
            return ret;
        c->rows_out = rows;
    }
    return 0;
}
