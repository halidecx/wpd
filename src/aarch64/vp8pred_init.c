#include "vp8pred.h"

void ff_pred16x16_vert_neon(uint8_t *, int);
void ff_pred16x16_hor_neon(uint8_t *, int);

void ff_pred4x4_tm_neon(uint8_t *, const uint8_t *, int);
void ff_pred8x8_tm_neon(uint8_t *, int);
void ff_pred16x16_tm_neon(uint8_t *, int);
void ff_pred8x8_dc_neon(uint8_t *, int);
void ff_pred16x16_dc_neon(uint8_t *, int);

void ff_pred4x4_dc_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_vert_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_hor_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_down_left_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_down_right_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_vert_left_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_vert_right_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_hor_up_neon(uint8_t *, const uint8_t *, int);
void ff_pred4x4_hor_down_neon(uint8_t *, const uint8_t *, int);
void ff_pred8x8_vert_neon(uint8_t *, int);

wpd_cold void ff_vp8_pred_init_aarch64(VP8PredContext *pred) {
    if (!wpd_have_neon(wpd_get_cpu_flags()))
        return;
    pred->pred4x4[TM_VP8_PRED]          = ff_pred4x4_tm_neon;
    pred->pred4x4[DC_PRED]              = ff_pred4x4_dc_neon;
    pred->pred4x4[VERT_PRED]            = ff_pred4x4_vert_neon;
    pred->pred4x4[HOR_PRED]             = ff_pred4x4_hor_neon;
    pred->pred4x4[DIAG_DOWN_LEFT_PRED]  = ff_pred4x4_down_left_neon;
    pred->pred4x4[DIAG_DOWN_RIGHT_PRED] = ff_pred4x4_down_right_neon;
    pred->pred4x4[VERT_LEFT_PRED]       = ff_pred4x4_vert_left_neon;
    pred->pred4x4[VERT_RIGHT_PRED]      = ff_pred4x4_vert_right_neon;
    pred->pred4x4[HOR_UP_PRED]          = ff_pred4x4_hor_up_neon;
    pred->pred4x4[HOR_DOWN_PRED]        = ff_pred4x4_hor_down_neon;

    pred->pred8x8[VERT_PRED8x8] = ff_pred8x8_vert_neon;

    pred->pred8x8[DC_PRED8x8]    = ff_pred8x8_dc_neon;
    pred->pred8x8[PLANE_PRED8x8] = ff_pred8x8_tm_neon;

    pred->pred16x16[DC_PRED8x8]    = ff_pred16x16_dc_neon;
    pred->pred16x16[VERT_PRED8x8]  = ff_pred16x16_vert_neon;
    pred->pred16x16[HOR_PRED8x8]   = ff_pred16x16_hor_neon;
    pred->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_neon;
}
