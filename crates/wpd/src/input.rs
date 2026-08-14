//! The geometry of the bytes that have arrived.
//!
//! A stream is described by two offsets rather than by pointers: how far it
//! has been seen, and how much of the front has been dropped. Everything the
//! decoder remembers about a position is an offset into the stream, so
//! compaction can move the bytes without invalidating any of it.
//!
//! What is here is the arithmetic a caller drives: how far the buffer has to
//! grow for the next append, and whether dropping the front is worth the move.
//! The memory itself is the shim's, because a borrowed buffer is the caller's.

use crate::error::{Error, Result};
use crate::image::FILE_PADDING;

/// Below this, a compaction moves more bytes than it frees.
pub const COMPACT_THRESHOLD: usize = 1 << 16;

/// The first capacity a growing buffer takes, and the step it doubles from.
pub const INITIAL_CAPACITY: usize = 1 << 16;

/// The most a buffer may hold. The decoder hands chunk sizes to the codecs as
/// `int`, so a stream that grew past this could not be described to them.
pub const MAX_BUFFERED: usize = i32::MAX as usize - FILE_PADDING;

/// Where a stream has got to, in stream offsets.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Window {
    /// How far the stream has been seen.
    pub size: usize,
    /// How much of the front has been dropped.
    pub discarded: usize,
}

impl Window {
    pub fn buffered(&self) -> usize {
        self.size.saturating_sub(self.discarded)
    }

    /// Where `offset` lands in the buffer, or `None` when it names a byte that
    /// has been dropped or has not arrived.
    pub fn index_of(&self, offset: usize) -> Option<usize> {
        if offset < self.discarded || offset > self.size {
            return None;
        }
        Some(offset - self.discarded)
    }
}

/// The capacity a buffer needs to take `size` more bytes, or `None` when what
/// it has is already enough.
///
/// The padding every kernel is allowed to read past the end is counted in, and
/// the growth doubles so that appending a stream byte by byte is still linear.
pub fn grow_to(capacity: usize, buffered: usize, size: usize) -> Result<Option<usize>> {
    if size > MAX_BUFFERED || buffered > MAX_BUFFERED - size {
        return Err(Error::TooLarge);
    }
    let needed = buffered + size + FILE_PADDING;

    if capacity >= needed {
        return Ok(None);
    }
    let mut grown = if capacity == 0 {
        INITIAL_CAPACITY
    } else {
        capacity
    };

    while grown < needed {
        grown = grown.checked_mul(2).ok_or(Error::TooLarge)?;
    }
    Ok(Some(grown))
}

/// Whether dropping everything before `keep` is worth the move, and how many
/// bytes would have to be moved if it is.
///
/// A `keep` behind what has already been dropped is not an error: the decoder
/// asks to keep the chunk it is working on, and a compaction may already have
/// stopped short of it.
pub fn compact(window: Window, keep: usize) -> Option<usize> {
    if keep < window.discarded || keep - window.discarded < COMPACT_THRESHOLD {
        return None;
    }
    Some(window.size - keep + FILE_PADDING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_that_already_fits_does_not_grow() {
        assert_eq!(grow_to(1 << 20, 100, 100), Ok(None));
    }

    #[test]
    fn growth_doubles_from_the_initial_capacity() {
        assert_eq!(grow_to(0, 0, 1), Ok(Some(INITIAL_CAPACITY)));
        assert_eq!(
            grow_to(0, 0, INITIAL_CAPACITY),
            Ok(Some(INITIAL_CAPACITY * 2))
        );
        assert_eq!(
            grow_to(INITIAL_CAPACITY, INITIAL_CAPACITY, 1),
            Ok(Some(INITIAL_CAPACITY * 2))
        );
    }

    /// Appending one byte at a time must not be quadratic: every growth at
    /// least doubles, so the number of them is logarithmic in the total.
    #[test]
    fn appending_a_byte_at_a_time_grows_a_logarithmic_number_of_times() {
        let mut capacity = 0;
        let mut grows = 0;

        for buffered in 0..(1 << 20) {
            if let Ok(Some(next)) = grow_to(capacity, buffered, 1) {
                assert!(next >= capacity * 2 || capacity == 0);
                capacity = next;
                grows += 1;
            }
        }
        assert!(grows < 8, "grew {grows} times");
    }

    #[test]
    fn a_stream_past_what_an_int_can_describe_is_refused() {
        assert_eq!(grow_to(0, 0, MAX_BUFFERED + 1), Err(Error::TooLarge));
        assert_eq!(grow_to(0, MAX_BUFFERED, 1), Err(Error::TooLarge));
        assert!(grow_to(0, 0, MAX_BUFFERED).is_ok());
    }

    #[test]
    fn compaction_below_the_threshold_is_not_worth_the_move() {
        let w = Window {
            size: 1 << 20,
            discarded: 0,
        };

        assert_eq!(compact(w, COMPACT_THRESHOLD - 1), None);
        assert_eq!(
            compact(w, COMPACT_THRESHOLD),
            Some((1 << 20) - COMPACT_THRESHOLD + FILE_PADDING)
        );
    }

    /// The decoder asks to keep the chunk it is working on, which an earlier
    /// compaction may already have stopped short of.
    #[test]
    fn a_keep_behind_what_was_already_dropped_is_declined_not_wrapped() {
        let w = Window {
            size: 1 << 20,
            discarded: 1 << 19,
        };

        assert_eq!(compact(w, 0), None);
        assert_eq!(compact(w, (1 << 19) - 1), None);
    }

    #[test]
    fn an_offset_outside_the_window_has_no_index() {
        let w = Window {
            size: 100,
            discarded: 40,
        };

        assert_eq!(w.index_of(40), Some(0));
        assert_eq!(w.index_of(100), Some(60));
        assert_eq!(w.index_of(39), None);
        assert_eq!(w.index_of(101), None);
        assert_eq!(w.buffered(), 60);
    }
}
