

#include "vp8dsp.h"
#if WPD_HAVE_ASM
#if WPD_ARCH_AARCH64
#include "src/aarch64/vp8dsp_init.h"
#elif WPD_ARCH_ARM
#include "src/arm/vp8dsp_init.h"
#elif WPD_ARCH_X86
#include "src/x86/vp8dsp_init.h"
#endif
#endif
#include "wpd_codec.h"

static void vp8_luma_dc_wht_c(WpdDctElem block[4][4][16], WpdDctElem dc[16]) {
    int i, t0, t1, t2, t3;

    for (i = 0; i < 4; i++) {
        t0 = dc[0 * 4 + i] + dc[3 * 4 + i];
        t1 = dc[1 * 4 + i] + dc[2 * 4 + i];
        t2 = dc[1 * 4 + i] - dc[2 * 4 + i];
        t3 = dc[0 * 4 + i] - dc[3 * 4 + i];

        dc[0 * 4 + i] = t0 + t1;
        dc[1 * 4 + i] = t3 + t2;
        dc[2 * 4 + i] = t0 - t1;
        dc[3 * 4 + i] = t3 - t2;
    }

    for (i = 0; i < 4; i++) {
        t0            = dc[i * 4 + 0] + dc[i * 4 + 3] + 3;
        t1            = dc[i * 4 + 1] + dc[i * 4 + 2];
        t2            = dc[i * 4 + 1] - dc[i * 4 + 2];
        t3            = dc[i * 4 + 0] - dc[i * 4 + 3] + 3;
        dc[i * 4 + 0] = 0;
        dc[i * 4 + 1] = 0;
        dc[i * 4 + 2] = 0;
        dc[i * 4 + 3] = 0;

        block[i][0][0] = (t0 + t1) >> 3;
        block[i][1][0] = (t3 + t2) >> 3;
        block[i][2][0] = (t0 - t1) >> 3;
        block[i][3][0] = (t3 - t2) >> 3;
    }
}

static void vp8_luma_dc_wht_dc_c(WpdDctElem block[4][4][16],
                                 WpdDctElem dc[16]) {
    int i, val = (dc[0] + 3) >> 3;
    dc[0] = 0;

    for (i = 0; i < 4; i++) {
        block[i][0][0] = val;
        block[i][1][0] = val;
        block[i][2][0] = val;
        block[i][3][0] = val;
    }
}

#define MUL_20091(a) ((((a) * 20091) >> 16) + (a))
#define MUL_35468(a) (((a) * 35468) >> 16)

static void vp8_idct_add_c(uint8_t *dst, WpdDctElem block[16],
                           ptrdiff_t stride) {
    int        i, t0, t1, t2, t3;
    WpdDctElem tmp[16];

    for (i = 0; i < 4; i++) {
        t0 = block[0 * 4 + i] + block[2 * 4 + i];
        t1 = block[0 * 4 + i] - block[2 * 4 + i];
        t2 = MUL_35468(block[1 * 4 + i]) - MUL_20091(block[3 * 4 + i]);
        t3 = MUL_20091(block[1 * 4 + i]) + MUL_35468(block[3 * 4 + i]);
        block[0 * 4 + i] = 0;
        block[1 * 4 + i] = 0;
        block[2 * 4 + i] = 0;
        block[3 * 4 + i] = 0;

        tmp[i * 4 + 0] = t0 + t3;
        tmp[i * 4 + 1] = t1 + t2;
        tmp[i * 4 + 2] = t1 - t2;
        tmp[i * 4 + 3] = t0 - t3;
    }

    for (i = 0; i < 4; i++) {
        t0 = tmp[0 * 4 + i] + tmp[2 * 4 + i];
        t1 = tmp[0 * 4 + i] - tmp[2 * 4 + i];
        t2 = MUL_35468(tmp[1 * 4 + i]) - MUL_20091(tmp[3 * 4 + i]);
        t3 = MUL_20091(tmp[1 * 4 + i]) + MUL_35468(tmp[3 * 4 + i]);

        dst[0] = wpd_clip_uint8(dst[0] + ((t0 + t3 + 4) >> 3));
        dst[1] = wpd_clip_uint8(dst[1] + ((t1 + t2 + 4) >> 3));
        dst[2] = wpd_clip_uint8(dst[2] + ((t1 - t2 + 4) >> 3));
        dst[3] = wpd_clip_uint8(dst[3] + ((t0 - t3 + 4) >> 3));
        dst += stride;
    }
}

static void vp8_idct_dc_add_c(uint8_t *dst, WpdDctElem block[16],
                              ptrdiff_t stride) {
    int i, dc = (block[0] + 4) >> 3;
    block[0] = 0;

    for (i = 0; i < 4; i++) {
        dst[0] = wpd_clip_uint8(dst[0] + dc);
        dst[1] = wpd_clip_uint8(dst[1] + dc);
        dst[2] = wpd_clip_uint8(dst[2] + dc);
        dst[3] = wpd_clip_uint8(dst[3] + dc);
        dst += stride;
    }
}

static void vp8_idct_dc_add4uv_c(uint8_t *dst, WpdDctElem block[4][16],
                                 ptrdiff_t stride) {
    vp8_idct_dc_add_c(dst + stride * 0 + 0, block[0], stride);
    vp8_idct_dc_add_c(dst + stride * 0 + 4, block[1], stride);
    vp8_idct_dc_add_c(dst + stride * 4 + 0, block[2], stride);
    vp8_idct_dc_add_c(dst + stride * 4 + 4, block[3], stride);
}

static void vp8_idct_dc_add4y_c(uint8_t *dst, WpdDctElem block[4][16],
                                ptrdiff_t stride) {
    vp8_idct_dc_add_c(dst + 0, block[0], stride);
    vp8_idct_dc_add_c(dst + 4, block[1], stride);
    vp8_idct_dc_add_c(dst + 8, block[2], stride);
    vp8_idct_dc_add_c(dst + 12, block[3], stride);
}

#define LOAD_PIXELS                     \
    int wpd_unused p3 = p[-4 * stride]; \
    int wpd_unused p2 = p[-3 * stride]; \
    int wpd_unused p1 = p[-2 * stride]; \
    int wpd_unused p0 = p[-1 * stride]; \
    int wpd_unused q0 = p[0 * stride];  \
    int wpd_unused q1 = p[1 * stride];  \
    int wpd_unused q2 = p[2 * stride];  \
    int wpd_unused q3 = p[3 * stride];

static wpd_always_inline int clip_int8(int value) {
    return (int)wpd_clip_uint8(value + 0x80) - 0x80;
}

static wpd_always_inline void filter_common(uint8_t *p, ptrdiff_t stride,
                                            int is4tap) {
    LOAD_PIXELS
    int a, f1, f2;

    a = 3 * (q0 - p0);

    if (is4tap)
        a += clip_int8(p1 - q1);

    a = clip_int8(a);

    /* Match libvpx's c(a + 3) >> 3 behavior rather than the spec wording. */
    f1 = WPD_MIN(a + 4, 127) >> 3;
    f2 = WPD_MIN(a + 3, 127) >> 3;

    /* Clamping here is required for libvpx bit-exact output. */
    p[-1 * stride] = wpd_clip_uint8(p0 + f2);
    p[0 * stride]  = wpd_clip_uint8(q0 - f1);

    if (!is4tap) {
        a              = (f1 + 1) >> 1;
        p[-2 * stride] = wpd_clip_uint8(p1 + a);
        p[1 * stride]  = wpd_clip_uint8(q1 - a);
    }
}

static wpd_always_inline int simple_limit(uint8_t *p, ptrdiff_t stride,
                                          int flim) {
    LOAD_PIXELS
    return 2 * WPD_ABS(p0 - q0) + (WPD_ABS(p1 - q1) >> 1) <= flim;
}

static wpd_always_inline int normal_limit(uint8_t *p, ptrdiff_t stride, int E,
                                          int I) {
    LOAD_PIXELS
    return simple_limit(p, stride, E) && WPD_ABS(p3 - p2) <= I &&
        WPD_ABS(p2 - p1) <= I && WPD_ABS(p1 - p0) <= I &&
        WPD_ABS(q3 - q2) <= I && WPD_ABS(q2 - q1) <= I && WPD_ABS(q1 - q0) <= I;
}

static wpd_always_inline int hev(uint8_t *p, ptrdiff_t stride, int thresh) {
    LOAD_PIXELS
    return WPD_ABS(p1 - p0) > thresh || WPD_ABS(q1 - q0) > thresh;
}

static wpd_always_inline void filter_mbedge(uint8_t *p, ptrdiff_t stride) {
    int a0, a1, a2, w;

    LOAD_PIXELS

    w = clip_int8(p1 - q1);
    w = clip_int8(w + 3 * (q0 - p0));

    a0 = (27 * w + 63) >> 7;
    a1 = (18 * w + 63) >> 7;
    a2 = (9 * w + 63) >> 7;

    p[-3 * stride] = wpd_clip_uint8(p2 + a2);
    p[-2 * stride] = wpd_clip_uint8(p1 + a1);
    p[-1 * stride] = wpd_clip_uint8(p0 + a0);
    p[0 * stride]  = wpd_clip_uint8(q0 - a0);
    p[1 * stride]  = wpd_clip_uint8(q1 - a1);
    p[2 * stride]  = wpd_clip_uint8(q2 - a2);
}

#define LOOP_FILTER(dir, size, stridea, strideb, maybe_inline)              \
    static maybe_inline void vp8_##dir##_loop_filter##size##_c(             \
        uint8_t  *dst,                                                      \
        ptrdiff_t stride,                                                   \
        int       flim_E,                                                   \
        int       flim_I,                                                   \
        int       hev_thresh) {                                             \
        int i;                                                              \
                                                                            \
        for (i = 0; i < size; i++)                                          \
            if (normal_limit(dst + i * stridea, strideb, flim_E, flim_I)) { \
                if (hev(dst + i * stridea, strideb, hev_thresh))            \
                    filter_common(dst + i * stridea, strideb, 1);           \
                else                                                        \
                    filter_mbedge(dst + i * stridea, strideb);              \
            }                                                               \
    }                                                                       \
                                                                            \
    static maybe_inline void vp8_##dir##_loop_filter##size##_inner_c(       \
        uint8_t  *dst,                                                      \
        ptrdiff_t stride,                                                   \
        int       flim_E,                                                   \
        int       flim_I,                                                   \
        int       hev_thresh) {                                             \
        int i;                                                              \
                                                                            \
        for (i = 0; i < size; i++)                                          \
            if (normal_limit(dst + i * stridea, strideb, flim_E, flim_I)) { \
                int hv = hev(dst + i * stridea, strideb, hev_thresh);       \
                if (hv)                                                     \
                    filter_common(dst + i * stridea, strideb, 1);           \
                else                                                        \
                    filter_common(dst + i * stridea, strideb, 0);           \
            }                                                               \
    }

LOOP_FILTER(v, 16, 1, stride, )
LOOP_FILTER(h, 16, stride, 1, )

#define UV_LOOP_FILTER(dir, stridea, strideb)                               \
    LOOP_FILTER(dir, 8, stridea, strideb, wpd_always_inline)                \
    static void vp8_##dir##_loop_filter8uv_c(uint8_t  *dstU,                \
                                             uint8_t  *dstV,                \
                                             ptrdiff_t stride,              \
                                             int       fE,                  \
                                             int       fI,                  \
                                             int       hev_thresh) {        \
        vp8_##dir##_loop_filter8_c(dstU, stride, fE, fI, hev_thresh);       \
        vp8_##dir##_loop_filter8_c(dstV, stride, fE, fI, hev_thresh);       \
    }                                                                       \
    static void vp8_##dir##_loop_filter8uv_inner_c(uint8_t  *dstU,          \
                                                   uint8_t  *dstV,          \
                                                   ptrdiff_t stride,        \
                                                   int       fE,            \
                                                   int       fI,            \
                                                   int       hev_thresh) {  \
        vp8_##dir##_loop_filter8_inner_c(dstU, stride, fE, fI, hev_thresh); \
        vp8_##dir##_loop_filter8_inner_c(dstV, stride, fE, fI, hev_thresh); \
    }

UV_LOOP_FILTER(v, 1, stride)
UV_LOOP_FILTER(h, stride, 1)

static void vp8_v_loop_filter_simple_c(uint8_t *dst, ptrdiff_t stride,
                                       int flim) {
    int i;

    for (i = 0; i < 16; i++)
        if (simple_limit(dst + i, stride, flim))
            filter_common(dst + i, stride, 1);
}

static void vp8_h_loop_filter_simple_c(uint8_t *dst, ptrdiff_t stride,
                                       int flim) {
    int i;

    for (i = 0; i < 16; i++)
        if (simple_limit(dst + i * stride, 1, flim))
            filter_common(dst + i * stride, 1, 1);
}

VP8_H_LOOP_FILTER_SIMPLE_MB(vp8_h_loop_filter_simple_mb_c,
                            vp8_h_loop_filter_simple_c)
VP8_V_LOOP_FILTER_SIMPLE_MB(vp8_v_loop_filter_simple_mb_c,
                            vp8_v_loop_filter_simple_c)

wpd_cold void ff_vp8dsp_init(VP8DSPContext *dsp) {
    dsp->vp8_luma_dc_wht    = vp8_luma_dc_wht_c;
    dsp->vp8_luma_dc_wht_dc = vp8_luma_dc_wht_dc_c;
    dsp->vp8_idct_add       = vp8_idct_add_c;
    dsp->vp8_idct_dc_add    = vp8_idct_dc_add_c;
    dsp->vp8_idct_dc_add4y  = vp8_idct_dc_add4y_c;
    dsp->vp8_idct_dc_add4uv = vp8_idct_dc_add4uv_c;

    dsp->vp8_v_loop_filter16y = vp8_v_loop_filter16_c;
    dsp->vp8_h_loop_filter16y = vp8_h_loop_filter16_c;
    dsp->vp8_v_loop_filter8uv = vp8_v_loop_filter8uv_c;
    dsp->vp8_h_loop_filter8uv = vp8_h_loop_filter8uv_c;

    dsp->vp8_v_loop_filter16y_inner = vp8_v_loop_filter16_inner_c;
    dsp->vp8_h_loop_filter16y_inner = vp8_h_loop_filter16_inner_c;
    dsp->vp8_v_loop_filter8uv_inner = vp8_v_loop_filter8uv_inner_c;
    dsp->vp8_h_loop_filter8uv_inner = vp8_h_loop_filter8uv_inner_c;

    dsp->vp8_v_loop_filter_simple = vp8_v_loop_filter_simple_c;
    dsp->vp8_h_loop_filter_simple = vp8_h_loop_filter_simple_c;

    dsp->vp8_h_loop_filter_simple_mb = vp8_h_loop_filter_simple_mb_c;
    dsp->vp8_v_loop_filter_simple_mb = vp8_v_loop_filter_simple_mb_c;

#if WPD_HAVE_ASM
#if WPD_ARCH_AARCH64
    ff_vp8dsp_init_aarch64(dsp);
#elif WPD_ARCH_ARM
    ff_vp8dsp_init_arm(dsp);
#elif WPD_ARCH_X86
    ff_vp8dsp_init_x86(dsp);
#endif
#endif
}
