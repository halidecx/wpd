#ifndef WPD_CONTAINER_H
#define WPD_CONTAINER_H

#include "wpd_internal.h"

#define VP8X_FLAG_XMP 0x04
#define VP8X_FLAG_EXIF 0x08
#define VP8X_FLAG_ICCP 0x20
#define VP8X_FLAG_ANIMATION 0x02
#define VP8X_FLAG_ALPHA 0x10

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

/* Far above any animation a player would sit through, and low enough that the
   table cannot be made to eat memory by a file that is all ANMF headers. A
   file past it still decodes; the table simply stops growing. */
#define WPD_MAX_FRAMES (1 << 20)

typedef struct HeaderScan {
    size_t   pos;
    uint64_t riff_end;
    size_t   end;
    int      width, height;
    int      has_alpha;
    /* Alpha the image chunks themselves carry, without the VP8X declaration
       folded in, which is what a decoded frame reports. */
    int       image_has_alpha;
    int       animation;
    int       images;
    int       vp8x;
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
    /* The frame table, built only when 'collect_frames' asks for it, so that
       wpd_get_info() keeps its promise not to allocate. 'nb_frames' counts the
       frames whose payload is all here; a frame whose ANMF header has arrived
       but whose payload has not occupies the slot past them and is rebuilt on
       every rescan until it completes. */
    int collect_frames;
    int partial_frame;
    int frames_capped;
    int nb_frames;
    int frames_capacity;
    /* How far the alpha scan has walked into the sub-chunk list starting at
       'anmf_scan_at', so an ANMF arriving in pieces is not re-walked from its
       first sub-chunk on every delivery. The offset is never zero, so a scan
       that has moved on to another ANMF invalidates this by itself. */
    size_t      anmf_scan_at;
    size_t      anmf_scan_pos;
    int         anmf_scan_done;
    int         anmf_scan_alpha;
    FrameEntry *frames;
} HeaderScan;

extern const uint32_t meta_tag[WPD_METADATA_NB];
extern const uint8_t  meta_vp8x_flag[WPD_METADATA_NB];

void scan_free(HeaderScan *hs);

/* Walks the RIFF structure from 'base' without decoding any image data.
   'complete' says the input is the whole file, which lets a short chunk be
   reported as truncated rather than merely not here yet. */
WPDStatus scan_headers(HeaderScan *hs, const uint8_t *data, size_t base,
                       size_t size, int complete);
void      info_from_scan(WPDImageInfo *info, const HeaderScan *hs);

int  info_valid(const WPDImageInfo *info);
void info_clear(WPDImageInfo *info);

#endif
