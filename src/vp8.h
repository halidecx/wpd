#ifndef WPD_VP8_H
#define WPD_VP8_H

#include "vp56rac.h"
#include "vp8dsp.h"
#include "vp8pred.h"

#define VP8_MAX_QUANT 127

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

// used to signal 4x4 intra pred in luma MBs
#define MODE_I4x4 4

typedef struct {
    uint8_t filter_level;
    uint8_t inner_limit;
    uint8_t inner_filter;
} VP8FilterStrength;

typedef struct {
    uint8_t skip;
    uint8_t mode;
} VP8Macroblock;

typedef struct {
    WpdCodecContext *avctx;
    WpdFrame         frame;

    uint16_t mb_width; /* number of horizontal MB */
    uint16_t mb_height; /* number of vertical MB */
    int      linesize;
    int      uvlinesize;

    uint8_t deblock_filter;
    uint8_t mbskip_enabled;
    uint8_t segment; ///< segment of the current macroblock
    uint8_t chroma_pred_mode; ///< 8x8c pred mode of the current macroblock
    uint8_t profile;

    /**
     * Base parameters for segmentation, i.e. per-macroblock parameters.
     */
    struct {
        uint8_t enabled;
        uint8_t absolute_vals;
        uint8_t update_map;
        int8_t  base_quant[4];
        int8_t  filter_level[4]; ///< base loop filter level
    } segmentation;

    struct {
        uint8_t simple;
        uint8_t level;
        uint8_t sharpness;
    } filter;

    VP8FilterStrength *filter_strength;

    uint8_t *intra4x4_pred_mode_top;
    uint8_t  intra4x4_pred_mode_left[4];

    /**
     * Macroblocks can have one of 4 different quants in a frame when
     * segmentation is enabled.
     * If segmentation is disabled, only the first segment's values are used.
     */
    struct {
        // [0] - DC qmul  [1] - AC qmul
        int16_t luma_qmul[2];
        int16_t luma_dc_qmul[2]; ///< luma dc-only block quant
        int16_t chroma_qmul[2];
    } qmat[4];

    /**
     * Every macroblock of a keyframe references the current frame and is
     * either i16x16 or i4x4, so only two of the coded loop filter deltas can
     * ever apply. The rest are parsed and dropped.
     */
    struct {
        uint8_t enabled;
        int8_t  ref_intra; ///< adjustment for intra-referencing macroblocks
        int8_t  mode_i4x4; ///< adjustment for i4x4 macroblocks
    } lf_delta;

    /**
     * Cache of the top row needed for intra prediction
     * 16 for luma, 8 for each chroma plane
     */
    uint8_t (*top_border)[16 + 8 + 8];

    /**
     * For coeff decode, we need to know whether the above block had non-zero
     * coefficients. This means for each macroblock, we need data for 4 luma
     * blocks, 2 u blocks, 2 v blocks, and the luma dc block, for a total of 9
     * per macroblock. We keep the last row in top_nnz.
     */
    uint8_t (*top_nnz)[9];
    WPD_DECLARE_ALIGNED(8, uint8_t, left_nnz)[9];

    /**
     * This is the index plus one of the last non-zero coeff
     * for each of the blocks in the current macroblock.
     * So, 0 -> no coeffs
     *     1 -> dc-only (special transform)
     *     2+-> full transform
     */
    WPD_DECLARE_ALIGNED(16, uint8_t, non_zero_count_cache)[6][4];
    VP56RangeCoder c; ///< header context, includes mb modes
    WPD_DECLARE_ALIGNED(16, WpdDctElem, block)[6][4][16];
    WPD_DECLARE_ALIGNED(16, WpdDctElem, block_dc)[16];
    uint8_t intra4x4_pred_mode_mb[16];

    /**
     * Updatable probabilities for binary decisions. A keyframe resets these
     * to their defaults before applying the updates coded in its header.
     */
    struct {
        uint8_t segmentid[3];
        uint8_t mbskip;
        uint8_t token[4][16][3][NUM_DCT_TOKENS - 1];
    } prob;

    /**
     * All coefficients are contained in separate arith coding contexts.
     * There can be 1, 2, 4, or 8 of these after the header context.
     */
    int            num_coeff_partitions;
    VP56RangeCoder coeff_partition[8];
    VP8DSPContext  vp8dsp;
    VP8PredContext pred;
} VP8Context;

#ifdef WPD_CHECKASM
/**
 * The C coefficient decoder, exposed so checkasm can use it as the reference
 * for arch-specific replacements such as ff_decode_block_coeffs_armv6. Only
 * present in the checkasm build; see src/vp8.c for why.
 */
int wpd_decode_block_coeffs_c(VP56RangeCoder *c, WpdDctElem block[16],
                              uint8_t probs[16][3][NUM_DCT_TOKENS - 1], int i,
                              uint8_t *token_prob, int16_t qmul[2]);
#endif

int vp8_decode_init(WpdCodecContext *context);
int vp8_decode_frame(WpdCodecContext *context, void *frame, WpdPacket *packet);
int vp8_decode_free(WpdCodecContext *context);

#endif /* WPD_VP8_H */
