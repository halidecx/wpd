#ifndef WPD_VP8L_H
#define WPD_VP8L_H

#include "image.h"

#define VP8L_NEED_MORE 1

/* The lossless decoder. Everything it tracks — the bit reader, the transform
   list, the prefix codes and the images it decodes into — lives behind this
   pointer; the container only ever sees finished pictures. */
typedef struct VP8LContext VP8LContext;

/* Which of the module's output images a decode fills in. Both are kept across
   calls and resized on use, so a lossy animation with alpha alternates between
   them without reallocating either. */
enum VP8LTarget {
    VP8L_TARGET_ARGB,
    VP8L_TARGET_ALPHA,
};

VP8LContext *vp8l_alloc(void);
void         vp8l_free(VP8LContext **ctx);

/* Drops everything derived from a file, keeping the decode buffers, which are
   sized on use and reused. vp8l_release() gives those back too, for when the
   file is closed and nothing is looking at the pictures any more. */
void vp8l_reset(VP8LContext *ctx);
void vp8l_release(VP8LContext *ctx);

/* The canvas the container has already committed to, and what the module made
   of it: a lossless frame header carries its own dimensions, an alpha chunk
   does not, and the two have to agree. */
void vp8l_set_canvas(VP8LContext *ctx, int width, int height);
int  vp8l_width(const VP8LContext *ctx);
int  vp8l_height(const VP8LContext *ctx);
int  vp8l_has_alpha(const VP8LContext *ctx);

/* Where an ALPH chunk's alpha goes when the module can write it straight out,
   and whether it did. When it did not, the caller extracts green itself. */
void vp8l_set_alpha_dst(VP8LContext *ctx, uint8_t *dst, int stride);
int  vp8l_alpha_dst_used(const VP8LContext *ctx);

/* Decodes a whole frame in one call. 'out' is filled in with a view of memory
   the context owns, valid until the next decode into the same target, and must
   not be freed. */
int vp8l_decode_frame(VP8LContext *ctx, enum VP8LTarget target, WebPImage *out,
                      const uint8_t *data, unsigned size, int is_alpha_chunk);

/* The resumable still-image path. _step consumes as much of the payload as has
   arrived, returning 1 once the whole image is out and 0 while more is needed;
   _peek switches it over to handing rows out as they finish, which needs
   somewhere to put them because backward references keep reading the
   untransformed pixels for as long as the image is being decoded. */
int vp8l_still_step(VP8LContext *ctx, const uint8_t *payload, unsigned avail,
                    unsigned size, int complete);
int vp8l_still_peek(VP8LContext *ctx);
int vp8l_still_active(const VP8LContext *ctx);
int vp8l_still_rows_out(const VP8LContext *ctx);

/* A view of the image the still path is filling in, on the same terms as the
   one vp8l_decode_frame() hands back. */
void vp8l_still_frame(const VP8LContext *ctx, WebPImage *out);

#endif
