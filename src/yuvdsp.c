#include "yuvdsp.h"

#include "src/cpu.h"
#include "wpd_compat.h"

#ifndef WPD_ARCH_X86
#error "src/cpu.h did not define the architecture macros"
#endif

#if WPD_HAVE_ASM
#if WPD_ARCH_X86
#include "src/x86/yuvdsp_init.h"
#elif WPD_ARCH_AARCH64
#include "src/aarch64/yuvdsp_init.h"
#endif
#endif

#define YUV_FIX2 6
#define YUV_MASK2 ((256 << YUV_FIX2) - 1)

static wpd_always_inline int yuv_mult_hi(int v, int coeff) {
    return (v * coeff) >> 8;
}

static wpd_always_inline int yuv_clip8(int v) {
    return (v & ~YUV_MASK2) == 0 ? v >> YUV_FIX2 : v < 0 ? 0 : 255;
}

static wpd_always_inline int yuv_to_r(int y, int v) {
    return yuv_clip8(yuv_mult_hi(y, 19077) + yuv_mult_hi(v, 26149) - 14234);
}

static wpd_always_inline int yuv_to_g(int y, int u, int v) {
    return yuv_clip8(yuv_mult_hi(y, 19077) - yuv_mult_hi(u, 6419) -
                     yuv_mult_hi(v, 13320) + 8708);
}

static wpd_always_inline int yuv_to_b(int y, int u) {
    return yuv_clip8(yuv_mult_hi(y, 19077) + yuv_mult_hi(u, 33050) - 17685);
}

#define YUV_TO_OUT(name, ia, ir, ig, ib)         \
    static wpd_always_inline void yuv_to_##name( \
        int y, int u, int v, uint8_t *out) {     \
        out[ia] = 0xff;                          \
        out[ir] = (uint8_t)yuv_to_r(y, v);       \
        out[ig] = (uint8_t)yuv_to_g(y, u, v);    \
        out[ib] = (uint8_t)yuv_to_b(y, u);       \
    }

#define YUV_TO_OUT3(name, ir, ig, ib)            \
    static wpd_always_inline void yuv_to_##name( \
        int y, int u, int v, uint8_t *out) {     \
        out[ir] = (uint8_t)yuv_to_r(y, v);       \
        out[ig] = (uint8_t)yuv_to_g(y, u, v);    \
        out[ib] = (uint8_t)yuv_to_b(y, u);       \
    }

YUV_TO_OUT(argb, 0, 1, 2, 3)
YUV_TO_OUT(rgba, 3, 0, 1, 2)
YUV_TO_OUT(bgra, 3, 2, 1, 0)
YUV_TO_OUT3(rgb, 0, 1, 2)
YUV_TO_OUT3(bgr, 2, 1, 0)
#undef YUV_TO_OUT
#undef YUV_TO_OUT3

#define LOAD_UV(u, v) ((u) | ((v) << 16))

#define UPSAMPLE_PAIRS(name, bpp)                                              \
    static wpd_always_inline void upsample_pairs_##name(                       \
        const uint8_t *top_y,                                                  \
        const uint8_t *bottom_y,                                               \
        const uint8_t *top_u,                                                  \
        const uint8_t *top_v,                                                  \
        const uint8_t *cur_u,                                                  \
        const uint8_t *cur_v,                                                  \
        uint8_t       *top_dst,                                                \
        uint8_t       *bottom_dst,                                             \
        int            first,                                                  \
        int            last) {                                                 \
        uint32_t tl_uv = LOAD_UV(top_u[first - 1], top_v[first - 1]);          \
        uint32_t l_uv  = LOAD_UV(cur_u[first - 1], cur_v[first - 1]);          \
                                                                               \
        for (int x = first; x <= last; x++) {                                  \
            const uint32_t t_uv    = LOAD_UV(top_u[x], top_v[x]);              \
            const uint32_t uv      = LOAD_UV(cur_u[x], cur_v[x]);              \
            const uint32_t avg     = tl_uv + t_uv + l_uv + uv + 0x00080008u;   \
            const uint32_t diag_12 = (avg + 2 * (t_uv + l_uv)) >> 3;           \
            const uint32_t diag_03 = (avg + 2 * (tl_uv + uv)) >> 3;            \
            const uint32_t uv0     = (diag_12 + tl_uv) >> 1;                   \
            const uint32_t uv1     = (diag_03 + t_uv) >> 1;                    \
                                                                               \
            yuv_to_##name(top_y[2 * x - 1],                                    \
                          uv0 & 0xff,                                          \
                          uv0 >> 16,                                           \
                          top_dst + 2 * (bpp) * x - (bpp));                    \
            yuv_to_##name(                                                     \
                top_y[2 * x], uv1 & 0xff, uv1 >> 16, top_dst + 2 * (bpp) * x); \
            if (bottom_y) {                                                    \
                const uint32_t b0 = (diag_03 + l_uv) >> 1;                     \
                const uint32_t b1 = (diag_12 + uv) >> 1;                       \
                                                                               \
                yuv_to_##name(bottom_y[2 * x - 1],                             \
                              b0 & 0xff,                                       \
                              b0 >> 16,                                        \
                              bottom_dst + 2 * (bpp) * x - (bpp));             \
                yuv_to_##name(bottom_y[2 * x],                                 \
                              b1 & 0xff,                                       \
                              b1 >> 16,                                        \
                              bottom_dst + 2 * (bpp) * x);                     \
            }                                                                  \
            tl_uv = t_uv;                                                      \
            l_uv  = uv;                                                        \
        }                                                                      \
    }                                                                          \
                                                                               \
    static void upsample_block_##name##_c(const uint8_t *top_y,                \
                                          const uint8_t *bottom_y,             \
                                          const uint8_t *top_u,                \
                                          const uint8_t *top_v,                \
                                          const uint8_t *cur_u,                \
                                          const uint8_t *cur_v,                \
                                          uint8_t       *top_dst,              \
                                          uint8_t       *bottom_dst,           \
                                          int            num_blocks) {         \
        upsample_pairs_##name(top_y - 1,                                       \
                              bottom_y ? bottom_y - 1 : NULL,                  \
                              top_u,                                           \
                              top_v,                                           \
                              cur_u,                                           \
                              cur_v,                                           \
                              top_dst - (bpp),                                 \
                              bottom_dst ? bottom_dst - (bpp) : NULL,          \
                              1,                                               \
                              num_blocks * (WPD_UPSAMPLE_BLOCK / 2));          \
    }                                                                          \
                                                                               \
    static void upsample_row_##name(const WPDYUVDSP *dsp,                      \
                                    const uint8_t   *top_y,                    \
                                    const uint8_t   *bottom_y,                 \
                                    const uint8_t   *top_u,                    \
                                    const uint8_t   *top_v,                    \
                                    const uint8_t   *cur_u,                    \
                                    const uint8_t   *cur_v,                    \
                                    uint8_t         *top_dst,                  \
                                    uint8_t         *bottom_dst,               \
                                    int              len) {                    \
        const int last_pair = (len - 1) >> 1;                                  \
        const int blocks    = len >= WPD_UPSAMPLE_BLOCK + 2                    \
            ? (len - 2) / WPD_UPSAMPLE_BLOCK                                   \
            : 0;                                                               \
        const int done      = blocks * (WPD_UPSAMPLE_BLOCK / 2);               \
        uint32_t  tl_uv     = LOAD_UV(top_u[0], top_v[0]);                     \
        uint32_t  l_uv      = LOAD_UV(cur_u[0], cur_v[0]);                     \
        uint32_t  uv0       = (3 * tl_uv + l_uv + 0x00020002u) >> 2;           \
                                                                               \
        yuv_to_##name(top_y[0], uv0 & 0xff, uv0 >> 16, top_dst);               \
        if (bottom_y) {                                                        \
            uv0 = (3 * l_uv + tl_uv + 0x00020002u) >> 2;                       \
            yuv_to_##name(bottom_y[0], uv0 & 0xff, uv0 >> 16, bottom_dst);     \
        }                                                                      \
        if (blocks)                                                            \
            dsp->upsample_block[WPD_LAYOUT_##name](                            \
                top_y + 1,                                                     \
                bottom_y ? bottom_y + 1 : NULL,                                \
                top_u,                                                         \
                top_v,                                                         \
                cur_u,                                                         \
                cur_v,                                                         \
                top_dst + (bpp),                                               \
                bottom_dst ? bottom_dst + (bpp) : NULL,                        \
                blocks);                                                       \
        upsample_pairs_##name(top_y,                                           \
                              bottom_y,                                        \
                              top_u,                                           \
                              top_v,                                           \
                              cur_u,                                           \
                              cur_v,                                           \
                              top_dst,                                         \
                              bottom_dst,                                      \
                              done + 1,                                        \
                              last_pair);                                      \
        if (!(len & 1)) {                                                      \
            tl_uv = LOAD_UV(top_u[last_pair], top_v[last_pair]);               \
            l_uv  = LOAD_UV(cur_u[last_pair], cur_v[last_pair]);               \
            uv0   = (3 * tl_uv + l_uv + 0x00020002u) >> 2;                     \
            yuv_to_##name(top_y[len - 1],                                      \
                          uv0 & 0xff,                                          \
                          uv0 >> 16,                                           \
                          top_dst + (bpp) * (len - 1));                        \
            if (bottom_y) {                                                    \
                uv0 = (3 * l_uv + tl_uv + 0x00020002u) >> 2;                   \
                yuv_to_##name(bottom_y[len - 1],                               \
                              uv0 & 0xff,                                      \
                              uv0 >> 16,                                       \
                              bottom_dst + (bpp) * (len - 1));                 \
            }                                                                  \
        }                                                                      \
    }

#define WPD_LAYOUT_argb WPD_LAYOUT_ARGB
#define WPD_LAYOUT_rgba WPD_LAYOUT_RGBA
#define WPD_LAYOUT_bgra WPD_LAYOUT_BGRA
#define WPD_LAYOUT_rgb WPD_LAYOUT_RGB
#define WPD_LAYOUT_bgr WPD_LAYOUT_BGR

UPSAMPLE_PAIRS(argb, 4)
UPSAMPLE_PAIRS(rgba, 4)
UPSAMPLE_PAIRS(bgra, 4)
UPSAMPLE_PAIRS(rgb, 3)
UPSAMPLE_PAIRS(bgr, 3)
#undef UPSAMPLE_PAIRS

static void dispatch_alpha_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) dst[4 * i] = src[i];
}

static void pack_rgba_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        dst[4 * i + 0] = src[4 * i + 1];
        dst[4 * i + 1] = src[4 * i + 2];
        dst[4 * i + 2] = src[4 * i + 3];
        dst[4 * i + 3] = src[4 * i + 0];
    }
}

static void pack_bgra_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        dst[4 * i + 0] = src[4 * i + 3];
        dst[4 * i + 1] = src[4 * i + 2];
        dst[4 * i + 2] = src[4 * i + 1];
        dst[4 * i + 3] = src[4 * i + 0];
    }
}

static void pack_rgb_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        dst[3 * i + 0] = src[4 * i + 1];
        dst[3 * i + 1] = src[4 * i + 2];
        dst[3 * i + 2] = src[4 * i + 3];
    }
}

static void pack_bgr_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        dst[3 * i + 0] = src[4 * i + 3];
        dst[3 * i + 1] = src[4 * i + 2];
        dst[3 * i + 2] = src[4 * i + 1];
    }
}

static void pack_rgb565_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        const int r = src[4 * i + 1];
        const int g = src[4 * i + 2];
        const int b = src[4 * i + 3];

        dst[2 * i]     = (uint8_t)((r & 0xf8) | g >> 5);
        dst[2 * i + 1] = (uint8_t)((g << 3 & 0xe0) | b >> 3);
    }
}

static void pack_rgba4444_c(uint8_t *dst, const uint8_t *src, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        dst[2 * i] = (uint8_t)((src[4 * i + 1] & 0xf0) | src[4 * i + 2] >> 4);
        dst[2 * i + 1] = (uint8_t)((src[4 * i + 3] & 0xf0) | src[4 * i] >> 4);
    }
}

static void premultiply_row_4444_c(uint8_t *rgba4444, int num_pixels) {
    for (int i = 0; i < num_pixels; i++) {
        const unsigned rg   = rgba4444[2 * i];
        const unsigned ba   = rgba4444[2 * i + 1];
        const unsigned a    = ba & 0x0f;
        const unsigned mult = a * 0x1111u;
        const unsigned r    = (((rg & 0xf0) | rg >> 4) * mult) >> 16;
        const unsigned g    = (((rg & 0x0f) | (rg << 4 & 0xf0)) * mult) >> 16;
        const unsigned b    = (((ba & 0xf0) | ba >> 4) * mult) >> 16;

        rgba4444[2 * i]     = (uint8_t)((r & 0xf0) | (g >> 4 & 0x0f));
        rgba4444[2 * i + 1] = (uint8_t)((b & 0xf0) | a);
    }
}

/* (x * a * 32897) >> 23 is bit-exact with (int)(x * a / 255.) for 8-bit x,a. */
#define WPD_PREMULTIPLY(x, m) (uint8_t)(((x) * (m)) >> 23)

static void premultiply_row_c(uint8_t *rgba, int alpha_first, int num_pixels) {
    uint8_t *const rgb   = rgba + (alpha_first ? 1 : 0);
    const uint8_t *alpha = rgba + (alpha_first ? 0 : 3);

    for (int i = 0; i < num_pixels; i++) {
        const uint32_t a = alpha[4 * i];

        if (a != 0xff) {
            const uint32_t m = a * 32897u;

            rgb[4 * i + 0] = WPD_PREMULTIPLY(rgb[4 * i + 0], m);
            rgb[4 * i + 1] = WPD_PREMULTIPLY(rgb[4 * i + 1], m);
            rgb[4 * i + 2] = WPD_PREMULTIPLY(rgb[4 * i + 2], m);
        }
    }
}

#undef WPD_PREMULTIPLY

#undef LOAD_UV

#define GAMMA_FIX 12
#define GAMMA_TAB_FIX 7
#define GAMMA_TAB_SIZE (1 << (GAMMA_FIX - GAMMA_TAB_FIX))
#define GAMMA_TAB_SCALE (1 << GAMMA_TAB_FIX)
#define GAMMA_TAB_ROUNDER (GAMMA_TAB_SCALE >> 1)
#define ALPHA_FIX 19

/* pow(v / 255, 0.8) * 4095, rounded, with one padding entry so a gather may
   read a whole dword at the last index. */
const uint16_t wpd_gamma_to_linear_tab[257] = {
    0,    49,   85,   117,  147,  176,  204,  231,  257,  282,  307,  331,
    355,  379,  402,  425,  447,  469,  491,  513,  534,  556,  577,  598,
    618,  639,  659,  679,  699,  719,  739,  759,  778,  798,  817,  836,
    855,  874,  893,  912,  930,  949,  967,  986,  1004, 1022, 1040, 1059,
    1077, 1094, 1112, 1130, 1148, 1165, 1183, 1200, 1218, 1235, 1252, 1270,
    1287, 1304, 1321, 1338, 1355, 1372, 1389, 1406, 1422, 1439, 1456, 1472,
    1489, 1505, 1522, 1538, 1555, 1571, 1587, 1604, 1620, 1636, 1652, 1668,
    1684, 1700, 1716, 1732, 1748, 1764, 1780, 1796, 1812, 1827, 1843, 1859,
    1874, 1890, 1905, 1921, 1937, 1952, 1967, 1983, 1998, 2014, 2029, 2044,
    2059, 2075, 2090, 2105, 2120, 2135, 2151, 2166, 2181, 2196, 2211, 2226,
    2241, 2256, 2270, 2285, 2300, 2315, 2330, 2345, 2359, 2374, 2389, 2403,
    2418, 2433, 2447, 2462, 2477, 2491, 2506, 2520, 2535, 2549, 2564, 2578,
    2592, 2607, 2621, 2636, 2650, 2664, 2679, 2693, 2707, 2721, 2736, 2750,
    2764, 2778, 2792, 2806, 2820, 2835, 2849, 2863, 2877, 2891, 2905, 2919,
    2933, 2947, 2961, 2975, 2988, 3002, 3016, 3030, 3044, 3058, 3072, 3085,
    3099, 3113, 3127, 3140, 3154, 3168, 3182, 3195, 3209, 3222, 3236, 3250,
    3263, 3277, 3291, 3304, 3318, 3331, 3345, 3358, 3372, 3385, 3399, 3412,
    3426, 3439, 3452, 3466, 3479, 3493, 3506, 3519, 3533, 3546, 3559, 3573,
    3586, 3599, 3612, 3626, 3639, 3652, 3665, 3678, 3692, 3705, 3718, 3731,
    3744, 3757, 3771, 3784, 3797, 3810, 3823, 3836, 3849, 3862, 3875, 3888,
    3901, 3914, 3927, 3940, 3953, 3966, 3979, 3992, 4005, 4018, 4031, 4044,
    4056, 4069, 4082, 4095, 0};

/* 255 * pow(v * 128 / 4095, 1 / 0.8), rounded. */
const uint16_t wpd_linear_to_gamma_tab[GAMMA_TAB_SIZE + 1] = {
    0,   3,   8,   13,  19,  25,  31,  38,  45,  52,  60,
    67,  75,  83,  91,  99,  107, 116, 124, 133, 142, 151,
    160, 169, 178, 187, 197, 206, 216, 226, 235, 245, 255};

static wpd_always_inline unsigned gamma_to_linear(uint8_t v) {
    return wpd_gamma_to_linear_tab[v];
}

static wpd_always_inline int linear_to_gamma(unsigned base_value, int shift) {
    const unsigned v   = base_value << shift;
    const int      pos = (int)(v >> (GAMMA_TAB_FIX + 2));
    const int      x   = (int)(v & ((GAMMA_TAB_SCALE << 2) - 1));
    const int      v0  = wpd_linear_to_gamma_tab[pos];
    const int      v1  = wpd_linear_to_gamma_tab[pos + 1];
    const int      y   = v1 * x + v0 * ((GAMMA_TAB_SCALE << 2) - x);

    return (y + GAMMA_TAB_ROUNDER) >> GAMMA_TAB_FIX;
}

#define YUV_FIX 16
#define YUV_HALF (1 << (YUV_FIX - 1))

static wpd_always_inline int rgb_to_y(int r, int g, int b) {
    return (16839 * r + 33059 * g + 6420 * b + YUV_HALF + (16 << YUV_FIX)) >>
        YUV_FIX;
}

static wpd_always_inline int clip_uv(int uv) {
    uv = (uv + (YUV_HALF << 2) + (128 << (YUV_FIX + 2))) >> (YUV_FIX + 2);
    return (uv & ~0xff) == 0 ? uv : uv < 0 ? 0 : 255;
}

static wpd_always_inline int rgb_to_u(int r, int g, int b) {
    return clip_uv(-9719 * r - 19081 * g + 28800 * b);
}

static wpd_always_inline int rgb_to_v(int r, int g, int b) {
    return clip_uv(28800 * r - 24116 * g - 4684 * b);
}

static void argb_to_y_c(uint8_t *y, const uint8_t *argb, int num_pixels) {
    for (int i = 0; i < num_pixels; i++)
        y[i] = (uint8_t)rgb_to_y(
            argb[4 * i + 1], argb[4 * i + 2], argb[4 * i + 3]);
}

static wpd_always_inline int sum4(const uint8_t *p, ptrdiff_t stride) {
    return linear_to_gamma(gamma_to_linear(p[0]) + gamma_to_linear(p[4]) +
                               gamma_to_linear(p[stride]) +
                               gamma_to_linear(p[stride + 4]),
                           0);
}

static wpd_always_inline int sum2(const uint8_t *p, ptrdiff_t stride) {
    return linear_to_gamma(gamma_to_linear(p[0]) + gamma_to_linear(p[stride]),
                           1);
}

static wpd_always_inline int sum_weighted(const uint8_t *p, const uint8_t *a,
                                          unsigned total_a, ptrdiff_t step,
                                          ptrdiff_t stride) {
    const unsigned sum = a[0] * gamma_to_linear(p[0]) +
        a[step] * gamma_to_linear(p[step]) +
        a[stride] * gamma_to_linear(p[stride]) +
        a[stride + step] * gamma_to_linear(p[stride + step]);
    const unsigned inv = (1u << ALPHA_FIX) / total_a;

    return linear_to_gamma((sum * inv) >> (ALPHA_FIX - 2), 0);
}

static void argb_to_uv_c(uint8_t *u, uint8_t *v, const uint8_t *argb,
                         ptrdiff_t argb_stride, int num_pixels,
                         int weight_alpha) {
    const uint8_t *a = argb, *r = argb + 1, *g = argb + 2, *b = argb + 3;
    int            i, j;

    for (i = 0, j = 0; i < (num_pixels >> 1); i++, j += 8) {
        const unsigned total_a = a[j] + a[j + 4] + a[j + argb_stride] +
            a[j + argb_stride + 4];
        int rr, gg, bb;

        if (!weight_alpha || total_a == 4 * 0xff || total_a == 0) {
            rr = sum4(r + j, argb_stride);
            gg = sum4(g + j, argb_stride);
            bb = sum4(b + j, argb_stride);
        } else {
            rr = sum_weighted(r + j, a + j, total_a, 4, argb_stride);
            gg = sum_weighted(g + j, a + j, total_a, 4, argb_stride);
            bb = sum_weighted(b + j, a + j, total_a, 4, argb_stride);
        }
        u[i] = (uint8_t)rgb_to_u(rr, gg, bb);
        v[i] = (uint8_t)rgb_to_v(rr, gg, bb);
    }
    if (num_pixels & 1) {
        const unsigned total_a = 2u * (a[j] + a[j + argb_stride]);
        int            rr, gg, bb;

        if (!weight_alpha || total_a == 4 * 0xff || total_a == 0) {
            rr = sum2(r + j, argb_stride);
            gg = sum2(g + j, argb_stride);
            bb = sum2(b + j, argb_stride);
        } else {
            rr = sum_weighted(r + j, a + j, total_a, 0, argb_stride);
            gg = sum_weighted(g + j, a + j, total_a, 0, argb_stride);
            bb = sum_weighted(b + j, a + j, total_a, 0, argb_stride);
        }
        u[i] = (uint8_t)rgb_to_u(rr, gg, bb);
        v[i] = (uint8_t)rgb_to_v(rr, gg, bb);
    }
}

void wpd_argb_to_yuva(const WPDYUVDSP *dsp, uint8_t *y, ptrdiff_t y_stride,
                      uint8_t *u, uint8_t *v, ptrdiff_t uv_stride, uint8_t *a,
                      ptrdiff_t a_stride, const uint8_t *argb,
                      ptrdiff_t argb_stride, int width, int row_start,
                      int row_end) {
    const int weight_alpha = a != NULL;
    int       row;

    for (row = row_start; row + 1 < row_end; row += 2) {
        const uint8_t *src = argb + (ptrdiff_t)row * argb_stride;

        dsp->argb_to_y(y + (ptrdiff_t)row * y_stride, src, width);
        dsp->argb_to_y(
            y + (ptrdiff_t)(row + 1) * y_stride, src + argb_stride, width);
        dsp->argb_to_uv(u + (ptrdiff_t)(row >> 1) * uv_stride,
                        v + (ptrdiff_t)(row >> 1) * uv_stride,
                        src,
                        argb_stride,
                        width,
                        weight_alpha);
    }
    if (row < row_end) {
        const uint8_t *src = argb + (ptrdiff_t)row * argb_stride;

        dsp->argb_to_y(y + (ptrdiff_t)row * y_stride, src, width);
        dsp->argb_to_uv(u + (ptrdiff_t)(row >> 1) * uv_stride,
                        v + (ptrdiff_t)(row >> 1) * uv_stride,
                        src,
                        0,
                        width,
                        weight_alpha);
    }
    if (!a)
        return;
    for (row = row_start; row < row_end; row++) {
        const uint8_t *src = argb + (ptrdiff_t)row * argb_stride;
        uint8_t       *dst = a + (ptrdiff_t)row * a_stride;

        for (int i = 0; i < width; i++) dst[i] = src[4 * i];
    }
}

static int upsample_first_pair(int row_start) {
    return row_start ? (row_start + 1) / 2 : 1;
}

static int upsample_first_row(int row_start) {
    return row_start ? 2 * upsample_first_pair(row_start) - 1 : 0;
}

#define UPSAMPLE_IMAGE(name)                                                  \
    static void yuv420_to_##name(const WPDYUVDSP *dsp,                        \
                                 uint8_t         *dst,                        \
                                 ptrdiff_t        dst_stride,                 \
                                 const uint8_t   *y,                          \
                                 ptrdiff_t        y_stride,                   \
                                 const uint8_t   *u,                          \
                                 const uint8_t   *v,                          \
                                 ptrdiff_t        uv_stride,                  \
                                 int              width,                      \
                                 int              height,                     \
                                 int              row_start,                  \
                                 int              row_end) {                  \
        if (!row_start)                                                       \
            upsample_row_##name(dsp, y, NULL, u, v, u, v, dst, NULL, width);  \
        for (int j = upsample_first_pair(row_start); 2 * j < row_end; j++) {  \
            const uint8_t *top_u = u + (ptrdiff_t)(j - 1) * uv_stride;        \
            const uint8_t *top_v = v + (ptrdiff_t)(j - 1) * uv_stride;        \
            uint8_t       *top   = dst + (ptrdiff_t)(2 * j - 1) * dst_stride; \
                                                                              \
            upsample_row_##name(dsp,                                          \
                                y + (ptrdiff_t)(2 * j - 1) * y_stride,        \
                                y + (ptrdiff_t)(2 * j) * y_stride,            \
                                top_u,                                        \
                                top_v,                                        \
                                top_u + uv_stride,                            \
                                top_v + uv_stride,                            \
                                top,                                          \
                                top + dst_stride,                             \
                                width);                                       \
        }                                                                     \
        if (!(height & 1) && row_end == height) {                             \
            const ptrdiff_t off = (ptrdiff_t)((height + 1) / 2 - 1) *         \
                uv_stride;                                                    \
                                                                              \
            upsample_row_##name(dsp,                                          \
                                y + (ptrdiff_t)(height - 1) * y_stride,       \
                                NULL,                                         \
                                u + off,                                      \
                                v + off,                                      \
                                u + off,                                      \
                                v + off,                                      \
                                dst + (ptrdiff_t)(height - 1) * dst_stride,   \
                                NULL,                                         \
                                width);                                       \
        }                                                                     \
    }

UPSAMPLE_IMAGE(argb)
UPSAMPLE_IMAGE(rgba)
UPSAMPLE_IMAGE(bgra)
UPSAMPLE_IMAGE(rgb)
UPSAMPLE_IMAGE(bgr)
#undef UPSAMPLE_IMAGE

int wpd_yuv420_to_packed_rows(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                              ptrdiff_t dst_stride, const uint8_t *y,
                              ptrdiff_t y_stride, const uint8_t *u,
                              const uint8_t *v, ptrdiff_t uv_stride,
                              const uint8_t *a, ptrdiff_t a_stride, int width,
                              int height, int row_start, int row_end) {
    const int first = upsample_first_row(row_start);

    if (width <= 0 || height <= 0 || row_start >= row_end)
        return row_start;

    switch (layout) {
    case WPD_LAYOUT_RGBA:
        yuv420_to_rgba(dsp,
                       dst,
                       dst_stride,
                       y,
                       y_stride,
                       u,
                       v,
                       uv_stride,
                       width,
                       height,
                       row_start,
                       row_end);
        break;
    case WPD_LAYOUT_BGRA:
        yuv420_to_bgra(dsp,
                       dst,
                       dst_stride,
                       y,
                       y_stride,
                       u,
                       v,
                       uv_stride,
                       width,
                       height,
                       row_start,
                       row_end);
        break;
    case WPD_LAYOUT_RGB:
        yuv420_to_rgb(dsp,
                      dst,
                      dst_stride,
                      y,
                      y_stride,
                      u,
                      v,
                      uv_stride,
                      width,
                      height,
                      row_start,
                      row_end);
        break;
    case WPD_LAYOUT_BGR:
        yuv420_to_bgr(dsp,
                      dst,
                      dst_stride,
                      y,
                      y_stride,
                      u,
                      v,
                      uv_stride,
                      width,
                      height,
                      row_start,
                      row_end);
        break;
    default:
        yuv420_to_argb(dsp,
                       dst,
                       dst_stride,
                       y,
                       y_stride,
                       u,
                       v,
                       uv_stride,
                       width,
                       height,
                       row_start,
                       row_end);
        break;
    }

    if (!a || layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR)
        return first;
    dst += layout == WPD_LAYOUT_ARGB ? 0 : 3;
    for (int j = first; j < row_end; j++)
        dsp->dispatch_alpha(dst + (ptrdiff_t)j * dst_stride,
                            a + (ptrdiff_t)j * a_stride,
                            width);
    return first;
}

void wpd_yuv420_to_packed(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                          ptrdiff_t dst_stride, const uint8_t *y,
                          ptrdiff_t y_stride, const uint8_t *u,
                          const uint8_t *v, ptrdiff_t uv_stride,
                          const uint8_t *a, ptrdiff_t a_stride, int width,
                          int height) {
    wpd_yuv420_to_packed_rows(dsp,
                              layout,
                              dst,
                              dst_stride,
                              y,
                              y_stride,
                              u,
                              v,
                              uv_stride,
                              a,
                              a_stride,
                              width,
                              height,
                              0,
                              height);
}

void wpd_yuv444_to_packed(int layout, uint8_t *dst, ptrdiff_t dst_stride,
                          const uint8_t *y, ptrdiff_t y_stride,
                          const uint8_t *u, const uint8_t *v,
                          ptrdiff_t uv_stride, int width, int height) {
    const int bpp = layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR ? 3
                                                                         : 4;

    for (int j = 0; j < height; j++) {
        const uint8_t *yy  = y + (ptrdiff_t)j * y_stride;
        const uint8_t *uu  = u + (ptrdiff_t)j * uv_stride;
        const uint8_t *vv  = v + (ptrdiff_t)j * uv_stride;
        uint8_t       *out = dst + (ptrdiff_t)j * dst_stride;

        for (int i = 0; i < width; i++) {
            switch (layout) {
            case WPD_LAYOUT_RGBA:
                yuv_to_rgba(yy[i], uu[i], vv[i], out + bpp * i);
                break;
            case WPD_LAYOUT_BGRA:
                yuv_to_bgra(yy[i], uu[i], vv[i], out + bpp * i);
                break;
            case WPD_LAYOUT_RGB:
                yuv_to_rgb(yy[i], uu[i], vv[i], out + bpp * i);
                break;
            case WPD_LAYOUT_BGR:
                yuv_to_bgr(yy[i], uu[i], vv[i], out + bpp * i);
                break;
            default: yuv_to_argb(yy[i], uu[i], vv[i], out + bpp * i); break;
            }
        }
    }
}

void wpd_yuv420_to_packed_simple(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                                 ptrdiff_t dst_stride, const uint8_t *y,
                                 ptrdiff_t y_stride, const uint8_t *u,
                                 const uint8_t *v, ptrdiff_t uv_stride,
                                 const uint8_t *a, ptrdiff_t a_stride,
                                 int width, int row_start, int row_end) {
    const int bpp = layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR ? 3
                                                                         : 4;

    for (int j = row_start; j < row_end; j++) {
        uint8_t *out = dst + (ptrdiff_t)j * dst_stride;

        for (int i = 0; i < width; i++) {
            const int yy = y[(ptrdiff_t)j * y_stride + i];
            const int uu = u[(ptrdiff_t)(j >> 1) * uv_stride + (i >> 1)];
            const int vv = v[(ptrdiff_t)(j >> 1) * uv_stride + (i >> 1)];

            switch (layout) {
            case WPD_LAYOUT_RGBA: yuv_to_rgba(yy, uu, vv, out + bpp * i); break;
            case WPD_LAYOUT_BGRA: yuv_to_bgra(yy, uu, vv, out + bpp * i); break;
            case WPD_LAYOUT_RGB: yuv_to_rgb(yy, uu, vv, out + bpp * i); break;
            case WPD_LAYOUT_BGR: yuv_to_bgr(yy, uu, vv, out + bpp * i); break;
            default: yuv_to_argb(yy, uu, vv, out + bpp * i); break;
            }
        }
        if (a && layout != WPD_LAYOUT_RGB && layout != WPD_LAYOUT_BGR)
            dsp->dispatch_alpha(out + (layout == WPD_LAYOUT_ARGB ? 0 : 3),
                                a + (ptrdiff_t)j * a_stride,
                                width);
    }
}

wpd_cold void wpd_yuv_dsp_init(WPDYUVDSP *dsp) {
    const WPDYUVDSP c = {
        .upsample_block       = {upsample_block_argb_c,
                                 upsample_block_rgba_c,
                                 upsample_block_bgra_c,
                                 upsample_block_rgb_c,
                                 upsample_block_bgr_c},
        .dispatch_alpha       = dispatch_alpha_c,
        .pack_rgba            = pack_rgba_c,
        .pack_bgra            = pack_bgra_c,
        .pack_rgb             = pack_rgb_c,
        .pack_bgr             = pack_bgr_c,
        .pack_rgb565          = pack_rgb565_c,
        .pack_rgba4444        = pack_rgba4444_c,
        .premultiply_row      = premultiply_row_c,
        .premultiply_row_4444 = premultiply_row_4444_c,
        .argb_to_y            = argb_to_y_c,
        .argb_to_uv           = argb_to_uv_c,
    };

    *dsp = c;

#if WPD_HAVE_ASM
#if WPD_ARCH_X86
    wpd_yuv_dsp_init_x86(dsp);
#elif WPD_ARCH_AARCH64
    wpd_yuv_dsp_init_aarch64(dsp);
#endif
#endif
}
