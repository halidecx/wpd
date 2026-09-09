use crate::dsp::rescale::{frac, ExportExpand, ExportShrink, Import, RescaleDsp, RFIX};
use crate::error::{Error, Result};
use crate::picture::{PlaneMut, PlaneRef};

pub fn accumulate(irow: &mut [u32], frow: &[u32]) {
    for (i, f) in irow.iter_mut().zip(frow) {
        *i = i.wrapping_add(*f);
    }
}

pub fn export_row_direct(dst: &mut [u8], irow: &mut [u32]) {
    for (d, i) in dst.iter_mut().zip(irow.iter_mut()) {
        *d = *i as u8;
        *i = 0;
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

/// One accumulator per plane, so the planes carry nothing between them and can
/// be rescaled at the same time.
pub type Scratches = [Scratch; 4];

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

pub struct Rescaler<'a> {
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
    dsp: &'a RescaleDsp,
}

impl<'a> Rescaler<'a> {
    pub fn new(
        dsp: &'a RescaleDsp,
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
            dsp,
        }
    }

    fn width(&self) -> usize {
        self.num_channels * self.dst_width
    }

    fn rows<'w>(&self, work: &'w mut [u32]) -> (&'w mut [u32], &'w mut [u32]) {
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
            (self.dsp.import_row_expand)(frow, src, p);
        } else {
            (self.dsp.import_row_shrink)(frow, src, p);
        }
        if !self.y_expand {
            accumulate(irow, frow);
        }
        self.y_accum -= self.y_sub;
    }

    pub fn export(&mut self, work: &mut [u32], dst: &mut PlaneMut<'_>) {
        let width = self.width();

        while !self.wants_row() {
            let out = dst.row_mut(self.dst_y, 0, width);
            let (irow, frow) = self.rows(work);

            if self.y_expand {
                let p = ExportExpand {
                    y_accum: self.y_accum,
                    y_sub: self.y_sub as u32,
                    fy_scale: self.fy_scale,
                };

                (self.dsp.export_row_expand)(out, irow, frow, p);
            } else if self.fxy_scale != 0 {
                let p = ExportShrink {
                    y_accum: self.y_accum,
                    fy_scale: self.fy_scale,
                    fxy_scale: self.fxy_scale,
                };

                (self.dsp.export_row_shrink)(out, irow, frow, p);
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
    dsp: &RescaleDsp,
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
        dsp,
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
    dsp: &crate::dsp::yuv::YuvDsp,
    rdsp: &RescaleDsp,
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
        rdsp,
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
                    (dsp.multiply_row)(row, alpha.row(y, 0, src_width as usize), false)
                }
                None => (dsp.premultiply_argb_row)(row, false),
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
    use crate::dsp::rescale::import_row_shrink;
    use crate::dsp::yuv::premultiply_argb_row;

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
