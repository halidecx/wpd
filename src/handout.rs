//! What a finished decode hands back.
//!
//! The C ABI's `WPDFrame` is four pointers, four strides and a versioned tail,
//! and the C filled it in from deep inside the conversion. Nothing here knows
//! about that struct: an export produces a [`Handout`], and the shim turns it
//! into whichever revision of `WPDFrame` the caller declared.
//!
//! Two shapes of destination exist, because `wpd_decoder_set_output_buffer`
//! lets a caller supply the memory. When it has, the rows go through a
//! [`RowSink`] the shim implements over its planes, so the decoder still packs
//! straight into the caller's buffer and the only code that touches it is on
//! the far side of the boundary.

use crate::image::Format;
use crate::picture::Frame;

/// Where the pixels a handout describes actually are.
pub enum Pixels<'a> {
    /// In memory the decoder owns, which the next decode may reuse.
    Own(Frame<'a>),
    /// In the caller's own planes, already written through a [`RowSink`].
    Sink,
    /// Nowhere: the decode produced no picture.
    None,
}

/// One decoded picture, and what the container says about it.
pub struct Handout<'a> {
    pub pixels: Pixels<'a>,
    pub format: Format,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub timestamp: i64,
    /// Where a sub-frame lands on the canvas.
    pub pos_x: i32,
    pub pos_y: i32,
    /// Whether the canvas is cleared behind this frame before the next.
    pub dispose_to_background: bool,
    /// Whether this frame is alpha-blended over what is already there.
    pub blend: bool,
    pub has_alpha: bool,
}

impl Default for Handout<'_> {
    fn default() -> Self {
        Handout {
            pixels: Pixels::None,
            format: Format::Argb,
            width: 0,
            height: 0,
            duration: 0,
            timestamp: 0,
            pos_x: 0,
            pos_y: 0,
            dispose_to_background: false,
            blend: true,
            has_alpha: false,
        }
    }
}

impl<'a> Handout<'a> {
    /// How many planes this handout's format describes.
    pub fn planes(&self) -> usize {
        self.format.nb_components()
    }

    /// The picture, when it is in the decoder's own memory.
    pub fn frame(&self) -> Option<&Frame<'a>> {
        match &self.pixels {
            Pixels::Own(frame) => Some(frame),
            _ => None,
        }
    }
}

/// Rows of memory an export writes into that it does not own.
///
/// The implementation lives with the C ABI, because a caller's plane is a
/// pointer, a length and a stride that may run backwards, and none of those
/// three is something the decoder can check.
pub trait RowSink {
    /// Whether plane `p` has room for `rows` rows of `row_len` bytes.
    ///
    /// Asked before anything is written, for every plane the format uses.
    fn fits(&self, p: usize, row_len: usize, rows: i32) -> bool;

    /// Row `y` of plane `p`, exactly `len` bytes.
    ///
    /// Only called after [`Self::fits`] has agreed to the geometry.
    fn row(&mut self, p: usize, y: i32, len: usize) -> &mut [u8];
}
