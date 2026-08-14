#ifndef WPD_EXPORT_H
#define WPD_EXPORT_H

#include "container.h"
#include "convert.h"

/* What the export needs to know about the frame it is handing out. Scalars
   only, so a mirror of it has nothing to get wrong but the order. 'has_alpha'
   and 'timestamp' arrive already resolved: which of the decoder's three alpha
   flags applies, and what a timestamp is measured from, are questions about
   the decode rather than about the output. */
typedef struct ExportSettings {
    WPDPixelFormat   out_format;
    int              premultiply;
    int              animation;
    WPDAnimationMode anim_mode;
    int              ext_active;
    int              duration;
    int              pos_x, pos_y;
    int              anmf_flags;
    int              has_alpha;
    int64_t          timestamp;
} ExportSettings;

/* Everything the export reads through, writes into, or carries between calls.
   Pointers only, and none of the memory is owned here: 'converted_rows' and
   'converted_format' stay the decoder's, so a partial export resumes where the
   last one stopped. */
typedef struct ExportTargets {
    const WPDYUVDSP         *dsp;
    const WPDDecoderOptions *options;
    RescaleScratch          *rescale;
    WebPImage               *transformed;
    WebPImage               *output;
    WebPImage               *converted;
    const WPDOutputPlane    *ext;
    int                     *converted_rows;
    WPDPixelFormat          *converted_format;
} ExportTargets;

void export_frame(const ExportSettings *set, const WebPImage *img,
                  WPDPixelFormat format, WPDFrame *frame);
int  export_packed(const ExportSettings *set, const ExportTargets *t,
                   WebPImage *img, WPDFrame *frame);
int  export_still_packed(const ExportSettings *set, const ExportTargets *t,
                         const WebPImage *src, WPDFrame *frame, int upto);
int  export_still_lossless(const ExportSettings *set, const ExportTargets *t,
                           WebPImage *img, WPDFrame *frame, int upto);
int  export_external_planar_rows(const ExportSettings *set,
                                 const ExportTargets *t, const WebPImage *img,
                                 WPDPixelFormat format, WPDFrame *frame,
                                 int row_start, int row_end);

void   frame_clear(WPDFrame *frame);
size_t frame_extent(const WPDFrame *frame);

#endif
