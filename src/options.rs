//! What a decode was asked to do to the picture on the way out.
//!
//! The C ABI declares this as a versioned `WPDDecoderOptions` whose flags are
//! `int`s and whose crop and scale are four and two fields guarded by a
//! separate `use_` flag. That shape is the ABI's; here a crop either exists or
//! does not, which is the same rule the flags were encoding and one the
//! compiler can check.

/// The crop, scale and flip a decode may apply on the way out.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
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
    /// Whether anything here changes the picture's geometry.
    ///
    /// A decode that transforms cannot hand out a codec's own picture, so this
    /// is what picks between the direct paths and the ones that go through a
    /// buffer of the decoder's.
    pub fn transforms(&self) -> bool {
        self.crop.is_some() || self.scale.is_some() || self.flip
    }

    /// The crop rectangle, or the whole of a `w` by `h` picture.
    pub fn crop_or(&self, w: i32, h: i32) -> (i32, i32, i32, i32) {
        self.crop.unwrap_or((0, 0, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_geometry_counts_as_a_transform() {
        let mut o = Options::default();

        assert!(!o.transforms());
        o.bypass_filtering = true;
        o.no_fancy_upsampling = true;
        assert!(!o.transforms());
        o.flip = true;
        assert!(o.transforms());
    }

    #[test]
    fn a_crop_replaces_the_whole_picture_and_nothing_else_does() {
        let mut o = Options::default();

        assert_eq!(o.crop_or(8, 6), (0, 0, 8, 6));
        o.crop = Some((1, 2, 3, 4));
        assert_eq!(o.crop_or(8, 6), (1, 2, 3, 4));
    }
}
