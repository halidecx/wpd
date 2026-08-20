#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Options {
    pub bypass_filtering: bool,
    pub no_fancy_upsampling: bool,
    pub crop: Option<(i32, i32, i32, i32)>,
    pub scale: Option<(i32, i32)>,
    pub flip: bool,
}

impl Options {
    pub fn transforms(&self) -> bool {
        self.crop.is_some() || self.scale.is_some() || self.flip
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
}
