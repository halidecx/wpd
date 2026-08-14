#ifndef WPD_CONTAINER_H
#define WPD_CONTAINER_H

#include "wpd_internal.h"

#define ANMF_FLAG_DISPOSE (1 << 0)
#define ANMF_FLAG_NO_BLEND (1 << 1)

/* One ANMF header, as the scanner reads it without decoding anything. */
typedef struct FrameEntry {
    int pos_x, pos_y;
    int width, height;
    int duration;
    int dispose, blend;
    int has_alpha;
    int complete;
} FrameEntry;

/* What a scan found. The scanner's own state — where it stopped, the frame
   table, how far into an ANMF the alpha walk has gone — stays behind
   HeaderScan; this is only the part the container reads back. */
typedef struct ScanInfo {
    size_t end;
    int    width, height;
    int    has_alpha;
    /* Alpha the image chunks themselves carry, without the VP8X declaration
       folded in, which is what a decoded frame reports. */
    int       image_has_alpha;
    int       animation;
    int       images;
    int       frame_count;
    int       loop_count;
    uint32_t  background_argb;
    WPDCoding coding;
    int       truncated;
    int       metadata;
    size_t    meta_offset[WPD_METADATA_NB];
    uint32_t  meta_size[WPD_METADATA_NB];
    int       raw_kind;
    size_t    raw_image_offset;
    size_t    raw_image_size;
    size_t    raw_alpha_offset;
    size_t    raw_alpha_size;
} ScanInfo;

/* The RIFF scanner. It walks the chunk list of a file that may still be
   arriving, and remembers where it stopped so that feeding a stream one piece
   at a time stays linear. */
typedef struct HeaderScan HeaderScan;

HeaderScan *scan_alloc(void);
void        scan_free(HeaderScan **hs);

/* Puts the scanner back to before any file, keeping the frame table's
   allocation, which the next file sizes on use and reuses. */
void scan_reset(HeaderScan *hs);

/* Walks the RIFF structure from 'base' without decoding any image data.
   'partial' says more input may still be coming, which lets a short chunk be
   reported as merely not here yet rather than truncated. 'collect_frames'
   asks for the ANMF table, which is the only thing here that allocates. */
WPDStatus scan_headers(HeaderScan *hs, const uint8_t *data, size_t base,
                       size_t size, int partial, int collect_frames);

void scan_info(const HeaderScan *hs, ScanInfo *out);

/* Fills in the 'index'th ANMF header, returning 0 when there is no such frame.
   A frame whose header has arrived but whose payload has not is the last one
   and reports itself incomplete. */
int scan_frame(const HeaderScan *hs, int index, FrameEntry *out);

int  info_valid(const WPDImageInfo *info);
void info_clear(WPDImageInfo *info);

#endif
