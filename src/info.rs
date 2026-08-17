//! What a decoder can say about a file before any of it is decoded.
//!
//! These are the answers, not the questions: `wpd::container::Info` is what a
//! scan of the chunk list leaves behind, and much of it — offsets, sizes, how
//! far the walk got — is the driver's own bookkeeping. What is here is the
//! part a caller asked for, which the C ABI declares as `WPDImageInfo` and
//! `WPDFrameInfo` and versions by `struct_size`.

use crate::container::Coding;

/// What a file says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: i32,
    pub height: i32,
    pub has_alpha: bool,
    pub is_animation: bool,
    pub frame_count: i32,
    pub loop_count: i32,
    pub background_argb: u32,
    pub coding: Coding,
    /// Which metadata chunks the file carries, as the bit per kind that
    /// `WPD_METADATA_*` names.
    pub metadata: i32,
}

/// What one frame says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub pos_x: i32,
    pub pos_y: i32,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    /// Whether the canvas is cleared to the background colour behind this
    /// frame before the next one is drawn.
    pub dispose_to_background: bool,
    /// Whether this frame is alpha-blended over what is already there.
    pub blend: bool,
    pub has_alpha: bool,
    /// Whether the whole payload is buffered. A streamed file may expose an
    /// incomplete final frame.
    pub complete: bool,
}

/// A frame that declares nothing, which is what a still is: it covers the
/// canvas, is shown once, and disposes of nothing. `blend` is true because
/// alpha blending is what the ANMF flag's zero means, so a frame that carries
/// no flags is a blended one.
impl Default for FrameInfo {
    fn default() -> Self {
        FrameInfo {
            pos_x: 0,
            pos_y: 0,
            width: 0,
            height: 0,
            duration: 0,
            dispose_to_background: false,
            blend: true,
            has_alpha: false,
            complete: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Blend, Dispose};

    /// A frame that declares nothing must read the same way whether it came
    /// from the scanner or from this default, which is the trap: the ANMF
    /// flags are zero for *blended*, so a bool called `blend` defaults true.
    #[test]
    fn a_frame_that_declares_nothing_is_blended() {
        let d = FrameInfo::default();

        assert_eq!(d.blend, Blend::default() == Blend::Alpha);
        assert_eq!(
            d.dispose_to_background,
            Dispose::default() == Dispose::Background
        );
    }
}
