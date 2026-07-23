#include "compat.h"

void ff_prefetch_aarch64(uint8_t *buf, int stride, int h);

av_cold void ff_dsputil_init_aarch64(DSPContext *dsp)
{
    dsp->prefetch = ff_prefetch_aarch64;
}
