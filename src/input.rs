use crate::error::{Error, Result};
use crate::image::FILE_PADDING;

pub const COMPACT_THRESHOLD: usize = 1 << 16;

pub const INITIAL_CAPACITY: usize = 1 << 16;

pub const MAX_BUFFERED: usize = i32::MAX as usize - FILE_PADDING;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Window {
    pub size: usize,
    pub discarded: usize,
}

impl Window {
    pub fn buffered(&self) -> usize {
        self.size.saturating_sub(self.discarded)
    }

    pub fn index_of(&self, offset: usize) -> Option<usize> {
        if offset < self.discarded || offset > self.size {
            return None;
        }
        Some(offset - self.discarded)
    }
}

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

pub fn compact(window: Window, keep: usize) -> Option<usize> {
    if keep < window.discarded || keep - window.discarded < COMPACT_THRESHOLD {
        return None;
    }
    Some(window.size - keep + FILE_PADDING)
}

#[derive(Default)]
pub struct Input<'a> {
    owned: Vec<u8>,
    borrowed: Option<&'a [u8]>,
    window: Window,
}

impl<'a> Input<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.borrowed = None;
        self.window = Window::default();
    }

    pub fn size(&self) -> usize {
        self.window.size
    }

    pub fn discarded(&self) -> usize {
        self.window.discarded
    }

    pub fn bytes(&self) -> &[u8] {
        match self.borrowed {
            Some(data) => data,
            None => &self.owned[..self.window.buffered().min(self.owned.len())],
        }
    }

    pub fn at(&self, offset: usize) -> &[u8] {
        match self.window.index_of(offset) {
            Some(i) => &self.bytes()[i.min(self.bytes().len())..],
            None => &[],
        }
    }

    pub fn chunk(&self, offset: usize, size: usize) -> &[u8] {
        let bytes = self.at(offset);

        &bytes[..size.min(bytes.len())]
    }

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

    #[test]
    fn a_discarded_offset_reads_back_as_nothing() {
        let mut input = Input::new();

        input.append(&vec![0u8; COMPACT_THRESHOLD * 2]).unwrap();
        input.compact(COMPACT_THRESHOLD);
        assert!(input.at(0).is_empty());
        assert!(input.at(COMPACT_THRESHOLD - 1).is_empty());
    }

    #[test]
    fn a_borrowed_file_keeps_every_byte() {
        let file = vec![9u8; COMPACT_THRESHOLD * 2];
        let mut input = Input::new();

        input.borrow(&file);
        input.compact(COMPACT_THRESHOLD);
        assert_eq!(input.discarded(), 0);
        assert_eq!(input.bytes().len(), file.len());
    }

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
