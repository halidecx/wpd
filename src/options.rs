/// Chroma upsampling for unscaled lossy frames converted to packed output.
/// Native planar output, lossless frames, and scaled output do not use it.
///
/// The discriminants are the C API's and the CLI's `uvmode` values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum UvMode {
    /// Copy the nearest chroma sample, without interpolation. Fastest, but
    /// can step colours at edges.
    Point = 0,
    /// The same chroma upsampling as libwebp, for identical output.
    #[default]
    Fancy = 1,
    /// A three-tap filter on centred chroma positions. Better measured
    /// quality than [`Fancy`](Self::Fancy) for a little more time.
    FastFancy = 2,
    /// A ten-tap Lanczos-5 filter on centred chroma positions. The highest
    /// effort.
    ExtraFancy = 3,
}

impl UvMode {
    /// The mode with C API and CLI value `v`, or `None` if there is none.
    #[must_use]
    pub fn from_raw(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Point,
            1 => Self::Fancy,
            2 => Self::FastFancy,
            3 => Self::ExtraFancy,
            _ => return None,
        })
    }
}

/// Processing options for [`Decoder::set_options`](crate::api::Decoder::set_options).
/// Cropping precedes scaling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    /// Skip the lossy in-loop filter. Faster, at the cost of blocking.
    pub bypass_filtering: bool,
    /// Chroma upsampling for unscaled lossy packed output.
    pub uvmode: UvMode,
    /// Crop to `(left, top, width, height)` before scaling.
    pub crop: Option<(i32, i32, i32, i32)>,
    /// Scale to `(width, height)`. A zero dimension keeps the aspect ratio.
    pub scale: Option<(i32, i32)>,
    /// Flip the output vertically.
    pub flip: bool,
    /// Threads a decode may use, counting the calling thread. Zero asks the
    /// decoder to choose, one keeps everything here.
    pub n_threads: i32,
    /// The most pixels a canvas or a scaled output may hold, 0 for no limit
    /// beyond the format's own 16384x16384. A frame over it fails with
    /// `TooLarge` before anything the size of the frame is allocated or
    /// decoded, so a caller facing untrusted input can bound the memory and
    /// time a few bytes of header may ask for; dav1d's `frame_size_limit`.
    pub frame_size_limit: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            bypass_filtering: false,
            uvmode: UvMode::Fancy,
            crop: None,
            scale: None,
            flip: false,
            n_threads: 0,
            frame_size_limit: 0,
        }
    }
}

impl Options {
    pub(crate) fn sampling(&self) -> crate::convert::Sampling {
        match self.uvmode {
            UvMode::Point => crate::convert::Sampling::Simple,
            UvMode::Fancy => crate::convert::Sampling::Fancy,
            UvMode::FastFancy => crate::convert::Sampling::FastFancy,
            UvMode::ExtraFancy => crate::convert::Sampling::ExtraFancy,
        }
    }

    /// Whether cropping, scaling, or flipping is set.
    #[must_use]
    pub fn transforms(&self) -> bool {
        self.crop.is_some() || self.scale.is_some() || self.flip
    }

    /// Whether `width` x `height` pixels fit `frame_size_limit`.
    #[must_use]
    pub fn fits(&self, width: i32, height: i32) -> bool {
        self.frame_size_limit == 0
            || u64::from(width.max(0) as u32) * u64::from(height.max(0) as u32)
                <= u64::from(self.frame_size_limit)
    }

    /// The crop rectangle, or the whole `w` x `h` picture if none is set.
    #[must_use]
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
        o.uvmode = UvMode::Point;
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

    #[test]
    fn the_size_limit_counts_pixels_and_zero_lifts_it() {
        let mut o = Options::default();

        assert!(o.fits(16384, 16384));
        o.frame_size_limit = 100;
        assert!(o.fits(10, 10));
        assert!(o.fits(100, 1));
        assert!(!o.fits(101, 1));
        assert!(!o.fits(11, 10));
        o.frame_size_limit = u32::MAX;
        assert!(o.fits(16384, 16384));
    }
}
