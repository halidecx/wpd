//! The public decoding interface.
//!
//! Everything a caller needs is reachable from here; the crate's other modules
//! are internal.

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::driver;
use crate::handout::Handout;
use crate::picture::Frame;

pub use crate::container::Coding;
pub use crate::error::{Error, Result};
pub use crate::image::Format;
pub use crate::info::{FrameInfo, ImageInfo};
pub use crate::options::{Options, UvMode};

/// How an animation's frames are handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Animation {
    /// Each picture is the full canvas with the frame composited onto it.
    #[default]
    Composited,
    /// Each picture is the frame alone. Its position, disposal, and blending
    /// describe how the caller must draw it. This mode cannot be combined
    /// with cropping, scaling, or flipping.
    Subframe,
}

/// A metadata chunk a WebP file may carry. The decoder does not apply any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Metadata {
    /// The ICC colour profile (`ICCP`).
    Iccp,
    /// Exif metadata (`EXIF`).
    Exif,
    /// XMP metadata (`XMP `).
    Xmp,
}

/// A decoded picture, borrowed from the [`Decoder`] that produced it.
pub struct Picture<'a> {
    out: Handout<'a>,
}

impl<'a> Picture<'a> {
    fn pixels(&self) -> Option<&Frame<'a>> {
        self.out.frame()
    }

    /// Width in pixels, after cropping and scaling.
    #[must_use]
    pub fn width(&self) -> i32 {
        self.out.width
    }

    /// Height in pixels, after cropping and scaling.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.out.height
    }

    /// Pixel format of the planes.
    #[must_use]
    pub fn format(&self) -> Format {
        self.out.format
    }

    /// Display duration in milliseconds. Zero for a still.
    #[must_use]
    pub fn duration(&self) -> i32 {
        self.out.duration
    }

    /// Presentation timestamp in milliseconds.
    #[must_use]
    pub fn timestamp(&self) -> i64 {
        self.out.timestamp
    }

    /// The frame's `(x, y)` offset on the canvas, for drawing an
    /// [`Animation::Subframe`] picture.
    #[must_use]
    pub fn position(&self) -> (i32, i32) {
        (self.out.pos_x, self.out.pos_y)
    }

    /// Whether the frame itself has alpha.
    #[must_use]
    pub fn has_alpha(&self) -> bool {
        self.out.has_alpha
    }

    /// Whether the frame's area is cleared to the background after display.
    #[must_use]
    pub fn dispose_to_background(&self) -> bool {
        self.out.dispose_to_background
    }

    /// Whether the frame is alpha-blended onto the canvas, rather than
    /// replacing it.
    #[must_use]
    pub fn blend(&self) -> bool {
        !self.out.no_blend
    }

    /// Number of planes: one for packed formats, three or four for planar.
    #[must_use]
    pub fn planes(&self) -> usize {
        self.out.planes()
    }

    /// Number of rows in `plane`. Chroma planes of 4:2:0 output have half
    /// as many as luma, rounded up.
    #[must_use]
    pub fn rows(&self, plane: usize) -> i32 {
        self.pixels().map_or(0, |img| img.rows(plane))
    }

    /// Row `y` of `plane`, in output order (top to bottom, flips applied).
    ///
    /// # Panics
    ///
    /// Panics if `plane` is not below [`planes`](Self::planes) or `y` is not
    /// below [`rows`](Self::rows).
    #[must_use]
    pub fn row(&self, plane: usize, y: i32) -> &'a [u8] {
        assert!(plane < self.planes(), "no such plane");

        let img = self.pixels().expect("no pixels");

        assert!(y >= 0 && y < img.rows(plane), "no such row");
        img.row(plane, y)
    }

    /// Every row of `plane` in output order. Flipped output needs no special
    /// stride handling.
    ///
    /// # Panics
    ///
    /// Panics while iterating if `plane` is not below
    /// [`planes`](Self::planes).
    pub fn rows_of(&self, plane: usize) -> impl Iterator<Item = &'a [u8]> + '_ {
        (0..self.rows(plane)).map(move |y| self.row(plane, y))
    }
}

/// A WebP decoder.
///
/// Open input with [`open`](Self::open), [`open_borrowed`](Self::open_borrowed),
/// or [`open_stream`](Self::open_stream), then call
/// [`next_frame`](Self::next_frame) until it returns `None`. A decoder can be
/// reopened. `'a` is the lifetime of borrowed input.
pub struct Decoder<'a> {
    inner: Box<driver::Decoder<'a>>,
    failed: Option<String>,
    input: PhantomData<&'a [u8]>,
}

/// The decoder after [`Decoder::update`]. Dereferences to the decoder, and
/// can lend the buffer back with [`into_buffer`](Self::into_buffer).
pub struct UpdatedDecoder<'d, 'a> {
    decoder: &'d mut Decoder<'a>,
}

impl<'a> Deref for UpdatedDecoder<'_, 'a> {
    type Target = Decoder<'a>;

    fn deref(&self) -> &Self::Target {
        self.decoder
    }
}

impl DerefMut for UpdatedDecoder<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.decoder
    }
}

impl<'d, 'a> UpdatedDecoder<'d, 'a> {
    /// Take the streaming buffer back to extend it in place, without copying.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if the stream has ended or was not fed
    /// with [`Decoder::update`].
    pub fn into_buffer(self) -> Result<UpdateBuffer<'d, 'a>> {
        let data = self.decoder.driver().take_update_buffer()?;

        Ok(UpdateBuffer {
            decoder: Some(self.decoder),
            data: Some(data),
        })
    }
}

/// The streaming buffer, lent out by [`UpdatedDecoder::into_buffer`] to be
/// extended. It dereferences to the `Vec`. [`update`](Self::update) hands it
/// back; dropping it does the same, discarding any error.
pub struct UpdateBuffer<'d, 'a> {
    decoder: Option<&'d mut Decoder<'a>>,
    data: Option<Vec<u8>>,
}

impl Deref for UpdateBuffer<'_, '_> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        self.data.as_ref().expect("update buffer is attached")
    }
}

impl DerefMut for UpdateBuffer<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.as_mut().expect("update buffer is attached")
    }
}

impl<'d, 'a> UpdateBuffer<'d, 'a> {
    /// Hand the extended buffer back to the decoder. Bytes already supplied
    /// must be unchanged.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if the buffer shrank, or a header error from
    /// the new bytes. On error the stream is reopened empty.
    // The `expect`s cannot fire: both fields are only taken here, by value,
    // or in `drop`.
    #[allow(clippy::missing_panics_doc)]
    pub fn update(mut self) -> Result<UpdatedDecoder<'d, 'a>> {
        let data = self.data.take().expect("update buffer is attached");
        let decoder = self.decoder.take().expect("update decoder is attached");

        if let Err(e) = decoder.driver().update_owned(data) {
            decoder.driver().open_stream()?;
            return Err(e);
        }
        Ok(UpdatedDecoder { decoder })
    }
}

impl Drop for UpdateBuffer<'_, '_> {
    fn drop(&mut self) {
        let Some(data) = self.data.take() else {
            return;
        };
        let Some(decoder) = self.decoder.take() else {
            return;
        };
        if decoder.driver().update_owned(data).is_err() {
            let _ = decoder.driver().open_stream();
        }
    }
}

impl Default for Decoder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Decoder<'a> {
    /// Create a decoder. Until [`set_format`](Self::set_format) is called it
    /// outputs each picture in the format it decodes to, without conversion;
    /// see [`Picture::format`]. Animations are [`Animation::Composited`], and
    /// [`Options`] are the defaults.
    #[must_use]
    pub fn new() -> Self {
        crate::cpu::init();

        Decoder {
            inner: Box::new(driver::Decoder::new()),
            failed: None,
            input: PhantomData,
        }
    }

    fn driver(&mut self) -> &mut driver::Decoder<'a> {
        self.failed = None;
        &mut self.inner
    }

    /// Set the output pixel format. It applies from the next decoded frame;
    /// set it before decoding an animation.
    ///
    /// # Errors
    ///
    /// Infallible today; reserved for formats a build cannot produce.
    pub fn set_format(&mut self, format: Format) -> Result<()> {
        self.driver().set_output_format(format as i32)
    }

    /// Set how animation frames are handed out.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if [`Animation::Subframe`] is combined
    /// with cropping, scaling, or flipping, or if the mode changes partway
    /// through an animation.
    pub fn set_animation(&mut self, mode: Animation) -> Result<()> {
        self.driver().set_animation_mode(match mode {
            Animation::Composited => driver::ANIM_COMPOSITED,
            Animation::Subframe => driver::ANIM_SUBFRAME,
        })
    }

    /// Set processing options. They apply from the next decoded frame.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an empty or negative crop, a negative
    /// or all-zero scale, a negative thread count, or transforms combined
    /// with [`Animation::Subframe`].
    pub fn set_options(&mut self, options: Options) -> Result<()> {
        self.driver().set_core_options(options)
    }

    /// Open a complete image, copying it.
    ///
    /// # Errors
    ///
    /// [`Error::NotWebp`], [`Error::InvalidData`], [`Error::Truncated`],
    /// [`Error::Unsupported`], or [`Error::TooLarge`] if the headers are bad;
    /// [`Error::NoMemory`] if the copy cannot be allocated.
    pub fn open(&mut self, data: &[u8]) -> Result<()> {
        self.driver().open(data)
    }

    /// Open a complete image without copying it.
    ///
    /// # Errors
    ///
    /// [`Error::NotWebp`], [`Error::InvalidData`], [`Error::Truncated`],
    /// [`Error::Unsupported`], or [`Error::TooLarge`] if the headers are bad.
    pub fn open_borrowed(&mut self, data: &'a [u8]) -> Result<()> {
        self.driver().open_borrowed(data)
    }

    /// Begin a streamed decode. Feed it with [`append`](Self::append) or
    /// [`update`](Self::update), not both, then call
    /// [`end_of_stream`](Self::end_of_stream).
    ///
    /// # Errors
    ///
    /// Infallible today.
    pub fn open_stream(&mut self) -> Result<()> {
        self.driver().open_stream()
    }

    /// Append a copy of `chunk` to a streamed decode. Bytes past the end the
    /// RIFF header declares are dropped.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if no stream is open, it has ended, or it
    /// is fed with [`update`](Self::update); a header error once the headers
    /// are complete and bad; [`Error::NoMemory`] if buffering fails.
    pub fn append(&mut self, chunk: &[u8]) -> Result<()> {
        self.driver().append(chunk)
    }

    /// Replace the streaming buffer with `data`, which must hold every byte
    /// supplied so far, unchanged, followed by any new ones.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if no stream is open, it has ended, it is
    /// fed with [`append`](Self::append), or `data` is shorter than before;
    /// a header error once the headers are complete and bad.
    pub fn update(&mut self, data: Vec<u8>) -> Result<UpdatedDecoder<'_, 'a>> {
        self.driver().update_owned(data)?;
        Ok(UpdatedDecoder { decoder: self })
    }

    /// Mark a streamed input complete. After this, `None` from
    /// [`next_frame`](Self::next_frame) means the end.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if no stream is open; a header error, such
    /// as [`Error::Truncated`], if the input ended early or is bad.
    pub fn end_of_stream(&mut self) -> Result<()> {
        self.driver().end_of_stream()
    }

    /// Information about the open image. For a stream, the frame count grows
    /// as frames arrive.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if nothing is open; [`Error::Truncated`]
    /// if a stream's headers are still incomplete.
    pub fn info(&mut self) -> Result<ImageInfo> {
        self.driver().image_info()
    }

    /// Information about frame `index`, counting from zero, without decoding
    /// it. A still is one canvas-sized frame. A stream may report an
    /// incomplete final frame.
    ///
    /// # Errors
    ///
    /// As [`info`](Self::info), and [`Error::InvalidArgument`] if there is
    /// no such frame.
    pub fn frame_info(&mut self, index: i32) -> Result<FrameInfo> {
        self.driver().frame_entry(index)
    }

    /// The raw payload of a metadata chunk, or `None` if the image has none,
    /// nothing is open, or a stream has not delivered it yet.
    pub fn metadata(&mut self, kind: Metadata) -> Option<&[u8]> {
        let kind = match kind {
            Metadata::Iccp => 1,
            Metadata::Exif => 2,
            Metadata::Xmp => 4,
        };

        self.driver().metadata(kind).ok().flatten()
    }

    /// Go back to the first frame. Complete inputs and updated streams can be
    /// rewound; appended streams cannot.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if nothing is open or its headers are
    /// incomplete; [`Error::Unsupported`] for an appended stream.
    pub fn rewind(&mut self) -> Result<()> {
        self.driver().rewind()
    }

    /// Decode and return the next frame, or `None` when there is no frame to
    /// give: the image has ended, or a stream needs more input.
    ///
    /// # Errors
    ///
    /// Any [`Error`] the input or the output can cause.
    /// [`error`](Self::error) describes it.
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

    /// The rows decoded so far of a still being streamed, with how many of
    /// them are valid, or `None` if there is none yet. The count is zero for
    /// animations and for cropped, scaled, or flipped output. The frame is not
    /// consumed.
    ///
    /// # Errors
    ///
    /// As [`next_frame`](Self::next_frame).
    pub fn partial_frame(&mut self) -> Result<Option<(Picture<'_>, i32)>> {
        let mut out = Handout::default();
        let mut rows = 0;

        self.failed = None;
        match self.inner.partial_picture(&mut out, &mut rows) {
            Ok(false) => Ok(None),
            Ok(true) => Ok(Some((Picture { out }, rows))),
            Err(failure) => {
                self.failed = Some(driver::described(failure));
                Err(failure.1)
            }
        }
    }

    /// A description of the last failure.
    #[must_use]
    pub fn error(&self) -> &str {
        match &self.failed {
            Some(message) => message,
            None => self.inner.error_message(),
        }
    }
}

/// The library version, as `major.minor.patch`.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Restrict the CPU features the assembly may use to those in `mask`, for
/// testing and benchmarking. It affects every decoder in the process.
pub fn set_cpu_flags_mask(mask: u32) {
    crate::cpu::set_mask(mask);
}

/// Read image information from `data` without decoding or retaining it.
///
/// # Errors
///
/// [`Error::NotWebp`], [`Error::InvalidData`], [`Error::Truncated`],
/// [`Error::Unsupported`], or [`Error::TooLarge`] if the headers are bad.
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

    #[test]
    fn a_partial_frame_without_pixels_is_none() {
        let mut decoder = Decoder::new();

        decoder.open_stream().unwrap();
        decoder.append(&ONE_PIXEL[..12]).unwrap();
        assert!(decoder.partial_frame().unwrap().is_none());
        decoder.append(&ONE_PIXEL[12..]).unwrap();
        assert!(decoder.next_frame().unwrap().is_some());
        let (picture, rows) = decoder.partial_frame().unwrap().unwrap();
        assert_eq!(rows, 1);
        assert_eq!(picture.row(0, 0).len(), 4);
    }

    #[test]
    fn an_updated_stream_reuses_the_callers_growing_buffer() {
        let mut d = Decoder::new();
        let mut data = Vec::with_capacity(ONE_PIXEL.len());

        data.extend_from_slice(&ONE_PIXEL[..12]);
        let allocation = data.as_ptr();

        d.open_stream().unwrap();
        let mut updated = d.update(data).unwrap();

        assert!(updated.info().is_err());

        let mut data = updated.into_buffer().unwrap();

        assert_eq!(data.as_ptr(), allocation);
        data.extend_from_slice(&ONE_PIXEL[12..]);
        assert_eq!(data.as_ptr(), allocation);

        let mut updated = data.update().unwrap();

        updated.end_of_stream().unwrap();
        assert!(updated.next_frame().unwrap().is_some());
    }

    #[test]
    fn dropping_an_update_buffer_reattaches_it() {
        let mut d = Decoder::new();

        d.open_stream().unwrap();
        let updated = d.update(ONE_PIXEL[..12].to_vec()).unwrap();

        {
            let mut data = updated.into_buffer().unwrap();

            data.extend_from_slice(&ONE_PIXEL[12..]);
        }

        d.end_of_stream().unwrap();
        assert!(d.next_frame().unwrap().is_some());
    }
}
