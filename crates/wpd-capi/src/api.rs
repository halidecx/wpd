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
//! This module lives here rather than in the core crate because the driver it
//! wraps does; it moves up with the driver. Nothing in it reaches through the
//! C ABI — the calls are to this crate's own Rust functions, with the raw
//! pointers confined to the one line each needs.

use std::ffi::CStr;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr;

use wpd::image::Format;

use crate::container::WPDImageInfo;
use crate::convert::WPDDecoderOptions;
use crate::decoder::{WPDDecoder, WPDFrameInfo};
use crate::export::WPDFrame;

/// What a decode can fail with, as `WPDStatus` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArgument,
    NotWebp,
    Bitstream,
    Truncated,
    Unsupported,
    NoMemory,
    TooLarge,
    BufferTooSmall,
    /// A status this build does not know a name for.
    Other(i32),
}

impl Error {
    fn from_status(status: i32) -> Self {
        match status {
            crate::decoder::WPD_ERR_INVALID_ARG => Error::InvalidArgument,
            crate::decoder::WPD_ERR_NOT_WEBP => Error::NotWebp,
            crate::decoder::WPD_ERR_BITSTREAM => Error::Bitstream,
            crate::decoder::WPD_ERR_TRUNCATED => Error::Truncated,
            crate::decoder::WPD_ERR_UNSUPPORTED => Error::Unsupported,
            crate::decoder::WPD_ERR_NO_MEMORY => Error::NoMemory,
            crate::decoder::WPD_ERR_TOO_LARGE => Error::TooLarge,
            crate::decoder::WPD_ERR_BUFFER_TOO_SMALL => Error::BufferTooSmall,
            other => Error::Other(other),
        }
    }

    /// The same one-line description `wpd_status_string` returns.
    pub fn message(self) -> &'static str {
        let status = match self {
            Error::InvalidArgument => crate::decoder::WPD_ERR_INVALID_ARG,
            Error::NotWebp => crate::decoder::WPD_ERR_NOT_WEBP,
            Error::Bitstream => crate::decoder::WPD_ERR_BITSTREAM,
            Error::Truncated => crate::decoder::WPD_ERR_TRUNCATED,
            Error::Unsupported => crate::decoder::WPD_ERR_UNSUPPORTED,
            Error::NoMemory => crate::decoder::WPD_ERR_NO_MEMORY,
            Error::TooLarge => crate::decoder::WPD_ERR_TOO_LARGE,
            Error::BufferTooSmall => crate::decoder::WPD_ERR_BUFFER_TOO_SMALL,
            Error::Other(status) => status,
        };
        let s = unsafe { CStr::from_ptr(crate::decoder::wpd_status_string(status)) };

        s.to_str().unwrap_or("unknown error")
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn check(status: i32) -> Result<()> {
    if status < 0 {
        return Err(Error::from_status(status));
    }
    Ok(())
}

/// What an animation hands out: the composited canvas, or each sub-frame on
/// its own at its own position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animation {
    #[default]
    Composited,
    Subframe,
}

/// How the image was coded, which a still declares before it is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    Unknown,
    Lossy,
    Lossless,
}

impl Coding {
    pub fn name(self) -> &'static str {
        match self {
            Coding::Unknown => "unknown",
            Coding::Lossy => "lossy",
            Coding::Lossless => "lossless",
        }
    }
}

/// Which metadata chunk to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Iccp,
    Exif,
    Xmp,
}

/// The crop, scale and flip a decode may apply on the way out.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Skip the in-loop filter of a lossy frame. Faster and not bit-exact.
    pub bypass_filtering: bool,
    /// Point-sample chroma instead of interpolating it.
    pub no_fancy_upsampling: bool,
    /// `(left, top, width, height)`, in the source's coordinates.
    pub crop: Option<(i32, i32, i32, i32)>,
    /// `(width, height)`; either may be zero to keep the aspect ratio.
    pub scale: Option<(i32, i32)>,
    /// Hand the picture out bottom row first.
    pub flip: bool,
}

impl Options {
    fn to_c(self) -> WPDDecoderOptions {
        let mut c = WPDDecoderOptions::new();

        c.bypass_filtering = i32::from(self.bypass_filtering);
        c.no_fancy_upsampling = i32::from(self.no_fancy_upsampling);
        if let Some((left, top, width, height)) = self.crop {
            c.use_cropping = 1;
            c.crop_left = left;
            c.crop_top = top;
            c.crop_width = width;
            c.crop_height = height;
        }
        if let Some((width, height)) = self.scale {
            c.use_scaling = 1;
            c.scaled_width = width;
            c.scaled_height = height;
        }
        c.flip = i32::from(self.flip);
        c
    }
}

/// What a file says about itself before any of it is decoded.
#[derive(Debug, Clone, Copy)]
pub struct ImageInfo {
    pub width: i32,
    pub height: i32,
    pub has_alpha: bool,
    pub is_animation: bool,
    pub frame_count: i32,
    pub loop_count: i32,
    pub background_argb: u32,
    pub coding: Coding,
}

impl ImageInfo {
    fn from_c(c: &WPDImageInfo) -> Self {
        ImageInfo {
            width: c.width,
            height: c.height,
            has_alpha: c.has_alpha != 0,
            is_animation: c.is_animation != 0,
            frame_count: c.frame_count,
            loop_count: c.loop_count,
            background_argb: c.background_argb,
            coding: match c.coding {
                1 => Coding::Lossy,
                2 => Coding::Lossless,
                _ => Coding::Unknown,
            },
        }
    }
}

/// What one frame says about itself before it is decoded.
#[derive(Debug, Clone, Copy)]
pub struct FrameInfo {
    pub pos_x: i32,
    pub pos_y: i32,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub dispose_to_background: bool,
    pub blend: bool,
    pub has_alpha: bool,
    /// Whether the whole payload is buffered. A streamed file may expose an
    /// incomplete final frame.
    pub complete: bool,
}

/// One decoded picture, borrowed from the decoder that produced it.
///
/// The next decode reuses this memory, which is why the borrow is held: the
/// C ABI hands out the same pointers and can only ask the caller to be
/// careful.
pub struct Picture<'a> {
    frame: WPDFrame,
    decoder: PhantomData<&'a Decoder<'a>>,
}

impl Picture<'_> {
    pub fn width(&self) -> i32 {
        self.frame.width
    }

    pub fn height(&self) -> i32 {
        self.frame.height
    }

    pub fn format(&self) -> Option<Format> {
        Format::from_raw(self.frame.format)
    }

    /// How long this frame is shown, in milliseconds, or zero for a still.
    pub fn duration(&self) -> i32 {
        self.frame.duration
    }

    /// When this frame is shown, in milliseconds from the start.
    pub fn timestamp(&self) -> i64 {
        self.frame.timestamp
    }

    /// Where a sub-frame lands on the canvas, in
    /// [`Animation::Subframe`] mode.
    pub fn position(&self) -> (i32, i32) {
        (self.frame.pos_x, self.frame.pos_y)
    }

    pub fn has_alpha(&self) -> bool {
        self.frame.has_alpha != 0
    }

    /// Whether the canvas is cleared to the background colour behind this
    /// frame before the next one is drawn.
    pub fn dispose_to_background(&self) -> bool {
        self.frame.dispose == 1
    }

    /// Whether this frame is alpha-blended over what is already there, as
    /// opposed to replacing it.
    pub fn blend(&self) -> bool {
        self.frame.blend == 0
    }

    /// How many planes this picture's format hands out.
    pub fn planes(&self) -> usize {
        match self.format() {
            Some(Format::Yuva420p) => 4,
            Some(Format::Yuv420p) => 3,
            _ => 1,
        }
    }

    fn row_len(&self, plane: usize) -> usize {
        let format = self.format().unwrap_or(Format::Argb);

        if self.planes() == 1 {
            self.width() as usize * format.bpp()
        } else {
            let shift = u32::from(plane == 1 || plane == 2);

            wpd::image::ceil_rshift(self.width(), shift) as usize
        }
    }

    /// How many rows `plane` has, which the chroma planes halve.
    pub fn rows(&self, plane: usize) -> i32 {
        if self.planes() == 1 {
            self.height()
        } else {
            let shift = u32::from(plane == 1 || plane == 2);

            wpd::image::ceil_rshift(self.height(), shift)
        }
    }

    /// The distance between rows of `plane`, which is negative when the
    /// picture was asked for flipped.
    pub fn stride(&self, plane: usize) -> isize {
        self.frame.stride[plane]
    }

    /// Row `y` of `plane`, top row first however the picture is stored.
    ///
    /// # Panics
    ///
    /// If `plane` or `y` is outside the picture.
    pub fn row(&self, plane: usize, y: i32) -> &[u8] {
        assert!(plane < self.planes(), "no such plane");
        assert!(y >= 0 && y < self.rows(plane), "no such row");

        let stride = self.frame.stride[plane];
        let at = y as isize * stride;

        unsafe {
            std::slice::from_raw_parts(
                self.frame.data[plane].offset(at),
                self.row_len(plane),
            )
        }
    }

    /// Every row of `plane`, in order.
    pub fn rows_of(&self, plane: usize) -> impl Iterator<Item = &[u8]> + '_ {
        (0..self.rows(plane)).map(move |y| self.row(plane, y))
    }
}

/// A WebP decoder.
///
/// The lifetime is the input's: it is `'static` for a decoder that owns its
/// bytes, and the input's own for one opened with
/// [`Decoder::open_borrowed`].
pub struct Decoder<'a> {
    inner: Box<WPDDecoder>,
    input: PhantomData<&'a [u8]>,
}

impl Default for Decoder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Decoder<'a> {
    pub fn new() -> Self {
        Decoder {
            inner: Box::new(WPDDecoder::new()),
            input: PhantomData,
        }
    }

    /// The format frames come out in. Leaving it unset hands out whatever the
    /// file codes natively, which is planar for a lossy frame and ARGB for a
    /// lossless one.
    pub fn set_format(&mut self, format: Format) -> Result<()> {
        check(unsafe {
            crate::decoder::wpd_decoder_set_output_format(
                &mut *self.inner,
                format as i32,
            )
        })
    }

    pub fn set_animation(&mut self, mode: Animation) -> Result<()> {
        let mode = match mode {
            Animation::Composited => 0,
            Animation::Subframe => 1,
        };

        check(unsafe {
            crate::decoder::wpd_decoder_set_animation_mode(&mut *self.inner, mode)
        })
    }

    pub fn set_options(&mut self, options: Options) -> Result<()> {
        let c = options.to_c();

        check(unsafe { crate::decoder::wpd_decoder_set_options(&mut *self.inner, &c) })
    }

    /// Opens a file the decoder copies, so nothing has to outlive the call.
    pub fn open(&mut self, data: &[u8]) -> Result<()> {
        check(unsafe {
            crate::decoder::wpd_decoder_open(
                &mut *self.inner,
                data.as_ptr(),
                data.len(),
            )
        })
    }

    /// Opens a file the decoder reads in place. The bytes must outlive the
    /// decoder, which is what the lifetime says.
    pub fn open_borrowed(&mut self, data: &'a [u8]) -> Result<()> {
        check(unsafe {
            crate::decoder::wpd_decoder_open_borrowed(
                &mut *self.inner,
                data.as_ptr(),
                data.len(),
            )
        })
    }

    /// Starts a stream the caller appends to as bytes arrive.
    pub fn open_stream(&mut self) -> Result<()> {
        check(unsafe { crate::decoder::wpd_decoder_open_stream(&mut *self.inner) })
    }

    pub fn append(&mut self, chunk: &[u8]) -> Result<()> {
        check(unsafe {
            crate::decoder::wpd_decoder_append(
                &mut *self.inner,
                chunk.as_ptr(),
                chunk.len(),
            )
        })
    }

    /// Replaces the stream's contents with a longer prefix of the same file,
    /// which is what a caller reading into a growing buffer has.
    pub fn update(&mut self, data: &'a [u8]) -> Result<()> {
        check(unsafe {
            crate::decoder::wpd_decoder_update(
                &mut *self.inner,
                data.as_ptr(),
                data.len(),
            )
        })
    }

    pub fn end_of_stream(&mut self) -> Result<()> {
        check(unsafe { crate::decoder::wpd_decoder_end_of_stream(&mut *self.inner) })
    }

    pub fn info(&self) -> Result<ImageInfo> {
        let mut info = WPDImageInfo::zeroed();

        check(unsafe {
            crate::decoder::wpd_decoder_get_info(&*self.inner, &mut info)
        })?;
        Ok(ImageInfo::from_c(&info))
    }

    pub fn frame_info(&self, index: i32) -> Result<FrameInfo> {
        let mut info = MaybeUninit::<WPDFrameInfo>::zeroed();

        unsafe {
            (*info.as_mut_ptr()).struct_size = std::mem::size_of::<WPDFrameInfo>();
        }
        check(unsafe {
            crate::decoder::wpd_decoder_frame_info(
                &*self.inner,
                index,
                info.as_mut_ptr(),
            )
        })?;

        let info = unsafe { info.assume_init() };

        Ok(FrameInfo {
            pos_x: info.pos_x,
            pos_y: info.pos_y,
            width: info.width,
            height: info.height,
            duration: info.duration,
            dispose_to_background: info.dispose == 1,
            blend: info.blend == 0,
            has_alpha: info.has_alpha != 0,
            complete: info.complete != 0,
        })
    }

    /// The named metadata chunk, or none when the file carries no such chunk.
    pub fn metadata(&self, kind: Metadata) -> Option<&[u8]> {
        let kind = match kind {
            Metadata::Iccp => 1,
            Metadata::Exif => 2,
            Metadata::Xmp => 4,
        };
        let mut data = ptr::null();
        let mut size = 0usize;
        let ok = unsafe {
            crate::decoder::wpd_decoder_metadata(
                &*self.inner,
                kind,
                &mut data,
                &mut size,
            )
        };

        if ok < 0 || data.is_null() || size == 0 {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(data, size) })
    }

    /// Returns to the first frame. A stream that was appended to cannot be
    /// rewound, because the bytes it has read are gone.
    pub fn rewind(&mut self) -> Result<()> {
        check(unsafe { crate::decoder::wpd_decoder_rewind(&mut *self.inner) })
    }

    /// The next frame, or none when the file is finished or the stream has
    /// not buffered enough of it yet.
    ///
    /// The picture borrows the decoder: the next call reuses its memory.
    pub fn next_frame(&mut self) -> Result<Option<Picture<'_>>> {
        let mut frame = WPDFrame::zeroed();
        let got = unsafe {
            crate::decoder::wpd_decoder_next_frame(&mut *self.inner, &mut frame)
        };

        check(got)?;
        if got == 0 {
            return Ok(None);
        }
        Ok(Some(Picture {
            frame,
            decoder: PhantomData,
        }))
    }

    /// As much of the frame in progress as has been decoded, and how many of
    /// its rows are valid.
    ///
    /// Rows past the count hold whatever the buffer held before; a caller that
    /// wants only finished pixels stops there.
    pub fn partial_frame(&mut self) -> Result<(Picture<'_>, i32)> {
        let mut frame = WPDFrame::zeroed();
        let mut rows = 0;

        check(unsafe {
            crate::decoder::wpd_decoder_partial_frame(
                &mut *self.inner,
                &mut frame,
                &mut rows,
            )
        })?;
        Ok((
            Picture {
                frame,
                decoder: PhantomData,
            },
            rows,
        ))
    }

    /// The last failure's message, which says more than the status does.
    pub fn error(&self) -> &str {
        let s =
            unsafe { CStr::from_ptr(crate::decoder::wpd_decoder_error(&*self.inner)) };

        s.to_str().unwrap_or("")
    }
}

/// The library's version, as `wpd_version_string` reports it.
pub fn version() -> &'static str {
    let s = unsafe { CStr::from_ptr(crate::compat::wpd_version_string()) };

    s.to_str().unwrap_or("")
}

/// Restricts which instruction sets the DSP tables dispatch to, which is what
/// the test harnesses use to compare the assembly against the fallbacks.
pub fn set_cpu_flags_mask(mask: u32) {
    wpd::cpu::set_mask(mask);
}

/// What a file says about itself, without opening a decoder for it.
pub fn info(data: &[u8]) -> Result<ImageInfo> {
    let mut c = WPDImageInfo::zeroed();

    check(unsafe {
        crate::container::wpd_get_info(data.as_ptr(), data.len(), &mut c)
    })?;
    Ok(ImageInfo::from_c(&c))
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
            assert_eq!(picture.format(), Some(Format::Rgba));
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
