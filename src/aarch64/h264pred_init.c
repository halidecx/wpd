#include "compat.h"
#include "h264pred.h"

void ff_pred16x16_vert_neon(uint8_t *, int);
void ff_pred16x16_hor_neon(uint8_t *, int);

av_cold void ff_h264_pred_init_aarch64(H264PredContext *h, int codec_id,
                                        const int bit_depth)
{
    if (codec_id != CODEC_ID_VP8 || bit_depth != 8)
        return;
    h->pred16x16[VERT_PRED8x8] = ff_pred16x16_vert_neon;
    h->pred16x16[HOR_PRED8x8]  = ff_pred16x16_hor_neon;
}
