#ifndef WPD_VP8_H
#define WPD_VP8_H

#include "vp56rac.h"
#include "vp8dsp.h"
#include "vp8pred.h"

#define VP8_MAX_QUANT 127
#define MODE_I4 4

enum dct_token {
    DCT_0,
    DCT_1,
    DCT_2,
    DCT_3,
    DCT_4,
    DCT_CAT1,
    DCT_CAT2,
    DCT_CAT3,
    DCT_CAT4,
    DCT_CAT5,
    DCT_CAT6,
    DCT_EOB,

    NUM_DCT_TOKENS
};

typedef struct VP8FilterStrength {
    uint8_t filter_level;
    uint8_t inner_limit;
    uint8_t inner_filter;
} VP8FilterStrength;

typedef struct VP8Macroblock {
    uint8_t skip, mode;
} VP8Macroblock;

typedef struct VP8Context {
    WpdCodecContext *avctx;
    WpdFrame         frame;

    uint16_t mb_width;
    uint16_t mb_height;
    int      linesize;
    int      uvlinesize;

    uint8_t deblock_filter;
    uint8_t mbskip_enabled;
    uint8_t segment;
    uint8_t chroma_pred_mode;
    uint8_t profile;

    struct {
        uint8_t enabled;
        uint8_t absolute_vals;
        uint8_t update_map;
        int8_t  base_quant[4];
        int8_t  filter_level[4];
    } segmentation;

    struct {
        uint8_t simple;
        uint8_t level;
        uint8_t sharpness;
    } filter;

    VP8FilterStrength *filter_strength;

    uint8_t *intra4x4_pred_mode_top;
    uint8_t  intra4x4_pred_mode_left[4];

    struct {
        int16_t luma_qmul[2];
        int16_t luma_dc_qmul[2];
        int16_t chroma_qmul[2];
    } qmat[4];

    struct {
        uint8_t enabled;
        int8_t  ref_intra;
        int8_t  mode_i4;
    } lf_delta;

    uint8_t (*top_border)[16 + 8 + 8];

    uint8_t (*top_nnz)[9];
    WPD_DECLARE_ALIGNED(8, uint8_t, left_nnz)[9];

    WPD_DECLARE_ALIGNED(16, uint8_t, non_zero_count_cache)[6][4];
    VP56RangeCoder c;
    WPD_DECLARE_ALIGNED(16, WpdDctElem, block)[6][4][16];
    WPD_DECLARE_ALIGNED(16, WpdDctElem, block_dc)[16];
    uint8_t intra4x4_pred_mode_mb[16];

    struct {
        uint8_t segmentid[3];
        uint8_t mbskip;
        uint8_t token[4][16][3][NUM_DCT_TOKENS - 1];
    } prob;

    int            num_coeff_partitions;
    VP56RangeCoder coeff_partition[8];
    VP8DSPContext  vp8dsp;
    VP8PredContext pred;
} VP8Context;

#ifdef WPD_CHECKASM
int wpd_decode_block_coeffs_c(VP56RangeCoder *c, WpdDctElem block[16],
                              uint8_t probs[16][3][NUM_DCT_TOKENS - 1], int i,
                              uint8_t *token_prob, int16_t qmul[2]);
#endif

int vp8_decode_init(WpdCodecContext *context);
int vp8_decode_frame(WpdCodecContext *context, void *frame, WpdPacket *packet);
int vp8_decode_free(WpdCodecContext *context);

#endif
