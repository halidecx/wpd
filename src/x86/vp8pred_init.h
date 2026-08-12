#ifndef WPD_X86_VP8PRED_INIT_H
#define WPD_X86_VP8PRED_INIT_H

#include "src/cpu.h"
#include "src/vp8pred.h"
#include "src/wpd_codec.h"

#define PRED4(name, opt) \
    void ff_pred4x4_##name##_8_##opt(uint8_t *, const uint8_t *, ptrdiff_t)
#define PRED8(name, opt) void ff_pred8x8_##name##_8_##opt(uint8_t *, ptrdiff_t)
#define PRED16(name, opt) \
    void ff_pred16x16_##name##_8_##opt(uint8_t *, ptrdiff_t)

PRED4(dc, sse2);
PRED4(horizontal_vp8, sse2);
PRED4(vertical_left_vp8, ssse3);
PRED4(down_left, sse2);
PRED4(down_right, sse2);
PRED4(horizontal_down, sse2);
PRED4(horizontal_up, sse2);
PRED4(tm_vp8, sse2);
PRED4(tm_vp8, ssse3);
PRED4(vertical_right, sse2);
PRED4(vertical_vp8, sse2);
PRED8(dc_vp8, sse2);
PRED8(top_dc, sse2);
PRED8(top_dc, ssse3);
PRED8(left_dc, sse2);
PRED8(left_dc, ssse3);
PRED8(horizontal, sse2);
PRED8(horizontal, ssse3);
PRED8(tm_vp8, sse2);
PRED8(tm_vp8, ssse3);
PRED8(vertical, sse2);
PRED16(vertical, sse);
PRED16(horizontal, sse2);
PRED16(horizontal, ssse3);
PRED16(dc, sse2);
PRED16(dc, ssse3);
PRED16(top_dc, sse2);
PRED16(top_dc, ssse3);
PRED16(left_dc, sse2);
PRED16(left_dc, ssse3);
PRED16(tm_vp8, sse2);
PRED16(tm_vp8, ssse3);
PRED16(tm_vp8, avx2);

static wpd_always_inline void ff_vp8_pred_init_x86(VP8PredContext *pred) {
    const unsigned flags = wpd_get_cpu_flags();

    if (flags & WPD_X86_CPU_FLAG_SSE)
        pred->pred16x16[VERT_PRED8x8] = ff_pred16x16_vertical_8_sse;
    if (flags & WPD_X86_CPU_FLAG_SSE2) {
        pred->pred4x4[DIAG_DOWN_LEFT_PRED]  = ff_pred4x4_down_left_8_sse2;
        pred->pred4x4[DIAG_DOWN_RIGHT_PRED] = ff_pred4x4_down_right_8_sse2;
        pred->pred4x4[VERT_RIGHT_PRED]      = ff_pred4x4_vertical_right_8_sse2;
        pred->pred4x4[HOR_DOWN_PRED]        = ff_pred4x4_horizontal_down_8_sse2;
        pred->pred4x4[HOR_UP_PRED]          = ff_pred4x4_horizontal_up_8_sse2;
        pred->pred4x4[DC_PRED]              = ff_pred4x4_dc_8_sse2;
        pred->pred4x4[TM_VP8_PRED]          = ff_pred4x4_tm_vp8_8_sse2;
        pred->pred4x4[VERT_PRED]            = ff_pred4x4_vertical_vp8_8_sse2;
        pred->pred8x8[DC_PRED8x8]           = ff_pred8x8_dc_vp8_8_sse2;

        pred->pred4x4[HOR_PRED]        = ff_pred4x4_horizontal_vp8_8_sse2;
        pred->pred16x16[HOR_PRED8x8]   = ff_pred16x16_horizontal_8_sse2;
        pred->pred16x16[DC_PRED8x8]    = ff_pred16x16_dc_8_sse2;
        pred->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_vp8_8_sse2;
        pred->pred8x8[HOR_PRED8x8]     = ff_pred8x8_horizontal_8_sse2;
        pred->pred8x8[VERT_PRED8x8]    = ff_pred8x8_vertical_8_sse2;
        pred->pred8x8[PLANE_PRED8x8]   = ff_pred8x8_tm_vp8_8_sse2;

        pred->pred16x16[TOP_DC_PRED8x8]  = ff_pred16x16_top_dc_8_sse2;
        pred->pred16x16[LEFT_DC_PRED8x8] = ff_pred16x16_left_dc_8_sse2;
        pred->pred8x8[TOP_DC_PRED8x8]    = ff_pred8x8_top_dc_8_sse2;
        pred->pred8x8[LEFT_DC_PRED8x8]   = ff_pred8x8_left_dc_8_sse2;
    }
    if (flags & WPD_X86_CPU_FLAG_SSSE3) {
        pred->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_vp8_8_ssse3;
        pred->pred16x16[HOR_PRED8x8]   = ff_pred16x16_horizontal_8_ssse3;
        pred->pred16x16[DC_PRED8x8]    = ff_pred16x16_dc_8_ssse3;
        pred->pred8x8[HOR_PRED8x8]     = ff_pred8x8_horizontal_8_ssse3;
        pred->pred8x8[PLANE_PRED8x8]   = ff_pred8x8_tm_vp8_8_ssse3;
        pred->pred4x4[TM_VP8_PRED]     = ff_pred4x4_tm_vp8_8_ssse3;
        pred->pred4x4[VERT_LEFT_PRED]  = ff_pred4x4_vertical_left_vp8_8_ssse3;

        pred->pred16x16[TOP_DC_PRED8x8]  = ff_pred16x16_top_dc_8_ssse3;
        pred->pred16x16[LEFT_DC_PRED8x8] = ff_pred16x16_left_dc_8_ssse3;
        pred->pred8x8[TOP_DC_PRED8x8]    = ff_pred8x8_top_dc_8_ssse3;
        pred->pred8x8[LEFT_DC_PRED8x8]   = ff_pred8x8_left_dc_8_ssse3;
    }
    if (flags & WPD_X86_CPU_FLAG_AVX2)
        pred->pred16x16[PLANE_PRED8x8] = ff_pred16x16_tm_vp8_8_avx2;
}

#endif
