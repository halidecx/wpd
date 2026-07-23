#include "compat.h"
#include "vp8pred.h"

#define PRED8(name)  void ff_pred8x8_##name##_neon(uint8_t *, int)
#define PRED16(name) void ff_pred16x16_##name##_neon(uint8_t *, int)
PRED8(vert);
PRED8(hor);
PRED8(128_dc);
PRED16(dc);
PRED16(vert);
PRED16(hor);
PRED16(left_dc);
PRED16(top_dc);
PRED16(128_dc);

av_cold void ff_vp8_pred_init_arm(VP8PredContext *pred)
{
    if (!have_neon(av_get_cpu_flags()))
        return;
    pred->pred8x8[VERT_PRED8x8]      = ff_pred8x8_vert_neon;
    pred->pred8x8[HOR_PRED8x8]       = ff_pred8x8_hor_neon;
    pred->pred8x8[DC_128_PRED8x8]    = ff_pred8x8_128_dc_neon;
    pred->pred16x16[DC_PRED8x8]      = ff_pred16x16_dc_neon;
    pred->pred16x16[VERT_PRED8x8]    = ff_pred16x16_vert_neon;
    pred->pred16x16[HOR_PRED8x8]     = ff_pred16x16_hor_neon;
    pred->pred16x16[LEFT_DC_PRED8x8] = ff_pred16x16_left_dc_neon;
    pred->pred16x16[TOP_DC_PRED8x8]  = ff_pred16x16_top_dc_neon;
    pred->pred16x16[DC_128_PRED8x8]  = ff_pred16x16_128_dc_neon;
}
