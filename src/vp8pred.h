#ifndef WPD_PRED_H
#define WPD_PRED_H

#include "wpd_codec.h"

enum VP8Pred4x4Mode {
    VERT_PRED,
    HOR_PRED,
    DC_PRED,
    DIAG_DOWN_LEFT_PRED,
    DIAG_DOWN_RIGHT_PRED,
    VERT_RIGHT_PRED,
    HOR_DOWN_PRED,
    VERT_LEFT_PRED,
    HOR_UP_PRED,
    TM_VP8_PRED,
    VP8_PRED4X4_COUNT,
};

enum VP8Pred8x8Mode {
    DC_PRED8x8,
    HOR_PRED8x8,
    VERT_PRED8x8,
    PLANE_PRED8x8,
    LEFT_DC_PRED8x8,
    TOP_DC_PRED8x8,
    DC_128_PRED8x8,
    VP8_PRED8X8_COUNT,
};

typedef struct VP8PredContext {
    void (*pred4x4[VP8_PRED4X4_COUNT])(uint8_t *src, const uint8_t *topright,
                                       int stride);
    void (*pred8x8[VP8_PRED8X8_COUNT])(uint8_t *src, int stride);
    void (*pred16x16[VP8_PRED8X8_COUNT])(uint8_t *src, int stride);
} VP8PredContext;

void ff_vp8_pred_init(VP8PredContext *pred);
void ff_vp8_pred_init_x86(VP8PredContext *pred);
void ff_vp8_pred_init_arm(VP8PredContext *pred);
void ff_vp8_pred_init_aarch64(VP8PredContext *pred);

#endif
