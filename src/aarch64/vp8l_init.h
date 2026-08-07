#ifndef WPD_AARCH64_VP8L_INIT_H
#define WPD_AARCH64_VP8L_INIT_H

#include "src/cpu.h"
#include "src/vp8l_dsp.h"

#define PRED_ADD_NEON(x)                                    \
    void ff_pred_add_##x##_neon(const uint32_t *in,         \
                                const uint32_t *upper,      \
                                int             num_pixels, \
                                uint32_t       *out);

PRED_ADD_NEON(0)
PRED_ADD_NEON(1)
PRED_ADD_NEON(2)
PRED_ADD_NEON(3)
PRED_ADD_NEON(4)
PRED_ADD_NEON(5)
PRED_ADD_NEON(6)
PRED_ADD_NEON(7)
PRED_ADD_NEON(8)
PRED_ADD_NEON(9)
PRED_ADD_NEON(10)
PRED_ADD_NEON(11)
PRED_ADD_NEON(12)
PRED_ADD_NEON(13)
#undef PRED_ADD_NEON

void ff_extract_green_neon(uint8_t *dst, const uint8_t *src, int num_pixels);

static wpd_always_inline void wpd_vp8l_dsp_init_aarch64(WPDLosslessDSP *dsp) {
    if (!(wpd_get_cpu_flags() & WPD_ARM_CPU_FLAG_NEON))
        return;
    dsp->pred_add[0]  = ff_pred_add_0_neon;
    dsp->pred_add[1]  = ff_pred_add_1_neon;
    dsp->pred_add[2]  = ff_pred_add_2_neon;
    dsp->pred_add[3]  = ff_pred_add_3_neon;
    dsp->pred_add[4]  = ff_pred_add_4_neon;
    dsp->pred_add[5]  = ff_pred_add_5_neon;
    dsp->pred_add[6]  = ff_pred_add_6_neon;
    dsp->pred_add[7]  = ff_pred_add_7_neon;
    dsp->pred_add[8]  = ff_pred_add_8_neon;
    dsp->pred_add[9]  = ff_pred_add_9_neon;
    dsp->pred_add[10] = ff_pred_add_10_neon;
    dsp->pred_add[11] = ff_pred_add_11_neon;
    dsp->pred_add[12] = ff_pred_add_12_neon;
    dsp->pred_add[13] = ff_pred_add_13_neon;

    dsp->extract_green = ff_extract_green_neon;
}

#endif
