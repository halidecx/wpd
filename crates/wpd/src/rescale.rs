//! The row-at-a-time area rescaler, matching libwebp's `WebPRescaler` bit for
//! bit.
//!
//! The arithmetic is unsigned and deliberately wrapping in several places —
//! `left - right` in the expanding importer, `sum * x_sub - frac` in the
//! shrinking one — so every operation here is spelled out as wrapping rather
//! than left to the profile's overflow checks.

const RFIX: u32 = 32;
const ONE: u64 = 1 << RFIX;
const ROUNDER: u64 = ONE >> 1;

fn mult_fix(x: u32, y: u32) -> u32 {
    ((u64::from(x) * u64::from(y) + ROUNDER) >> RFIX) as u32
}

fn mult_fix_floor(x: u32, y: u32) -> u32 {
    ((u64::from(x) * u64::from(y)) >> RFIX) as u32
}

/// `x / y` in 32.32 fixed point.
pub fn frac(x: u32, y: u32) -> u32 {
    ((u64::from(x) << RFIX) / u64::from(y)) as u32
}

/// What the importers need from the rescaler state.
#[derive(Clone, Copy)]
pub struct Import {
    pub num_channels: usize,
    pub src_width: usize,
    pub dst_width: usize,
    pub x_add: u32,
    pub x_sub: u32,
    pub fx_scale: u32,
}

/// What the exporters need from it.
#[derive(Clone, Copy)]
pub struct Export {
    pub y_accum: i32,
    pub y_sub: u32,
    pub fy_scale: u32,
    pub fxy_scale: u32,
}

fn clip8(v: u32) -> u8 {
    if v > 255 {
        255
    } else {
        v as u8
    }
}

/// Widens one source row into `frow`.
pub fn import_row_expand(frow: &mut [u32], src: &[u8], p: Import) {
    let stride = p.num_channels;
    let x_out_max = p.dst_width * stride;

    for channel in 0..stride {
        let mut x_in = channel;
        let mut x_out = channel;
        let mut accum = p.x_add as i32;
        let mut leftv = u32::from(src[x_in]);
        let mut right = if p.src_width > 1 {
            u32::from(src[x_in + stride])
        } else {
            leftv
        };

        x_in += stride;
        loop {
            frow[x_out] = right
                .wrapping_mul(p.x_add)
                .wrapping_add(leftv.wrapping_sub(right).wrapping_mul(accum as u32));
            x_out += stride;
            if x_out >= x_out_max {
                break;
            }
            accum -= p.x_sub as i32;
            if accum < 0 {
                leftv = right;
                x_in += stride;
                right = u32::from(src[x_in]);
                accum += p.x_add as i32;
            }
        }
    }
}

/// Narrows one source row into `frow`, accumulating the fractional tail.
pub fn import_row_shrink(frow: &mut [u32], src: &[u8], p: Import) {
    let stride = p.num_channels;
    let x_out_max = p.dst_width * stride;

    for channel in 0..stride {
        let mut x_in = channel;
        let mut x_out = channel;
        let mut sum = 0u32;
        let mut accum = 0i32;

        while x_out < x_out_max {
            let mut base = 0u32;

            accum += p.x_add as i32;
            while accum > 0 {
                accum -= p.x_sub as i32;
                base = u32::from(src[x_in]);
                sum = sum.wrapping_add(base);
                x_in += stride;
            }

            let fract = base.wrapping_mul((-accum) as u32);

            frow[x_out] = sum.wrapping_mul(p.x_sub).wrapping_sub(fract);
            sum = mult_fix(fract, p.fx_scale);
            x_out += stride;
        }
    }
}

/// Adds the just-imported row into the accumulator, for the shrinking case.
pub fn accumulate(irow: &mut [u32], frow: &[u32]) {
    for (i, f) in irow.iter_mut().zip(frow) {
        *i = i.wrapping_add(*f);
    }
}

/// Emits one output row, interpolating between the two buffered rows.
pub fn export_row_expand(dst: &mut [u8], irow: &[u32], frow: &[u32], p: Export) {
    if p.y_accum == 0 {
        for (d, &f) in dst.iter_mut().zip(frow) {
            *d = clip8(mult_fix(f, p.fy_scale));
        }
        return;
    }

    let b = frac((-p.y_accum) as u32, p.y_sub);
    let a = 0u32.wrapping_sub(b);

    for ((d, &f), &i) in dst.iter_mut().zip(frow).zip(irow) {
        let acc = u64::from(a) * u64::from(f) + u64::from(b) * u64::from(i);
        let j = ((acc + ROUNDER) >> RFIX) as u32;

        *d = clip8(mult_fix(j, p.fy_scale));
    }
}

/// Emits one output row, leaving the part of the accumulator that belongs to
/// the next one behind in `irow`.
pub fn export_row_shrink(dst: &mut [u8], irow: &mut [u32], frow: &[u32], p: Export) {
    let yscale = p.fy_scale.wrapping_mul((-p.y_accum) as u32);

    if yscale != 0 {
        for ((d, i), &f) in dst.iter_mut().zip(irow.iter_mut()).zip(frow) {
            let fract = mult_fix_floor(f, yscale);

            *d = clip8(mult_fix(i.wrapping_sub(fract), p.fxy_scale));
            *i = fract;
        }
    } else {
        for (d, i) in dst.iter_mut().zip(irow.iter_mut()) {
            *d = clip8(mult_fix(*i, p.fxy_scale));
            *i = 0;
        }
    }
}

/// The degenerate case where the ratio does not fit in 32.32: `src_width == 1`
/// with `dst_width <= 2`.
pub fn export_row_direct(dst: &mut [u8], irow: &mut [u32]) {
    for (d, i) in dst.iter_mut().zip(irow.iter_mut()) {
        *d = *i as u8;
        *i = 0;
    }
}

const MFIX: u32 = 24;
const MHALF: u32 = 1 << (MFIX - 1);
const KINV_255: u32 = (1 << MFIX) / 255;

fn alpha_mult(x: u8, scale: u32) -> u8 {
    ((u32::from(x).wrapping_mul(scale).wrapping_add(MHALF)) >> MFIX) as u8
}

fn alpha_scale(a: u8, inverse: bool) -> u32 {
    if inverse {
        (255 << MFIX) / u32::from(a)
    } else {
        u32::from(a) * KINV_255
    }
}

/// Multiplies alpha into an ARGB row, or divides it back out.
pub fn premultiply_argb_row(argb: &mut [u8], inverse: bool) {
    for p in argb.chunks_exact_mut(4) {
        let a = p[0];

        if a == 0xFF {
            continue;
        }
        if a == 0 {
            p[1..4].fill(0);
            continue;
        }

        let scale = alpha_scale(a, inverse);

        for v in &mut p[1..4] {
            *v = alpha_mult(*v, scale);
        }
    }
}

/// The same against a separate alpha plane.
pub fn multiply_row(plane: &mut [u8], alpha: &[u8], inverse: bool) {
    for (p, &a) in plane.iter_mut().zip(alpha) {
        if a == 255 {
            continue;
        }
        *p = if a != 0 {
            alpha_mult(*p, alpha_scale(a, inverse))
        } else {
            0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplying_and_undoing_it_round_trips_opaque_pixels() {
        let mut argb = [255u8, 10, 200, 90];
        let want = argb;

        premultiply_argb_row(&mut argb, false);
        premultiply_argb_row(&mut argb, true);
        assert_eq!(argb, want);
    }

    #[test]
    fn a_transparent_pixel_loses_its_colour() {
        let mut argb = [0u8, 10, 200, 90];

        premultiply_argb_row(&mut argb, false);
        assert_eq!(argb, [0, 0, 0, 0]);
    }

    #[test]
    fn halving_a_row_averages_pairs() {
        let src = [10u8, 20, 30, 40];
        let mut frow = [0u32; 2];
        let p = Import {
            num_channels: 1,
            src_width: 4,
            dst_width: 2,
            x_add: 4,
            x_sub: 2,
            fx_scale: frac(1, 2),
        };
        let mut irow = [0u32; 2];

        import_row_shrink(&mut frow, &src, p);
        accumulate(&mut irow, &frow);
        assert_eq!(frow, [(10 + 20) * 2, (30 + 40) * 2]);
    }
}
