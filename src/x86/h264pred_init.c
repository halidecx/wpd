#include "compat.h"
#include "h264pred.h"

#define PRED4(name, opt) \
    void ff_pred4x4_##name##_8_##opt(uint8_t *, const uint8_t *, int)
#define PRED8(name, opt) \
    void ff_pred8x8_##name##_8_##opt(uint8_t *, int)
#define PRED16(name, opt) \
    void ff_pred16x16_##name##_8_##opt(uint8_t *, int)

PRED4(dc, mmxext);
PRED4(down_left, mmxext);
PRED4(down_right, mmxext);
PRED4(horizontal_down, mmxext);
PRED4(horizontal_up, mmxext);
PRED4(tm_vp8, mmxext);
PRED4(tm_vp8, ssse3);
PRED4(vertical_right, mmxext);
PRED4(vertical_vp8, mmxext);
PRED8(dc_rv40, mmxext);
PRED8(horizontal, sse2);
PRED8(horizontal, ssse3);
PRED8(horizontal, avx2);
PRED8(tm_vp8, sse2);
PRED8(tm_vp8, ssse3);
PRED8(vertical, sse2);
PRED16(vertical, sse);
PRED16(horizontal, sse2);
PRED16(horizontal, ssse3);
PRED16(horizontal, avx2);
PRED16(dc, sse2);
PRED16(dc, ssse3);
PRED16(tm_vp8, sse2);
PRED16(tm_vp8, avx2);

av_cold void ff_h264_pred_init_x86(H264PredContext *h, int codec_id,
                                    const int bit_depth)
{
    int flags;
    if (codec_id != CODEC_ID_VP8 || bit_depth != 8)
        return;
    flags = av_get_cpu_flags();

    if (EXTERNAL_MMXEXT(flags)) {
        h->pred4x4[DIAG_DOWN_LEFT_PRED]  = ff_pred4x4_down_left_8_mmxext;
        h->pred4x4[DIAG_DOWN_RIGHT_PRED] = ff_pred4x4_down_right_8_mmxext;
        h->pred4x4[VERT_RIGHT_PRED]      = ff_pred4x4_vertical_right_8_mmxext;
        h->pred4x4[HOR_DOWN_PRED]        = ff_pred4x4_horizontal_down_8_mmxext;
        h->pred4x4[HOR_UP_PRED]          = ff_pred4x4_horizontal_up_8_mmxext;
        h->pred4x4[DC_PRED]              = ff_pred4x4_dc_8_mmxext;
        h->pred4x4[TM_VP8_PRED]          = ff_pred4x4_tm_vp8_8_mmxext;
        h->pred4x4[VERT_PRED]            = ff_pred4x4_vertical_vp8_8_mmxext;
        h->pred8x8[DC_PRED8x8]           = ff_pred8x8_dc_rv40_8_mmxext;
    }
    if (EXTERNAL_SSE(flags))
        h->pred16x16[VERT_PRED8x8] = ff_pred16x16_vertical_8_sse;
    if (EXTERNAL_SSE2(flags)) {
        h->pred16x16[HOR_PRED8x8]   = ff_pred16x16_horizontal_8_sse2;
        h->pred16x16[DC_PRED8x8]    = ff_pred16x16_dc_8_sse2;
        h->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_vp8_8_sse2;
        h->pred8x8[HOR_PRED8x8]     = ff_pred8x8_horizontal_8_sse2;
        h->pred8x8[VERT_PRED8x8]    = ff_pred8x8_vertical_8_sse2;
        h->pred8x8[PLANE_PRED8x8]   = ff_pred8x8_tm_vp8_8_sse2;
    }
    if (EXTERNAL_SSSE3(flags)) {
        h->pred16x16[HOR_PRED8x8]   = ff_pred16x16_horizontal_8_ssse3;
        h->pred16x16[DC_PRED8x8]    = ff_pred16x16_dc_8_ssse3;
        h->pred8x8[HOR_PRED8x8]     = ff_pred8x8_horizontal_8_ssse3;
        h->pred8x8[PLANE_PRED8x8]   = ff_pred8x8_tm_vp8_8_ssse3;
        h->pred4x4[TM_VP8_PRED]     = ff_pred4x4_tm_vp8_8_ssse3;
    }
    if (EXTERNAL_AVX2(flags)) {
        h->pred16x16[HOR_PRED8x8]   = ff_pred16x16_horizontal_8_avx2;
        h->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_vp8_8_avx2;
        h->pred8x8[HOR_PRED8x8]     = ff_pred8x8_horizontal_8_avx2;
    }
}
