/*
 * NEON inverse predictors for the VP8L predictor transform, ported from
 * libwebp's src/dsp/lossless_neon.c.
 *
 * Included by wpd_decoder.c after the scalar predictors, which supply the
 * definitions used for the sub-quad tails. Every operation here is per
 * byte, so the decoder's [A, R, G, B] pixel order needs no special care;
 * only PRED_MODE_BLACK names a channel, and it takes its constant from
 * pred_black().
 *
 * Predictors 5 to 13 depend on the pixel to their left, so those work one
 * pixel per vector lane, rotating the freshly reconstructed pixel into
 * place for the next lane. Predictors 0 to 4, 8 and 9 have no such
 * dependency and process four pixels at a time.
 */

#ifndef WPD_LOSSLESS_NEON_H
#define WPD_LOSSLESS_NEON_H

#include <arm_neon.h>

#define LOAD_U32_AS_U8(v) vreinterpret_u8_u32(vdup_n_u32(v))
#define LOADQ_U32_AS_U8(v) vreinterpretq_u8_u32(vdupq_n_u32(v))
#define LOADQ_U32P_AS_U8(p) vreinterpretq_u8_u32(vld1q_u32(p))
#define STOREQ_U8_AS_U32P(p, v) vst1q_u32((p), vreinterpretq_u32_u8(v))
/* D|C|B|A -> C|B|A|D: move the pixel just built into the next lane */
#define ROTATE32_LEFT(l) vextq_u8((l), (l), 12)

/* PRED_MODE_BLACK */
static void pred_add_0_neon(const uint32_t *in, const uint32_t *upper,
                            int num_pixels, uint32_t *out) {
    const uint8x16_t black = LOADQ_U32_AS_U8(pred_black());
    int              i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);
        STOREQ_U8_AS_U32P(&out[i], vaddq_u8(src, black));
    }
    pred_add_0(in + i, upper + i, num_pixels - i, out + i);
}

/* PRED_MODE_L, as a running per-byte prefix sum over four pixels */
static void pred_add_1_neon(const uint32_t *in, const uint32_t *upper,
                            int num_pixels, uint32_t *out) {
    const uint8x16_t zero = LOADQ_U32_AS_U8(0);
    int              i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        /* a | b | c | d */
        const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);
        /* 0 | a | b | c */
        const uint8x16_t shift0 = vextq_u8(zero, src, 12);
        /* a | a+b | b+c | c+d */
        const uint8x16_t sum0 = vaddq_u8(src, shift0);
        /* 0 | 0 | a | a+b */
        const uint8x16_t shift1 = vextq_u8(zero, sum0, 8);
        /* a | a+b | a+b+c | a+b+c+d */
        const uint8x16_t sum1 = vaddq_u8(sum0, shift1);
        const uint8x16_t prev = LOADQ_U32_AS_U8(out[i - 1]);
        STOREQ_U8_AS_U32P(&out[i], vaddq_u8(sum1, prev));
    }
    pred_add_1(in + i, upper + i, num_pixels - i, out + i);
}

/* predictors that just add a neighbouring pixel */
#define WPD_PRED_NEON_1(x, src_expr)                                \
    static void pred_add_##x##_neon(const uint32_t *in,             \
                                    const uint32_t *upper,          \
                                    int             num_pixels,     \
                                    uint32_t       *out) {          \
        int i;                                                      \
        for (i = 0; i + 4 <= num_pixels; i += 4) {                  \
            const uint8x16_t src   = LOADQ_U32P_AS_U8(&in[i]);      \
            const uint8x16_t other = LOADQ_U32P_AS_U8(&(src_expr)); \
            STOREQ_U8_AS_U32P(&out[i], vaddq_u8(src, other));       \
        }                                                           \
        pred_add_##x(in + i, upper + i, num_pixels - i, out + i);   \
    }

WPD_PRED_NEON_1(2, upper[i]) /* T  */
WPD_PRED_NEON_1(3, upper[i + 1]) /* TR */
WPD_PRED_NEON_1(4, upper[i - 1]) /* TL */
#undef WPD_PRED_NEON_1

/* predictors that average two of the pixels above */
#define WPD_PRED_NEON_2(x, other_expr)                                \
    static void pred_add_##x##_neon(const uint32_t *in,               \
                                    const uint32_t *upper,            \
                                    int             num_pixels,       \
                                    uint32_t       *out) {            \
        int i;                                                        \
        for (i = 0; i + 4 <= num_pixels; i += 4) {                    \
            const uint8x16_t src   = LOADQ_U32P_AS_U8(&in[i]);        \
            const uint8x16_t other = LOADQ_U32P_AS_U8(&(other_expr)); \
            const uint8x16_t t     = LOADQ_U32P_AS_U8(&upper[i]);     \
            const uint8x16_t avg   = vhaddq_u8(t, other);             \
            STOREQ_U8_AS_U32P(&out[i], vaddq_u8(avg, src));           \
        }                                                             \
        pred_add_##x(in + i, upper + i, num_pixels - i, out + i);     \
    }

WPD_PRED_NEON_2(8, upper[i - 1]) /* average TL, T */
WPD_PRED_NEON_2(9, upper[i + 1]) /* average T, TR */
#undef WPD_PRED_NEON_2

/* PRED_MODE_AVG_T_AVG_L_TR: average(average(L, TR), T) */
#define DO_PRED5(lane)                                                       \
    do {                                                                     \
        const uint8x16_t avg_ltr = vhaddq_u8(l, tr);                         \
        const uint8x16_t avg     = vhaddq_u8(avg_ltr, t);                    \
        const uint8x16_t res     = vaddq_u8(avg, src);                       \
        vst1q_lane_u32(&out[i + (lane)], vreinterpretq_u32_u8(res), (lane)); \
        l = ROTATE32_LEFT(res);                                              \
    } while (0)

static void pred_add_5_neon(const uint32_t *in, const uint32_t *upper,
                            int num_pixels, uint32_t *out) {
    uint8x16_t l = LOADQ_U32_AS_U8(out[-1]);
    int        i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);
        const uint8x16_t t   = LOADQ_U32P_AS_U8(&upper[i]);
        const uint8x16_t tr  = LOADQ_U32P_AS_U8(&upper[i + 1]);
        DO_PRED5(0);
        DO_PRED5(1);
        DO_PRED5(2);
        DO_PRED5(3);
    }
    pred_add_5(in + i, upper + i, num_pixels - i, out + i);
}
#undef DO_PRED5

/* PRED_MODE_AVG_L_TL and PRED_MODE_AVG_L_T: average(L, one pixel above) */
#define DO_PRED67(lane)                                                      \
    do {                                                                     \
        const uint8x16_t avg = vhaddq_u8(l, top);                            \
        const uint8x16_t res = vaddq_u8(avg, src);                           \
        vst1q_lane_u32(&out[i + (lane)], vreinterpretq_u32_u8(res), (lane)); \
        l = ROTATE32_LEFT(res);                                              \
    } while (0)

#define WPD_PRED_NEON_67(x, top_expr)                             \
    static void pred_add_##x##_neon(const uint32_t *in,           \
                                    const uint32_t *upper,        \
                                    int             num_pixels,   \
                                    uint32_t       *out) {        \
        uint8x16_t l = LOADQ_U32_AS_U8(out[-1]);                  \
        int        i;                                             \
        for (i = 0; i + 4 <= num_pixels; i += 4) {                \
            const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);      \
            const uint8x16_t top = LOADQ_U32P_AS_U8(&(top_expr)); \
            DO_PRED67(0);                                         \
            DO_PRED67(1);                                         \
            DO_PRED67(2);                                         \
            DO_PRED67(3);                                         \
        }                                                         \
        pred_add_##x(in + i, upper + i, num_pixels - i, out + i); \
    }

WPD_PRED_NEON_67(6, upper[i - 1])
WPD_PRED_NEON_67(7, upper[i])
#undef WPD_PRED_NEON_67
#undef DO_PRED67

/* PRED_MODE_AVG_AVG_L_TL_AVG_T_TR */
#define DO_PRED10(lane)                                                      \
    do {                                                                     \
        const uint8x16_t avg_ltl = vhaddq_u8(l, tl);                         \
        const uint8x16_t avg     = vhaddq_u8(avg_ttr, avg_ltl);              \
        const uint8x16_t res     = vaddq_u8(avg, src);                       \
        vst1q_lane_u32(&out[i + (lane)], vreinterpretq_u32_u8(res), (lane)); \
        l = ROTATE32_LEFT(res);                                              \
    } while (0)

static void pred_add_10_neon(const uint32_t *in, const uint32_t *upper,
                             int num_pixels, uint32_t *out) {
    uint8x16_t l = LOADQ_U32_AS_U8(out[-1]);
    int        i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t src     = LOADQ_U32P_AS_U8(&in[i]);
        const uint8x16_t tl      = LOADQ_U32P_AS_U8(&upper[i - 1]);
        const uint8x16_t t       = LOADQ_U32P_AS_U8(&upper[i]);
        const uint8x16_t tr      = LOADQ_U32P_AS_U8(&upper[i + 1]);
        const uint8x16_t avg_ttr = vhaddq_u8(t, tr);
        DO_PRED10(0);
        DO_PRED10(1);
        DO_PRED10(2);
        DO_PRED10(3);
    }
    pred_add_10(in + i, upper + i, num_pixels - i, out + i);
}
#undef DO_PRED10

/*
 * PRED_MODE_SELECT: pick T when sum|L - TL| <= sum|T - TL|, else L. The
 * pairwise adds reduce each pixel's four byte differences into its lane.
 */
#define DO_PRED11(lane)                                                      \
    do {                                                                     \
        const uint8x16_t sum_l_in = vaddq_u8(l, src);                        \
        const uint8x16_t p_ltl    = vabdq_u8(l, tl);                         \
        const uint32x4_t pa       = vpaddlq_u16(vpaddlq_u8(p_ltl));          \
        const uint32x4_t mask     = vcleq_u32(pa, pb);                       \
        const uint8x16_t res      = vbslq_u8(                                \
            vreinterpretq_u8_u32(mask), sum_t_in, sum_l_in);                 \
        vst1q_lane_u32(&out[i + (lane)], vreinterpretq_u32_u8(res), (lane)); \
        l = ROTATE32_LEFT(res);                                              \
    } while (0)

static void pred_add_11_neon(const uint32_t *in, const uint32_t *upper,
                             int num_pixels, uint32_t *out) {
    uint8x16_t l = LOADQ_U32_AS_U8(out[-1]);
    int        i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t t        = LOADQ_U32P_AS_U8(&upper[i]);
        const uint8x16_t tl       = LOADQ_U32P_AS_U8(&upper[i - 1]);
        const uint32x4_t pb       = vpaddlq_u16(vpaddlq_u8(vabdq_u8(t, tl)));
        const uint8x16_t src      = LOADQ_U32P_AS_U8(&in[i]);
        const uint8x16_t sum_t_in = vaddq_u8(t, src);
        DO_PRED11(0);
        DO_PRED11(1);
        DO_PRED11(2);
        DO_PRED11(3);
    }
    pred_add_11(in + i, upper + i, num_pixels - i, out + i);
}
#undef DO_PRED11

/* PRED_MODE_ADD_SUBTRACT_FULL: clamp(L + T - TL) */
#define DO_PRED12(diff, lane)                                                  \
    do {                                                                       \
        const uint8x8_t pred = vqmovun_s16(                                    \
            vaddq_s16(vreinterpretq_s16_u16(l), (diff)));                      \
        const uint8x8_t res = vadd_u8(                                         \
            pred, (lane) <= 1 ? vget_low_u8(src) : vget_high_u8(src));         \
        const uint16x8_t res16 = vmovl_u8(res);                                \
        vst1_lane_u32(&out[i + (lane)], vreinterpret_u32_u8(res), (lane) & 1); \
        l = vextq_u16(res16, res16, 4);                                        \
    } while (0)

static void pred_add_12_neon(const uint32_t *in, const uint32_t *upper,
                             int num_pixels, uint32_t *out) {
    uint16x8_t l = vmovl_u8(LOAD_U32_AS_U8(out[-1]));
    int        i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);
        const uint8x16_t tl  = LOADQ_U32P_AS_U8(&upper[i - 1]);
        const uint8x16_t t   = LOADQ_U32P_AS_U8(&upper[i]);
        /* T - TL is independent of the reconstruction, so hoist it */
        const int16x8_t diff_lo = vreinterpretq_s16_u16(
            vsubl_u8(vget_low_u8(t), vget_low_u8(tl)));
        const int16x8_t diff_hi = vreinterpretq_s16_u16(
            vsubl_u8(vget_high_u8(t), vget_high_u8(tl)));
        DO_PRED12(diff_lo, 0);
        DO_PRED12(diff_lo, 1);
        DO_PRED12(diff_hi, 2);
        DO_PRED12(diff_hi, 3);
    }
    pred_add_12(in + i, upper + i, num_pixels - i, out + i);
}
#undef DO_PRED12

/* PRED_MODE_ADD_SUBTRACT_HALF: clamp(avg + (avg - TL) / 2), avg = (L + T) / 2 */
#define DO_PRED13(lane, half)                                                  \
    do {                                                                       \
        const uint8x16_t avg = vhaddq_u8(l, t);                                \
        /* halving subtract rounds down, so bias TL where it exceeds avg */    \
        const uint8x16_t cmp      = vcgtq_u8(tl, avg);                         \
        const uint8x16_t tl_1     = vaddq_u8(tl, cmp);                         \
        const int8x8_t   diff_avg = vreinterpret_s8_u8(                        \
            half(vhsubq_u8(avg, tl_1)));                                       \
        const int16x8_t  avg16 = vreinterpretq_s16_u16(vmovl_u8(half(avg)));   \
        const uint8x8_t  delta = vqmovun_s16(vaddw_s8(avg16, diff_avg));       \
        const uint8x8_t  res   = vadd_u8(half(src), delta);                    \
        const uint8x16_t res2  = vcombine_u8(res, res);                        \
        vst1_lane_u32(&out[i + (lane)], vreinterpret_u32_u8(res), (lane) & 1); \
        l = ROTATE32_LEFT(res2);                                               \
    } while (0)

static void pred_add_13_neon(const uint32_t *in, const uint32_t *upper,
                             int num_pixels, uint32_t *out) {
    uint8x16_t l = LOADQ_U32_AS_U8(out[-1]);
    int        i;

    for (i = 0; i + 4 <= num_pixels; i += 4) {
        const uint8x16_t src = LOADQ_U32P_AS_U8(&in[i]);
        const uint8x16_t t   = LOADQ_U32P_AS_U8(&upper[i]);
        const uint8x16_t tl  = LOADQ_U32P_AS_U8(&upper[i - 1]);
        DO_PRED13(0, vget_low_u8);
        DO_PRED13(1, vget_low_u8);
        DO_PRED13(2, vget_high_u8);
        DO_PRED13(3, vget_high_u8);
    }
    pred_add_13(in + i, upper + i, num_pixels - i, out + i);
}
#undef DO_PRED13

#undef LOAD_U32_AS_U8
#undef LOADQ_U32_AS_U8
#undef LOADQ_U32P_AS_U8
#undef STOREQ_U8_AS_U32P
#undef ROTATE32_LEFT

#endif /* WPD_LOSSLESS_NEON_H */
