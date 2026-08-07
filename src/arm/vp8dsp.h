
#ifndef WPD_ARM_VP8DSP_H
#define WPD_ARM_VP8DSP_H

#include "../vp8dsp.h"

#define VP8_LF_Y(hv, inner, opt)                                      \
    void ff_vp8_##hv##_loop_filter16##inner##_##opt(uint8_t  *dst,    \
                                                    ptrdiff_t stride, \
                                                    int       flim_E, \
                                                    int       flim_I, \
                                                    int       hev_thresh)

#define VP8_LF_UV(hv, inner, opt)                                      \
    void ff_vp8_##hv##_loop_filter8uv##inner##_##opt(uint8_t  *dstU,   \
                                                     uint8_t  *dstV,   \
                                                     ptrdiff_t stride, \
                                                     int       flim_E, \
                                                     int       flim_I, \
                                                     int       hev_thresh)

#define VP8_LF_SIMPLE(hv, opt)                     \
    void ff_vp8_##hv##_loop_filter16_simple_##opt( \
        uint8_t *dst, ptrdiff_t stride, int flim)

#define VP8_LF_HV(inner, opt) \
    VP8_LF_Y(h, inner, opt);  \
    VP8_LF_Y(v, inner, opt);  \
    VP8_LF_UV(h, inner, opt); \
    VP8_LF_UV(v, inner, opt)

#define VP8_LF(opt)         \
    VP8_LF_HV(, opt);       \
    VP8_LF_HV(_inner, opt); \
    VP8_LF_SIMPLE(h, opt);  \
    VP8_LF_SIMPLE(v, opt)

#endif
