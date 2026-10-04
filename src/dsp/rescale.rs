/* Row kernels for the libwebp-compatible area rescaler. The horizontal pass
 * imports one source row into 32-bit fixed point; the vertical pass exports
 * one destination row out of the accumulator. */

pub const RFIX: u32 = 32;
pub const ONE: u64 = 1 << RFIX;
pub const ROUNDER: u64 = ONE >> 1;

pub fn mult_fix(x: u32, y: u32) -> u32 {
    ((u64::from(x) * u64::from(y) + ROUNDER) >> RFIX) as u32
}

pub fn mult_fix_floor(x: u32, y: u32) -> u32 {
    ((u64::from(x) * u64::from(y)) >> RFIX) as u32
}

pub fn frac(x: u32, y: u32) -> u32 {
    ((u64::from(x) << RFIX) / u64::from(y)) as u32
}

#[derive(Clone, Copy)]
pub struct Import {
    pub num_channels: usize,
    pub src_width: usize,
    pub dst_width: usize,
    pub x_add: u32,
    pub x_sub: u32,
    pub fx_scale: u32,
}

#[derive(Clone, Copy)]
pub struct ExportExpand {
    pub y_accum: i32,
    pub y_sub: u32,
    pub fy_scale: u32,
}

#[derive(Clone, Copy)]
pub struct ExportShrink {
    pub y_accum: i32,
    pub fy_scale: u32,
    pub fxy_scale: u32,
}

impl ExportExpand {
    /* A destination row that lands on a source row takes it whole; any
     * other one blends the pair the accumulator sits between, and only
     * then is irow read at all. */
    pub fn blend(&self) -> Option<(u32, u32)> {
        (self.y_accum != 0).then(|| {
            let b = frac((-self.y_accum) as u32, self.y_sub);

            (0u32.wrapping_sub(b), b)
        })
    }
}

impl ExportShrink {
    /* Zero means the accumulator holds whole source rows, so nothing
     * carries into the next one and frow goes unread. */
    pub fn yscale(&self) -> u32 {
        self.fy_scale.wrapping_mul((-self.y_accum) as u32)
    }
}

fn clip8(v: u32) -> u8 {
    if v > 255 {
        255
    } else {
        v as u8
    }
}

pub fn import_row_expand(frow: &mut [u32], src: &[u8], p: Import) {
    let stride = p.num_channels;
    let x_out_max = p.dst_width * stride;

    /* The window is primed before the first store, so an empty row has to
     * turn back here rather than at the loop's foot. */
    if x_out_max == 0 {
        return;
    }
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

pub fn import_row_shrink(frow: &mut [u32], src: &[u8], p: Import) {
    match p.num_channels {
        4 => import_row_shrink_channels::<4>(frow, src, p),
        _ => import_row_shrink_generic(frow, src, p),
    }
}

fn import_row_shrink_channels<const C: usize>(frow: &mut [u32], src: &[u8], p: Import) {
    let mut x_in = 0;
    let mut sum = [0u32; C];
    let mut accum = 0i32;
    for out in frow[..p.dst_width * C].chunks_exact_mut(C) {
        let mut base = [0u32; C];
        accum += p.x_add as i32;
        while accum > 0 {
            accum -= p.x_sub as i32;
            let pixel = &src[x_in..x_in + C];
            for c in 0..C {
                base[c] = u32::from(pixel[c]);
                sum[c] = sum[c].wrapping_add(base[c]);
            }
            x_in += C;
        }
        for c in 0..C {
            let fract = base[c].wrapping_mul((-accum) as u32);
            out[c] = sum[c].wrapping_mul(p.x_sub).wrapping_sub(fract);
            sum[c] = mult_fix(fract, p.fx_scale);
        }
    }
}

fn import_row_shrink_generic(frow: &mut [u32], src: &[u8], p: Import) {
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

pub fn export_row_expand(dst: &mut [u8], irow: &[u32], frow: &[u32], p: ExportExpand) {
    let Some((a, b)) = p.blend() else {
        for (d, &f) in dst.iter_mut().zip(frow) {
            *d = clip8(mult_fix(f, p.fy_scale));
        }
        return;
    };

    for ((d, &f), &i) in dst.iter_mut().zip(frow).zip(irow) {
        let acc = u64::from(a) * u64::from(f) + u64::from(b) * u64::from(i);
        let j = ((acc + ROUNDER) >> RFIX) as u32;

        *d = clip8(mult_fix(j, p.fy_scale));
    }
}

pub fn export_row_shrink(
    dst: &mut [u8],
    irow: &mut [u32],
    frow: &[u32],
    p: ExportShrink,
) {
    let yscale = p.yscale();

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

pub type ImportFn = fn(&mut [u32], &[u8], Import);
pub type ExportExpandFn = fn(&mut [u8], &[u32], &[u32], ExportExpand);
pub type ExportShrinkFn = fn(&mut [u8], &mut [u32], &[u32], ExportShrink);

#[derive(Clone, Copy)]
pub struct RescaleDsp {
    pub import_row_expand: ImportFn,
    pub import_row_shrink: ImportFn,
    pub export_row_expand: ExportExpandFn,
    pub export_row_shrink: ExportShrinkFn,
}

impl RescaleDsp {
    pub const fn scalar() -> Self {
        RescaleDsp {
            import_row_expand,
            import_row_shrink,
            export_row_expand,
            export_row_shrink,
        }
    }

    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = Self::scalar();

        #[cfg(feature = "asm")]
        crate::asm::rescale::init(&mut table, crate::cpu::flags());
        table
    }
}

impl Default for RescaleDsp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specialized_import_matches_channel_at_a_time_scaling() {
        let mut seed = 17u32;
        for channels in [1, 4] {
            for width in [
                1, 2, 3, 4, 7, 8, 15, 16, 17, 31, 32, 33, 64, 127, 128, 129, 511,
            ] {
                if cfg!(miri) && ![1, 2, 7, 16].contains(&width) {
                    continue;
                }
                let src: Vec<_> = (0..width * channels)
                    .map(|_| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed >> 24) as u8
                    })
                    .collect();
                for dst_width in 0..=width {
                    for scale in [
                        if dst_width == 0 {
                            0
                        } else {
                            frac(1, dst_width as u32)
                        },
                        u32::MAX,
                    ] {
                        let p = Import {
                            num_channels: channels,
                            src_width: width,
                            dst_width,
                            x_add: width as u32,
                            x_sub: dst_width as u32,
                            fx_scale: scale,
                        };
                        let mut actual = vec![0x1234_5678; channels * dst_width + 7];
                        let mut expected = actual.clone();
                        import_row_shrink_generic(&mut expected[3..], &src, p);
                        import_row_shrink(&mut actual[3..], &src, p);
                        assert_eq!(actual, expected, "channels={channels}, width={width}, dst_width={dst_width}, scale={scale}");
                    }
                }
            }
        }
    }
}
