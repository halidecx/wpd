
#include "vp8l_dsp.h"
#if WPD_HAVE_ASM
#if WPD_ARCH_AARCH64
#include "src/aarch64/vp8l_init.h"
#elif WPD_ARCH_X86
#include "src/x86/vp8l_init.h"
#endif
#endif

#include <string.h>

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

/* Add four channels independently by discarding carries between bytes. */
static wpd_always_inline uint32_t pred_add_pixels(uint32_t a, uint32_t b) {
    const uint32_t ag = (a & 0xFF00FF00u) + (b & 0xFF00FF00u);
    const uint32_t rb = (a & 0x00FF00FFu) + (b & 0x00FF00FFu);
    return (ag & 0xFF00FF00u) | (rb & 0x00FF00FFu);
}

static wpd_always_inline int pred_sub3(int a, int b, int c) {
    return WPD_ABS(b - c) - WPD_ABS(a - c);
}

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

static void pred_add_0(const uint32_t *in, const uint32_t *upper,
                       int num_pixels, uint32_t *out) {
    const uint32_t black = pred_black();
    for (int x = 0; x < num_pixels; x++) out[x] = pred_add_pixels(in[x], black);
}

static void pred_add_1(const uint32_t *in, const uint32_t *upper,
                       int num_pixels, uint32_t *out) {
    uint32_t left = out[-1];
    for (int x = 0; x < num_pixels; x++)
        out[x] = left = pred_add_pixels(in[x], left);
}

static void pred_add_2(const uint32_t *in, const uint32_t *upper,
                       int num_pixels, uint32_t *out) {
    for (int x = 0; x < num_pixels; x++)
        out[x] = pred_add_pixels(in[x], upper[x]);
}

PRED_ADD(pred_add_3, tr)
PRED_ADD(pred_add_4, tl)
PRED_ADD(pred_add_5, pred_avg3(l, t, tr))
PRED_ADD(pred_add_6, pred_avg2(l, tl))
PRED_ADD(pred_add_7, pred_avg2(l, t))
PRED_ADD(pred_add_8, pred_avg2(tl, t))
PRED_ADD(pred_add_9, pred_avg2(t, tr))
PRED_ADD(pred_add_10, pred_avg4(l, tl, t, tr))
PRED_ADD(pred_add_11, pred_select(t, l, tl))
PRED_ADD(pred_add_12, pred_clamped_add_sub_full(l, t, tl))
PRED_ADD(pred_add_13, pred_clamped_add_sub_half(l, t, tl))

static void extract_green_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int x = 0; x < num_pixels; x++, src += 4, dst++) *dst = src[2];
}

static void map_color32_c(uint8_t *dst, const uint8_t *src,
                          const uint32_t *palette, int num_pixels) {
    for (int x = 0; x < num_pixels; x++, dst += 4, src += 4)
        memcpy(dst, &palette[src[2]], 4);
}

static void blend_row_argb_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int x = 0; x < num_pixels; x++, dst += 4, src += 4) {
        const int src_alpha = src[0];
        int       tmp_alpha, blend_alpha, scale;

        if (src_alpha == 255) {
            memcpy(dst, src, 4);
            continue;
        }
        if (src_alpha == 0)
            continue;

        tmp_alpha   = (dst[0] * (256 - src_alpha)) >> 8;
        blend_alpha = src_alpha + tmp_alpha;
        scale       = (1 << 24) / blend_alpha;

        dst[1] = ((uint32_t)(src[1] * src_alpha + dst[1] * tmp_alpha) *
                  scale) >>
            24;
        dst[2] = ((uint32_t)(src[2] * src_alpha + dst[2] * tmp_alpha) *
                  scale) >>
            24;
        dst[3] = ((uint32_t)(src[3] * src_alpha + dst[3] * tmp_alpha) *
                  scale) >>
            24;
        dst[0] = blend_alpha;
    }
}

static void blend_row_argb_premult_c(uint8_t *dst, const uint8_t *src,
                                     int num_pixels) {
    for (int x = 0; x < num_pixels; x++, dst += 4, src += 4) {
        const uint32_t scale = 256 - src[0];

        if (src[0] == 255) {
            memcpy(dst, src, 4);
            continue;
        }
        dst[0] = src[0] + ((dst[0] * scale) >> 8);
        dst[1] = src[1] + ((dst[1] * scale) >> 8);
        dst[2] = src[2] + ((dst[2] * scale) >> 8);
        dst[3] = src[3] + ((dst[3] * scale) >> 8);
    }
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
        .extract_green          = extract_green_c,
        .map_color32            = map_color32_c,
        .blend_row_argb         = blend_row_argb_c,
        .blend_row_argb_premult = blend_row_argb_premult_c,
    };

    *dsp = c;

#if WPD_HAVE_ASM
#if WPD_ARCH_AARCH64
    wpd_vp8l_dsp_init_aarch64(dsp);
#elif WPD_ARCH_X86
    wpd_vp8l_dsp_init_x86(dsp);
#endif
#endif
}
