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
                                 int              height) {                   \
        upsample_row_##name(dsp, y, NULL, u, v, u, v, dst, NULL, width);      \
        for (int j = 1; 2 * j < height; j++) {                                \
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
        if (!(height & 1)) {                                                  \
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

void wpd_yuv420_to_packed(const WPDYUVDSP *dsp, int layout, uint8_t *dst,
                          ptrdiff_t dst_stride, const uint8_t *y,
                          ptrdiff_t y_stride, const uint8_t *u,
                          const uint8_t *v, ptrdiff_t uv_stride,
                          const uint8_t *a, ptrdiff_t a_stride, int width,
                          int height) {
    if (width <= 0 || height <= 0)
        return;

    switch (layout) {
    case WPD_LAYOUT_RGBA:
        yuv420_to_rgba(
            dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, width, height);
        break;
    case WPD_LAYOUT_BGRA:
        yuv420_to_bgra(
            dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, width, height);
        break;
    case WPD_LAYOUT_RGB:
        yuv420_to_rgb(
            dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, width, height);
        break;
    case WPD_LAYOUT_BGR:
        yuv420_to_bgr(
            dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, width, height);
        break;
    default:
        yuv420_to_argb(
            dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, width, height);
        break;
    }

    if (!a || layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR)
        return;
    dst += layout == WPD_LAYOUT_ARGB ? 0 : 3;
    for (int j = 0; j < height; j++)
        dsp->dispatch_alpha(dst + (ptrdiff_t)j * dst_stride,
                            a + (ptrdiff_t)j * a_stride,
                            width);
}

wpd_cold void wpd_yuv_dsp_init(WPDYUVDSP *dsp) {
    const WPDYUVDSP c = {
        .upsample_block  = {upsample_block_argb_c,
                            upsample_block_rgba_c,
                            upsample_block_bgra_c,
                            upsample_block_rgb_c,
                            upsample_block_bgr_c},
        .dispatch_alpha  = dispatch_alpha_c,
        .pack_rgba       = pack_rgba_c,
        .pack_bgra       = pack_bgra_c,
        .pack_rgb        = pack_rgb_c,
        .pack_bgr        = pack_bgr_c,
        .premultiply_row = premultiply_row_c,
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
