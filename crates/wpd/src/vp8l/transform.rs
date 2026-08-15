//! The four VP8L transforms, undone.
//!
//! Each one works over a range of rows of a flat picture, addressed by the
//! offset of its first row and a stride, because the predictor reads the row
//! above the one it is writing and the two have to live in the same
//! allocation for that to be an ordinary index — see [`super`].
//!
//! Three of the four are per-pixel byte arithmetic on the `[A, R, G, B]` a
//! `u32` holds; the predictor and the cross-colour transform go through the
//! DSP table, because those are the two with assembly.

use super::AlphaDst;
use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{Error, Result};

/// Undoes the spatial predictor over rows `y0..y1`.
///
/// `base` is the offset of row `y0`, and `upper0` the offset of the row above
/// it, which is not simply `base - stride` when the caller keeps that row
/// somewhere else. For `y0 == 0` there is no row above and the first row is
/// coded against its own left neighbour instead.
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
        /* The top-right of the last pixel in a row is the leftmost pixel of
        that same row, which falls out of the layout only while the row
        above is physically adjacent. */
        if up + width != row {
            plane[up + width] = plane[row];
        }

        let mut x = 1usize;

        while x < width {
            let mode = modes[modes_row + (x >> tile_bits)].to_ne_bytes()[2];
            let mut x_end = (x & !tile_mask) + tile_size;

            if mode > 13 {
                crate::log::error(&format!("invalid predictor mode: {mode}"));
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

/// Undoes the cross-colour transform, which predicts red from green and blue
/// from both.
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

/// Adds green back into red and blue.
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

/// Replaces each palette index by the colour it stands for.
///
/// When the palette is small enough that several indices were packed into one
/// pixel, the row also widens, which is why the source and destination strides
/// are separate: the rows are rewritten bottom-up and right-to-left so every
/// write lands at or past the index it derives from.
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
        /* Specialised on the group size so both the expansion table and the
        copy out of it are fixed-width, as the C's switch over an
        always-inline helper made them. */
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

/// How many groups a row is expanded in at a time. Small enough that the
/// scratch stays in L1 and costs nothing to keep on the stack.
const BLOCK: usize = 128;

/// Rewrites rows whose pixels each pack `PPB` palette indices.
///
/// The expansion table is built per group rather than per index, so a whole
/// group is one copy; it is indexed by a whole byte, which puts the lookup
/// unconditionally in bounds.
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

        /* The indices of a block are lifted out before it is written, which is
        what lets the write walk forwards over a slice taken once. Writing them
        in place instead costs two bounds checks per group that no amount of
        rearranging persuades LLVM to drop, because the relation between `off`,
        `full` and the row length is not one it can see. */
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

/// The palette transform when the picture is an alpha plane and nothing else
/// was applied to it, so the green channel can be looked up straight into the
/// caller's plane instead of being expanded to ARGB first.
pub fn color_indexing_alpha(
    src: &[u32],
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
            2 => expand_alpha_rows::<2>(src, src_stride, width, height, &palette, dst),
            4 => expand_alpha_rows::<4>(src, src_stride, width, height, &palette, dst),
            _ => expand_alpha_rows::<8>(src, src_stride, width, height, &palette, dst),
        }
        return;
    }

    let AlphaDst { data, stride } = dst;

    for y in 0..height as usize {
        let row = &src[y * src_stride..];
        let out = &mut data[y * stride..][..width];

        for (o, px) in out.iter_mut().zip(row) {
            *o = palette[usize::from(px.to_ne_bytes()[2])];
        }
    }
}

/// The packed-palette case of [`color_indexing_alpha`], specialised on the
/// group size the way [`expand_palette_rows`] is.
///
/// Without the specialisation the group size, the shift and the mask are all
/// runtime values, which costs a variable-count shift per pixel and a
/// `chunks_exact_mut` whose stride the compiler cannot fold. Expanding a whole
/// group in one table lookup makes the inner loop a fixed-width copy.
fn expand_alpha_rows<const PPB: usize>(
    src: &[u32],
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
            group.copy_from_slice(&expand[usize::from(px.to_ne_bytes()[2])]);
        }
        if tail != 0 {
            let index = usize::from(row[full].to_ne_bytes()[2]);

            out[full * PPB..].copy_from_slice(&expand[index][..tail]);
        }
    }
}
