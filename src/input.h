#ifndef WPD_INPUT_H
#define WPD_INPUT_H

#include "wpd_internal.h"

/* The bytes of the file that have arrived, however they arrived.
 *
 * Three ways in: a whole file copied in, a whole file borrowed from the
 * caller, or a stream appended to a chunk at a time. Only the last owns a
 * growing allocation, and only it ever drops bytes off the front, so every
 * position the decoder remembers is an offset into the stream rather than a
 * pointer that compaction would move. */
typedef struct InputBuffer InputBuffer;

InputBuffer *input_alloc(void);
void         input_free(InputBuffer **in);
/* Forgets the input, keeping the allocation for the next file. */
void input_reset(InputBuffer *in);

/* Copies the whole file in. */
WPDStatus input_own(InputBuffer *in, const uint8_t *data, size_t size);
/* Points at the caller's memory rather than copying it. The caller must keep
   it alive and unmoved until the next call that replaces it. */
void input_borrow(InputBuffer *in, const uint8_t *data, size_t size);
/* Appends to a stream, growing the buffer as needed. */
WPDStatus input_append(InputBuffer *in, const uint8_t *data, size_t size);
/* Drops everything before 'keep', if there is enough of it to be worth the
   move. A borrowed buffer keeps every byte, since it costs nothing. */
void input_compact(InputBuffer *in, size_t keep);

size_t         input_size(const InputBuffer *in);
size_t         input_discarded(const InputBuffer *in);
size_t         input_buffered(const InputBuffer *in);
const uint8_t *input_at(const InputBuffer *in, size_t offset);

#endif
