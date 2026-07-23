#include "compat.h"
#include "vp8pred.h"

void ff_pred16x16_vert_neon(uint8_t *, int);
void ff_pred16x16_hor_neon(uint8_t *, int);

av_cold void ff_vp8_pred_init_aarch64(VP8PredContext *pred)
{
    pred->pred16x16[VERT_PRED8x8] = ff_pred16x16_vert_neon;
    pred->pred16x16[HOR_PRED8x8]  = ff_pred16x16_hor_neon;
}
