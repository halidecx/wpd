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

    let tiles = ((width - 1) >> tile_bits) + 1;
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
        let row_modes = &modes[modes_row..modes_row + tiles];
        /* Two rows of a tile row share its modes, and some predictors run
         * two rows at once faster than one after the other. */
        let pair = y + 1 < y1 && (y + 1) >> tile_bits == y >> tile_bits;
        let below = row + stride;

        (dsp.pred_add[2])(plane, row, up, 1);
        if up + width != row {
            plane[up + width] = plane[row];
        }
        if pair {
            (dsp.pred_add[2])(plane, below, row, 1);
            if row + width != below {
                plane[row + width] = plane[below];
            }
        }

        let mut x = 1usize;
        let mut tile = 0;
        let mut held: Option<(usize, usize, usize)> = None;

        /* Neighbouring tiles often share a mode, and one call over the run
         * does what a call per tile would. */
        while x < width {
            let mode = row_modes[tile].to_ne_bytes()[2];

            if mode > 13 {
                crate::log::error_args(format_args!("invalid predictor mode: {mode}"));
                return Err(Error::InvalidData);
            }
            tile += 1;
            while tile < tiles && row_modes[tile].to_ne_bytes()[2] == mode {
                tile += 1;
            }

            let x_end = (tile << tile_bits).min(width);
            let m = usize::from(mode);
            let n = x_end - x;

            if !pair {
                (dsp.pred_add[m])(plane, row + x, up + x, n);
            } else if let (Some(both), None) = (dsp.pred_add_pair[m], held) {
                both(plane, row + x, up + x, below + x, n);
            } else {
                (dsp.pred_add[m])(plane, row + x, up + x, n);
                if let Some((m, x, n)) = held.take() {
                    (dsp.pred_add[m])(plane, below + x, row + x, n);
                }
                /* The last pixel's top right is the upper row's next run,
                 * so the lower row waits for it. */
                if matches!(mode, 3 | 5 | 9 | 10) {
                    held = Some((m, x, n));
                } else {
                    (dsp.pred_add[m])(plane, below + x, row + x, n);
                }
            }
            x = x_end;
        }
        if let Some((m, x, n)) = held {
            (dsp.pred_add[m])(plane, below + x, row + x, n);
        }

        if pair {
            up = below;
            row = below + stride;
            y += 2;
        } else {
            up = row;
            row = below;
            y += 1;
        }
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
    dsp: &Vp8lDsp,
    plane: &mut [u32],
    base: usize,
    stride: usize,
    width: usize,
    rows: i32,
) {
    let mut row = base;

    for _ in 0..rows {
        (dsp.add_green)(&mut plane[row..row + width]);
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

    /// The indices as bytes, when they are bytes.
    fn bytes(src: &[Self]) -> Option<&[u8]>;
}

impl Indexed for u32 {
    #[inline(always)]
    fn palette_index(self) -> usize {
        usize::from(self.to_ne_bytes()[2])
    }

    fn bytes(_: &[Self]) -> Option<&[u8]> {
        None
    }
}

impl Indexed for u8 {
    #[inline(always)]
    fn palette_index(self) -> usize {
        usize::from(self)
    }

    fn bytes(src: &[Self]) -> Option<&[u8]> {
        Some(src)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn color_indexing_alpha<T: Indexed>(
    dsp: &Vp8lDsp,
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

    if let (1, Some(expand), Some(src)) =
        (size_reduction, dsp.expand_alpha_nibbles, T::bytes(src))
    {
        let AlphaDst { data, stride, .. } = dst;
        let lut: &[u8; 16] = palette.first_chunk().unwrap();

        for y in 0..height as usize {
            expand(
                &mut data[y * stride..][..width],
                &src[y * src_stride..],
                lut,
            );
        }
        return;
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

    let AlphaDst { data, stride, .. } = dst;

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
    let AlphaDst { data, stride, .. } = dst;
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

/// The channels of a pixel other than green, which an alpha image keeps.
pub const NOT_GREEN: u32 = u32::from_ne_bytes([0xFF, 0xFF, 0x00, 0xFF]);

/// Inverse predicts the green of row 0, whose pixels are each predicted
/// from the one to their left and the first from black.
pub fn predict_green_first_row(dsp: &Vp8lDsp, res: &[u32], row: &mut [u8]) {
    row[0] = res[0].to_ne_bytes()[2];
    (dsp.pred_green[1])(row, &[], &res[1..]);
}

/// Inverse predicts the green of a row below the first, on its own. The
/// predictors work on each channel apart, except the select one, which
/// sums differences over all four; when every pixel's other channels are
/// the same, they add nothing to its sums, and green alone decides.
///
/// `above` is the row above's green, one longer than `row`: its last byte
/// is where the top right of the row's last pixel is read, which is the
/// row's first pixel, as it is in a contiguous plane.
pub fn predict_green_row(
    dsp: &Vp8lDsp,
    modes: &[u32],
    tile_bits: u32,
    res: &[u32],
    above: &mut [u8],
    row: &mut [u8],
) -> Result<()> {
    let width = row.len();
    let tiles = ((width - 1) >> tile_bits) + 1;
    let modes = &modes[..tiles];

    row[0] = above[0].wrapping_add(res[0].to_ne_bytes()[2]);
    above[width] = row[0];

    let mut x = 1usize;
    let mut tile = 0;

    while x < width {
        let mode = modes[tile].to_ne_bytes()[2];

        if mode > 13 {
            crate::log::error_args(format_args!("invalid predictor mode: {mode}"));
            return Err(Error::InvalidData);
        }
        tile += 1;
        while tile < tiles && modes[tile].to_ne_bytes()[2] == mode {
            tile += 1;
        }

        let end = (tile << tile_bits).min(width);

        (dsp.pred_green[usize::from(mode)])(
            &mut row[x - 1..end],
            &above[x - 1..end + 1],
            &res[x..end],
        );
        x = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *state
    }

    /* A row at a time and a pixel at a time, with the scalar predictors. */
    fn reference(
        plane: &mut [u32],
        stride: usize,
        width: usize,
        height: usize,
        modes: &[u32],
        tile_bits: u32,
    ) {
        let dsp = Vp8lDsp::scalar();
        let modes_stride = ((width - 1) >> tile_bits) + 1;

        (dsp.pred_add[0])(plane, 0, 0, 1);
        (dsp.pred_add[1])(plane, 1, 0, width - 1);
        for y in 1..height {
            let row = y * stride;
            let up = row - stride;

            (dsp.pred_add[2])(plane, row, up, 1);
            plane[up + width] = plane[row];
            for x in 1..width {
                let tile = (y >> tile_bits) * modes_stride + (x >> tile_bits);
                let mode = modes[tile].to_ne_bytes()[2];

                (dsp.pred_add[usize::from(mode)])(plane, row + x, up + x, 1);
            }
        }
    }

    /* Rows go two at a time where they share a tile row, and a run whose
     * mode reads the top right holds the lower row back a run. */
    #[test]
    fn paired_rows_match_one_row_at_a_time() {
        let mut state = 5;
        let shapes = [(1, 9, 0), (6, 9, 2), (37, 22, 0), (37, 21, 3), (130, 41, 1)];

        crate::cpu::init();
        for dsp in [Vp8lDsp::scalar(), Vp8lDsp::new()] {
            for (width, height, pad) in shapes {
                for tile_bits in [2, 3, 5] {
                    for pattern in 0..3 {
                        let stride = width + pad;
                        let modes_stride = ((width - 1) >> tile_bits) + 1;
                        let modes_rows = ((height - 1) >> tile_bits) + 1;
                        let modes: Vec<u32> = (0..modes_stride * modes_rows)
                            .map(|_| {
                                let r = (lcg(&mut state) >> 8) as usize;
                                let mode = match pattern {
                                    0 => r % 14,
                                    1 => [11, 12, 13, 3, 5, 9, 10, 11, 11, 0][r % 10],
                                    _ => 11 + r % 3,
                                };
                                u32::from_ne_bytes([0, 0, mode as u8, 0])
                            })
                            .collect();
                        let plane: Vec<u32> =
                            (0..stride * height).map(|_| lcg(&mut state)).collect();
                        let mut expected = plane.clone();

                        reference(
                            &mut expected,
                            stride,
                            width,
                            height,
                            &modes,
                            tile_bits,
                        );

                        /* In one go, then split so pairs start on either parity. */
                        for split in [height, 4, 5] {
                            let split = split.min(height);
                            let mut actual = plane.clone();

                            predictor_rows(
                                &dsp,
                                &mut actual,
                                0,
                                stride,
                                width,
                                &modes,
                                modes_stride,
                                tile_bits,
                                0,
                                split as i32,
                                None,
                            )
                            .unwrap();
                            predictor_rows(
                                &dsp,
                                &mut actual,
                                split * stride,
                                stride,
                                width,
                                &modes,
                                modes_stride,
                                tile_bits,
                                split as i32,
                                height as i32,
                                Some((split - 1) * stride),
                            )
                            .unwrap();
                            for y in 0..height {
                                let row = y * stride;

                                assert_eq!(
                                    actual[row..row + width],
                                    expected[row..row + width],
                                    "{width}x{height}+{pad} tile bits {tile_bits} pattern \
                                     {pattern} split {split} row {y}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /* Byte indices a nibble each go through the dsp's kernel, where it has
     * one, and must land as the table does, tails and all. */
    #[test]
    fn nibble_alpha_matches_the_table() {
        let mut state = 9;

        crate::cpu::init();
        for width in [1usize, 2, 15, 16, 17, 31, 32, 33, 63, 64, 65, 600] {
            let height = 3;
            let src_stride = width.div_ceil(2);
            let stride = width + 5;
            let src: Vec<u8> = (0..src_stride * height)
                .map(|_| (lcg(&mut state) >> 24) as u8)
                .collect();
            let pal: Vec<u32> = (0..16).map(|_| lcg(&mut state)).collect();
            let mut planes = [vec![7u8; stride * height], vec![7u8; stride * height]];

            for (dsp, data) in
                [Vp8lDsp::scalar(), Vp8lDsp::new()].iter().zip(&mut planes)
            {
                let dst = AlphaDst {
                    data,
                    stride,
                    unfilter: None,
                };

                color_indexing_alpha(
                    dsp,
                    &src,
                    src_stride,
                    width,
                    height as i32,
                    &pal,
                    1,
                    dst,
                );
            }
            assert!(planes[0] == planes[1], "width {width}");
        }
    }
}
