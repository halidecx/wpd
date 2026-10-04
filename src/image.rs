use std::ops::Range;

use crate::error::{Error, Result};

pub const FILE_PADDING: usize = 64;

pub const MAX_SCALED: i32 = 16384;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Format {
    Yuv420p,
    Yuva420p,
    #[default]
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

pub fn ceil_rshift(v: i32, shift: u32) -> i32 {
    -((-v) >> shift)
}

pub fn plane_shift(p: usize) -> u32 {
    u32::from(p == 1 || p == 2)
}

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
    if w <= 0
        || h <= 0
        || w > i64::from(MAX_SCALED)
        || h > i64::from(MAX_SCALED)
        || u64::from(w as u32) * u64::from(h as u32) >= 1u64 << 32
    {
        return Err(Error::TooLarge);
    }
    Ok((w as i32, h as i32))
}

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

    if crop.left < 0 || crop.top < 0 || crop.width <= 0 || crop.height <= 0 {
        return Err(Error::InvalidData);
    }
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

pub fn stride_magnitude(stride: isize) -> usize {
    if stride < 0 {
        (-(stride + 1)) as usize + 1
    } else {
        stride as usize
    }
}

pub fn external_plane_fits(
    size: usize,
    stride: isize,
    row: usize,
    height: i32,
) -> bool {
    let advance = stride_magnitude(stride);

    if height <= 0 || advance < row || size < row {
        return false;
    }
    match advance {
        0 => height == 1,
        // Only the used bytes of the last row need storage. Divide instead
        // of multiplying so even an unaddressable stride cannot overflow.
        _ => (height - 1) as usize <= (size - row) / advance,
    }
}

enum Mix {
    TakeSrc,
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

/* Animation frames are mostly made of runs that are wholly opaque or wholly
 * clear, which need no arithmetic; runs of this many samples are sorted by
 * their source alphas before any is blended one by one. */
const RUN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Opaque,
    Clear,
    Mixed,
}

/// Whether a run of alphas is all opaque, and whether any of it is not clear.
fn run_alpha(alpha: &[u8]) -> (bool, bool) {
    let mut runs = alpha.chunks_exact(RUN);
    let (mut all, mut any) = (true, false);

    /* A byte-wise AND and OR are not vectorised; one wide word is a load and
     * a compare. */
    for run in &mut runs {
        let word = u128::from_ne_bytes(run.try_into().unwrap_or_default());

        all &= word == u128::MAX;
        any |= word != 0;
    }
    for &a in runs.remainder() {
        all &= a == 0xff;
        any |= a != 0;
    }
    (all, any)
}

fn run_class((all, any): (bool, bool)) -> Run {
    match (all, any) {
        (true, _) => Run::Opaque,
        (_, false) => Run::Clear,
        _ => Run::Mixed,
    }
}

/// Hands `each` the spans of `0..n` over which consecutive runs sort alike,
/// so that a row wholly opaque or clear is one copy or none.
fn stretches(
    n: usize,
    class: impl Fn(Range<usize>) -> Run,
    mut each: impl FnMut(Run, Range<usize>),
) {
    let mut start = 0;
    let mut current = class(0..RUN.min(n));
    let mut x = RUN;

    while x < n {
        let end = (x + RUN).min(n);
        let next = class(x..end);

        if next != current {
            each(current, start..x);
            (start, current) = (x, next);
        }
        x = end;
    }
    if n > 0 {
        each(current, start..n);
    }
}

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

    stretches(
        n,
        |span| run_class(run_alpha(&src_a[span])),
        |class, span| {
            let (dy, da) = (&mut dst_y[span.clone()], &mut dst_a[span.clone()]);
            let (sy, sa) = (&src_y[span.clone()], &src_a[span]);

            match class {
                Run::Opaque => {
                    dy.copy_from_slice(sy);
                    da.fill(0xff);
                }
                Run::Clear => {}
                Run::Mixed => {
                    for (((dy, da), sy), sa) in
                        dy.iter_mut().zip(da.iter_mut()).zip(sy).zip(sa)
                    {
                        let m = mix(*sa, *da);

                        *dy = m.apply(*dy, *sy);
                        *da = m.alpha(*da);
                    }
                }
            }
        },
    );
}

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
    let full = if width % 2 == 0 {
        n
    } else {
        n.saturating_sub(1)
    };

    stretches(
        full,
        |span| {
            run_class(src_alpha.iter().fold((true, false), |(all, any), row| {
                let (a, o) = run_alpha(&row[span.start * 2..span.end * 2]);

                (all && a, any || o)
            }))
        },
        |class, span| match class {
            Run::Opaque => {
                dst_u[span.clone()].copy_from_slice(&src_u[span.clone()]);
                dst_v[span.clone()].copy_from_slice(&src_v[span]);
            }
            Run::Clear => {}
            Run::Mixed => {
                for x in span {
                    let m = mix(
                        block_alpha::<ROWS, 2>(src_alpha, x),
                        block_alpha::<ROWS, 2>(dst_alpha, x),
                    );

                    dst_u[x] = m.apply(dst_u[x], src_u[x]);
                    dst_v[x] = m.apply(dst_v[x], src_v[x]);
                }
            }
        },
    );
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
        assert_eq!(scaled_size(0, 3, 1_431_655_766, 1), Err(Error::TooLarge));
        assert_eq!(scaled_size(3, 0, 1, 1_431_655_766), Err(Error::TooLarge));
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
    fn a_crop_with_a_negative_corner_or_an_empty_extent_is_rejected() {
        for (left, top, width, height) in [
            (-1, 0, 4, 4),
            (0, -1, 4, 4),
            (0, 0, 0, 4),
            (0, 0, 4, -4),
            (i32::MIN, 0, 4, 4),
        ] {
            let crop = Crop {
                left,
                top,
                width,
                height,
            };

            assert_eq!(crop_origin(&crop, 32, 32, true), Err(Error::InvalidData));
            assert_eq!(crop_origin(&crop, 32, 32, false), Err(Error::InvalidData));
        }
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
    fn opaque_and_clear_runs_blend_as_their_samples_would() {
        /* Runs by index: two opaque, two clear, a mix of 0 and 255, one of
         * alphas between, then opaque, clear, opaque and a short tail. */
        let alpha = |run: usize, i: usize| match run {
            0 | 1 | 6 | 8 => 255,
            2 | 3 | 7 => 0,
            4 => [0, 255][i % 3 / 2],
            _ => 60 + i as u8,
        };
        let n = RUN * 9 + 5;
        let src_a: Vec<u8> = (0..n).map(|i| alpha(i / RUN, i)).collect();
        let src_y: Vec<u8> = (0..n).map(|i| (3 * i) as u8).collect();
        let mut dst_y: Vec<u8> = (0..n).map(|i| 200 - i as u8).collect();
        let mut dst_a: Vec<u8> = (0..n).map(|i| (5 * i) as u8).collect();
        let mut want = (dst_y.clone(), dst_a.clone());

        for i in 0..n {
            let m = mix(src_a[i], want.1[i]);

            want.0[i] = m.apply(want.0[i], src_y[i]);
            want.1[i] = m.alpha(want.1[i]);
        }
        blend_row_ya(&mut dst_y, &mut dst_a, &src_y, &src_a);
        assert_eq!((dst_y, dst_a), want);
    }

    #[test]
    fn chroma_runs_blend_as_their_samples_would() {
        /* Chroma runs by index: two opaque, one clear, one mixed and a
         * short tail of alphas between. */
        let alpha = |run: usize, x: usize| match run {
            0 | 1 => 255,
            2 => 0,
            3 => [0, 255][x % 5 / 3],
            _ => 40 + x as u8,
        };
        let n = RUN * 4 + 11;
        let rows: Vec<Vec<u8>> = (0..2)
            .map(|y| (0..n * 2).map(|x| alpha(x / 2 / RUN, x + y)).collect())
            .collect();
        let dst_rows: Vec<Vec<u8>> = (0..2)
            .map(|y| (0..n * 2).map(|x| (x * 7 + y * 3) as u8).collect())
            .collect();
        let src_alpha = [&rows[0][..], &rows[1][..]];
        let dst_alpha = [&dst_rows[0][..], &dst_rows[1][..]];
        let src_u: Vec<u8> = (0..n).map(|x| (3 * x) as u8).collect();
        let src_v: Vec<u8> = (0..n).map(|x| 250 - x as u8).collect();
        let mut dst_u: Vec<u8> = (0..n).map(|x| 90 + x as u8).collect();
        let mut dst_v: Vec<u8> = (0..n).map(|x| (5 * x) as u8).collect();
        let mut want = (dst_u.clone(), dst_v.clone());

        for x in 0..n {
            let m = mix(
                block_alpha::<2, 2>(&src_alpha, x),
                block_alpha::<2, 2>(&dst_alpha, x),
            );

            want.0[x] = m.apply(want.0[x], src_u[x]);
            want.1[x] = m.apply(want.1[x], src_v[x]);
        }
        blend_row_uv(
            &mut dst_u,
            &mut dst_v,
            &src_u,
            &src_v,
            &src_alpha,
            &dst_alpha,
            n * 2,
        );
        assert_eq!((dst_u, dst_v), want);
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
        assert_eq!(dst_u, [200, 10]);
        assert_eq!(dst_v, [100, 20]);
    }

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

    #[test]
    fn external_planes_need_no_padding_after_the_last_row() {
        for stride in [16, -16] {
            assert!(external_plane_fits(40, stride, 8, 3));
            assert!(!external_plane_fits(39, stride, 8, 3));
            assert!(external_plane_fits(8, stride, 8, 1));
            assert!(!external_plane_fits(7, stride, 8, 1));
        }
        assert!(!external_plane_fits(40, 16, 8, 0));
        assert!(!external_plane_fits(40, 16, 8, -1));
        assert!(external_plane_fits(1, isize::MIN, 1, 1));
        assert!(!external_plane_fits(isize::MAX as usize, isize::MIN, 1, 2));
        assert!(!external_plane_fits(usize::MAX, isize::MAX, 8, 4));
    }

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
