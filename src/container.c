
#include "container.h"

/* The order of the WPDMetadata bits, so a bit indexes these tables. */
const uint32_t meta_tag[WPD_METADATA_NB] = {
    MKTAG('I', 'C', 'C', 'P'),
    MKTAG('E', 'X', 'I', 'F'),
    MKTAG('X', 'M', 'P', ' '),
};

const uint8_t meta_vp8x_flag[WPD_METADATA_NB] = {
    VP8X_FLAG_ICCP,
    VP8X_FLAG_EXIF,
    VP8X_FLAG_XMP,
};

void scan_free(HeaderScan *hs) {
    wpd_free(hs->frames);
    hs->frames          = NULL;
    hs->frames_capacity = 0;
    hs->nb_frames       = 0;
    hs->partial_frame   = 0;
    hs->frames_capped   = 0;
    hs->collect_frames  = 0;
}

int info_valid(const WPDImageInfo *info) {
    return info && info->struct_size >= WPD_FIELD_END(WPDImageInfo, metadata);
}

void info_clear(WPDImageInfo *info) {
    const size_t struct_size = info->struct_size;

    memset((uint8_t *)info + sizeof(info->struct_size),
           0,
           WPD_FIELD_END(WPDImageInfo, metadata) - sizeof(info->struct_size));
    info->struct_size = struct_size;
}

static void scan_still_header(HeaderScan *hs, uint32_t tag, const uint8_t *p,
                              size_t avail, size_t size) {
    if (tag == MKTAG('V', 'P', '8', 'L')) {
        hs->coding = WPD_CODING_LOSSLESS;
        if (avail >= 5 && p[0] == 0x2f) {
            uint32_t bits = WPD_RL32(p + 1);

            if (bits >> 29)
                return;
            hs->width  = (bits & 0x3fff) + 1;
            hs->height = ((bits >> 14) & 0x3fff) + 1;
            hs->image_has_alpha |= (bits >> 28) & 1;
            hs->has_alpha |= (bits >> 28) & 1;
        }
    } else {
        hs->coding = WPD_CODING_LOSSY;
        if (avail >= 10 && size >= 10 && p[3] == 0x9d && p[4] == 0x01 &&
            p[5] == 0x2a) {
            uint32_t bits = WPD_RL24(p);

            if ((bits & 1) || ((bits >> 1) & 7) > 3 || !(bits & 0x10) ||
                (bits >> 5) > size - 10)
                return;
            hs->width  = WPD_RL16(p + 6) & 0x3fff;
            hs->height = WPD_RL16(p + 8) & 0x3fff;
        }
    }
}

/* Walks an ANMF's sub-chunks for the one WebPIterator field the 16-byte ANMF
   header does not carry. Stops at the image chunk either way, and simply
   leaves has_alpha alone when the payload has not all arrived. Resumes where
   the last delivery of the same ANMF left off, so a frame that arrives in many
   pieces costs one walk of its sub-chunks in total rather than one per piece;
   only a sub-chunk stepped over whole advances that mark. */
static void scan_anmf_alpha(HeaderScan *hs, FrameEntry *entry, const uint8_t *p,
                            size_t size) {
    const uint8_t *const start = p;
    const uint8_t *const end   = p + size;
    const size_t         at    = hs->pos + 24;

    if (hs->anmf_scan_at != at || hs->anmf_scan_pos > size) {
        hs->anmf_scan_at    = at;
        hs->anmf_scan_pos   = 0;
        hs->anmf_scan_done  = 0;
        hs->anmf_scan_alpha = 0;
    }
    if (hs->anmf_scan_done) {
        entry->has_alpha = hs->anmf_scan_alpha;
        return;
    }

    p += hs->anmf_scan_pos;
    while (end - p >= 8) {
        const uint32_t tag   = WPD_RL32(p);
        const uint32_t size_ = WPD_RL32(p + 4);
        uint32_t       padded;

        if (size_ == UINT32_MAX)
            return;
        padded = size_ + (size_ & 1);
        p += 8;
        if ((size_t)(end - p) < padded)
            return;
        if (tag == MKTAG('A', 'L', 'P', 'H')) {
            hs->anmf_scan_alpha = 1;
        } else if (tag == MKTAG('V', 'P', '8', 'L')) {
            if (size_ >= 5 && p[0] == 0x2f)
                hs->anmf_scan_alpha = WPD_RL32(p + 1) >> 28 & 1;
        } else if (tag != MKTAG('V', 'P', '8', ' ')) {
            p += padded;
            hs->anmf_scan_pos = (size_t)(p - start);
            continue;
        }
        hs->anmf_scan_done = 1;
        entry->has_alpha   = hs->anmf_scan_alpha;
        return;
    }
}

/* Records the ANMF at 'p', of which 'avail' bytes are buffered. 'complete'
   says whether the scan is stepping past the whole padded chunk, which is the
   only thing that may promote the entry: a frame still arriving takes the slot
   past the complete ones and is rewritten by the next scan, so the table never
   double-counts. Deriving it from 'avail' instead would count an odd-sized
   chunk twice, once when every byte but its pad has landed and again once the
   scan finally walks over it. */
static WPDStatus scan_anmf(HeaderScan *hs, const uint8_t *p, size_t avail,
                           int complete) {
    FrameEntry *entry;

    if (avail < 16)
        return WPD_OK;
    if (hs->nb_frames >= WPD_MAX_FRAMES) {
        if (!hs->frames_capped)
            wpd_log(NULL,
                    WPD_LOG_WARNING,
                    "frame table capped at %d entries\n",
                    WPD_MAX_FRAMES);
        hs->frames_capped = 1;
        return WPD_OK;
    }
    if (hs->nb_frames == hs->frames_capacity) {
        const int capacity = hs->frames_capacity ? hs->frames_capacity * 2 : 16;
        FrameEntry *grown  = wpd_realloc(hs->frames,
                                         (size_t)capacity * sizeof(*grown));

        if (!grown)
            return WPD_ERR_NO_MEMORY;
        hs->frames          = grown;
        hs->frames_capacity = capacity;
    }

    entry = &hs->frames[hs->nb_frames];
    memset(entry, 0, sizeof(*entry));
    entry->pos_x    = WPD_RL24(p) * 2;
    entry->pos_y    = WPD_RL24(p + 3) * 2;
    entry->width    = WPD_RL24(p + 6) + 1;
    entry->height   = WPD_RL24(p + 9) + 1;
    entry->duration = WPD_RL24(p + 12);
    entry->dispose  = p[15] & ANMF_FLAG_DISPOSE ? WPD_DISPOSE_BACKGROUND
                                                : WPD_DISPOSE_NONE;
    entry->blend    = p[15] & ANMF_FLAG_NO_BLEND ? WPD_BLEND_NONE
                                                 : WPD_BLEND_ALPHA;
    entry->complete = complete;
    scan_anmf_alpha(hs, entry, p + 16, avail - 16);
    if (entry->complete)
        hs->nb_frames++;
    else
        hs->partial_frame = 1;
    return WPD_OK;
}

static WPDStatus scan_raw_headers(HeaderScan *hs, const uint8_t *data,
                                  size_t size, int partial) {
    uint32_t tag;

    hs->truncated = 0;
    if (!size)
        return WPD_ERR_TRUNCATED;
    if (data[0] == 0x2f) {
        hs->raw_kind         = 1;
        hs->raw_image_offset = 0;
        hs->raw_image_size   = size;
        if (size < 5)
            return WPD_ERR_TRUNCATED;
        scan_still_header(hs, MKTAG('V', 'P', '8', 'L'), data, size, size);
    } else if (size >= 6 && data[3] == 0x9d && data[4] == 0x01 &&
               data[5] == 0x2a) {
        /* A bare stream declares no payload length, so until the caller says
           the stream has ended the keyframe header's own first partition is
           the only length to measure it against. */
        size_t payload;

        hs->raw_kind         = 2;
        hs->raw_image_offset = 0;
        hs->raw_image_size   = size;
        if (size < 10)
            return WPD_ERR_TRUNCATED;
        payload = 10 + (size_t)(WPD_RL24(data) >> 5);
        if (!partial || payload < size)
            payload = size;
        scan_still_header(hs, MKTAG('V', 'P', '8', ' '), data, size, payload);
        if (hs->width && payload > size)
            hs->truncated = 1;
    } else if (size >= 4 && WPD_RL32(data) == MKTAG('A', 'L', 'P', 'H')) {
        uint32_t alpha_size, image_size;
        uint64_t padded;
        size_t   image_header, have;

        hs->raw_kind = 3;
        if (size < 8)
            return WPD_ERR_TRUNCATED;
        alpha_size = WPD_RL32(data + 4);
        if (alpha_size == UINT32_MAX)
            return WPD_ERR_BITSTREAM;
        padded = (uint64_t)alpha_size + (alpha_size & 1);
        if (padded > (uint64_t)(size - 8) || size - 8 - padded < 8)
            return WPD_ERR_TRUNCATED;
        image_header = 8 + (size_t)padded;
        tag          = WPD_RL32(data + image_header);
        if (tag != MKTAG('V', 'P', '8', ' '))
            return WPD_ERR_BITSTREAM;
        image_size = WPD_RL32(data + image_header + 4);
        have       = image_size;
        if ((size_t)image_size > size - image_header - 8) {
            hs->truncated = 1;
            if (!partial)
                return WPD_ERR_TRUNCATED;
            have = size - image_header - 8;
        }
        hs->raw_alpha_offset = 8;
        hs->raw_alpha_size   = alpha_size;
        hs->raw_image_offset = image_header + 8;
        hs->raw_image_size   = have;
        hs->has_alpha = hs->image_has_alpha = 1;
        if (have < 10)
            return WPD_ERR_TRUNCATED;
        scan_still_header(
            hs, tag, data + hs->raw_image_offset, have, image_size);
    } else {
        return size < 12 && partial ? WPD_ERR_TRUNCATED : WPD_ERR_NOT_WEBP;
    }
    hs->frame_count = 1;
    hs->images      = 1;
    hs->end         = size;
    return hs->width && hs->height ? WPD_OK : WPD_ERR_BITSTREAM;
}

/* Walks the chunk list without decoding anything, so it is safe to run on the
   caller's memory before the file is copied. Resumes from where it stopped
   last time, so feeding a stream one piece at a time stays linear; 'base' is
   the stream offset the buffer now starts at, once earlier bytes have been
   dropped. */
WPDStatus scan_headers(HeaderScan *hs, const uint8_t *data, size_t base,
                       size_t size, int partial) {
    int partial_still = 0;

    hs->truncated     = 0;
    hs->partial_frame = 0;

    if (!hs->pos) {
        if (size < 12 && size >= 4 &&
            WPD_RL32(data) == MKTAG('R', 'I', 'F', 'F'))
            return WPD_ERR_TRUNCATED;
        if (size < 12 || WPD_RL32(data) != MKTAG('R', 'I', 'F', 'F') ||
            WPD_RL32(data + 8) != MKTAG('W', 'E', 'B', 'P'))
            return scan_raw_headers(hs, data, size, partial);
        hs->riff_end = (uint64_t)WPD_RL32(data + 4) + 8;
        hs->pos      = 12;
    }

    hs->end = size;
    if (hs->riff_end < (uint64_t)size)
        hs->end = (size_t)hs->riff_end;
    else if (hs->riff_end > (uint64_t)size)
        hs->truncated = 1;

    while (hs->pos + 8 <= hs->end) {
        const uint8_t *chunk = data + (hs->pos - base);
        uint32_t       tag   = WPD_RL32(chunk);
        uint32_t       size_ = WPD_RL32(chunk + 4);
        uint32_t       padded_size;

        if (size_ == UINT32_MAX) {
            hs->truncated = 1;
            break;
        }
        padded_size = size_ + (size_ & 1);
        if (hs->end - (hs->pos + 8) < padded_size) {
            hs->truncated = 1;
            if (hs->collect_frames && tag == MKTAG('A', 'N', 'M', 'F')) {
                const WPDStatus status = scan_anmf(
                    hs, chunk + 8, hs->end - (hs->pos + 8), 0);

                if (status != WPD_OK)
                    return status;
            }
            if (partial && !hs->images &&
                (tag == MKTAG('V', 'P', '8', ' ') ||
                 tag == MKTAG('V', 'P', '8', 'L'))) {
                const int width = hs->width, height = hs->height;

                partial_still = 1;
                scan_still_header(
                    hs, tag, chunk + 8, hs->end - (hs->pos + 8), size_);
                if (hs->vp8x && width && height) {
                    hs->width  = width;
                    hs->height = height;
                }
            }
            break;
        }

        switch (tag) {
        case MKTAG('V', 'P', '8', 'X'):
            hs->vp8x = 1;
            if (size_ >= 10) {
                hs->has_alpha |= (chunk[8] & VP8X_FLAG_ALPHA) != 0;
                for (int i = 0; i < WPD_METADATA_NB; i++)
                    if (chunk[8] & meta_vp8x_flag[i])
                        hs->metadata |= 1 << i;
                hs->width  = WPD_RL24(chunk + 12) + 1;
                hs->height = WPD_RL24(chunk + 15) + 1;
                if ((uint64_t)hs->width * (uint64_t)hs->height >= 1ULL << 32)
                    return WPD_ERR_TOO_LARGE;
            }
            break;
        case MKTAG('A', 'L', 'P', 'H'):
            hs->has_alpha = hs->image_has_alpha = 1;
            break;
        case MKTAG('A', 'N', 'I', 'M'):
            hs->animation = 1;
            if (size_ >= 6) {
                hs->background_argb = WPD_RL32(chunk + 8);
                hs->loop_count      = WPD_RL16(chunk + 12);
            }
            break;
        case MKTAG('A', 'N', 'M', 'F'):
            hs->frame_count++;
            if (hs->collect_frames) {
                const WPDStatus status = scan_anmf(hs, chunk + 8, size_, 1);

                if (status != WPD_OK)
                    return status;
            }
            break;
        case MKTAG('V', 'P', '8', ' '):
        case MKTAG('V', 'P', '8', 'L'):
            if (!hs->images++) {
                int width = hs->width, height = hs->height;

                scan_still_header(hs, tag, chunk + 8, size_, size_);
                if (hs->vp8x && width && height) {
                    hs->width  = width;
                    hs->height = height;
                }
            }
            break;
        default:
            for (int i = 0; i < WPD_METADATA_NB; i++) {
                if (tag != meta_tag[i])
                    continue;
                hs->metadata |= 1 << i;
                if (!hs->meta_offset[i] && size_) {
                    hs->meta_offset[i] = hs->pos + 8;
                    hs->meta_size[i]   = size_;
                }
            }
            break;
        }
        hs->pos += 8 + padded_size;
    }

    /* An animation may mix lossy and lossless frames, which libwebp reports as
       an undefined coding; only the first still's coding is meaningful. */
    if (hs->animation)
        hs->coding = WPD_CODING_UNKNOWN;
    else
        hs->frame_count = hs->images || partial_still ? 1 : 0;

    if (!hs->width || !hs->height)
        return hs->truncated ? WPD_ERR_TRUNCATED : WPD_ERR_BITSTREAM;
    return WPD_OK;
}

void info_from_scan(WPDImageInfo *info, const HeaderScan *hs) {
    info->width           = hs->width;
    info->height          = hs->height;
    info->has_alpha       = hs->has_alpha;
    info->is_animation    = hs->animation;
    info->frame_count     = hs->frame_count;
    info->loop_count      = hs->loop_count;
    info->background_argb = hs->background_argb;
    info->coding          = hs->coding;
    info->metadata        = hs->metadata;
}

WPDStatus wpd_get_info(const uint8_t *data, size_t size, WPDImageInfo *info) {
    HeaderScan hs;
    WPDStatus  status;

    if (!data || !info_valid(info))
        return WPD_ERR_INVALID_ARG;

    info_clear(info);
    /* collect_frames stays clear, so this allocates nothing, as documented. */
    memset(&hs, 0, sizeof(hs));
    status = scan_headers(&hs, data, 0, size, 1);
    if (status == WPD_OK)
        info_from_scan(info, &hs);
    scan_free(&hs);
    return status;
}
