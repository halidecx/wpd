
#include <stdint.h>

#include "vp8dsp.h"
#include "wpd_codec.h"

wpd_cold void ff_vp8dsp_init_arm(VP8DSPContext *dsp) {
    int cpu_flags = wpd_get_cpu_flags();

    if (wpd_have_armv6(cpu_flags))
        ff_vp8dsp_init_armv6(dsp);
    if (wpd_have_neon(cpu_flags))
        ff_vp8dsp_init_neon(dsp);
}
