/* VP8 uses the VP5/6 reference-frame numbering from the original decoder. */
#ifndef FFVP8_VP56DATA_H
#define FFVP8_VP56DATA_H

#include <stdint.h>

typedef enum VP56Frame {
    VP56_FRAME_NONE     = -1,
    VP56_FRAME_CURRENT  =  0,
    VP56_FRAME_PREVIOUS =  1,
    VP56_FRAME_GOLDEN   =  2,
    VP56_FRAME_GOLDEN2  =  3,
} VP56Frame;

typedef struct VP56Tree {
    int8_t val;
    int8_t prob_idx;
} VP56Tree;

#endif
