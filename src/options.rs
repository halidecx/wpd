#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Options {
    pub bypass_filtering: bool,
    pub no_fancy_upsampling: bool,
    pub crop: Option<(i32, i32, i32, i32)>,
    pub scale: Option<(i32, i32)>,
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

impl Options {
    pub fn transforms(&self) -> bool {
        self.crop.is_some() || self.scale.is_some() || self.flip
    }

    /// Whether `width` x `height` pixels fit `frame_size_limit`.
    pub fn fits(&self, width: i32, height: i32) -> bool {
        self.frame_size_limit == 0
            || u64::from(width.max(0) as u32) * u64::from(height.max(0) as u32)
                <= u64::from(self.frame_size_limit)
    }

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
