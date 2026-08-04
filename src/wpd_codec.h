#ifndef WPD_CODEC_H
#define WPD_CODEC_H

#include "wpd_cpu.h"
#include "wpd_util.h"

#define WPD_PIXEL_FORMAT_YUV420P 0

typedef int16_t WpdDctElem;

#define WPD_MAX_NEG_CROP 1024
extern uint8_t wpd_crop_table[256 + 2 * WPD_MAX_NEG_CROP];

typedef struct WpdFrame {
    uint8_t *data[3];
    uint8_t *allocation[3];
    int linesize[3];
} WpdFrame;

typedef struct WpdCodecContext {
    void *priv_data;
    int width, height;
    int coded_width, coded_height;
    int pix_fmt;
} WpdCodecContext;

typedef struct WpdPacket {
    const uint8_t *data;
    int size;
} WpdPacket;

void wpd_set_dimensions(WpdCodecContext *context, int width, int height);
void wpd_dsp_data_init(void);

#endif
