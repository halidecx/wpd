#include "vp8pred.h"

static void fill(uint8_t *src, int stride, int width, int height,
                 uint8_t value) {
    for (int y = 0; y < height; y++) {
        for (int x = 0; x < width; x++) src[x] = value;
        src += stride;
    }
}

static void pred4x4_vertical_vp8(uint8_t *src, const uint8_t *topright,
                                 int stride) {
    const int     lt = src[-stride - 1];
    const int     t0 = src[-stride], t1 = src[1 - stride], t2 = src[2 - stride],
                  t3 = src[3 - stride], t4 = topright[0];
    const uint8_t p[4] = {
        (lt + 2 * t0 + t1 + 2) >> 2,
        (t0 + 2 * t1 + t2 + 2) >> 2,
        (t1 + 2 * t2 + t3 + 2) >> 2,
        (t2 + 2 * t3 + t4 + 2) >> 2,
    };
    for (int y = 0; y < 4; y++) {
        memcpy(src, p, 4);
        src += stride;
    }
}

static void pred4x4_horizontal_vp8(uint8_t *src, const uint8_t *topright,
                                   int stride) {
    const int     lt = src[-stride - 1];
    const int     l0 = src[-1], l1 = src[stride - 1], l2 = src[2 * stride - 1],
                  l3   = src[3 * stride - 1];
    const uint8_t p[4] = {
        (lt + 2 * l0 + l1 + 2) >> 2,
        (l0 + 2 * l1 + l2 + 2) >> 2,
        (l1 + 2 * l2 + l3 + 2) >> 2,
        (l2 + 3 * l3 + 2) >> 2,
    };
    for (int y = 0; y < 4; y++) memset(src + y * stride, p[y], 4);
}

static void pred4x4_dc(uint8_t *src, const uint8_t *topright, int stride) {
    int dc = 4;
    for (int i = 0; i < 4; i++) dc += src[i - stride] + src[i * stride - 1];
    fill(src, stride, 4, 4, dc >> 3);
}

static void pred4x4_down_right(uint8_t *src, const uint8_t *topright,
                               int stride) {
    const int     lt = src[-stride - 1];
    const int     t0 = src[-stride], t1 = src[1 - stride], t2 = src[2 - stride],
                  t3 = src[3 - stride];
    const int     l0 = src[-1], l1 = src[stride - 1], l2 = src[2 * stride - 1],
                  l3   = src[3 * stride - 1];
    const uint8_t p[7] = {
        (l3 + 2 * l2 + l1 + 2) >> 2,
        (l2 + 2 * l1 + l0 + 2) >> 2,
        (l1 + 2 * l0 + lt + 2) >> 2,
        (l0 + 2 * lt + t0 + 2) >> 2,
        (lt + 2 * t0 + t1 + 2) >> 2,
        (t0 + 2 * t1 + t2 + 2) >> 2,
        (t1 + 2 * t2 + t3 + 2) >> 2,
    };
    for (int y = 0; y < 4; y++)
        for (int x = 0; x < 4; x++) src[y * stride + x] = p[3 + x - y];
}

static void pred4x4_down_left(uint8_t *src, const uint8_t *topright,
                              int stride) {
    uint8_t t[8] = {src[-stride],
                    src[1 - stride],
                    src[2 - stride],
                    src[3 - stride],
                    topright[0],
                    topright[1],
                    topright[2],
                    topright[3]};
    uint8_t p[7];
    for (int i = 0; i < 6; i++)
        p[i] = (t[i] + 2 * t[i + 1] + t[i + 2] + 2) >> 2;
    p[6] = (t[6] + 3 * t[7] + 2) >> 2;
    for (int y = 0; y < 4; y++)
        for (int x = 0; x < 4; x++) src[y * stride + x] = p[x + y];
}

static void pred4x4_vertical_right(uint8_t *src, const uint8_t *topright,
                                   int stride) {
    const int     lt = src[-stride - 1];
    const int     t0 = src[-stride], t1 = src[1 - stride], t2 = src[2 - stride],
                  t3 = src[3 - stride];
    const int     l0 = src[-1], l1 = src[stride - 1], l2 = src[2 * stride - 1];
    const uint8_t p[4][4] = {
        {(lt + t0 + 1) >> 1,
         (t0 + t1 + 1) >> 1,
         (t1 + t2 + 1) >> 1,
         (t2 + t3 + 1) >> 1},
        {(l0 + 2 * lt + t0 + 2) >> 2,
         (lt + 2 * t0 + t1 + 2) >> 2,
         (t0 + 2 * t1 + t2 + 2) >> 2,
         (t1 + 2 * t2 + t3 + 2) >> 2},
        {(lt + 2 * l0 + l1 + 2) >> 2,
         (lt + t0 + 1) >> 1,
         (t0 + t1 + 1) >> 1,
         (t1 + t2 + 1) >> 1},
        {(l0 + 2 * l1 + l2 + 2) >> 2,
         (l0 + 2 * lt + t0 + 2) >> 2,
         (lt + 2 * t0 + t1 + 2) >> 2,
         (t0 + 2 * t1 + t2 + 2) >> 2},
    };
    for (int y = 0; y < 4; y++) memcpy(src + y * stride, p[y], 4);
}

static void pred4x4_vertical_left(uint8_t *src, const uint8_t *topright,
                                  int stride) {
    const int t0 = src[-stride], t1 = src[1 - stride], t2 = src[2 - stride],
              t3 = src[3 - stride];
    const int t4 = topright[0], t5 = topright[1], t6 = topright[2],
              t7 = topright[3];

    src[0] = (t0 + t1 + 1) >> 1;
    src[1] = src[2 * stride] = (t1 + t2 + 1) >> 1;
    src[2] = src[1 + 2 * stride] = (t2 + t3 + 1) >> 1;
    src[3] = src[2 + 2 * stride] = (t3 + t4 + 1) >> 1;
    src[stride]                  = (t0 + 2 * t1 + t2 + 2) >> 2;
    src[1 + stride] = src[3 * stride] = (t1 + 2 * t2 + t3 + 2) >> 2;
    src[2 + stride] = src[1 + 3 * stride] = (t2 + 2 * t3 + t4 + 2) >> 2;
    src[3 + stride] = src[2 + 3 * stride] = (t3 + 2 * t4 + t5 + 2) >> 2;
    src[3 + 2 * stride]                   = (t4 + 2 * t5 + t6 + 2) >> 2;
    src[3 + 3 * stride]                   = (t5 + 2 * t6 + t7 + 2) >> 2;
}

static void pred4x4_horizontal_up(uint8_t *src, const uint8_t *topright,
                                  int stride) {
    uint8_t l[4] = {
        src[-1], src[stride - 1], src[2 * stride - 1], src[3 * stride - 1]};
    const uint8_t p[4][4] = {
        {(l[0] + l[1] + 1) >> 1,
         (l[0] + 2 * l[1] + l[2] + 2) >> 2,
         (l[1] + l[2] + 1) >> 1,
         (l[1] + 2 * l[2] + l[3] + 2) >> 2},
        {(l[1] + l[2] + 1) >> 1,
         (l[1] + 2 * l[2] + l[3] + 2) >> 2,
         (l[2] + l[3] + 1) >> 1,
         (l[2] + 2 * l[3] + l[3] + 2) >> 2},
        {(l[2] + l[3] + 1) >> 1, (l[2] + 2 * l[3] + l[3] + 2) >> 2, l[3], l[3]},
        {l[3], l[3], l[3], l[3]},
    };
    for (int y = 0; y < 4; y++) memcpy(src + y * stride, p[y], 4);
}

static void pred4x4_horizontal_down(uint8_t *src, const uint8_t *topright,
                                    int stride) {
    const int     lt = src[-stride - 1];
    const int     t0 = src[-stride], t1 = src[1 - stride], t2 = src[2 - stride];
    const int     l0 = src[-1], l1 = src[stride - 1], l2 = src[2 * stride - 1],
                  l3      = src[3 * stride - 1];
    const uint8_t p[4][4] = {
        {(lt + l0 + 1) >> 1,
         (l0 + 2 * lt + t0 + 2) >> 2,
         (lt + 2 * t0 + t1 + 2) >> 2,
         (t0 + 2 * t1 + t2 + 2) >> 2},
        {(l0 + l1 + 1) >> 1,
         (lt + 2 * l0 + l1 + 2) >> 2,
         (lt + l0 + 1) >> 1,
         (l0 + 2 * lt + t0 + 2) >> 2},
        {(l1 + l2 + 1) >> 1,
         (l0 + 2 * l1 + l2 + 2) >> 2,
         (l0 + l1 + 1) >> 1,
         (lt + 2 * l0 + l1 + 2) >> 2},
        {(l2 + l3 + 1) >> 1,
         (l1 + 2 * l2 + l3 + 2) >> 2,
         (l1 + l2 + 1) >> 1,
         (l0 + 2 * l1 + l2 + 2) >> 2},
    };
    for (int y = 0; y < 4; y++) memcpy(src + y * stride, p[y], 4);
}

static void pred4x4_tm(uint8_t *src, const uint8_t *topright, int stride) {
    uint8_t *cm  = wpd_crop_table + WPD_MAX_NEG_CROP - src[-1 - stride];
    uint8_t *top = src - stride;
    for (int y = 0; y < 4; y++) {
        uint8_t *cm_in = cm + src[-1];
        for (int x = 0; x < 4; x++) src[x] = cm_in[top[x]];
        src += stride;
    }
}

static void pred_vertical(uint8_t *src, int stride, int size) {
    for (int y = 0; y < size; y++) memcpy(src + y * stride, src - stride, size);
}

static void pred_horizontal(uint8_t *src, int stride, int size) {
    for (int y = 0; y < size; y++)
        memset(src + y * stride, src[y * stride - 1], size);
}

static void pred_tm(uint8_t *src, int stride, int size) {
    const int top_left = src[-stride - 1];
    for (int y = 0; y < size; y++)
        for (int x = 0; x < size; x++)
            src[y * stride + x] = wpd_clip_uint8(src[y * stride - 1] +
                                                 src[x - stride] - top_left);
}

static void pred_dc(uint8_t *src, int stride, int size) {
    int dc = size;
    for (int i = 0; i < size; i++) dc += src[i - stride] + src[i * stride - 1];
    fill(src, stride, size, size, dc >> (size == 8 ? 4 : 5));
}

static void pred_left_dc(uint8_t *src, int stride, int size) {
    int dc = size / 2;
    for (int i = 0; i < size; i++) dc += src[i * stride - 1];
    fill(src, stride, size, size, dc / size);
}

static void pred_top_dc(uint8_t *src, int stride, int size) {
    int dc = size / 2;
    for (int i = 0; i < size; i++) dc += src[i - stride];
    fill(src, stride, size, size, dc / size);
}

static void pred8x8_vertical(uint8_t *src, int stride) {
    pred_vertical(src, stride, 8);
}
static void pred16x16_vertical(uint8_t *src, int stride) {
    pred_vertical(src, stride, 16);
}
static void pred8x8_horizontal(uint8_t *src, int stride) {
    pred_horizontal(src, stride, 8);
}
static void pred16x16_horizontal(uint8_t *src, int stride) {
    pred_horizontal(src, stride, 16);
}
static void pred8x8_tm(uint8_t *src, int stride) { pred_tm(src, stride, 8); }
static void pred16x16_tm(uint8_t *src, int stride) { pred_tm(src, stride, 16); }
static void pred8x8_dc(uint8_t *src, int stride) { pred_dc(src, stride, 8); }
static void pred16x16_dc(uint8_t *src, int stride) { pred_dc(src, stride, 16); }
static void pred8x8_left_dc(uint8_t *src, int stride) {
    pred_left_dc(src, stride, 8);
}
static void pred16x16_left_dc(uint8_t *src, int stride) {
    pred_left_dc(src, stride, 16);
}
static void pred8x8_top_dc(uint8_t *src, int stride) {
    pred_top_dc(src, stride, 8);
}
static void pred16x16_top_dc(uint8_t *src, int stride) {
    pred_top_dc(src, stride, 16);
}

static void pred8x8_dc128(uint8_t *src, int stride) {
    fill(src, stride, 8, 8, 128);
}
static void pred16x16_dc128(uint8_t *src, int stride) {
    fill(src, stride, 16, 16, 128);
}

void ff_vp8_pred_init(VP8PredContext *pred) {
    *pred = (VP8PredContext){
        .pred4x4 =
            {
                pred4x4_vertical_vp8,
                pred4x4_horizontal_vp8,
                pred4x4_dc,
                pred4x4_down_left,
                pred4x4_down_right,
                pred4x4_vertical_right,
                pred4x4_horizontal_down,
                pred4x4_vertical_left,
                pred4x4_horizontal_up,
                pred4x4_tm,
            },
        .pred8x8 =
            {
                pred8x8_dc,
                pred8x8_horizontal,
                pred8x8_vertical,
                pred8x8_tm,
                pred8x8_left_dc,
                pred8x8_top_dc,
                pred8x8_dc128,
            },
        .pred16x16 =
            {
                pred16x16_dc,
                pred16x16_horizontal,
                pred16x16_vertical,
                pred16x16_tm,
                pred16x16_left_dc,
                pred16x16_top_dc,
                pred16x16_dc128,
            },
    };
#if WPD_HAVE_ASM
#if WPD_ARCH_AARCH64
    ff_vp8_pred_init_aarch64(pred);
#elif WPD_ARCH_ARM
    ff_vp8_pred_init_arm(pred);
#elif WPD_ARCH_X86
    ff_vp8_pred_init_x86(pred);
#endif
#endif
}
