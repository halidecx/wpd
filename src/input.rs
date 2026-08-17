//! The bytes that have arrived, and the geometry of where they sit.
//!
//! A stream is described by two offsets rather than by pointers: how far it
//! has been seen, and how much of the front has been dropped. Everything the
//! decoder remembers about a position is an offset into the stream, so
//! compaction can move the bytes without invalidating any of it.
//!
//! [`Input`] holds the bytes themselves in one of two ways. A stream is copied
//! into a growing `Vec`; a whole file the caller promises not to touch is
//! borrowed, and the borrow is what the promise becomes. The C had a pointer
//! and a `borrowed` flag, and nothing but the flag said which of the two it
//! was pointing at.

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

/// The buffered part of a stream.
///
/// The lifetime is the caller's file, for the borrowed shape only; a stream
/// that is appended to owns every byte it holds and `'a` never appears in it.
#[derive(Default)]
pub struct Input<'a> {
    /// Kept across a reset, so a decoder reused for a second file does not
    /// give the allocation back and take it again.
    owned: Vec<u8>,
    borrowed: Option<&'a [u8]>,
    window: Window,
}

impl<'a> Input<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the input, keeping the allocation for the next file.
    pub fn reset(&mut self) {
        self.borrowed = None;
        self.window = Window::default();
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn size(&self) -> usize {
        self.window.size
    }

    pub fn discarded(&self) -> usize {
        self.window.discarded
    }

    /// Everything currently held, which starts at stream offset
    /// [`Self::discarded`].
    pub fn bytes(&self) -> &[u8] {
        match self.borrowed {
            Some(data) => data,
            None => &self.owned[..self.window.buffered().min(self.owned.len())],
        }
    }

    /// The stream from `offset` on, or nothing when that byte has been dropped
    /// or has not arrived.
    pub fn at(&self, offset: usize) -> &[u8] {
        match self.window.index_of(offset) {
            Some(i) => &self.bytes()[i.min(self.bytes().len())..],
            None => &[],
        }
    }

    /// The `size` bytes at `offset`, or as many of them as have arrived.
    pub fn chunk(&self, offset: usize, size: usize) -> &[u8] {
        let bytes = self.at(offset);

        &bytes[..size.min(bytes.len())]
    }

    /// Grows the owned buffer to take `size` more bytes, padding included.
    ///
    /// The capacity doubles, so appending a stream a byte at a time stays
    /// linear; the length only reaches what is actually used, because
    /// lengthening a `Vec` zeroes what it adds and the C's `realloc` did not.
    /// Sizing the length to the capacity instead costs a 64 KiB memset on
    /// every decoder, which is most of the work for a small file.
    fn reserve(&mut self, size: usize) -> Result<()> {
        let buffered = self.window.buffered();

        if let Some(grown) = grow_to(self.owned.capacity(), buffered, size)? {
            self.owned
                .try_reserve_exact(grown.saturating_sub(self.owned.len()))
                .map_err(|_| Error::NoMemory)?;
        }
        let needed = buffered + size + FILE_PADDING;

        if self.owned.len() < needed {
            self.owned.resize(needed, 0);
        }
        Ok(())
    }

    /// Takes a copy of a whole file, which is what `wpd_decoder_open` does.
    pub fn own(&mut self, data: &[u8]) -> Result<()> {
        self.borrowed = None;
        self.window = Window::default();
        self.reserve(data.len())?;

        let end = data.len();

        self.owned[..end].copy_from_slice(data);
        self.owned[end..end + FILE_PADDING].fill(0);
        self.window.size = end;
        Ok(())
    }

    /// Points at a whole file the caller keeps, which is the promise
    /// `wpd_decoder_open_borrowed` asks for made into a borrow.
    pub fn borrow(&mut self, data: &'a [u8]) {
        self.borrowed = Some(data);
        self.window = Window {
            size: data.len(),
            discarded: 0,
        };
    }

    pub fn replace_owned(&mut self, data: Vec<u8>) {
        self.borrowed = None;
        self.window = Window {
            size: data.len(),
            discarded: 0,
        };
        self.owned = data;
    }

    pub fn take_owned(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.owned)
    }

    pub fn append(&mut self, data: &[u8]) -> Result<()> {
        self.reserve(data.len())?;

        let at = self.window.buffered();
        let end = at + data.len();

        self.owned[at..end].copy_from_slice(data);
        self.owned[end..end + FILE_PADDING].fill(0);
        self.window.size += data.len();
        Ok(())
    }

    /// Drops everything before `keep`, if there is enough of it to be worth
    /// the move. A borrowed file keeps every byte, since holding them costs
    /// nothing.
    pub fn compact(&mut self, keep: usize) {
        if self.borrowed.is_some() {
            return;
        }
        let Some(moved) = compact(self.window, keep) else {
            return;
        };
        let from = keep - self.window.discarded;

        self.owned.copy_within(from..from + moved, 0);
        self.window.discarded = keep;
    }
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

    #[test]
    fn an_appended_stream_reads_back_as_one_run_of_bytes() {
        let mut input = Input::new();

        input.append(&[1, 2, 3]).unwrap();
        input.append(&[4, 5]).unwrap();
        assert_eq!(input.bytes(), &[1, 2, 3, 4, 5]);
        assert_eq!(input.at(3), &[4, 5]);
        assert_eq!(input.size(), 5);
    }

    /// Compaction moves the bytes; the offsets the decoder remembers must
    /// still name the same ones.
    #[test]
    fn compaction_keeps_offsets_pointing_at_the_same_bytes() {
        let mut input = Input::new();
        let big = vec![0u8; COMPACT_THRESHOLD * 2];

        input.append(&big).unwrap();
        input.append(&[7, 8, 9]).unwrap();

        let at = COMPACT_THRESHOLD * 2;

        assert_eq!(input.at(at), &[7, 8, 9]);
        input.compact(COMPACT_THRESHOLD);
        assert_eq!(input.discarded(), COMPACT_THRESHOLD);
        assert_eq!(input.at(at), &[7, 8, 9]);
    }

    /// A dropped byte has no index, and asking for one is how the decoder
    /// learns it cannot go back.
    #[test]
    fn a_discarded_offset_reads_back_as_nothing() {
        let mut input = Input::new();

        input.append(&vec![0u8; COMPACT_THRESHOLD * 2]).unwrap();
        input.compact(COMPACT_THRESHOLD);
        assert!(input.at(0).is_empty());
        assert!(input.at(COMPACT_THRESHOLD - 1).is_empty());
    }

    /// A borrowed file is never copied and never compacted, so every offset
    /// stays reachable however far the decoder has gone.
    #[test]
    fn a_borrowed_file_keeps_every_byte() {
        let file = vec![9u8; COMPACT_THRESHOLD * 2];
        let mut input = Input::new();

        input.borrow(&file);
        input.compact(COMPACT_THRESHOLD);
        assert_eq!(input.discarded(), 0);
        assert_eq!(input.bytes().len(), file.len());
    }

    /// Reset keeps the allocation but forgets the stream, which is what
    /// reopening a decoder does.
    #[test]
    fn a_reset_forgets_the_stream_but_not_the_allocation() {
        let mut input = Input::new();

        input.append(&[1, 2, 3]).unwrap();
        input.reset();
        assert_eq!(input.size(), 0);
        assert!(input.bytes().is_empty());
        input.own(&[4, 5]).unwrap();
        assert_eq!(input.bytes(), &[4, 5]);
    }
}
