use crate::error::{Error, Result};
use crate::picture::{PlaneMut, PlaneRef};

const RFIX: u32 = 32;
const ONE: u64 = 1 << RFIX;
const ROUNDER: u64 = ONE >> 1;

fn mult_fix(x: u32, y: u32) -> u32 {
    ((u64::from(x) * u64::from(y) + ROUNDER) >> RFIX) as u32
}

fn mult_fix_floor(x: u32, y: u32) -> u32 {
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

pub fn accumulate(irow: &mut [u32], frow: &[u32]) {
    for (i, f) in irow.iter_mut().zip(frow) {
        *i = i.wrapping_add(*f);
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

#[derive(Default)]
pub struct Scratch {
    work: Vec<u32>,
    row: Vec<u8>,
}

fn grow<T: Clone + Default>(v: &mut Vec<T>, need: usize) -> Result<()> {
    if v.len() >= need {
        return Ok(());
    }
    v.clear();
    v.try_reserve_exact(need).map_err(|_| Error::NoMemory)?;
    v.resize(need, T::default());
    Ok(())
}

impl Scratch {
    pub fn grow(
        &mut self,
        dst_width: i32,
        src_width: i32,
        channels: usize,
    ) -> Result<()> {
        let (Ok(dst_width), Ok(src_width)) =
            (usize::try_from(dst_width), usize::try_from(src_width))
        else {
            return Err(Error::TooLarge);
        };
        let accum = dst_width
            .checked_mul(channels)
            .and_then(|n| n.checked_mul(2))
            .ok_or(Error::TooLarge)?;
        let row = src_width.checked_mul(channels).ok_or(Error::TooLarge)?;

        grow(&mut self.work, accum)?;
        grow(&mut self.row, row)
    }

    pub fn release(&mut self) {
        self.work = Vec::new();
        self.row = Vec::new();
    }

    pub fn work_mut(&mut self) -> &mut [u32] {
        &mut self.work
    }

    fn split(&mut self) -> (&mut [u32], &mut [u8]) {
        (&mut self.work, &mut self.row)
    }
}

pub struct Rescaler {
    x_expand: bool,
    y_expand: bool,
    num_channels: usize,
    fx_scale: u32,
    fy_scale: u32,
    fxy_scale: u32,
    y_accum: i32,
    y_add: i32,
    y_sub: i32,
    x_add: i32,
    x_sub: i32,
    src_width: usize,
    dst_width: usize,
    dst_height: i32,
    dst_y: i32,
    swapped: bool,
}

impl Rescaler {
    pub fn new(
        work: &mut [u32],
        src_width: i32,
        src_height: i32,
        dst_width: i32,
        dst_height: i32,
        num_channels: usize,
    ) -> Self {
        let x_expand = src_width < dst_width;
        let y_expand = src_height < dst_height;
        let width = num_channels * dst_width as usize;

        work[..2 * width].fill(0);

        let x_add = if x_expand { dst_width - 1 } else { src_width };
        let x_sub = if x_expand { src_width - 1 } else { dst_width };
        let y_add = if y_expand { src_height - 1 } else { src_height };
        let y_sub = if y_expand { dst_height - 1 } else { dst_height };

        Rescaler {
            x_expand,
            y_expand,
            num_channels,
            fx_scale: if x_expand { 0 } else { frac(1, x_sub as u32) },
            fy_scale: frac(1, if y_expand { x_add as u32 } else { y_sub as u32 }),
            fxy_scale: if y_expand {
                0
            } else {
                let den = u64::from(x_add as u32) * u64::from(y_add as u32);
                let ratio = (u64::from(dst_height as u32) << RFIX)
                    .checked_div(den)
                    .unwrap_or(0);

                if ratio > u64::from(u32::MAX) {
                    0
                } else {
                    ratio as u32
                }
            },
            y_accum: if y_expand { y_sub } else { y_add },
            y_add,
            y_sub,
            x_add,
            x_sub,
            src_width: src_width as usize,
            dst_width: dst_width as usize,
            dst_height,
            dst_y: 0,
            swapped: false,
        }
    }

    fn width(&self) -> usize {
        self.num_channels * self.dst_width
    }

    fn rows<'a>(&self, work: &'a mut [u32]) -> (&'a mut [u32], &'a mut [u32]) {
        let width = self.width();
        let (a, b) = work[..2 * width].split_at_mut(width);

        if self.swapped {
            (b, a)
        } else {
            (a, b)
        }
    }

    pub fn wants_row(&self) -> bool {
        !(self.dst_y < self.dst_height && self.y_accum <= 0)
    }

    pub fn import_row(&mut self, work: &mut [u32], src: &[u8]) {
        let p = Import {
            num_channels: self.num_channels,
            src_width: self.src_width,
            dst_width: self.dst_width,
            x_add: self.x_add as u32,
            x_sub: self.x_sub as u32,
            fx_scale: self.fx_scale,
        };

        if self.y_expand {
            self.swapped = !self.swapped;
        }

        let (irow, frow) = self.rows(work);

        if self.x_expand {
            import_row_expand(frow, src, p);
        } else {
            import_row_shrink(frow, src, p);
        }
        if !self.y_expand {
            accumulate(irow, frow);
        }
        self.y_accum -= self.y_sub;
    }

    pub fn export(&mut self, work: &mut [u32], dst: &mut PlaneMut<'_>) {
        let width = self.width();

        while !self.wants_row() {
            let p = Export {
                y_accum: self.y_accum,
                y_sub: self.y_sub as u32,
                fy_scale: self.fy_scale,
                fxy_scale: self.fxy_scale,
            };
            let out = dst.row_mut(self.dst_y, 0, width);
            let (irow, frow) = self.rows(work);

            if self.y_expand {
                export_row_expand(out, irow, frow, p);
            } else if self.fxy_scale != 0 {
                export_row_shrink(out, irow, frow, p);
            } else {
                export_row_direct(out, irow);
            }
            self.y_accum += self.y_add;
            self.dst_y += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn rescale_plane(
    work: &mut [u32],
    dst: &mut PlaneMut<'_>,
    dst_width: i32,
    dst_height: i32,
    src: &PlaneRef<'_>,
    src_width: i32,
    src_height: i32,
    num_channels: usize,
) {
    let mut r = Rescaler::new(
        work,
        src_width,
        src_height,
        dst_width,
        dst_height,
        num_channels,
    );
    let len = src_width as usize * num_channels;
    let mut y = 0;

    while y < src_height {
        if r.wants_row() {
            r.import_row(work, src.row(y, 0, len));
            y += 1;
        }
        r.export(work, dst);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn rescale_plane_weighted(
    scratch: &mut Scratch,
    dst: &mut PlaneMut<'_>,
    dst_width: i32,
    dst_height: i32,
    src: &PlaneRef<'_>,
    alpha: Option<&PlaneRef<'_>>,
    src_width: i32,
    src_height: i32,
    num_channels: usize,
) {
    let (work, row) = scratch.split();
    let len = src_width as usize * num_channels;
    let row = &mut row[..len];
    let mut r = Rescaler::new(
        work,
        src_width,
        src_height,
        dst_width,
        dst_height,
        num_channels,
    );
    let mut y = 0;

    while y < src_height {
        if r.wants_row() {
            row.copy_from_slice(src.row(y, 0, len));
            match alpha {
                Some(alpha) => {
                    multiply_row(row, alpha.row(y, 0, src_width as usize), false)
                }
                None => premultiply_argb_row(row, false),
            }
            r.import_row(work, row);
            y += 1;
        }
        r.export(work, dst);
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
