#ifndef WPD_LOSSLESS_DSP_H
#define WPD_LOSSLESS_DSP_H

#include "wpd_codec.h"

#define WPD_PRED_COUNT 14

typedef void (*pred_add_func)(const uint32_t *in, const uint32_t *upper,
                              int num_pixels, uint32_t *out);

typedef struct WPDLosslessDSP {
    pred_add_func pred_add[WPD_PRED_COUNT];
    void (*extract_green)(uint8_t *dst, const uint8_t *src, int num_pixels);
    /* Replace each pixel by palette[green]; dst may alias src. */
    void (*map_color32)(uint8_t *dst, const uint8_t *src,
                        const uint32_t *palette, int num_pixels);
    /* Alpha-blend an ARGB row of src over dst; alpha is the low byte. */
    void (*blend_row_argb)(uint8_t *dst, const uint8_t *src, int num_pixels);
    /* The same, for rows whose alpha is already multiplied in. */
    void (*blend_row_argb_premult)(uint8_t *dst, const uint8_t *src,
                                   int num_pixels);
} WPDLosslessDSP;

void wpd_vp8l_dsp_init(WPDLosslessDSP *dsp);

#endif
