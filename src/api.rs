//! The safe Rust API.
//!
//! Every entry point `include/wpd.h` declares is reachable from here without
//! writing `unsafe`, and two things the C ABI cannot say are said in the type
//! system instead:
//!
//! - **A picture borrows the decoder that produced it.** `wpd_decoder_next_frame`
//!   hands out pointers into memory the next call may reuse; [`Picture`] holds
//!   the borrow, so asking for the next frame while the previous one is still
//!   alive does not compile.
//! - **Opening without a copy borrows the input.** `wpd_decoder_open_borrowed`
//!   promises the caller keeps the bytes alive for the decoder's whole life;
//!   [`Decoder::open_borrowed`] makes that a lifetime.
//!
//! A picture is a [`Handout`] and nothing else, so a row is a slice the
//! compiler has bounded rather than a pointer and a stride that may run
//! backwards. That is the whole difference between this and the C ABI: the
//! negative stride the header promises exists only where a `WPDFrame` is
//! built, and a flip here is the order the rows come out in.

use std::marker::PhantomData;

use crate::driver;
use crate::handout::Handout;
use crate::image::Format;
use crate::picture::Frame;

pub use crate::container::Coding;
pub use crate::error::{Error, Result};
pub use crate::info::{FrameInfo, ImageInfo};
pub use crate::options::Options;

/// What an animation hands out: the composited canvas, or each sub-frame on
/// its own at its own position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animation {
    #[default]
    Composited,
    Subframe,
}

/// Which metadata chunk to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Iccp,
    Exif,
    Xmp,
}

/// One decoded picture, borrowed from the decoder that produced it.
///
/// The next decode reuses this memory, which is why the borrow is held: the
/// C ABI hands out the same pointers and can only ask the caller to be
/// careful.
pub struct Picture<'a> {
    out: Handout<'a>,
}

impl<'a> Picture<'a> {
    /// The pixels, or nothing when the decode produced no picture, which is
    /// what a partial frame with no finished rows has.
    fn pixels(&self) -> Option<&Frame<'a>> {
        self.out.frame()
    }

    pub fn width(&self) -> i32 {
        self.out.width
    }

    pub fn height(&self) -> i32 {
        self.out.height
    }

    pub fn format(&self) -> Format {
        self.out.format
    }

    /// How long this frame is shown, in milliseconds, or zero for a still.
    pub fn duration(&self) -> i32 {
        self.out.duration
    }

    /// When this frame is shown, in milliseconds from the start.
    pub fn timestamp(&self) -> i64 {
        self.out.timestamp
    }

    /// Where a sub-frame lands on the canvas, in [`Animation::Subframe`] mode.
    pub fn position(&self) -> (i32, i32) {
        (self.out.pos_x, self.out.pos_y)
    }

    pub fn has_alpha(&self) -> bool {
        self.out.has_alpha
    }

    /// Whether the canvas is cleared to the background colour behind this
    /// frame before the next one is drawn.
    pub fn dispose_to_background(&self) -> bool {
        self.out.dispose_to_background
    }

    /// Whether this frame is alpha-blended over what is already there, as
    /// opposed to replacing it.
    pub fn blend(&self) -> bool {
        self.out.blend
    }

    /// How many planes this picture's format hands out.
    pub fn planes(&self) -> usize {
        self.out.planes()
    }

    /// How many rows `plane` has, which the chroma planes halve.
    pub fn rows(&self, plane: usize) -> i32 {
        self.pixels().map_or(0, |img| img.rows(plane))
    }

    /// Row `y` of `plane`, top row first however the picture is stored.
    ///
    /// # Panics
    ///
    /// If `plane` or `y` is outside the picture.
    pub fn row(&self, plane: usize, y: i32) -> &'a [u8] {
        assert!(plane < self.planes(), "no such plane");

        let img = self.pixels().expect("no pixels");

        assert!(y >= 0 && y < img.rows(plane), "no such row");
        img.row(plane, y)
    }

    /// Every row of `plane`, in order.
    pub fn rows_of(&self, plane: usize) -> impl Iterator<Item = &'a [u8]> + '_ {
        (0..self.rows(plane)).map(move |y| self.row(plane, y))
    }
}

/// A WebP decoder.
///
/// The lifetime is the input's: it is `'static` for a decoder that owns its
/// bytes, and the input's own for one opened with [`Decoder::open_borrowed`].
pub struct Decoder<'a> {
    inner: Box<driver::Decoder<'a>>,
    /// What a decode that failed while it was producing a picture was doing.
    ///
    /// The decoder cannot write this down itself: the picture it was asked
    /// for borrows it, and the borrow outlives the call whether or not the
    /// call succeeded. So the two entry points that hand out a picture keep
    /// their own account, and [`Decoder::error`] prefers it.
    failed: Option<String>,
    input: PhantomData<&'a [u8]>,
}

impl Default for Decoder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Decoder<'a> {
    pub fn new() -> Self {
        crate::cpu::init();

        Decoder {
            inner: Box::new(driver::Decoder::new()),
            failed: None,
            input: PhantomData,
        }
    }

    /// The decoder, with anything this side was keeping about the last
    /// failure cleared: from here on its own account is the current one.
    ///
    /// Reaching the decoder through this is what keeps the two from
    /// disagreeing about which failure was last.
    fn driver(&mut self) -> &mut driver::Decoder<'a> {
        self.failed = None;
        &mut self.inner
    }

    /// The format frames come out in. Leaving it unset hands out whatever the
    /// file codes natively, which is planar for a lossy frame and ARGB for a
    /// lossless one.
    pub fn set_format(&mut self, format: Format) -> Result<()> {
        self.driver().set_output_format(format as i32)
    }

    pub fn set_animation(&mut self, mode: Animation) -> Result<()> {
        self.driver().set_animation_mode(match mode {
            Animation::Composited => driver::ANIM_COMPOSITED,
            Animation::Subframe => driver::ANIM_SUBFRAME,
        })
    }

    pub fn set_options(&mut self, options: Options) -> Result<()> {
        self.driver().set_core_options(options)
    }

    /// Opens a file the decoder copies, so nothing has to outlive the call.
    pub fn open(&mut self, data: &[u8]) -> Result<()> {
        self.driver().open(data)
    }

    /// Opens a file the decoder reads in place. The bytes must outlive the
    /// decoder, which is what the lifetime says.
    pub fn open_borrowed(&mut self, data: &'a [u8]) -> Result<()> {
        self.driver().open_borrowed(data)
    }

    /// Starts a stream the caller appends to as bytes arrive.
    pub fn open_stream(&mut self) -> Result<()> {
        self.driver().open_stream()
    }

    pub fn append(&mut self, chunk: &[u8]) -> Result<()> {
        self.driver().append(chunk)
    }

    /// Replaces the stream's contents with a longer prefix of the same file,
    /// which is what a caller reading into a growing buffer has.
    pub fn update(&mut self, data: &'a [u8]) -> Result<()> {
        self.driver().update(data)
    }

    pub fn end_of_stream(&mut self) -> Result<()> {
        self.driver().end_of_stream()
    }

    pub fn info(&mut self) -> Result<ImageInfo> {
        self.driver().image_info()
    }

    pub fn frame_info(&mut self, index: i32) -> Result<FrameInfo> {
        self.driver().frame_entry(index)
    }

    /// The named metadata chunk, or none when the file carries no such chunk.
    pub fn metadata(&mut self, kind: Metadata) -> Option<&[u8]> {
        let kind = match kind {
            Metadata::Iccp => 1,
            Metadata::Exif => 2,
            Metadata::Xmp => 4,
        };

        self.driver().metadata(kind).ok().flatten()
    }

    /// Returns to the first frame. A stream that was appended to cannot be
    /// rewound, because the bytes it has read are gone.
    pub fn rewind(&mut self) -> Result<()> {
        self.driver().rewind()
    }

    /// The next frame, or none when the file is finished or the stream has
    /// not buffered enough of it yet.
    ///
    /// The picture borrows the decoder: the next call reuses its memory.
    pub fn next_frame(&mut self) -> Result<Option<Picture<'_>>> {
        let mut out = Handout::default();

        self.failed = None;
        match self.inner.next_picture(&mut out) {
            Ok(false) => Ok(None),
            Ok(true) => Ok(Some(Picture { out })),
            Err(failure) => {
                self.failed = Some(driver::described(failure));
                Err(failure.1)
            }
        }
    }

    /// As much of the frame in progress as has been decoded, and how many of
    /// its rows are valid.
    ///
    /// Rows past the count hold whatever the buffer held before; a caller that
    /// wants only finished pixels stops there.
    pub fn partial_frame(&mut self) -> Result<(Picture<'_>, i32)> {
        let mut out = Handout::default();
        let mut rows = 0;

        self.failed = None;
        match self.inner.partial_picture(&mut out, &mut rows) {
            Ok(_) => Ok((Picture { out }, rows)),
            Err(failure) => {
                self.failed = Some(driver::described(failure));
                Err(failure.1)
            }
        }
    }

    /// The last failure's message, which says more than the status does.
    pub fn error(&self) -> &str {
        match &self.failed {
            Some(message) => message,
            None => self.inner.error_message(),
        }
    }
}

/// The library's version, as `wpd_version_string` reports it.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Restricts which instruction sets the DSP tables dispatch to, which is what
/// the test harnesses use to compare the assembly against the fallbacks.
pub fn set_cpu_flags_mask(mask: u32) {
    crate::cpu::set_mask(mask);
}

/// What a file says about itself, without opening a decoder for it.
pub fn info(data: &[u8]) -> Result<ImageInfo> {
    let scanned = crate::container::get_info(data)?;

    Ok(ImageInfo {
        width: scanned.width,
        height: scanned.height,
        has_alpha: scanned.has_alpha,
        is_animation: scanned.animation,
        frame_count: scanned.frame_count,
        loop_count: scanned.loop_count,
        background_argb: scanned.background_argb,
        coding: scanned.coding,
        metadata: scanned.metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-pixel lossless file, which is the smallest thing that exercises
    /// the whole path from open to pixels.
    const ONE_PIXEL: &[u8] = &[
        b'R', b'I', b'F', b'F', 0x1a, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P',
        b'8', b'L', 0x0e, 0, 0, 0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x07, 0x10, 0x11,
        0x11, 0x88, 0x88, 0xfe, 0x07, 0x00,
    ];

    #[test]
    fn a_still_decodes_to_one_picture_and_then_ends() {
        let mut d = Decoder::new();

        d.set_format(Format::Rgba).unwrap();
        d.open(ONE_PIXEL).unwrap();

        let info = d.info().unwrap();

        assert_eq!((info.width, info.height), (1, 1));
        assert!(!info.is_animation);

        {
            let picture = d.next_frame().unwrap().expect("a frame");

            assert_eq!(picture.width(), 1);
            assert_eq!(picture.format(), Format::Rgba);
            assert_eq!(picture.row(0, 0).len(), 4);
            assert_eq!(picture.rows_of(0).count(), 1);
        }
        assert!(d.next_frame().unwrap().is_none());
    }

    /// A complete input that stops inside a chunk is refused at the open,
    /// which is what tells it apart from a stream that has not caught up.
    #[test]
    fn a_truncated_file_is_refused_rather_than_panicking() {
        let mut d = Decoder::new();

        assert_eq!(d.open(&ONE_PIXEL[..20]).unwrap_err(), Error::Truncated);
        assert!(!d.error().is_empty());
    }

    #[test]
    fn what_is_not_a_webp_file_is_told_apart_from_a_damaged_one() {
        assert_eq!(info(b"not a webp file at all").unwrap_err(), Error::NotWebp);
    }

    #[test]
    fn a_borrowed_open_reads_the_caller_s_bytes_in_place() {
        let mut d = Decoder::new();

        d.open_borrowed(ONE_PIXEL).unwrap();
        assert_eq!(d.info().unwrap().width, 1);
    }

    #[test]
    fn a_stream_hands_out_nothing_until_the_frame_is_whole() {
        let mut d = Decoder::new();

        d.open_stream().unwrap();
        d.append(&ONE_PIXEL[..12]).unwrap();
        assert!(d.next_frame().unwrap().is_none());
        d.append(&ONE_PIXEL[12..]).unwrap();
        d.end_of_stream().unwrap();
        assert!(d.next_frame().unwrap().is_some());
    }
}
