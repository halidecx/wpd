//! Painting one picture onto another, which is what compositing an animation
//! frame comes down to.
//!
//! Each of the four takes a rectangle in the source's coordinates and a corner
//! in the destination's, because an animation lands a sub-frame at its own
//! position on the canvas. Which one runs is [`crate::anim`]'s decision; these
//! only walk rows.
//!
//! The chroma blend is the reason [`FrameMut::planes_mut`] exists: it writes U
//! and V while reading the alpha plane of the same picture, and only
//! destructuring the plane array proves those three do not overlap.

use crate::dsp::vp8l::Vp8lDsp;
use crate::image::{self, ceil_rshift, Format};
use crate::picture::{Frame, FrameMut};

/// A rectangle of the source picture.
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Alpha-blends an ARGB region.
pub fn blend_argb(
    dsp: &Vp8lDsp,
    premultiply: bool,
    dst: &mut FrameMut<'_>,
    src: &Frame<'_>,
    r: Rect,
    dst_x: i32,
    dst_y: i32,
) {
    let blend = if premultiply {
        dsp.blend_row_argb_premult
    } else {
        dsp.blend_row_argb
    };
    let len = r.w as usize * 4;
    let (from_src, from_dst) = (r.x as usize * 4, (dst_x + r.x) as usize * 4);

    for y in 0..r.h {
        let s = src.plane[0].row(r.y + y, from_src, len);
        let d = dst.planes_mut()[0].row_mut(dst_y + r.y + y, from_dst, len);

        blend(d, s);
    }
}

/// Copies an ARGB region, alpha and all.
pub fn copy_argb(
    dst: &mut FrameMut<'_>,
    src: &Frame<'_>,
    r: Rect,
    dst_x: i32,
    dst_y: i32,
) {
    let len = r.w as usize * 4;
    let (from_src, from_dst) = (r.x as usize * 4, (dst_x + r.x) as usize * 4);

    for y in 0..r.h {
        let s = src.plane[0].row(r.y + y, from_src, len);

        dst.planes_mut()[0]
            .row_mut(dst_y + r.y + y, from_dst, len)
            .copy_from_slice(s);
    }
}

/// Alpha-blends a YUVA region, chroma first so the luma pass can overwrite the
/// alpha plane it reads.
pub fn blend_yuva(
    dst: &mut FrameMut<'_>,
    src: &Frame<'_>,
    r: Rect,
    dst_x: i32,
    dst_y: i32,
) {
    let (base_x, base_y) = (dst_x + r.x, dst_y + r.y);
    let width = r.w as usize;
    let chroma = width.div_ceil(2);
    let (src_cx, dst_cx) = ((r.x >> 1) as usize, (base_x >> 1) as usize);

    for y in 0..ceil_rshift(r.h, 1) {
        /* A block is one or two rows tall, so the pair lives on the stack and
        only the rows the block actually spans are passed on. */
        let tile_h = (r.h - y * 2).min(2);
        let rows = [r.y + y * 2, r.y + y * 2 + tile_h - 1];
        let src_alpha = [
            src.plane[3].row(rows[0], r.x as usize, width),
            src.plane[3].row(rows[1], r.x as usize, width),
        ];
        let src_u = src.plane[1].row((r.y >> 1) + y, src_cx, chroma);
        let src_v = src.plane[2].row((r.y >> 1) + y, src_cx, chroma);
        let [_, u, v, alpha] = dst.planes_mut();
        let dst_alpha = [
            alpha.row(base_y + y * 2, base_x as usize, width),
            alpha.row(base_y + y * 2 + tile_h - 1, base_x as usize, width),
        ];
        let dst_u = u.row_mut((base_y >> 1) + y, dst_cx, chroma);
        let dst_v = v.row_mut((base_y >> 1) + y, dst_cx, chroma);

        if tile_h == 2 {
            image::blend_row_uv(
                dst_u, dst_v, src_u, src_v, &src_alpha, &dst_alpha, width,
            );
        } else {
            let src_alpha = [src_alpha[0]];
            let dst_alpha = [dst_alpha[0]];

            image::blend_row_uv(
                dst_u, dst_v, src_u, src_v, &src_alpha, &dst_alpha, width,
            );
        }
    }

    for y in 0..r.h {
        let src_y = src.plane[0].row(r.y + y, r.x as usize, width);
        let src_a = src.plane[3].row(r.y + y, r.x as usize, width);
        let [luma, _, _, alpha] = dst.planes_mut();

        image::blend_row_ya(
            luma.row_mut(base_y + y, base_x as usize, width),
            alpha.row_mut(base_y + y, base_x as usize, width),
            src_y,
            src_a,
        );
    }
}

/// Copies a YUVA region. A source with no alpha plane leaves the destination's
/// opaque, which is what a frame coded without one means.
pub fn copy_yuva(
    dst: &mut FrameMut<'_>,
    src: &Frame<'_>,
    r: Rect,
    dst_x: i32,
    dst_y: i32,
) {
    let nb = src.format.nb_components();
    let (base_x, base_y) = (dst_x + r.x, dst_y + r.y);

    for comp in 0..nb {
        let shift = u32::from(comp == 1 || comp == 2);
        let len = ceil_rshift(r.w, shift) as usize;
        let from_src = (r.x >> shift) as usize;
        let from_dst = (base_x >> shift) as usize;

        for y in 0..ceil_rshift(r.h, shift) {
            let s = src.plane[comp].row((r.y >> shift) + y, from_src, len);

            dst.planes_mut()[comp]
                .row_mut((base_y >> shift) + y, from_dst, len)
                .copy_from_slice(s);
        }
    }

    if nb < 4 {
        for y in 0..r.h {
            dst.planes_mut()[3]
                .row_mut(base_y + y, base_x as usize, r.w as usize)
                .fill(255);
        }
    }
}

/// Fills a rectangle with one colour, in whichever of the two canvas formats
/// is in use.
pub fn clear(dst: &mut FrameMut<'_>, argb: bool, colour: [u8; 4], r: Rect) {
    if argb {
        for y in 0..r.h {
            let row = dst.planes_mut()[0].row_mut(
                r.y + y,
                r.x as usize * 4,
                r.w as usize * 4,
            );

            for px in row.chunks_exact_mut(4) {
                px.copy_from_slice(&colour);
            }
        }
        return;
    }
    for (comp, &value) in colour.iter().enumerate() {
        let shift = u32::from(comp == 1 || comp == 2);
        let from = (r.x >> shift) as usize;
        let len = ceil_rshift(r.w, shift) as usize;

        for y in 0..ceil_rshift(r.h, shift) {
            dst.planes_mut()[comp]
                .row_mut((r.y >> shift) + y, from, len)
                .fill(value);
        }
    }
}

/// Whether a picture in `format` carries its own alpha plane.
pub fn has_alpha_plane(format: Format) -> bool {
    format != Format::Yuv420p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::Buffer;

    fn argb(w: i32, h: i32, fill: u8) -> Buffer {
        let mut buf = Buffer::default();

        buf.alloc_argb(w, h).unwrap();
        for y in 0..h {
            buf.frame_mut().row(0, y).fill(fill);
        }
        buf
    }

    #[test]
    fn copying_lands_the_rectangle_at_the_destination_corner() {
        let src = argb(4, 4, 0x11);
        let mut dst = argb(8, 8, 0x22);

        copy_argb(
            &mut dst.frame_mut(),
            &src.frame(),
            Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            },
            3,
            5,
        );

        let f = dst.frame();

        assert_eq!(f.row(0, 5)[3 * 4], 0x11);
        assert_eq!(f.row(0, 6)[4 * 4], 0x11);
        assert_eq!(f.row(0, 4)[3 * 4], 0x22, "the row above is untouched");
        assert_eq!(f.row(0, 5)[2 * 4], 0x22, "the column left is untouched");
        assert_eq!(f.row(0, 5)[5 * 4], 0x22, "and the column right");
    }

    /// A fully opaque source replaces the destination; a fully transparent one
    /// leaves it alone. Those two are what the region split relies on.
    #[test]
    fn blending_respects_the_two_extremes_of_alpha() {
        let dsp = Vp8lDsp::scalar();
        let mut src = argb(2, 1, 0x40);
        let mut dst = argb(2, 1, 0x80);

        src.frame_mut().row(0, 0)[0] = 255;
        src.frame_mut().row(0, 0)[4] = 0;

        blend_argb(
            &dsp,
            false,
            &mut dst.frame_mut(),
            &src.frame(),
            Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 1,
            },
            0,
            0,
        );

        let row = dst.frame().row(0, 0);

        assert_eq!(&row[..4], &[255, 0x40, 0x40, 0x40]);
        assert_eq!(&row[4..8], &[0x80, 0x80, 0x80, 0x80]);
    }

    #[test]
    fn a_source_without_alpha_leaves_the_destination_opaque() {
        let mut src = Buffer::default();

        src.alloc_planar(2, 2, true).unwrap();
        src.format = Some(Format::Yuv420p);

        let mut dst = Buffer::default();

        dst.alloc_planar(2, 2, true).unwrap();
        dst.frame_mut().row(3, 0).fill(7);

        copy_yuva(
            &mut dst.frame_mut(),
            &src.frame(),
            Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            },
            0,
            0,
        );
        assert_eq!(dst.frame().row(3, 0), &[255, 255]);
    }

    #[test]
    fn clearing_fills_only_the_rectangle() {
        let mut dst = argb(4, 4, 0x22);

        clear(
            &mut dst.frame_mut(),
            true,
            [1, 2, 3, 4],
            Rect {
                x: 1,
                y: 1,
                w: 2,
                h: 2,
            },
        );

        let f = dst.frame();

        assert_eq!(&f.row(0, 1)[4..8], &[1, 2, 3, 4]);
        assert_eq!(&f.row(0, 0)[4..8], &[0x22; 4]);
        assert_eq!(&f.row(0, 1)[..4], &[0x22; 4]);
        assert_eq!(&f.row(0, 1)[12..16], &[0x22; 4]);
    }
}
