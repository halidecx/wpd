#ifndef WPD_CODEC_H
#define WPD_CODEC_H

#include "wpd_cpu.h"
#include "wpd_util.h"

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
} WpdCodecContext;

typedef struct WpdPacket {
    const uint8_t *data;
    int size;
} WpdPacket;

void wpd_dsp_data_init(void);

#endif
