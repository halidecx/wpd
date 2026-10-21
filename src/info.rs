use crate::container::Coding;

/// What an image's headers say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    /// Canvas width in pixels.
    pub width: i32,
    /// Canvas height in pixels.
    pub height: i32,
    /// Whether any frame can have alpha.
    pub has_alpha: bool,
    /// Whether the image is an animation.
    pub is_animation: bool,
    /// Number of frames; one for a still. Grows as a stream arrives.
    pub frame_count: i32,
    /// How many times an animation plays; zero repeats indefinitely.
    pub loop_count: i32,
    /// The animation's suggested background colour, as ARGB.
    pub background_argb: u32,
    /// How a still is coded; [`Coding::Unknown`] for animations.
    pub coding: Coding,
    /// The metadata present, as a set of flags: 1 for ICCP, 2 for Exif, and
    /// 4 for XMP.
    pub metadata: i32,
}

/// What an image's headers say about one of its frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FrameInfo {
    /// Horizontal offset on the canvas.
    pub pos_x: i32,
    /// Vertical offset on the canvas.
    pub pos_y: i32,
    /// Frame width in pixels.
    pub width: i32,
    /// Frame height in pixels.
    pub height: i32,
    /// Display duration in milliseconds.
    pub duration: i32,
    /// Whether the frame's area is cleared to the background after display.
    pub dispose_to_background: bool,
    /// Whether the frame replaces the canvas under it rather than blending.
    pub no_blend: bool,
    /// Whether the frame has alpha. Valid once `complete` is set.
    pub has_alpha: bool,
    /// Whether the frame's whole payload is buffered.
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Blend, Dispose};

    #[test]
    fn a_frame_that_declares_nothing_is_blended() {
        let d = FrameInfo::default();

        assert_eq!(d.no_blend, Blend::default() != Blend::Alpha);
        assert_eq!(
            d.dispose_to_background,
            Dispose::default() == Dispose::Background
        );
    }
}
