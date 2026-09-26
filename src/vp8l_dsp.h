#ifndef WPD_LOSSLESS_DSP_H
#define WPD_LOSSLESS_DSP_H

#include "wpd_codec.h"

#define WPD_PRED_COUNT 14

typedef void (*pred_add_func)(const uint32_t *in, const uint32_t *upper,
                              int num_pixels, uint32_t *out);

/* An alpha image's green alone: out[-1] is the left pixel, upper[-1] the
 * top left, and the green of each of in is added to its prediction. */
typedef void (*pred_green_func)(const uint32_t *in, const uint8_t *upper,
                                int num_pixels, uint8_t *out);

typedef struct WPDLosslessDSP {
    pred_add_func pred_add[WPD_PRED_COUNT];
    void (*extract_green)(uint8_t *dst, const uint8_t *src, int num_pixels);
    void (*map_color32)(uint8_t *dst, const uint8_t *src,
                        const uint32_t *palette, int num_pixels);
    void (*blend_row_argb)(uint8_t *dst, const uint8_t *src, int num_pixels);
    void (*blend_row_argb_premult)(uint8_t *dst, const uint8_t *src,
                                   int num_pixels);
    void (*color_row)(uint32_t *dst, const uint32_t *src, int num_pixels,
                      uint32_t mult);
    void (*add_green)(uint32_t *dst, const uint32_t *src, int num_pixels);
    pred_green_func pred_green[WPD_PRED_COUNT];
} WPDLosslessDSP;

void wpd_vp8l_dsp_init(WPDLosslessDSP *dsp);

#endif
