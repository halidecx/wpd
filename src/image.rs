//! Pixel format policy, plane geometry, and the blend kernels the animation
//! compositor runs over YUVA.
//!
//! The C still owns the `WebPImage` struct and makes crop and flip views of it
//! by pointer arithmetic, so what lives here is everything that does not need
//! a pointer: the format predicates, the allocation and scale arithmetic that
//! a damaged header can drive, and the per-row kernels.

use crate::error::{Error, Result};

/// The slack every plane allocation carries past its last row, so a kernel
/// that reads a word at a time never runs off the end.
pub const FILE_PADDING: usize = 64;

/// The limit `scaled_size` puts on either output dimension.
pub const MAX_SCALED: i32 = 16384;

/// `WPDPixelFormat` from `include/wpd.h`, less `NONE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Yuv420p,
    Yuva420p,
    Argb,
    Rgba,
    Bgra,
    Rgb,
    Bgr,
    ArgbPre,
    RgbaPre,
    BgraPre,
    Rgb565,
    Rgba4444,
    Rgba4444Pre,
    Bgr565,
    Bgra4444,
    Bgra4444Pre,
}

impl Format {
    pub fn from_raw(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Yuv420p,
            1 => Self::Yuva420p,
            2 => Self::Argb,
            3 => Self::Rgba,
            4 => Self::Bgra,
            5 => Self::Rgb,
            6 => Self::Bgr,
            7 => Self::ArgbPre,
            8 => Self::RgbaPre,
            9 => Self::BgraPre,
            10 => Self::Rgb565,
            11 => Self::Rgba4444,
            12 => Self::Rgba4444Pre,
            13 => Self::Bgr565,
            14 => Self::Bgra4444,
            15 => Self::Bgra4444Pre,
            _ => return None,
        })
    }

    /// Packed formats sort after the two planar ones, which is what the C's
    /// `format >= WPD_PIX_FMT_ARGB` was testing.
    pub fn is_packed(self) -> bool {
        !matches!(self, Self::Yuv420p | Self::Yuva420p)
    }

    pub fn bpp(self) -> usize {
        match self {
            Self::Rgb565
            | Self::Rgba4444
            | Self::Rgba4444Pre
            | Self::Bgr565
            | Self::Bgra4444
            | Self::Bgra4444Pre => 2,
            Self::Rgb | Self::Bgr => 3,
            _ => 4,
        }
    }

    pub fn is_premultiplied(self) -> bool {
        matches!(
            self,
            Self::ArgbPre
                | Self::RgbaPre
                | Self::BgraPre
                | Self::Rgba4444Pre
                | Self::Bgra4444Pre
        )
    }

    /// The byte layout the upsampler can emit for this format without a second
    /// pass; the numbering is `WPD_LAYOUT_*` from `src/yuvdsp.h`.
    pub fn layout(self) -> usize {
        match self {
            Self::Rgba | Self::RgbaPre => crate::dsp::yuv::LAYOUT_RGBA,
            Self::Bgra | Self::BgraPre => crate::dsp::yuv::LAYOUT_BGRA,
            Self::Rgb => crate::dsp::yuv::LAYOUT_RGB,
            Self::Bgr => crate::dsp::yuv::LAYOUT_BGR,
            _ => crate::dsp::yuv::LAYOUT_ARGB,
        }
    }

    pub fn nb_components(self) -> usize {
        match self {
            Self::Yuv420p => 3,
            Self::Yuva420p => 4,
            _ => 1,
        }
    }
}

/// Rounds `v` up rather than down when shifting right, which is what the C's
/// `CEIL_RSHIFT` did without the double negation being obvious.
pub fn ceil_rshift(v: i32, shift: u32) -> i32 {
    -((-v) >> shift)
}

/// How far plane `p` is subsampled from the picture: planes one and two are
/// the chroma pair, half the picture each way, and luma and alpha are neither.
pub fn plane_shift(p: usize) -> u32 {
    u32::from(p == 1 || p == 2)
}

/// The byte count one plane of `w` by `h` samples at `bpp` needs, padding
/// included, or `TooLarge` when the multiplication would not fit.
pub fn plane_size(w: i32, h: i32, bpp: usize) -> Result<usize> {
    if w <= 0 || h <= 0 || bpp == 0 {
        return Err(Error::TooLarge);
    }
    let row = (w as usize).checked_mul(bpp).ok_or(Error::TooLarge)?;

    if row > i32::MAX as usize {
        return Err(Error::TooLarge);
    }
    row.checked_mul(h as usize)
        .and_then(|n| n.checked_add(FILE_PADDING))
        .ok_or(Error::TooLarge)
}

/// The output size a scaled decode lands on, resolving a zero dimension
/// against the aspect ratio the other one implies.
pub fn scaled_size(
    scaled_width: i32,
    scaled_height: i32,
    src_width: i32,
    src_height: i32,
) -> Result<(i32, i32)> {
    if src_width <= 0 || src_height <= 0 {
        return Err(Error::TooLarge);
    }
    let mut w = i64::from(scaled_width);
    let mut h = i64::from(scaled_height);

    if w == 0 {
        w = (i64::from(src_width) * h + i64::from(src_height) - 1)
            / i64::from(src_height);
    }
    if h == 0 {
        h = (i64::from(src_height) * w + i64::from(src_width) - 1)
            / i64::from(src_width);
    }
    let (w, h) = (w as i32, h as i32);

    if w <= 0
        || h <= 0
        || w > MAX_SCALED
        || h > MAX_SCALED
        || u64::from(w as u32) * u64::from(h as u32) >= 1u64 << 32
    {
        return Err(Error::TooLarge);
    }
    Ok((w, h))
}

/// Where a crop rectangle starts in each plane, once it has been checked
/// against the source.
///
/// A planar source rounds the corner down to an even sample so the chroma
/// offset stays exact, which is the `& ~1` the C applied before validating.
pub struct Crop {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

pub fn crop_origin(
    crop: &Crop,
    src_width: i32,
    src_height: i32,
    packed: bool,
) -> Result<(i32, i32)> {
    let align = if packed { 0 } else { 1 };
    let left = crop.left & !align;
    let top = crop.top & !align;

    if left > src_width
        || top > src_height
        || crop.width > src_width - left
        || crop.height > src_height - top
    {
        return Err(Error::InvalidData);
    }
    Ok((left, top))
}

/// How far one row advances in a caller's buffer, whichever direction it runs.
///
/// A negative stride is negated in `usize` rather than `isize`, so the most
/// negative stride has a magnitude too.
pub fn stride_magnitude(stride: isize) -> usize {
    if stride < 0 {
        (-(stride + 1)) as usize + 1
    } else {
        stride as usize
    }
}

/// Whether a caller's plane has room for `height` rows of `row` bytes at
/// `stride`.
///
/// The division the C did here had no guard on a zero stride: a zero-width
/// image reached it with both operands zero, which passed the first test and
/// divided by zero in the second. A plane that advances by nothing holds one
/// row at most, and that is what this says.
pub fn external_plane_fits(
    size: usize,
    stride: isize,
    row: usize,
    height: i32,
) -> bool {
    let advance = stride_magnitude(stride);

    if advance < row {
        return false;
    }
    match advance {
        0 => height <= 1,
        _ => (height as usize) <= size / advance,
    }
}

/// How a source alpha combines with a destination one, worked out once so a
/// pair of channels sharing an alpha shares the reciprocal too: the divide
/// dominates the blend, and chroma runs it for U and V together.
enum Mix {
    /// The source covers the sample completely.
    TakeSrc,
    /// The source is fully transparent.
    KeepDst,
    Blend {
        src_alpha: u32,
        tmp_alpha: u32,
        scale: u32,
        blend_alpha: u8,
    },
}

fn mix(src_alpha: u8, dst_alpha: u8) -> Mix {
    if src_alpha == 255 {
        return Mix::TakeSrc;
    }
    if src_alpha == 0 {
        return Mix::KeepDst;
    }
    let tmp_alpha = (u32::from(dst_alpha) * (256 - u32::from(src_alpha))) >> 8;
    let blend_alpha = u32::from(src_alpha) + tmp_alpha;

    Mix::Blend {
        src_alpha: u32::from(src_alpha),
        tmp_alpha,
        scale: (1u32 << 24) / blend_alpha,
        blend_alpha: blend_alpha as u8,
    }
}

impl Mix {
    fn apply(&self, dst: u8, src: u8) -> u8 {
        match *self {
            Self::TakeSrc => src,
            Self::KeepDst => dst,
            Self::Blend {
                src_alpha,
                tmp_alpha,
                scale,
                ..
            } => {
                let weighted = u32::from(src) * src_alpha + u32::from(dst) * tmp_alpha;

                ((weighted * scale) >> 24) as u8
            }
        }
    }

    fn alpha(&self, dst_alpha: u8) -> u8 {
        match *self {
            Self::TakeSrc => 255,
            Self::KeepDst => dst_alpha,
            Self::Blend { blend_alpha, .. } => blend_alpha,
        }
    }
}

/// Blends one row of luma and alpha, sample for sample.
pub fn blend_row_ya(dst_y: &mut [u8], dst_a: &mut [u8], src_y: &[u8], src_a: &[u8]) {
    let n = dst_y
        .len()
        .min(dst_a.len())
        .min(src_y.len())
        .min(src_a.len());
    let dst_y = &mut dst_y[..n];
    let dst_a = &mut dst_a[..n];
    let src_y = &src_y[..n];
    let src_a = &src_a[..n];

    for (((dy, da), sy), sa) in
        dst_y.iter_mut().zip(dst_a.iter_mut()).zip(src_y).zip(src_a)
    {
        let m = mix(*sa, *da);

        *dy = m.apply(*dy, *sy);
        *da = m.alpha(*da);
    }
}

/// The alpha a 2x2 block averages to, over the rows it spans and the one or
/// two columns it covers.
///
/// Both counts are known at the call, so they are constants here and the
/// summation unrolls; a dynamic bound left the whole chroma pass walking two
/// nested loops per sample.
fn block_alpha<const ROWS: usize, const COLS: usize>(
    rows: &[&[u8]; ROWS],
    x: usize,
) -> u8 {
    let mut sum = 0u32;

    for row in rows {
        for &a in &row[x * 2..x * 2 + COLS] {
            sum += u32::from(a);
        }
    }
    let shift = u32::from(ROWS == 2) + u32::from(COLS == 2);

    ceil_rshift(sum as i32, shift) as u8
}

/// Blends one row of a chroma pair, weighted by the alpha of the 2x2 block
/// each sample covers.
///
/// `src_alpha` and `dst_alpha` are the alpha rows the block spans, top first;
/// a block at the bottom edge of an odd-height region passes one row. `width`
/// counts luma samples, not chroma ones.
pub fn blend_row_uv<const ROWS: usize>(
    dst_u: &mut [u8],
    dst_v: &mut [u8],
    src_u: &[u8],
    src_v: &[u8],
    src_alpha: &[&[u8]; ROWS],
    dst_alpha: &[&[u8]; ROWS],
    width: usize,
) {
    let n = width
        .div_ceil(2)
        .min(dst_u.len())
        .min(dst_v.len())
        .min(src_u.len())
        .min(src_v.len());
    /* Only an odd width leaves a half block, and only at the far end, so the
    body runs on whole ones and the tail is at most a single sample. */
    let full = if width % 2 == 0 {
        n
    } else {
        n.saturating_sub(1)
    };

    for x in 0..full {
        let m = mix(
            block_alpha::<ROWS, 2>(src_alpha, x),
            block_alpha::<ROWS, 2>(dst_alpha, x),
        );

        dst_u[x] = m.apply(dst_u[x], src_u[x]);
        dst_v[x] = m.apply(dst_v[x], src_v[x]);
    }
    for x in full..n {
        let m = mix(
            block_alpha::<ROWS, 1>(src_alpha, x),
            block_alpha::<ROWS, 1>(dst_alpha, x),
        );

        dst_u[x] = m.apply(dst_u[x], src_u[x]);
        dst_v[x] = m.apply(dst_v[x], src_v[x]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_that_does_not_fit_in_a_size_t_is_too_large() {
        assert_eq!(plane_size(1, 1, 4), Ok(4 + FILE_PADDING));
        assert_eq!(
            plane_size(16384, 16384, 4),
            Ok(16384 * 16384 * 4 + FILE_PADDING)
        );
        assert_eq!(plane_size(0, 8, 4), Err(Error::TooLarge));
        assert_eq!(plane_size(8, -1, 4), Err(Error::TooLarge));
        assert_eq!(plane_size(i32::MAX, 8, 4), Err(Error::TooLarge));
    }

    #[test]
    fn a_zero_scaled_dimension_follows_the_aspect_ratio() {
        assert_eq!(scaled_size(0, 50, 200, 100), Ok((100, 50)));
        assert_eq!(scaled_size(100, 0, 200, 100), Ok((100, 50)));
        assert_eq!(scaled_size(40, 30, 200, 100), Ok((40, 30)));
        assert_eq!(scaled_size(0, 0, 200, 100), Err(Error::TooLarge));
        assert_eq!(scaled_size(16385, 10, 200, 100), Err(Error::TooLarge));
        assert_eq!(scaled_size(0, 1, 1, 0), Err(Error::TooLarge));
    }

    #[test]
    fn a_planar_crop_corner_rounds_down_to_an_even_sample() {
        let crop = Crop {
            left: 5,
            top: 7,
            width: 10,
            height: 10,
        };

        assert_eq!(crop_origin(&crop, 32, 32, false), Ok((4, 6)));
        assert_eq!(crop_origin(&crop, 32, 32, true), Ok((5, 7)));
    }

    #[test]
    fn a_crop_that_runs_past_the_source_is_rejected() {
        let crop = Crop {
            left: 0,
            top: 0,
            width: 33,
            height: 10,
        };

        assert_eq!(crop_origin(&crop, 32, 32, true), Err(Error::InvalidData));

        let crop = Crop {
            left: 40,
            top: 0,
            width: 1,
            height: 1,
        };

        assert_eq!(crop_origin(&crop, 32, 32, true), Err(Error::InvalidData));
    }

    #[test]
    fn an_opaque_source_replaces_and_a_clear_one_leaves_the_destination() {
        let m = mix(255, 200);

        assert_eq!((m.apply(10, 90), m.alpha(200)), (90, 255));

        let m = mix(0, 200);

        assert_eq!((m.apply(10, 90), m.alpha(200)), (10, 200));
    }

    #[test]
    fn blending_over_an_empty_destination_keeps_the_source() {
        let m = mix(128, 0);

        assert_eq!(m.alpha(0), 128);
        assert_eq!(m.apply(0, 137), 137);
    }

    #[test]
    fn a_luma_row_blends_sample_for_sample() {
        let mut dst_y = [0u8, 50, 100];
        let mut dst_a = [0u8, 255, 255];
        let src_y = [137u8, 20, 200];
        let src_a = [128u8, 0, 255];

        blend_row_ya(&mut dst_y, &mut dst_a, &src_y, &src_a);
        assert_eq!(dst_y, [137, 50, 200]);
        assert_eq!(dst_a, [128, 255, 255]);
    }

    #[test]
    fn a_chroma_sample_averages_the_block_alpha_it_covers() {
        let mut dst_u = [10u8, 10];
        let mut dst_v = [20u8, 20];
        let src_u = [200u8, 200];
        let src_v = [100u8, 100];
        let src_a_rows: [&[u8]; 2] = [&[255, 255, 0, 0], &[255, 255, 0, 0]];
        let dst_a_rows: [&[u8]; 2] = [&[0, 0, 0, 0], &[0, 0, 0, 0]];

        blend_row_uv(
            &mut dst_u,
            &mut dst_v,
            &src_u,
            &src_v,
            &src_a_rows,
            &dst_a_rows,
            4,
        );
        /* The left block is fully covered, so it takes the source; the right
        block is fully transparent, so it keeps what was there. */
        assert_eq!(dst_u, [200, 10]);
        assert_eq!(dst_v, [100, 20]);
    }

    /// The half block at an odd width is averaged over the samples it really
    /// covers, so a covered left column reads as covered rather than half so.
    #[test]
    fn an_odd_width_averages_its_last_block_over_one_column() {
        let mut dst_u = [10u8, 10];
        let mut dst_v = [20u8, 20];
        let src_u = [200u8, 200];
        let src_v = [100u8, 100];
        let src_a_rows: [&[u8]; 2] = [&[0, 0, 255], &[0, 0, 255]];
        let dst_a_rows: [&[u8]; 2] = [&[0, 0, 0], &[0, 0, 0]];

        blend_row_uv(
            &mut dst_u,
            &mut dst_v,
            &src_u,
            &src_v,
            &src_a_rows,
            &dst_a_rows,
            3,
        );
        assert_eq!(dst_u, [10, 200]);
        assert_eq!(dst_v, [20, 100]);
    }

    #[test]
    fn a_plane_that_advances_by_nothing_holds_one_row() {
        assert!(external_plane_fits(0, 0, 0, 1));
        assert!(!external_plane_fits(0, 0, 0, 2));
        assert!(external_plane_fits(40, 10, 10, 4));
        assert!(!external_plane_fits(40, 10, 10, 5));
        assert!(!external_plane_fits(4000, 4, 10, 1));
    }

    #[test]
    fn a_negative_stride_advances_as_far_as_a_positive_one() {
        assert_eq!(stride_magnitude(-10), 10);
        assert_eq!(stride_magnitude(10), 10);
        assert_eq!(stride_magnitude(0), 0);
        assert!(external_plane_fits(40, -10, 10, 4));
    }

    /// The bottom edge of an odd-height region spans one row, and the average
    /// shifts by one less because of it.
    #[test]
    fn an_odd_height_block_averages_over_the_single_row_it_spans() {
        let mut dst_u = [10u8];
        let mut dst_v = [20u8];
        let src_u = [200u8];
        let src_v = [100u8];
        let src_a_rows: [&[u8]; 1] = [&[255, 255]];
        let dst_a_rows: [&[u8]; 1] = [&[0, 0]];

        blend_row_uv(
            &mut dst_u,
            &mut dst_v,
            &src_u,
            &src_v,
            &src_a_rows,
            &dst_a_rows,
            2,
        );
        assert_eq!(dst_u, [200]);
        assert_eq!(dst_v, [100]);
    }
}
