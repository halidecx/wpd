#ifndef WPD_X86_VP8L_INIT_H
#define WPD_X86_VP8L_INIT_H

#include "src/cpu.h"
#include "src/vp8l_dsp.h"

#define PRED_ADD_AVX2(x)                                    \
    void ff_pred_add_##x##_avx2(const uint32_t *in,         \
                                const uint32_t *upper,      \
                                int             num_pixels, \
                                uint32_t       *out);

PRED_ADD_AVX2(0)
PRED_ADD_AVX2(1)
PRED_ADD_AVX2(2)
PRED_ADD_AVX2(3)
PRED_ADD_AVX2(4)
PRED_ADD_AVX2(5)
PRED_ADD_AVX2(6)
PRED_ADD_AVX2(7)
PRED_ADD_AVX2(8)
PRED_ADD_AVX2(9)
PRED_ADD_AVX2(10)
PRED_ADD_AVX2(11)
PRED_ADD_AVX2(12)
PRED_ADD_AVX2(13)
#undef PRED_ADD_AVX2

void ff_extract_green_avx2(uint8_t *dst, const uint8_t *src, int num_pixels);
void ff_map_color32_avx2(uint8_t *dst, const uint8_t *src,
                         const uint32_t *palette, int num_pixels);
void ff_blend_row_argb_avx2(uint8_t *dst, const uint8_t *src, int num_pixels);

static wpd_always_inline void wpd_vp8l_dsp_init_x86(WPDLosslessDSP *dsp) {
    const unsigned flags = wpd_get_cpu_flags();

    if (flags & WPD_X86_CPU_FLAG_AVX2) {
        dsp->pred_add[0]  = ff_pred_add_0_avx2;
        dsp->pred_add[1]  = ff_pred_add_1_avx2;
        dsp->pred_add[2]  = ff_pred_add_2_avx2;
        dsp->pred_add[3]  = ff_pred_add_3_avx2;
        dsp->pred_add[4]  = ff_pred_add_4_avx2;
        dsp->pred_add[5]  = ff_pred_add_5_avx2;
        dsp->pred_add[6]  = ff_pred_add_6_avx2;
        dsp->pred_add[7]  = ff_pred_add_7_avx2;
        dsp->pred_add[8]  = ff_pred_add_8_avx2;
        dsp->pred_add[9]  = ff_pred_add_9_avx2;
        dsp->pred_add[10] = ff_pred_add_10_avx2;
        dsp->pred_add[11] = ff_pred_add_11_avx2;
        dsp->pred_add[12] = ff_pred_add_12_avx2;
        dsp->pred_add[13] = ff_pred_add_13_avx2;

        dsp->extract_green  = ff_extract_green_avx2;
        dsp->map_color32    = ff_map_color32_avx2;
        dsp->blend_row_argb = ff_blend_row_argb_avx2;
    }
}

#endif
