#include "wpd_codec.h"

void ff_prefetch_aarch64(uint8_t *buf, int stride, int h);

wpd_cold void wpd_dsp_init_aarch64(WpdDSPContext *dsp)
{
    dsp->prefetch = ff_prefetch_aarch64;
}
