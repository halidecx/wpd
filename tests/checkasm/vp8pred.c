/*
 * Copyright (c) 2015 Henrik Gramner
 *
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 */

#include <string.h>

#include "checkasm.h"
#include "vp8pred.h"

#define BUF_SIZE (3 * 16 * 17)
#define src0 (buf0 + 4 * 16)
#define src1 (buf1 + 4 * 16)

static const char *const pred4x4_modes[VP8_PRED4X4_COUNT] = {
    "vertical_vp8", "horizontal_vp8", "dc", "down_left", "down_right",
    "vertical_right", "horizontal_down", "vertical_left_vp8", "horizontal_up",
    "tm_vp8", "vertical", "horizontal", "dc_127", "dc_129",
};

static const char *const pred8x8_modes[VP8_PRED8X8_COUNT] = {
    "dc", "horizontal", "vertical", "tm_vp8", "left_dc", "top_dc",
    "dc_128", "dc_127", "dc_129",
};

#define randomize_buffers()                    \
    do {                                       \
        int i;                                 \
        for (i = 0; i < BUF_SIZE; i += 4) {    \
            uint32_t r = rnd();                \
            WPD_WN32A(buf0 + i, r);             \
            WPD_WN32A(buf1 + i, r);             \
        }                                      \
    } while (0)

static void check_pred4x4(VP8PredContext *pred, uint8_t *buf0, uint8_t *buf1)
{
    uint8_t *topright = buf0 + 2 * 16;
    declare_func_emms(WPD_CPU_MMX | WPD_CPU_MMX2, void,
                      uint8_t *src, const uint8_t *topright, ptrdiff_t stride);

    for (int mode = 0; mode < VP8_PRED4X4_COUNT; mode++) {
        if (check_func(pred->pred4x4[mode], "pred4x4_%s", pred4x4_modes[mode])) {
            randomize_buffers();
            call_ref(src0, topright, 12);
            call_new(src1, topright, 12);
            if (memcmp(buf0, buf1, BUF_SIZE))
                fail();
            bench_new(src1, topright, 12);
        }
    }
}

static void check_pred8x8(VP8PredContext *pred, uint8_t *buf0, uint8_t *buf1)
{
    declare_func_emms(WPD_CPU_MMX | WPD_CPU_MMX2, void,
                      uint8_t *src, ptrdiff_t stride);

    for (int mode = 0; mode < VP8_PRED8X8_COUNT; mode++) {
        if (check_func(pred->pred8x8[mode], "pred8x8_%s", pred8x8_modes[mode])) {
            randomize_buffers();
            call_ref(src0, 24);
            call_new(src1, 24);
            if (memcmp(buf0, buf1, BUF_SIZE))
                fail();
            bench_new(src1, 24);
        }
    }
}

static void check_pred16x16(VP8PredContext *pred, uint8_t *buf0, uint8_t *buf1)
{
    declare_func_emms(WPD_CPU_MMX | WPD_CPU_MMX2, void,
                      uint8_t *src, ptrdiff_t stride);

    for (int mode = 0; mode < VP8_PRED8X8_COUNT; mode++) {
        if (check_func(pred->pred16x16[mode], "pred16x16_%s",
                       pred8x8_modes[mode])) {
            randomize_buffers();
            call_ref(src0, 48);
            call_new(src1, 48);
            if (memcmp(buf0, buf1, BUF_SIZE))
                fail();
            bench_new(src1, 48);
        }
    }
}

void checkasm_check_vp8pred(void)
{
    LOCAL_ALIGNED_16(uint8_t, buf0, [BUF_SIZE]);
    LOCAL_ALIGNED_16(uint8_t, buf1, [BUF_SIZE]);
    WpdCodecContext avctx = { 0 };
    WpdDSPContext dsp;
    VP8PredContext pred;

    wpd_dsp_init(&dsp, &avctx);
    ff_vp8_pred_init(&pred);
    check_pred4x4(&pred, buf0, buf1);
    report("pred4x4");
    check_pred8x8(&pred, buf0, buf1);
    report("pred8x8");
    check_pred16x16(&pred, buf0, buf1);
    report("pred16x16");
}
