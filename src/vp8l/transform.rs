use super::AlphaDst;
use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{Error, Result};

#[allow(clippy::too_many_arguments)]
pub fn predictor_rows(
    dsp: &Vp8lDsp,
    plane: &mut [u32],
    base: usize,
    stride: usize,
    width: usize,
    modes: &[u32],
    modes_stride: usize,
    tile_bits: u32,
    y0: i32,
    y1: i32,
    upper0: Option<usize>,
) -> Result<()> {
    if width == 0 || y1 <= y0 {
        return Ok(());
    }

    let tile_size = 1usize << tile_bits;
    let tile_mask = tile_size - 1;
    let mut row = base;
    let mut upper = upper0;
    let mut y = y0;

    if y0 == 0 {
        (dsp.pred_add[0])(plane, row, 0, 1);
        if width > 1 {
            (dsp.pred_add[1])(plane, row + 1, 0, width - 1);
        }
        upper = Some(row);
        row += stride;
        y = 1;
    }

    let Some(mut up) = upper else {
        return Ok(());
    };

    while y < y1 {
        let modes_row = (y >> tile_bits) as usize * modes_stride;

        (dsp.pred_add[2])(plane, row, up, 1);
        if up + width != row {
            plane[up + width] = plane[row];
        }

        let mut x = 1usize;

        while x < width {
            let mode = modes[modes_row + (x >> tile_bits)].to_ne_bytes()[2];
            let mut x_end = (x & !tile_mask) + tile_size;

            if mode > 13 {
                crate::log::error_args(format_args!("invalid predictor mode: {mode}"));
                return Err(Error::InvalidData);
            }
            if x_end > width {
                x_end = width;
            }
            (dsp.pred_add[usize::from(mode)])(plane, row + x, up + x, x_end - x);
            x = x_end;
        }

        up = row;
        row += stride;
        y += 1;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn color_rows(
    dsp: &Vp8lDsp,
    plane: &mut [u32],
    base: usize,
    stride: usize,
    width: usize,
    mult: &[u32],
    mult_stride: usize,
    tile_bits: u32,
    y0: i32,
    y1: i32,
) {
    let tile_size = 1usize << tile_bits;
    let tile_mask = tile_size - 1;
    let mut row = base;

    for y in y0..y1 {
        let mult_row = (y >> tile_bits) as usize * mult_stride;
        let mut x = 0usize;

        while x < width {
            let cp = mult[mult_row + (x >> tile_bits)];
            let mut x_end = (x & !tile_mask) + tile_size;

            if x_end > width {
                x_end = width;
            }
            (dsp.color_row)(&mut plane[row + x..row + x_end], cp);
            x = x_end;
        }
        row += stride;
    }
}

pub fn subtract_green_rows(
    plane: &mut [u32],
    base: usize,
    stride: usize,
    width: usize,
    rows: i32,
) {
    let mut row = base;

    for _ in 0..rows {
        for px in &mut plane[row..row + width] {
            let mut b = px.to_ne_bytes();

            b[1] = b[1].wrapping_add(b[2]);
            b[3] = b[3].wrapping_add(b[2]);
            *px = u32::from_ne_bytes(b);
        }
        row += stride;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn color_indexing_rows(
    dsp: &Vp8lDsp,
    plane: &mut [u32],
    base: usize,
    dst_stride: usize,
    src_stride: usize,
    width: usize,
    height: i32,
    pal: &[u32],
    size_reduction: u32,
    big: bool,
) {
    let mut palette = [0u32; 256];

    palette[..pal.len()].copy_from_slice(pal);

    if size_reduction > 0 {
        match 1usize << size_reduction {
            2 => expand_palette_rows::<2>(
                plane, base, dst_stride, src_stride, width, height, &palette,
            ),
            4 => expand_palette_rows::<4>(
                plane, base, dst_stride, src_stride, width, height, &palette,
            ),
            _ => expand_palette_rows::<8>(
                plane, base, dst_stride, src_stride, width, height, &palette,
            ),
        }
        return;
    }

    if big {
        for y in 0..height as usize {
            let row = base + y * dst_stride;

            (dsp.map_color32)(&mut plane[row..row + width], &palette);
        }
        return;
    }

    for y in 0..height as usize {
        for x in 0..width {
            let at = base + y * dst_stride + x;
            let index = usize::from(plane[at].to_ne_bytes()[2]);

            plane[at] = if index >= pal.len() { 0 } else { pal[index] };
        }
    }
}

const BLOCK: usize = 128;

fn expand_palette_rows<const PPB: usize>(
    plane: &mut [u32],
    base: usize,
    dst_stride: usize,
    src_stride: usize,
    width: usize,
    height: i32,
    palette: &[u32; 256],
) {
    let pixel_bits = 8 / PPB as u32;
    let bit_mask = (1u32 << pixel_bits) - 1;
    let expand: [[u32; PPB]; 256] = core::array::from_fn(|i| {
        let mut packed = i as u32;

        core::array::from_fn(|_| {
            let entry = palette[(packed & bit_mask) as usize];

            packed >>= pixel_bits;
            entry
        })
    });
    let full = width / PPB;
    let tail = width - full * PPB;

    let mut idx = [0u8; BLOCK];

    for y in (0..height as usize).rev() {
        let dst = base + y * dst_stride;
        let src = base + y * src_stride;
        let off = dst - src;
        let row = &mut plane[src..dst + width];

        if tail != 0 {
            let index = usize::from(row[full].to_ne_bytes()[2]);

            row[off + full * PPB..][..tail].copy_from_slice(&expand[index][..tail]);
        }

        let mut b = full;

        while b > 0 {
            let n = b.min(BLOCK);
            let start = b - n;

            for (slot, px) in idx[..n].iter_mut().zip(&row[start..b]) {
                *slot = px.to_ne_bytes()[2];
            }

            let out = &mut row[off + start * PPB..][..n * PPB];

            for (group, &i) in out.chunks_exact_mut(PPB).zip(&idx[..n]) {
                group.copy_from_slice(&expand[usize::from(i)]);
            }
            b = start;
        }
    }
}

pub trait Indexed: Copy {
    fn palette_index(self) -> usize;
}

impl Indexed for u32 {
    #[inline(always)]
    fn palette_index(self) -> usize {
        usize::from(self.to_ne_bytes()[2])
    }
}

impl Indexed for u8 {
    #[inline(always)]
    fn palette_index(self) -> usize {
        usize::from(self)
    }
}

pub fn color_indexing_alpha<T: Indexed>(
    src: &[T],
    src_stride: usize,
    width: usize,
    height: i32,
    pal: &[u32],
    size_reduction: u32,
    dst: AlphaDst<'_>,
) {
    let mut palette = [0u8; 256];

    for (slot, &entry) in palette.iter_mut().zip(pal) {
        *slot = entry.to_ne_bytes()[2];
    }

    if size_reduction > 0 {
        match 1usize << size_reduction {
            2 => {
                expand_alpha_rows::<2, T>(src, src_stride, width, height, &palette, dst)
            }
            4 => {
                expand_alpha_rows::<4, T>(src, src_stride, width, height, &palette, dst)
            }
            _ => {
                expand_alpha_rows::<8, T>(src, src_stride, width, height, &palette, dst)
            }
        }
        return;
    }

    let AlphaDst { data, stride } = dst;

    for y in 0..height as usize {
        let row = &src[y * src_stride..];
        let out = &mut data[y * stride..][..width];

        for (o, &px) in out.iter_mut().zip(row) {
            *o = palette[px.palette_index()];
        }
    }
}

fn expand_alpha_rows<const PPB: usize, T: Indexed>(
    src: &[T],
    src_stride: usize,
    width: usize,
    height: i32,
    palette: &[u8; 256],
    dst: AlphaDst<'_>,
) {
    let AlphaDst { data, stride } = dst;
    let pixel_bits = 8 / PPB as u32;
    let bit_mask = (1u32 << pixel_bits) - 1;
    let expand: [[u8; PPB]; 256] = core::array::from_fn(|i| {
        let mut packed = i as u32;

        core::array::from_fn(|_| {
            let entry = palette[(packed & bit_mask) as usize];

            packed >>= pixel_bits;
            entry
        })
    });
    let full = width / PPB;
    let tail = width - full * PPB;

    for y in 0..height as usize {
        let row = &src[y * src_stride..];
        let out = &mut data[y * stride..][..width];

        for (group, &px) in out.chunks_exact_mut(PPB).zip(row) {
            group.copy_from_slice(&expand[px.palette_index()]);
        }
        if tail != 0 {
            let index = row[full].palette_index();

            out[full * PPB..].copy_from_slice(&expand[index][..tail]);
        }
    }
}
