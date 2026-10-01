#ifndef WPD_LOSSLESS_DSP_H
#define WPD_LOSSLESS_DSP_H

#include "wpd_codec.h"

#define WPD_PRED_COUNT 14

typedef void (*pred_add_func)(const uint32_t *in, const uint32_t *upper,
                              int num_pixels, uint32_t *out);

/* Two rows at once, the second over the first: in_b and out_b take the
 * first's out as their upper. Only for predictors with no top right. */
typedef void (*pred_pair_func)(const uint32_t *in, const uint32_t *upper,
                               int num_pixels, uint32_t *out,
                               const uint32_t *in_b, uint32_t *out_b);

/* An alpha image's green alone: out[-1] is the left pixel, upper[-1] the
 * top left, and the green of each of in is added to its prediction. */
typedef void (*pred_green_func)(const uint32_t *in, const uint8_t *upper,
                                int num_pixels, uint8_t *out);

/* An alpha image's palette indices, two to a byte with the first pixel in
 * the low nibble, looked up among sixteen alphas: num_blocks blocks of
 * sixteen pixels, from eight bytes each. */
typedef void (*expand_alpha_func)(uint8_t *dst, const uint8_t *src,
                                  const uint8_t *palette, int num_blocks);

typedef struct WPDLosslessDSP {
    pred_add_func  pred_add[WPD_PRED_COUNT];
    pred_pair_func pred_add_pair[WPD_PRED_COUNT];
    void (*extract_green)(uint8_t *dst, const uint8_t *src, int num_pixels);
    void (*map_color32)(uint8_t *dst, const uint8_t *src,
                        const uint32_t *palette, int num_pixels);
    void (*blend_row_argb)(uint8_t *dst, const uint8_t *src, int num_pixels);
    void (*blend_row_argb_premult)(uint8_t *dst, const uint8_t *src,
                                   int num_pixels);
    void (*color_row)(uint32_t *dst, const uint32_t *src, int num_pixels,
                      uint32_t mult);
    void (*add_green)(uint32_t *dst, const uint32_t *src, int num_pixels);
    pred_green_func   pred_green[WPD_PRED_COUNT];
    expand_alpha_func expand_alpha_nibbles;
} WPDLosslessDSP;

void wpd_vp8l_dsp_init(WPDLosslessDSP *dsp);

#endif
