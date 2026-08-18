//! Scalar kernels for the lossless (VP8L) DSP.
//!
//! These are the fallbacks the assembly replaces at runtime. They are plain
//! safe Rust; the dispatch table that selects between them and the assembly
//! lives with whichever caller consumes it — see `LOG.md` on the two DSP tiers.
//!
//! # Shapes
//!
//! The predictors take the destination row as one `&mut [u32]` and the row
//! above as `&[u32]`, with the two out-of-row neighbours — `out[-1]` and
//! `upper[-1]` in the C — passed by value as `left` and `top_left`. Carrying
//! them in registers is what the C loop effectively does anyway, and it keeps
//! the two slices disjoint even when the rows are physically adjacent, where a
//! literal transcription would have `upper` and `out` overlap by one element.
//!
//! The input row is the output row: every call site passes the same pointer
//! for both, and so does checkasm. The C prototype keeps them separate only
//! because that is the assembly's ABI.

use super::clip_uint8;

const fn avg2(a: u32, b: u32) -> u32 {
    (((a ^ b) & 0xFEFE_FEFE) >> 1) + (a & b)
}

const fn avg3(a: u32, b: u32, c: u32) -> u32 {
    avg2(avg2(a, c), b)
}

const fn avg4(a: u32, b: u32, c: u32, d: u32) -> u32 {
    avg2(avg2(a, b), avg2(c, d))
}

/// Adds the four channels independently, discarding carries between bytes.
pub const fn add_pixels(a: u32, b: u32) -> u32 {
    let ag = (a & 0xFF00_FF00).wrapping_add(b & 0xFF00_FF00);
    let rb = (a & 0x00FF_00FF).wrapping_add(b & 0x00FF_00FF);
    (ag & 0xFF00_FF00) | (rb & 0x00FF_00FF)
}

const fn sub3(a: i32, b: i32, c: i32) -> i32 {
    (b - c).abs() - (a - c).abs()
}

const fn byte(v: u32, shift: u32) -> i32 {
    ((v >> shift) & 0xFF) as i32
}

const fn select(t: u32, l: u32, tl: u32) -> u32 {
    let diff = sub3(byte(t, 24), byte(l, 24), byte(tl, 24))
        + sub3(byte(t, 16), byte(l, 16), byte(tl, 16))
        + sub3(byte(t, 8), byte(l, 8), byte(tl, 8))
        + sub3(byte(t, 0), byte(l, 0), byte(tl, 0));
    if diff <= 0 {
        t
    } else {
        l
    }
}

const fn clamped_add_sub_full(c0: u32, c1: u32, c2: u32) -> u32 {
    let a = clip_uint8(byte(c0, 24) + byte(c1, 24) - byte(c2, 24)) as u32;
    let r = clip_uint8(byte(c0, 16) + byte(c1, 16) - byte(c2, 16)) as u32;
    let g = clip_uint8(byte(c0, 8) + byte(c1, 8) - byte(c2, 8)) as u32;
    let b = clip_uint8(byte(c0, 0) + byte(c1, 0) - byte(c2, 0)) as u32;
    a << 24 | r << 16 | g << 8 | b
}

const fn add_sub_half(a: i32, b: i32) -> u32 {
    clip_uint8(a + (a - b) / 2) as u32
}

const fn clamped_add_sub_half(c0: u32, c1: u32, c2: u32) -> u32 {
    let ave = avg2(c0, c1);
    let a = add_sub_half(byte(ave, 24), byte(c2, 24));
    let r = add_sub_half(byte(ave, 16), byte(c2, 16));
    let g = add_sub_half(byte(ave, 8), byte(c2, 8));
    let b = add_sub_half(byte(ave, 0), byte(c2, 0));
    a << 24 | r << 16 | g << 8 | b
}

const BLACK: u32 = u32::from_ne_bytes([0xFF, 0x00, 0x00, 0x00]);

/// Predictor 0: the constant opaque black.
pub fn pred_add_0(out: &mut [u32]) {
    for o in out.iter_mut() {
        *o = add_pixels(*o, BLACK);
    }
}

/// Predictor 1: the pixel to the left.
pub fn pred_add_1(out: &mut [u32], left: u32) {
    let mut l = left;
    for o in out.iter_mut() {
        l = add_pixels(*o, l);
        *o = l;
    }
}

/// Predictors that read `upper[x]` but never `upper[x + 1]`.
///
/// `upper` must be at least as long as `out`.
macro_rules! pred_add {
    ($(#[$doc:meta])* $name:ident, |$l:ident, $t:ident, $tl:ident| $expr:expr) => {
        $(#[$doc])*
        #[allow(unused_variables)]
        pub fn $name(out: &mut [u32], upper: &[u32], left: u32, top_left: u32) {
            let mut l = left;
            let mut tl = top_left;

            for (o, &t) in out.iter_mut().zip(upper) {
                let v = {
                    let ($l, $t, $tl) = (l, t, tl);
                    add_pixels(*o, $expr)
                };
                *o = v;
                l = v;
                tl = t;
            }
        }
    };
}

/// Predictors that also read `upper[x + 1]`.
///
/// `upper` must be strictly longer than `out`.
macro_rules! pred_add_tr {
    ($(#[$doc:meta])* $name:ident,
     |$l:ident, $t:ident, $tl:ident, $tr:ident| $expr:expr) => {
        $(#[$doc])*
        #[allow(unused_variables)]
        pub fn $name(out: &mut [u32], upper: &[u32], left: u32, top_left: u32) {
            assert!(upper.len() > out.len());

            let mut l = left;
            let mut tl = top_left;

            for ((o, &t), &tr) in
                out.iter_mut().zip(upper).zip(upper[1..].iter())
            {
                let v = {
                    let ($l, $t, $tl, $tr) = (l, t, tl, tr);
                    add_pixels(*o, $expr)
                };
                *o = v;
                l = v;
                tl = t;
            }
        }
    };
}

pred_add!(
    /// Predictor 2: the pixel above.
    pred_add_2, |l, t, tl| t
);
pred_add_tr!(
    /// Predictor 3: the pixel above and to the right.
    pred_add_3, |l, t, tl, tr| tr
);
pred_add!(
    /// Predictor 4: the pixel above and to the left.
    pred_add_4, |l, t, tl| tl
);
pred_add_tr!(
    /// Predictor 5: `avg2(avg2(left, top_right), top)`.
    pred_add_5, |l, t, tl, tr| avg3(l, t, tr)
);
pred_add!(
    /// Predictor 6: `avg2(left, top_left)`.
    pred_add_6, |l, t, tl| avg2(l, tl)
);
pred_add!(
    /// Predictor 7: `avg2(left, top)`.
    pred_add_7, |l, t, tl| avg2(l, t)
);
pred_add!(
    /// Predictor 8: `avg2(top_left, top)`.
    pred_add_8, |l, t, tl| avg2(tl, t)
);
pred_add_tr!(
    /// Predictor 9: `avg2(top, top_right)`.
    pred_add_9, |l, t, tl, tr| avg2(t, tr)
);
pred_add_tr!(
    /// Predictor 10: `avg4(left, top_left, top, top_right)`.
    pred_add_10, |l, t, tl, tr| avg4(l, tl, t, tr)
);
pred_add!(
    /// Predictor 11: whichever of top and left is closer to top_left.
    pred_add_11, |l, t, tl| select(t, l, tl)
);
pred_add!(
    /// Predictor 12: `clamp(left + top - top_left)` per channel.
    pred_add_12, |l, t, tl| clamped_add_sub_full(l, t, tl)
);
pred_add!(
    /// Predictor 13: predictor 12 applied at half strength.
    pred_add_13, |l, t, tl| clamped_add_sub_half(l, t, tl)
);

/// Extracts the green channel of each ARGB pixel of `src` into `dst`.
pub fn extract_green(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.iter_mut().zip(src.chunks_exact(4)) {
        *d = s[2];
    }
}

/// Replaces each pixel by `palette[green]`, in place.
pub fn map_color32_inplace(buf: &mut [u8], palette: &[u32]) {
    let palette = &palette[..256];

    for p in buf.chunks_exact_mut(4) {
        p.copy_from_slice(&palette[p[2] as usize].to_ne_bytes());
    }
}

/// Replaces each pixel of `src` by `palette[green]`, writing to `dst`.
pub fn map_color32(dst: &mut [u8], src: &[u8], palette: &[u32]) {
    let palette = &palette[..256];

    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d.copy_from_slice(&palette[s[2] as usize].to_ne_bytes());
    }
}

/// Alpha-blends an ARGB row of `src` over `dst`; alpha is the low byte.
pub fn blend_row_argb(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let src_alpha = u32::from(s[0]);

        if src_alpha == 255 {
            d.copy_from_slice(s);
            continue;
        }
        if src_alpha == 0 {
            continue;
        }

        let tmp_alpha = (u32::from(d[0]) * (256 - src_alpha)) >> 8;
        let blend_alpha = src_alpha + tmp_alpha;
        let scale = (1 << 24) / blend_alpha;

        for i in 1..4 {
            let v = u32::from(s[i]) * src_alpha + u32::from(d[i]) * tmp_alpha;
            d[i] = ((v * scale) >> 24) as u8;
        }
        d[0] = blend_alpha as u8;
    }
}

/// The same as [`blend_row_argb`], for rows whose alpha is already multiplied
/// in.
pub fn blend_row_argb_premult(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        if s[0] == 255 {
            d.copy_from_slice(s);
            continue;
        }

        let scale = 256 - u32::from(s[0]);

        for i in 0..4 {
            d[i] = (u32::from(s[i]) + ((u32::from(d[i]) * scale) >> 8)) as u8;
        }
    }
}

/// Replaces each pixel by `palette[green]`, in place, on a `u32` picture.
/// The product of two `i8`s fits an `i16` exactly, and saying so is what lets
/// the row loop vectorise on 16-bit lanes instead of widening to 32. The
/// multiplier is sign-extended by the caller, once per tile rather than once
/// per pixel.
#[inline(always)]
fn color_delta(pred: i16, color: u8) -> u8 {
    (pred.wrapping_mul(i16::from(color as i8)) >> 5) as u8
}

/// Undoes the cross-colour transform over one tile's worth of a row, which
/// predicts red from green and blue from both.
///
/// `mult` packs the three signed multipliers the tile carries, at bytes three,
/// two and one; a pixel is `[A, R, G, B]` in the same byte order.
pub fn color_row(row: &mut [u32], mult: u32) {
    let cp = mult.to_ne_bytes();
    let green_to_red = i16::from(cp[3] as i8);
    let green_to_blue = i16::from(cp[2] as i8);
    let red_to_blue = i16::from(cp[1] as i8);

    for px in row {
        let mut b = px.to_ne_bytes();

        b[1] = b[1].wrapping_add(color_delta(green_to_red, b[2]));
        b[3] = b[3].wrapping_add(
            color_delta(green_to_blue, b[2])
                .wrapping_add(color_delta(red_to_blue, b[1])),
        );
        *px = u32::from_ne_bytes(b);
    }
}

pub fn map_color32_pixels(row: &mut [u32], palette: &[u32]) {
    let palette = &palette[..256];

    for p in row.iter_mut() {
        *p = palette[usize::from(p.to_ne_bytes()[2])];
    }
}

/// One predictor over a whole picture, addressed by offset.
///
/// The destination row and the row above it are two windows on the same
/// allocation, and when the picture is contiguous — which it is, because the
/// entropy decode indexes it linearly — they touch: the top-right neighbour of
/// the last pixel in a row *is* the first pixel of that row. Handing the kernel
/// two slices would either deny that or have to lie about it, so the table
/// takes the picture whole and says where in it to work.
pub type PredAddFn = fn(plane: &mut [u32], out: usize, up: usize, n: usize);

/// The runtime-selected lossless kernels the decoder calls.
pub struct Vp8lDsp {
    pub pred_add: [PredAddFn; 14],
    pub map_color32: fn(&mut [u32], &[u32]),
    pub color_row: fn(&mut [u32], u32),
    /// Writes each pixel's green channel out as a byte.
    pub extract_green: fn(&mut [u8], &[u8]),
    /// Alpha-blends one ARGB row of `src` over `dst`.
    pub blend_row_argb: fn(&mut [u8], &[u8]),
    /// The same, for rows whose alpha is already multiplied in.
    pub blend_row_argb_premult: fn(&mut [u8], &[u8]),
}

fn plane_pred_0(plane: &mut [u32], out: usize, _up: usize, n: usize) {
    pred_add_0(&mut plane[out..out + n]);
}

fn plane_pred_1(plane: &mut [u32], out: usize, _up: usize, n: usize) {
    if n == 0 {
        return;
    }
    let (head, tail) = plane.split_at_mut(out);

    pred_add_1(&mut tail[..n], head[out - 1]);
}

/// Adapts a kernel to the table's shape. The two flags say which out-of-row
/// neighbours the predictor needs, because reading one it does not — `upper[-1]`
/// on the first row of a batch, say — would step outside the picture.
macro_rules! plane_pred {
    ($name:ident, $kernel:ident, $l:literal, $tl:literal) => {
        fn $name(plane: &mut [u32], out: usize, up: usize, n: usize) {
            if n == 0 {
                return;
            }
            let (head, tail) = plane.split_at_mut(out);
            let left = if $l { head[out - 1] } else { 0 };
            let top_left = if $tl { head[up - 1] } else { 0 };

            $kernel(&mut tail[..n], &head[up..], left, top_left);
        }
    };
}

plane_pred!(plane_pred_2, pred_add_2, false, false);
plane_pred!(plane_pred_3, pred_add_3, false, false);
plane_pred!(plane_pred_4, pred_add_4, false, true);
plane_pred!(plane_pred_5, pred_add_5, true, false);
plane_pred!(plane_pred_6, pred_add_6, true, true);
plane_pred!(plane_pred_7, pred_add_7, true, false);
plane_pred!(plane_pred_8, pred_add_8, false, true);
plane_pred!(plane_pred_9, pred_add_9, false, false);
plane_pred!(plane_pred_10, pred_add_10, true, true);
plane_pred!(plane_pred_11, pred_add_11, true, true);
plane_pred!(plane_pred_12, pred_add_12, true, true);
plane_pred!(plane_pred_13, pred_add_13, true, true);

impl Vp8lDsp {
    pub const fn scalar() -> Self {
        Self {
            pred_add: [
                plane_pred_0,
                plane_pred_1,
                plane_pred_2,
                plane_pred_3,
                plane_pred_4,
                plane_pred_5,
                plane_pred_6,
                plane_pred_7,
                plane_pred_8,
                plane_pred_9,
                plane_pred_10,
                plane_pred_11,
                plane_pred_12,
                plane_pred_13,
            ],
            map_color32: map_color32_pixels,
            color_row,
            extract_green,
            blend_row_argb,
            blend_row_argb_premult,
        }
    }

    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = Self::scalar();

        #[cfg(feature = "asm")]
        crate::asm::vp8l::init(&mut table, crate::cpu::flags());
        table
    }
}

impl Default for Vp8lDsp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *state
    }

    #[test]
    fn predictor_2_adds_the_row_above() {
        let upper = [0x0001_0203u32, 0x0405_0607];
        let mut out = [0x0102_0304u32, 0x0000_0001];

        pred_add_2(&mut out, &upper, 0, 0);
        assert_eq!(out, [0x0103_0507, 0x0405_0608]);
    }

    #[test]
    fn predictor_1_chains_through_the_row() {
        let mut out = [1u32, 1, 1];

        pred_add_1(&mut out, 0);
        assert_eq!(out, [1, 2, 3]);
    }

    #[test]
    fn channels_do_not_carry_into_each_other() {
        assert_eq!(add_pixels(0x00FF_00FF, 0x0001_0001), 0x0000_0000);
    }

    #[test]
    fn map_color32_matches_its_in_place_form() {
        let mut state = 12345;
        let palette: Vec<u32> = (0..256).map(|_| lcg(&mut state)).collect();
        let src: Vec<u8> = (0..64).map(|_| lcg(&mut state) as u8).collect();
        let mut dst = vec![0u8; 64];
        let mut inplace = src.clone();

        map_color32(&mut dst, &src, &palette);
        map_color32_inplace(&mut inplace, &palette);
        assert_eq!(dst, inplace);
    }

    #[test]
    fn blending_an_opaque_row_replaces_the_destination() {
        let src = [255u8, 1, 2, 3, 255, 4, 5, 6];
        let mut dst = [0u8; 8];

        blend_row_argb(&mut dst, &src);
        assert_eq!(dst, src);
        dst = [0u8; 8];
        blend_row_argb_premult(&mut dst, &src);
        assert_eq!(dst, src);
    }

    #[test]
    fn blending_a_transparent_row_leaves_the_destination() {
        let src = [0u8; 8];
        let mut dst = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let want = dst;

        blend_row_argb(&mut dst, &src);
        assert_eq!(dst, want);
    }
}
