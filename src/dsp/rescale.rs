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

pub type ImportFn = fn(&mut [u32], &[u8], Import);
pub type ExportExpandFn = fn(&mut [u8], &[u32], &[u32], Export);
pub type ExportShrinkFn = fn(&mut [u8], &mut [u32], &[u32], Export);

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
