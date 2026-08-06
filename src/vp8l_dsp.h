#ifndef WPD_LOSSLESS_DSP_H
#define WPD_LOSSLESS_DSP_H

#include "wpd_codec.h"

#define WPD_PRED_COUNT 14

/*
 * A predictor applied to a run of pixels. in and out alias each other: the
 * residual is read from out[x] and the reconstruction written back to it.
 * upper points at the pixel directly above out, so upper[-1] is top-left
 * and upper[+1] top-right.
 *
 * Predictors 0 and 1 ignore upper, which may be NULL for them; the others
 * read upper[-1] through upper[num_pixels].
 */
typedef void (*pred_add_func)(const uint32_t *in, const uint32_t *upper,
                              int num_pixels, uint32_t *out);

typedef struct WPDLosslessDSP {
    pred_add_func pred_add[WPD_PRED_COUNT];
    /* gather the green byte of each ARGB pixel into a packed plane */
    void (*extract_green)(uint8_t *dst, const uint8_t *src, int num_pixels);
} WPDLosslessDSP;

void wpd_vp8l_dsp_init(WPDLosslessDSP *dsp);
void wpd_vp8l_dsp_init_aarch64(WPDLosslessDSP *dsp);

#endif
