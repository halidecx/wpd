
#include "input.h"

struct InputBuffer {
    /* Where the buffered bytes start. Equal to 'alloc' unless the caller lent
       its own memory, in which case nothing here owns them. */
    const uint8_t *at;
    uint8_t       *alloc;
    size_t         capacity;
    /* Both are stream offsets: 'size' is how far the stream has been seen,
       'discarded' how much of the front has been dropped. */
    size_t size;
    size_t discarded;
    int    borrowed;
};

/* Below this a compaction moves more bytes than it frees. */
#define COMPACT_THRESHOLD (1 << 16)

InputBuffer *input_alloc(void) { return wpd_mallocz(sizeof(InputBuffer)); }

void input_free(InputBuffer **in) {
    if (*in) {
        free((*in)->alloc);
        free(*in);
        *in = NULL;
    }
}

void input_reset(InputBuffer *in) {
    in->at        = in->alloc;
    in->size      = 0;
    in->discarded = 0;
    in->borrowed  = 0;
}

size_t input_size(const InputBuffer *in) { return in->size; }

size_t input_discarded(const InputBuffer *in) { return in->discarded; }

size_t input_buffered(const InputBuffer *in) {
    return in->size - in->discarded;
}

const uint8_t *input_at(const InputBuffer *in, size_t offset) {
    return in->at + (offset - in->discarded);
}

/* Room for 'size' more bytes past what is buffered, plus the padding every
   kernel is allowed to read past the end. */
static WPDStatus input_reserve(InputBuffer *in, size_t size) {
    const size_t buffered = input_buffered(in);
    const size_t needed   = buffered + size + WPD_FILE_PADDING;
    size_t       capacity;
    uint8_t     *grown;

    if (size > (size_t)INT_MAX - WPD_FILE_PADDING ||
        buffered > (size_t)INT_MAX - WPD_FILE_PADDING - size)
        return WPD_ERR_TOO_LARGE;
    if (in->capacity >= needed)
        return WPD_OK;

    capacity = in->capacity ? in->capacity : COMPACT_THRESHOLD;
    while (capacity < needed) capacity *= 2;
    grown = realloc(in->alloc, capacity);
    if (!grown)
        return WPD_ERR_NO_MEMORY;
    in->alloc    = grown;
    in->at       = grown;
    in->capacity = capacity;
    return WPD_OK;
}

WPDStatus input_own(InputBuffer *in, const uint8_t *data, size_t size) {
    WPDStatus status;

    input_reset(in);
    status = input_reserve(in, size);
    if (status != WPD_OK)
        return status;
    memcpy(in->alloc, data, size);
    memset(in->alloc + size, 0, WPD_FILE_PADDING);
    in->at   = in->alloc;
    in->size = size;
    return WPD_OK;
}

void input_borrow(InputBuffer *in, const uint8_t *data, size_t size) {
    in->at        = data;
    in->size      = size;
    in->discarded = 0;
    in->borrowed  = 1;
}

WPDStatus input_append(InputBuffer *in, const uint8_t *data, size_t size) {
    WPDStatus status = input_reserve(in, size);

    if (status != WPD_OK)
        return status;
    memcpy(in->alloc + input_buffered(in), data, size);
    in->size += size;
    memset(in->alloc + input_buffered(in), 0, WPD_FILE_PADDING);
    return WPD_OK;
}

void input_compact(InputBuffer *in, size_t keep) {
    if (in->borrowed || keep < in->discarded ||
        keep - in->discarded < COMPACT_THRESHOLD)
        return;

    memmove(in->alloc, input_at(in, keep), in->size - keep + WPD_FILE_PADDING);
    in->at        = in->alloc;
    in->discarded = keep;
}
