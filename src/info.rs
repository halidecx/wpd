use crate::container::Coding;

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
    pub metadata: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub pos_x: i32,
    pub pos_y: i32,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub dispose_to_background: bool,
    pub blend: bool,
    pub has_alpha: bool,
    pub complete: bool,
}

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
