#ifndef WPD_ANIM_H
#define WPD_ANIM_H

#include "container.h"
#include "convert.h"
#include "wpd_dec.h"

/* Where a sub-frame lands on the canvas and what the frame before it left
   there. Scalars only, as ExportSettings is, so a mirror of it can be checked
   by its size. */
typedef struct Placement {
    int canvas_width, canvas_height;
    int pos_x, pos_y;
    int anmf_flags;
    int frame_index;
    int frame_has_alpha;
    int key_frame;
    int prev_anmf_flags;
    int prev_width, prev_height;
    int prev_pos_x, prev_pos_y;
    int prev_key_frame;
    int premultiply;
    int no_fancy_upsampling;
    /* What a disposed region is cleared to, in whichever of the two canvas
       formats is in use. */
    uint8_t clear_argb[4];
    uint8_t clear_yuva[4];
} Placement;

/* Pointers only, and the canvas is the only thing written. */
typedef struct CompositeTargets {
    const WPDLosslessDSP *ldsp;
    const WPDYUVDSP      *ydsp;
    WebPImage            *canvas;
} CompositeTargets;

/* Whether this frame stands on its own, so the canvas under it can be
   discarded rather than blended with. Called before the placement's own
   'key_frame' is set, and is what sets it. */
int anim_is_key_frame(const Placement *pl, int width, int height);

/* Brings the canvas into 'target' format, disposes what the previous frame
   asked to be disposed, and blends or copies the sub-frame onto it. */
int anim_composite(const Placement *pl, const CompositeTargets *ct,
                   const WebPImage *sub, WPDPixelFormat target);

int decode_anmf(WPDDecoder *s, const uint8_t *data, size_t size);

#endif
