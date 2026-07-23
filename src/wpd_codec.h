#ifndef WPD_CODEC_H
#define WPD_CODEC_H

#include "wpd_cpu.h"
#include "wpd_util.h"

#define WPD_PIXEL_FORMAT_YUV420P 0
#define WPD_CODEC_FLAG_EMU_EDGE 1
#define WPD_PICTURE_TYPE_I 1
#define WPD_PICTURE_TYPE_P 2

typedef int16_t WpdDctElem;

typedef enum WpdDiscard {
    WPD_DISCARD_NONE = -16,
    WPD_DISCARD_DEFAULT = 0,
    WPD_DISCARD_NONREF = 8,
    WPD_DISCARD_NONKEY = 32,
    WPD_DISCARD_ALL = 48
} WpdDiscard;

#define WPD_MAX_NEG_CROP 1024
extern uint8_t wpd_crop_table[256 + 2 * WPD_MAX_NEG_CROP];

typedef struct WpdFrame {
    uint8_t *data[4];
    uint8_t *allocation[3];
    int linesize[4];
    uint8_t *ref_index[4];
    int key_frame;
    int pict_type;
    int reference;
} WpdFrame;

typedef struct WpdCodecContext {
    void *priv_data;
    int width, height;
    int coded_width, coded_height;
    int flags;
    int skip_frame;
    int skip_loop_filter;
    int is_copy;
    int pix_fmt;
} WpdCodecContext;

typedef struct WpdPacket {
    const uint8_t *data;
    int size;
} WpdPacket;

typedef struct WpdDSPContext {
    void (*prefetch)(uint8_t *buf, int stride, int h);
    void (*emulated_edge_mc)(uint8_t *buf, const uint8_t *src,
                             ptrdiff_t dst_linesize, ptrdiff_t src_linesize,
                             int block_w, int block_h, int src_x, int src_y,
                             int width, int height);
} WpdDSPContext;

void wpd_set_dimensions(WpdCodecContext *context, int width, int height);
void wpd_dsp_init(WpdDSPContext *dsp, WpdCodecContext *context);
void wpd_dsp_init_x86(WpdDSPContext *dsp);
void wpd_dsp_init_arm(WpdDSPContext *dsp);
void wpd_dsp_init_aarch64(WpdDSPContext *dsp);
void wpd_dsp_data_init(void);

#endif
