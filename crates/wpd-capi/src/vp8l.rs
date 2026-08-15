//! The lossless decoder's pictures, seen as bytes.
//!
//! [`wpd::vp8l`] works a pixel at a time and stores its canvas as `u32`;
//! everything downstream of it — the packers, the compositor, the copy into a
//! caller's buffer — works a row of bytes at a time. Reinterpreting the one as
//! the other is the whole of this module, and it is the reason the driver
//! cannot yet be safe code end to end: the core promises no `unsafe` at all
//! without the `asm` feature, and a `&[u32]` cannot become a `&[u8]` without
//! either `unsafe` or a dependency the core does not have.
//!
//! Nothing else here reaches for a pointer. Which of the decoder's pictures a
//! caller means is a name, not a latched view.

use std::slice;

use wpd::image::Format;
use wpd::picture::{Frame, FrameMut, PlaneMut};
use wpd::vp8l::{Decoder, Picture, Target};

/// Which of the lossless decoder's pictures a decode left its output in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lossless {
    /// What [`Decoder::decode_frame`] filled in for [`Target::Argb`].
    Argb,
    /// What the resumable path is filling in, which alternates between two
    /// pictures as the caller peeks at it.
    Still,
}

impl Lossless {
    /// The named picture, or nothing when the decoder has not produced one.
    pub fn of(self, decoder: &Decoder) -> Option<Frame<'_>> {
        let pic = match self {
            Lossless::Argb => decoder.picture(Target::Argb),
            Lossless::Still => decoder.still_picture()?,
        };

        (pic.width > 0 && !pic.data.is_empty()).then(|| frame(pic))
    }

    /// As [`Self::of`], writable, which the compositor's per-frame
    /// premultiply weights in place.
    pub fn of_mut(self, decoder: &mut Decoder) -> Option<FrameMut<'_>> {
        let pic = match self {
            Lossless::Argb => decoder.picture_out_mut(Target::Argb),
            Lossless::Still => decoder.still_picture_mut()?,
        };

        if pic.width <= 0 || pic.data.is_empty() {
            return None;
        }
        Some(frame_mut(pic))
    }
}

/// A picture as a packed ARGB [`Frame`].
///
/// The cast is the module's reason for existing; the slice it produces covers
/// exactly the allocation it came from, so every row a caller can name is in
/// bounds.
pub fn frame(pic: &Picture) -> Frame<'_> {
    let bytes = unsafe {
        slice::from_raw_parts(pic.data.as_ptr().cast::<u8>(), pic.data.len() * 4)
    };

    Frame::packed(bytes, pic.stride * 4, pic.width, pic.height, Format::Argb)
}

/// As [`frame`], writable.
pub fn frame_mut(pic: &mut Picture) -> FrameMut<'_> {
    let len = pic.data.len() * 4;
    let bytes =
        unsafe { slice::from_raw_parts_mut(pic.data.as_mut_ptr().cast::<u8>(), len) };
    let plane = [
        PlaneMut::borrowed(bytes, pic.stride * 4),
        PlaneMut::borrowed(&mut [], 0),
        PlaneMut::borrowed(&mut [], 0),
        PlaneMut::borrowed(&mut [], 0),
    ];

    FrameMut::borrowed(plane, pic.width, pic.height, Format::Argb, false)
}
