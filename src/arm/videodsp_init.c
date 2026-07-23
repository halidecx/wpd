#include "compat.h"

void ff_prefetch_arm(uint8_t *buf, int stride, int h);

av_cold void ff_dsputil_init_arm(DSPContext *dsp)
{
    dsp->prefetch = ff_prefetch_arm;
}
