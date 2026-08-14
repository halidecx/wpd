#ifndef WPD_EXPORT_H
#define WPD_EXPORT_H

#include "convert.h"
#include "wpd_dec.h"

int  export_packed(WPDDecoder *s, WebPImage *img, WPDFrame *frame);
int  export_still_packed(WPDDecoder *s, WPDFrame *frame, int upto);
int  export_still_lossless(WPDDecoder *s, WPDFrame *frame, int upto);
void export_frame(const WPDDecoder *s, const WebPImage *img,
                  WPDPixelFormat format, WPDFrame *frame);

int export_external_planar_rows(WPDDecoder *s, const WebPImage *img,
                                WPDPixelFormat format, WPDFrame *frame,
                                int row_start, int row_end);

#endif
