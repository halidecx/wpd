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
const fn add_pixels(a: u32, b: u32) -> u32 {
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

const fn clip_uint8(v: i32) -> u32 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u32
    }
}

const fn clamped_add_sub_full(c0: u32, c1: u32, c2: u32) -> u32 {
    let a = clip_uint8(byte(c0, 24) + byte(c1, 24) - byte(c2, 24));
    let r = clip_uint8(byte(c0, 16) + byte(c1, 16) - byte(c2, 16));
    let g = clip_uint8(byte(c0, 8) + byte(c1, 8) - byte(c2, 8));
    let b = clip_uint8(byte(c0, 0) + byte(c1, 0) - byte(c2, 0));
    a << 24 | r << 16 | g << 8 | b
}

const fn add_sub_half(a: i32, b: i32) -> u32 {
    clip_uint8(a + (a - b) / 2)
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
