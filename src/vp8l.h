#ifndef WPD_VP8L_H
#define WPD_VP8L_H

#include "huffman.h"
#include "image.h"

#define HUFFMAN_CODES_PER_META_CODE 5
#define NUM_LITERAL_CODES 256
#define NUM_LENGTH_CODES 24
#define NUM_DISTANCE_CODES 40
#define NUM_SHORT_DISTANCES 120

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
    int            is_alpha_primary;
} ImageContext;

extern const uint16_t alphabet_sizes[HUFFMAN_CODES_PER_META_CODE];
extern const int8_t   lz77_distance_offsets[NUM_SHORT_DISTANCES][2];

void image_ctx_free(ImageContext *img);

#endif
