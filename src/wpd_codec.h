#ifndef WPD_CODEC_H
#define WPD_CODEC_H

#include "src/cpu.h"
#include "wpd_util.h"

typedef int16_t WpdDctElem;

typedef struct WpdFrame {
    uint8_t *data[3];
    uint8_t *allocation[3];
    int      linesize[3];
} WpdFrame;

typedef struct WpdCodecContext {
    void *priv_data;
    int   width, height;
    int   bypass_filtering;
} WpdCodecContext;

#endif
