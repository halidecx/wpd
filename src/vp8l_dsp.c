/*
 * Inverse prediction for the VP8L predictor transform, operating on whole
 * pixels held in a native word. Every operation below treats the word as
 * four independent bytes, so the decoder's [A, R, G, B] byte order does not
 * matter; only PRED_MODE_BLACK names a specific channel, and it builds its
 * constant from bytes.
 *
 * The SWAR helpers and the predictor set are ported from libwebp's
 * src/dsp/lossless.c.
 */

#include "vp8l_dsp.h"

#include <string.h>

/* per-byte (a + b) >> 1 */
static wpd_always_inline uint32_t pred_avg2(uint32_t a, uint32_t b) {
    return (((a ^ b) & 0xFEFEFEFEu) >> 1) + (a & b);
}

static wpd_always_inline uint32_t pred_avg3(uint32_t a, uint32_t b,
                                            uint32_t c) {
    return pred_avg2(pred_avg2(a, c), b);
}

static wpd_always_inline uint32_t pred_avg4(uint32_t a, uint32_t b, uint32_t c,
                                            uint32_t d) {
    return pred_avg2(pred_avg2(a, b), pred_avg2(c, d));
}

/* per-byte a + b, with the carries between bytes discarded */
static wpd_always_inline uint32_t pred_add_pixels(uint32_t a, uint32_t b) {
    const uint32_t ag = (a & 0xFF00FF00u) + (b & 0xFF00FF00u);
    const uint32_t rb = (a & 0x00FF00FFu) + (b & 0x00FF00FFu);
    return (ag & 0xFF00FF00u) | (rb & 0x00FF00FFu);
}

static wpd_always_inline int pred_sub3(int a, int b, int c) {
    return WPD_ABS(b - c) - WPD_ABS(a - c);
}

/* PRED_MODE_SELECT: whichever of t and l is closer to tl, channel-summed */
static wpd_always_inline uint32_t pred_select(uint32_t t, uint32_t l,
                                              uint32_t tl) {
    const int diff = pred_sub3(
                         (int)(t >> 24), (int)(l >> 24), (int)(tl >> 24)) +
        pred_sub3((int)((t >> 16) & 0xFF),
                  (int)((l >> 16) & 0xFF),
                  (int)((tl >> 16) & 0xFF)) +
        pred_sub3((int)((t >> 8) & 0xFF),
                  (int)((l >> 8) & 0xFF),
                  (int)((tl >> 8) & 0xFF)) +
        pred_sub3((int)(t & 0xFF), (int)(l & 0xFF), (int)(tl & 0xFF));
    return diff <= 0 ? t : l;
}

static wpd_always_inline uint32_t pred_clamped_add_sub_full(uint32_t c0,
                                                            uint32_t c1,
                                                            uint32_t c2) {
    const uint32_t a = wpd_clip_uint8((int)(c0 >> 24) + (int)(c1 >> 24) -
                                      (int)(c2 >> 24));
    const uint32_t r = wpd_clip_uint8((int)((c0 >> 16) & 0xFF) +
                                      (int)((c1 >> 16) & 0xFF) -
                                      (int)((c2 >> 16) & 0xFF));
    const uint32_t g = wpd_clip_uint8((int)((c0 >> 8) & 0xFF) +
                                      (int)((c1 >> 8) & 0xFF) -
                                      (int)((c2 >> 8) & 0xFF));
    const uint32_t b = wpd_clip_uint8((int)(c0 & 0xFF) + (int)(c1 & 0xFF) -
                                      (int)(c2 & 0xFF));
    return a << 24 | r << 16 | g << 8 | b;
}

static wpd_always_inline int pred_add_sub_half(int a, int b) {
    return (int)wpd_clip_uint8(a + (a - b) / 2);
}

static wpd_always_inline uint32_t pred_clamped_add_sub_half(uint32_t c0,
                                                            uint32_t c1,
                                                            uint32_t c2) {
    const uint32_t ave = pred_avg2(c0, c1);
    const uint32_t a   = pred_add_sub_half((int)(ave >> 24), (int)(c2 >> 24));
    const uint32_t r   = pred_add_sub_half((int)((ave >> 16) & 0xFF),
                                           (int)((c2 >> 16) & 0xFF));
    const uint32_t g   = pred_add_sub_half((int)((ave >> 8) & 0xFF),
                                           (int)((c2 >> 8) & 0xFF));
    const uint32_t b   = pred_add_sub_half((int)(ave & 0xFF), (int)(c2 & 0xFF));
    return a << 24 | r << 16 | g << 8 | b;
}

/* opaque black (0xFF000000 in [A, R, G, B] order) as a native word */
static wpd_always_inline uint32_t pred_black(void) {
    const uint8_t bytes[4] = {0xFF, 0x00, 0x00, 0x00};
    uint32_t      v;
    memcpy(&v, bytes, 4);
    return v;
}

#define PRED_ADD(name, expr)                         \
    static void name(const uint32_t *in,             \
                     const uint32_t *upper,          \
                     int             num_pixels,     \
                     uint32_t       *out) {          \
        for (int x = 0; x < num_pixels; x++) {       \
            const uint32_t l  = out[x - 1];          \
            const uint32_t t  = upper[x];            \
            const uint32_t tl = upper[x - 1];        \
            const uint32_t tr = upper[x + 1];        \
            (void)l;                                 \
            (void)t;                                 \
            (void)tl;                                \
            (void)tr;                                \
            out[x] = pred_add_pixels(in[x], (expr)); \
        }                                            \
    }

/* PRED_MODE_BLACK */
static void pred_add_0(const uint32_t *in, const uint32_t *upper,
                       int num_pixels, uint32_t *out) {
    const uint32_t black = pred_black();
    for (int x = 0; x < num_pixels; x++) out[x] = pred_add_pixels(in[x], black);
}

/* PRED_MODE_L: each pixel predicts from the one just reconstructed */
static void pred_add_1(const uint32_t *in, const uint32_t *upper,
                       int num_pixels, uint32_t *out) {
    uint32_t left = out[-1];
    for (int x = 0; x < num_pixels; x++)
        out[x] = left = pred_add_pixels(in[x], left);
}

PRED_ADD(pred_add_2, t) /* T          */
PRED_ADD(pred_add_3, tr) /* TR         */
PRED_ADD(pred_add_4, tl) /* TL         */
PRED_ADD(pred_add_5, pred_avg3(l, t, tr))
PRED_ADD(pred_add_6, pred_avg2(l, tl))
PRED_ADD(pred_add_7, pred_avg2(l, t))
PRED_ADD(pred_add_8, pred_avg2(tl, t))
PRED_ADD(pred_add_9, pred_avg2(t, tr))
PRED_ADD(pred_add_10, pred_avg4(l, tl, t, tr))
PRED_ADD(pred_add_11, pred_select(t, l, tl))
PRED_ADD(pred_add_12, pred_clamped_add_sub_full(l, t, tl))
PRED_ADD(pred_add_13, pred_clamped_add_sub_half(l, t, tl))

/*
 * Gather the green byte of each ARGB pixel into a packed plane. A VP8L
 * alpha chunk carries the alpha values in green, so this runs over the
 * whole image and is worth deinterleaving a vector at a time.
 */
static void extract_green_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int x = 0; x < num_pixels; x++, src += 4, dst++) *dst = src[2];
}

wpd_cold void wpd_vp8l_dsp_init(WPDLosslessDSP *dsp) {
    const WPDLosslessDSP c = {
        .pred_add =
            {
                pred_add_0,
                pred_add_1,
                pred_add_2,
                pred_add_3,
                pred_add_4,
                pred_add_5,
                pred_add_6,
                pred_add_7,
                pred_add_8,
                pred_add_9,
                pred_add_10,
                pred_add_11,
                pred_add_12,
                pred_add_13,
            },
        .extract_green = extract_green_c,
    };

    *dsp = c;

#if WPD_HAVE_ASM && WPD_ARCH_AARCH64
    if (wpd_have_neon(wpd_get_cpu_flags()))
        wpd_vp8l_dsp_init_aarch64(dsp);
#elif WPD_HAVE_ASM && WPD_ARCH_X86
    wpd_vp8l_dsp_init_x86(dsp);
#endif
}
