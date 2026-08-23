#include <string.h>

#include "checkasm.h"
#include "yuvdsp.h"

#define MAX_BLOCKS 8
#define MAX_PIXELS (MAX_BLOCKS * WPD_UPSAMPLE_BLOCK)
#define GUARD_PIXELS 8
#define UV_PIXELS (MAX_PIXELS / 2 + 16)

static const int blocks[] = {1, 2, 3, 5, MAX_BLOCKS};

static void check_upsample_block(WPDYUVDSP *dsp, int layout, const char *name) {
    LOCAL_ALIGNED_16(uint8_t, top_y, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, bottom_y, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, top_u, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, top_v, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, cur_u, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, cur_v, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [8 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [8 * (MAX_PIXELS + GUARD_PIXELS)]);
    uint8_t *const bot0 = dst0 + 4 * (MAX_PIXELS + GUARD_PIXELS);
    uint8_t *const bot1 = dst1 + 4 * (MAX_PIXELS + GUARD_PIXELS);
    declare_func(void,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 const uint8_t *,
                 uint8_t *,
                 uint8_t *,
                 int);

    if (check_func(dsp->upsample_block[layout], "upsample_block_%s", name)) {
        for (size_t i = 0; i < sizeof(blocks) / sizeof(*blocks); i++) {
            const int n = blocks[i];

            for (int x = 0; x < MAX_PIXELS; x++) {
                top_y[x]    = (uint8_t)rnd();
                bottom_y[x] = (uint8_t)rnd();
            }
            for (int x = 0; x < UV_PIXELS; x++) {
                top_u[x] = (uint8_t)rnd();
                top_v[x] = (uint8_t)rnd();
                cur_u[x] = (uint8_t)rnd();
                cur_v[x] = (uint8_t)rnd();
            }
            for (int x = 0; x < 8 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(
                top_y, bottom_y, top_u, top_v, cur_u, cur_v, dst0, bot0, n);
            call_new(
                top_y, bottom_y, top_u, top_v, cur_u, cur_v, dst1, bot1, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();

            for (int x = 0; x < 8 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();
            call_ref(top_y, NULL, top_u, top_v, cur_u, cur_v, dst0, NULL, n);
            call_new(top_y, NULL, top_u, top_v, cur_u, cur_v, dst1, NULL, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(top_y,
                  bottom_y,
                  top_u,
                  top_v,
                  cur_u,
                  cur_v,
                  dst1,
                  bot1,
                  MAX_BLOCKS);
    }
}

static void check_dispatch_alpha(dispatch_alpha_func func, const char *name) {
    static const int lengths[] = {1, 3, 8, 15, 16, 17, 31, 63, 64, MAX_PIXELS};
    LOCAL_ALIGNED_16(uint8_t, src, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(func, "%s", name)) {
        for (size_t i = 0; i < sizeof(lengths) / sizeof(*lengths); i++) {
            const int n = lengths[i];

            for (int x = 0; x < MAX_PIXELS; x++) src[x] = (uint8_t)rnd();
            for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            if (memcmp(dst0, dst1, sizeof(dst0)))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static const int row_lengths[] = {
    1, 2, 3, 4, 5, 7, 8, 15, 16, 17, 31, 63, 64, 255, MAX_PIXELS};

static void check_pack_row(pack_row_func func, const char *name, int bpp) {
    LOCAL_ALIGNED_16(uint8_t, src, [4 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(func, "%s", name)) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int x = 0; x < 4 * MAX_PIXELS; x++) src[x] = (uint8_t)rnd();
            for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, src, n);
            call_new(dst1, src, n);
            if (memcmp(dst0, dst1, (size_t)n * bpp) ||
                memcmp(dst0 + (size_t)n * bpp + 4,
                       dst1 + (size_t)n * bpp + 4,
                       4 * GUARD_PIXELS - 4))
                fail();
        }
        bench_new(dst1, src, MAX_PIXELS);
    }
}

static void check_premultiply_row_4444(premultiply_4444_row_func func,
                                       const char *name, int alpha_byte) {
    LOCAL_ALIGNED_16(uint8_t, rgba0, [2 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, rgba1, [2 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, int);

    if (check_func(func, "%s", name)) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int x = 0; x < 2 * (MAX_PIXELS + GUARD_PIXELS); x++)
                rgba0[x] = rgba1[x] = (uint8_t)rnd();
            rgba0[alpha_byte] = rgba1[alpha_byte] = (uint8_t)(rnd() | 0x0f);
            if (n > 1)
                rgba0[2 + alpha_byte] = rgba1[2 + alpha_byte] =
                    (uint8_t)(rnd() & 0xf0);

            call_ref(rgba0, n);
            call_new(rgba1, n);
            if (memcmp(rgba0, rgba1, sizeof(rgba0)))
                fail();
        }
        bench_new(rgba1, MAX_PIXELS);
    }
}

static void check_argb_to_y(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, argb, [4 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [MAX_PIXELS + GUARD_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->argb_to_y, "argb_to_y")) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int x = 0; x < 4 * MAX_PIXELS; x++) argb[x] = (uint8_t)rnd();
            for (int x = 0; x < MAX_PIXELS + GUARD_PIXELS; x++)
                dst0[x] = dst1[x] = (uint8_t)rnd();

            call_ref(dst0, argb, n);
            call_new(dst1, argb, n);
            if (memcmp(dst0, dst1, (size_t)n + GUARD_PIXELS))
                fail();
        }
        bench_new(dst1, argb, MAX_PIXELS);
    }
}

static void check_argb_to_yuv444(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, argb, [4 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, y0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, y1, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, u0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, u1, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, v0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, v1, [MAX_PIXELS + GUARD_PIXELS]);
    declare_func(void, uint8_t *, uint8_t *, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->argb_to_yuv444, "argb_to_yuv444")) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int x = 0; x < 4 * MAX_PIXELS; x++) argb[x] = (uint8_t)rnd();
            for (int x = 0; x < MAX_PIXELS + GUARD_PIXELS; x++) {
                y0[x] = y1[x] = (uint8_t)rnd();
                u0[x] = u1[x] = (uint8_t)rnd();
                v0[x] = v1[x] = (uint8_t)rnd();
            }

            call_ref(y0, u0, v0, argb, n);
            call_new(y1, u1, v1, argb, n);
            if (memcmp(y0, y1, (size_t)n + GUARD_PIXELS) ||
                memcmp(u0, u1, (size_t)n + GUARD_PIXELS) ||
                memcmp(v0, v1, (size_t)n + GUARD_PIXELS))
                fail();
        }
        bench_new(y1, u1, v1, argb, MAX_PIXELS);
    }
}

static void check_argb_to_uv(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, argb, [8 * MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, u0, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, u1, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, v0, [UV_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, v1, [UV_PIXELS]);
    declare_func(
        void, uint8_t *, uint8_t *, const uint8_t *, ptrdiff_t, int, int);

    if (check_func(dsp->argb_to_uv, "argb_to_uv")) {
        for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
             i++) {
            const int n = row_lengths[i];

            for (int weight = 0; weight < 2; weight++)
                for (int alpha = 0; alpha < 3; alpha++)
                    for (int pair = 0; pair < 2; pair++) {
                        const ptrdiff_t stride = pair ? 4 * MAX_PIXELS : 0;
                        const int       uv     = (n + 1) / 2;

                        for (int x = 0; x < 8 * MAX_PIXELS; x++)
                            argb[x] = (uint8_t)rnd();
                        if (alpha < 2)
                            for (int x = 0; x < 8 * MAX_PIXELS; x += 4)
                                argb[x] = alpha ? 0xff : 0;
                        for (int x = 0; x < UV_PIXELS; x++)
                            u0[x] = u1[x] = v0[x] = v1[x] = (uint8_t)rnd();

                        call_ref(u0, v0, argb, stride, n, weight);
                        call_new(u1, v1, argb, stride, n, weight);
                        if (memcmp(u0, u1, (size_t)uv) ||
                            memcmp(v0, v1, (size_t)uv))
                            fail();
                    }
        }
        bench_new(u1, v1, argb, (ptrdiff_t)4 * MAX_PIXELS, MAX_PIXELS, 1);
    }
}

static void check_premultiply_row(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, argb0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, argb1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, int, int);

    if (check_func(dsp->premultiply_row, "premultiply_row")) {
        for (int alpha_first = 0; alpha_first < 2; alpha_first++)
            for (size_t i = 0; i < sizeof(row_lengths) / sizeof(*row_lengths);
                 i++) {
                const int n   = row_lengths[i];
                const int off = alpha_first ? 0 : 3;

                for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                    argb0[x] = argb1[x] = (uint8_t)rnd();
                argb0[off] = argb1[off] = 0xff;
                if (n > 1)
                    argb0[4 + off] = argb1[4 + off] = 0;

                call_ref(argb0, alpha_first, n);
                call_new(argb1, alpha_first, n);
                if (memcmp(argb0, argb1, sizeof(argb0)))
                    fail();
            }
        bench_new(argb1, 1, MAX_PIXELS);
    }
}

static void check_multiply_row(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, alpha, [MAX_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, row0, [MAX_PIXELS + GUARD_PIXELS]);
    LOCAL_ALIGNED_16(uint8_t, row1, [MAX_PIXELS + GUARD_PIXELS]);
    declare_func(void, uint8_t *, const uint8_t *, int);

    if (check_func(dsp->multiply_row, "multiply_row")) {
        /* Every (value, alpha) pair fits in one 256-wide sweep. */
        for (int a = 0; a < 256; a++) {
            const int n =
                row_lengths[a % (sizeof(row_lengths) / sizeof(*row_lengths))];

            for (int x = 0; x < MAX_PIXELS; x++) alpha[x] = (uint8_t)a;
            for (int x = 0; x < MAX_PIXELS + GUARD_PIXELS; x++)
                row0[x] = row1[x] = (uint8_t)x;

            call_ref(row0, alpha, 256);
            call_new(row1, alpha, 256);
            if (memcmp(row0, row1, MAX_PIXELS + GUARD_PIXELS))
                fail();

            for (int x = 0; x < MAX_PIXELS; x++) alpha[x] = (uint8_t)rnd();
            for (int x = 0; x < MAX_PIXELS + GUARD_PIXELS; x++)
                row0[x] = row1[x] = (uint8_t)rnd();
            call_ref(row0, alpha, n);
            call_new(row1, alpha, n);
            if (memcmp(row0, row1, MAX_PIXELS + GUARD_PIXELS))
                fail();
        }
        bench_new(row1, alpha, MAX_PIXELS);
    }
}

static void check_premultiply_argb_row(WPDYUVDSP *dsp) {
    LOCAL_ALIGNED_16(uint8_t, row0, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    LOCAL_ALIGNED_16(uint8_t, row1, [4 * (MAX_PIXELS + GUARD_PIXELS)]);
    declare_func(void, uint8_t *, int);

    if (check_func(dsp->premultiply_argb_row, "premultiply_argb_row")) {
        for (int a = 0; a < 256; a++) {
            const int n =
                row_lengths[a % (sizeof(row_lengths) / sizeof(*row_lengths))];

            for (int x = 0; x < 4 * (MAX_PIXELS + GUARD_PIXELS); x++)
                row0[x] = row1[x] = (uint8_t)rnd();
            for (int x = 0; x < 4 * MAX_PIXELS; x += 4)
                row0[x] = row1[x] = (uint8_t)(x & 4 ? a : rnd());

            call_ref(row0, n);
            call_new(row1, n);
            if (memcmp(row0, row1, 4 * (MAX_PIXELS + GUARD_PIXELS)))
                fail();
        }
        bench_new(row1, MAX_PIXELS);
    }
}

#define MAX_W 133
#define MAX_H 35
#define MAX_CW ((MAX_W + 1) / 2)
#define MAX_CH ((MAX_H + 1) / 2)

static const struct {
    int w, h;
} sizes[] = {
    {1, 1},   {2, 1},   {1, 2},   {2, 2},   {3, 3},  {4, 4},   {5, 3},
    {7, 9},   {31, 5},  {32, 4},  {33, 3},  {34, 2}, {35, 7},  {63, 5},
    {64, 4},  {65, 3},  {66, 6},  {67, 35}, {97, 9}, {98, 10}, {129, 33},
    {130, 8}, {131, 7}, {132, 6}, {133, 5},
};

static int ref_clip8(int v) {
    return (v & ~((256 << 6) - 1)) == 0 ? v >> 6 : v < 0 ? 0 : 255;
}

static int ref_mult_hi(int v, int coeff) { return (v * coeff) >> 8; }

static int ref_chroma(const uint8_t *c, ptrdiff_t stride, int w, int h, int x,
                      int y) {
    int ra, rb, top, px, tl, t, l, cur, avg, d12, d03;

    if (y == 0) {
        ra = rb = 0;
        top     = 1;
    } else if (!(h & 1) && y == h - 1) {
        ra = rb = (h + 1) / 2 - 1;
        top     = 1;
    } else if (y & 1) {
        ra  = (y + 1) / 2 - 1;
        rb  = (y + 1) / 2;
        top = 1;
    } else {
        ra  = y / 2 - 1;
        rb  = y / 2;
        top = 0;
    }

    if (x == 0 || (!(w & 1) && x == w - 1)) {
        const int cc = x == 0 ? 0 : (w - 1) >> 1;
        const int a  = c[ra * stride + cc];
        const int b  = c[rb * stride + cc];

        return top ? (3 * a + b + 2) >> 2 : (3 * b + a + 2) >> 2;
    }

    px  = (x & 1) ? (x + 1) / 2 : x / 2;
    tl  = c[ra * stride + px - 1];
    t   = c[ra * stride + px];
    l   = c[rb * stride + px - 1];
    cur = c[rb * stride + px];
    avg = tl + t + l + cur + 8;
    d12 = (avg + 2 * (t + l)) >> 3;
    d03 = (avg + 2 * (tl + cur)) >> 3;
    if (x & 1)
        return top ? (d12 + tl) >> 1 : (d03 + l) >> 1;
    return top ? (d03 + t) >> 1 : (d12 + cur) >> 1;
}

static void ref_yuv420_to_packed(int layout, uint8_t *dst, ptrdiff_t dst_stride,
                                 const uint8_t *y, ptrdiff_t y_stride,
                                 const uint8_t *u, const uint8_t *v,
                                 ptrdiff_t uv_stride, const uint8_t *a,
                                 ptrdiff_t a_stride, int w, int h) {
    const int bpp = layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR ? 3
                                                                         : 4;

    for (int j = 0; j < h; j++)
        for (int i = 0; i < w; i++) {
            const int cu    = ref_chroma(u, uv_stride, w, h, i, j);
            const int cv    = ref_chroma(v, uv_stride, w, h, i, j);
            const int luma  = y[j * y_stride + i];
            const int r     = ref_clip8(ref_mult_hi(luma, 19077) +
                                        ref_mult_hi(cv, 26149) - 14234);
            const int g     = ref_clip8(ref_mult_hi(luma, 19077) -
                                        ref_mult_hi(cu, 6419) -
                                        ref_mult_hi(cv, 13320) + 8708);
            const int b     = ref_clip8(ref_mult_hi(luma, 19077) +
                                        ref_mult_hi(cu, 33050) - 17685);
            const int alpha = a ? a[j * a_stride + i] : 0xff;
            uint8_t  *out   = dst + j * dst_stride + i * bpp;

            switch (layout) {
            case WPD_LAYOUT_RGBA:
                out[0] = (uint8_t)r;
                out[1] = (uint8_t)g;
                out[2] = (uint8_t)b;
                out[3] = (uint8_t)alpha;
                break;
            case WPD_LAYOUT_BGRA:
                out[0] = (uint8_t)b;
                out[1] = (uint8_t)g;
                out[2] = (uint8_t)r;
                out[3] = (uint8_t)alpha;
                break;
            case WPD_LAYOUT_RGB:
                out[0] = (uint8_t)r;
                out[1] = (uint8_t)g;
                out[2] = (uint8_t)b;
                break;
            case WPD_LAYOUT_BGR:
                out[0] = (uint8_t)b;
                out[1] = (uint8_t)g;
                out[2] = (uint8_t)r;
                break;
            default:
                out[0] = (uint8_t)alpha;
                out[1] = (uint8_t)r;
                out[2] = (uint8_t)g;
                out[3] = (uint8_t)b;
                break;
            }
        }
}

static void check_yuv420_to_packed(WPDYUVDSP *dsp, int layout,
                                   const char *name) {
    LOCAL_ALIGNED_16(uint8_t, y, [MAX_W * MAX_H]);
    LOCAL_ALIGNED_16(uint8_t, a, [MAX_W * MAX_H]);
    LOCAL_ALIGNED_16(uint8_t, u, [MAX_CW * MAX_CH]);
    LOCAL_ALIGNED_16(uint8_t, v, [MAX_CW * MAX_CH]);
    LOCAL_ALIGNED_16(uint8_t, dst0, [4 * MAX_W * MAX_H]);
    LOCAL_ALIGNED_16(uint8_t, dst1, [4 * MAX_W * MAX_H]);

    if (!checkasm_check_key(
            (CheckasmKey)dsp->upsample_block[layout], "yuv420_to_%s", name))
        return;

    for (size_t i = 0; i < sizeof(sizes) / sizeof(*sizes); i++)
        for (int alpha = 0; alpha < 2; alpha++) {
            const int w   = sizes[i].w;
            const int h   = sizes[i].h;
            const int bpp = layout == WPD_LAYOUT_RGB || layout == WPD_LAYOUT_BGR
                ? 3
                : 4;
            const int stride = w * bpp;

            for (int x = 0; x < MAX_W * MAX_H; x++) {
                y[x] = (uint8_t)rnd();
                a[x] = (uint8_t)rnd();
            }
            for (int x = 0; x < MAX_CW * MAX_CH; x++) {
                u[x] = (uint8_t)rnd();
                v[x] = (uint8_t)rnd();
            }
            memset(dst0, 0xa5, 4 * MAX_W * MAX_H);
            memset(dst1, 0xa5, 4 * MAX_W * MAX_H);

            ref_yuv420_to_packed(layout,
                                 dst0,
                                 stride,
                                 y,
                                 MAX_W,
                                 u,
                                 v,
                                 MAX_CW,
                                 alpha ? a : NULL,
                                 MAX_W,
                                 w,
                                 h);
            wpd_yuv420_to_packed(layout,
                                 dst1,
                                 stride,
                                 y,
                                 MAX_W,
                                 u,
                                 v,
                                 MAX_CW,
                                 alpha ? a : NULL,
                                 MAX_W,
                                 w,
                                 h);
            if (memcmp(dst0, dst1, 4 * MAX_W * MAX_H))
                fail();

            for (int step = 1; step <= 3; step++) {
                int seen = 0;

                memset(dst1, 0xa5, 4 * MAX_W * MAX_H);
                for (int split = 0; split < h;) {
                    int end = split ? split + step : 1;
                    int from;

                    if (end > h)
                        end = h;

                    from = wpd_yuv420_to_packed_rows(layout,
                                                     dst1,
                                                     stride,
                                                     y,
                                                     MAX_W,
                                                     u,
                                                     v,
                                                     MAX_CW,
                                                     alpha ? a : NULL,
                                                     MAX_W,
                                                     w,
                                                     h,
                                                     split,
                                                     end);
                    if (from > split || from < (split ? split - 1 : 0))
                        fail();
                    seen  = end;
                    split = end;
                }
                if (seen != h || memcmp(dst0, dst1, 4 * MAX_W * MAX_H))
                    fail();
            }
        }
}

void checkasm_check_yuvdsp(void) {
    WPDYUVDSP dsp;

    wpd_yuv_dsp_init(&dsp);
    check_upsample_block(&dsp, WPD_LAYOUT_ARGB, "argb");
    check_upsample_block(&dsp, WPD_LAYOUT_RGBA, "rgba");
    check_upsample_block(&dsp, WPD_LAYOUT_BGRA, "bgra");
    check_upsample_block(&dsp, WPD_LAYOUT_RGB, "rgb");
    check_upsample_block(&dsp, WPD_LAYOUT_BGR, "bgr");
    report("upsample_block");
    check_dispatch_alpha(dsp.dispatch_alpha_first, "dispatch_alpha_first");
    check_dispatch_alpha(dsp.dispatch_alpha_last, "dispatch_alpha_last");
    report("dispatch_alpha");
    check_pack_row(dsp.pack_rgba, "pack_rgba", 4);
    check_pack_row(dsp.pack_bgra, "pack_bgra", 4);
    check_pack_row(dsp.pack_rgb, "pack_rgb", 3);
    check_pack_row(dsp.pack_bgr, "pack_bgr", 3);
    check_pack_row(dsp.pack_rgb565, "pack_rgb565", 2);
    check_pack_row(dsp.pack_rgba4444, "pack_rgba4444", 2);
    check_pack_row(dsp.pack_bgr565, "pack_bgr565", 2);
    check_pack_row(dsp.pack_bgra4444, "pack_bgra4444", 2);
    report("pack_row");
    check_premultiply_row(&dsp);
    check_premultiply_row_4444(
        dsp.premultiply_row_4444, "premultiply_row_4444", 1);
    check_premultiply_row_4444(
        dsp.premultiply_row_4444_swap, "premultiply_row_4444_swap", 0);
    report("premultiply_row");
    check_multiply_row(&dsp);
    check_premultiply_argb_row(&dsp);
    report("multiply_row");
    check_argb_to_y(&dsp);
    report("argb_to_y");
    check_argb_to_yuv444(&dsp);
    report("argb_to_yuv444");
    check_argb_to_uv(&dsp);
    report("argb_to_uv");
    check_yuv420_to_packed(&dsp, WPD_LAYOUT_ARGB, "argb");
    check_yuv420_to_packed(&dsp, WPD_LAYOUT_RGBA, "rgba");
    check_yuv420_to_packed(&dsp, WPD_LAYOUT_BGRA, "bgra");
    check_yuv420_to_packed(&dsp, WPD_LAYOUT_RGB, "rgb");
    check_yuv420_to_packed(&dsp, WPD_LAYOUT_BGR, "bgr");
    report("yuv420_to_packed");
}
