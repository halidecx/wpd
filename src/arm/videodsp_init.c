#include "wpd_codec.h"

void ff_prefetch_arm(uint8_t *buf, int stride, int h);

wpd_cold void wpd_dsp_init_arm(WpdDSPContext *dsp)
{
    dsp->prefetch = ff_prefetch_arm;
}
