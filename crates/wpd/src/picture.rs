//! Pictures, and the two ways the decoder holds one.
//!
//! A [`Buffer`] owns plane memory and is reused frame to frame; a [`Frame`] or
//! [`FrameMut`] is a borrowed view of pixels, which may be a buffer's or one
//! of the codecs'. Splitting them is what lets the decoder be safe code: the C
//! had a single `WebPImage` that was sometimes an owner and sometimes a view
//! into memory something else owned, and nothing in the type said which.
//!
//! # Crops and flips
//!
//! The C made both by pointer arithmetic — adding to `data[p]` for a crop,
//! pointing at the last row and negating `linesize[p]` for a flip. A view here
//! carries them instead: `origin` is the offset of row zero and `flip` says
//! the rows come out bottom-first. Nothing downstream has to know, because
//! every reader goes through [`Frame::row`], and a negative stride only has to
//! exist at the C ABI boundary, where the shim builds one on the way out.

use crate::error::{Error, Result};
use crate::image::{ceil_rshift, plane_size, Format};

/// How many planes a format hands out.
pub fn planes_of(format: Format) -> usize {
    match format {
        Format::Yuva420p => 4,
        Format::Yuv420p => 3,
        _ => 1,
    }
}

/// Whether plane `p` is subsampled by two in both directions.
fn chroma(p: usize) -> u32 {
    u32::from(p == 1 || p == 2)
}

/// One plane's memory, kept across frames and grown on demand.
#[derive(Default)]
pub struct Plane {
    data: Vec<u8>,
    stride: usize,
}

impl Plane {
    /// Zeroed room for `rows` rows of `stride` bytes, reusing what is there
    /// when it is already big enough.
    ///
    /// Only what is asked for is cleared, not the whole allocation: a plane
    /// that shrank between frames keeps its memory, and the tail past the
    /// image is never read.
    fn resize(&mut self, stride: usize, rows: i32, size: usize) -> Result<()> {
        if self.data.len() < size {
            self.data.clear();
            self.data
                .try_reserve_exact(size)
                .map_err(|_| Error::NoMemory)?;
            self.data.resize(size, 0);
        } else {
            self.data[..size].fill(0);
        }
        self.stride = stride;
        debug_assert!(rows >= 0);
        Ok(())
    }

    fn release(&mut self) {
        self.data = Vec::new();
        self.stride = 0;
    }
}

/// Plane memory the decoder owns.
#[derive(Default)]
pub struct Buffer {
    plane: [Plane; 4],
    pub width: i32,
    pub height: i32,
    pub format: Option<Format>,
    /// Set when the rescaler has brought U and V up to full resolution.
    pub chroma_full: bool,
    /// Set when the colour channels already carry alpha, as the animation
    /// canvas does for a premultiplied output format.
    pub premultiplied: bool,
}

impl Buffer {
    pub fn is_empty(&self) -> bool {
        self.plane[0].data.is_empty()
    }

    /// Gives every plane's memory back, which the C's `image_free` did.
    pub fn release(&mut self) {
        for plane in &mut self.plane {
            plane.release();
        }
        self.width = 0;
        self.height = 0;
        self.format = None;
        self.chroma_full = false;
        self.premultiplied = false;
    }

    /// One plane of `bpp` bytes per pixel, and nothing in the other three.
    pub fn alloc_packed(
        &mut self,
        w: i32,
        h: i32,
        bpp: usize,
        format: Format,
    ) -> Result<()> {
        let size = plane_size(w, h, bpp)?;

        for plane in &mut self.plane[1..] {
            plane.release();
        }
        self.plane[0].resize(w as usize * bpp, h, size)?;
        self.width = w;
        self.height = h;
        self.format = Some(format);
        Ok(())
    }

    pub fn alloc_argb(&mut self, w: i32, h: i32) -> Result<()> {
        self.alloc_packed(w, h, 4, Format::Argb)
    }

    /// Four planes, chroma subsampled or not.
    ///
    /// The unsubsampled shape is what the rescaler fills when it brings chroma
    /// up to the output size; both are labelled YUVA, and `chroma_full` is
    /// what tells them apart.
    pub fn alloc_planar(&mut self, w: i32, h: i32, subsample: bool) -> Result<()> {
        if w <= 0 || h <= 0 {
            return Err(Error::TooLarge);
        }
        for p in 0..4 {
            let shift = if subsample { chroma(p) } else { 0 };
            let pw = ceil_rshift(w, shift);
            let ph = ceil_rshift(h, shift);
            let size = plane_size(pw, ph, 1)?;

            if let Err(e) = self.plane[p].resize(pw as usize, ph, size) {
                self.release();
                return Err(e);
            }
        }
        self.width = w;
        self.height = h;
        self.format = Some(Format::Yuva420p);
        Ok(())
    }

    /// A read-only view of the whole picture.
    pub fn frame(&self) -> Frame<'_> {
        Frame {
            plane: [
                PlaneRef::new(&self.plane[0]),
                PlaneRef::new(&self.plane[1]),
                PlaneRef::new(&self.plane[2]),
                PlaneRef::new(&self.plane[3]),
            ],
            width: self.width,
            height: self.height,
            format: self.format.unwrap_or(Format::Argb),
            chroma_full: self.chroma_full,
            premultiplied: self.premultiplied,
            flip: false,
        }
    }

    /// A writable view of the whole picture.
    pub fn frame_mut(&mut self) -> FrameMut<'_> {
        let (width, height) = (self.width, self.height);
        let format = self.format.unwrap_or(Format::Argb);
        let [p0, p1, p2, p3] = &mut self.plane;

        FrameMut {
            plane: [
                PlaneMut::new(p0),
                PlaneMut::new(p1),
                PlaneMut::new(p2),
                PlaneMut::new(p3),
            ],
            width,
            height,
            format,
        }
    }
}

/// One plane of a borrowed picture.
#[derive(Clone, Copy)]
pub struct PlaneRef<'a> {
    data: &'a [u8],
    stride: usize,
    /// Offset of row zero, which a crop moves.
    origin: usize,
}

impl<'a> PlaneRef<'a> {
    fn new(plane: &'a Plane) -> Self {
        PlaneRef {
            data: &plane.data,
            stride: plane.stride,
            origin: 0,
        }
    }

    /// A plane whose memory the caller holds, which is how a codec's picture
    /// and the C ABI shim's images get in.
    pub fn borrowed(data: &'a [u8], stride: usize) -> Self {
        PlaneRef {
            data,
            stride,
            origin: 0,
        }
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// `len` bytes of row `y`, starting `from` bytes in.
    pub fn row(&self, y: i32, from: usize, len: usize) -> &'a [u8] {
        let at = self.origin + y as usize * self.stride + from;

        &self.data[at..at + len]
    }
}

/// One plane of a borrowed writable picture.
pub struct PlaneMut<'a> {
    data: &'a mut [u8],
    stride: usize,
    origin: usize,
}

impl<'a> PlaneMut<'a> {
    fn new(plane: &'a mut Plane) -> Self {
        PlaneMut {
            data: &mut plane.data,
            stride: plane.stride,
            origin: 0,
        }
    }

    /// As [`PlaneRef::borrowed`].
    pub fn borrowed(data: &'a mut [u8], stride: usize) -> Self {
        PlaneMut {
            data,
            stride,
            origin: 0,
        }
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn row(&self, y: i32, from: usize, len: usize) -> &[u8] {
        let at = self.origin + y as usize * self.stride + from;

        &self.data[at..at + len]
    }

    pub fn row_mut(&mut self, y: i32, from: usize, len: usize) -> &mut [u8] {
        let at = self.origin + y as usize * self.stride + from;

        &mut self.data[at..at + len]
    }
}

/// A picture the decoder may read: a buffer's, a codec's, or a crop of either.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub plane: [PlaneRef<'a>; 4],
    pub width: i32,
    pub height: i32,
    pub format: Format,
    pub chroma_full: bool,
    pub premultiplied: bool,
    /// Whether the rows come out bottom-first. Applied by [`Frame::row`], so
    /// only the C ABI shim ever turns it back into a negative stride.
    pub flip: bool,
}

impl<'a> Frame<'a> {
    /// A picture whose four planes the caller already holds.
    pub fn borrowed(
        plane: [PlaneRef<'a>; 4],
        width: i32,
        height: i32,
        format: Format,
    ) -> Self {
        Frame {
            plane,
            width,
            height,
            format,
            chroma_full: false,
            premultiplied: false,
            flip: false,
        }
    }

    /// A picture made of one packed plane the caller already holds.
    pub fn packed(
        data: &'a [u8],
        stride: usize,
        w: i32,
        h: i32,
        format: Format,
    ) -> Self {
        let empty = PlaneRef {
            data: &[],
            stride: 0,
            origin: 0,
        };

        Frame {
            plane: [
                PlaneRef {
                    data,
                    stride,
                    origin: 0,
                },
                empty,
                empty,
                empty,
            ],
            width: w,
            height: h,
            format,
            chroma_full: false,
            premultiplied: false,
            flip: false,
        }
    }

    /// The width of plane `p` in bytes.
    pub fn row_len(&self, p: usize) -> usize {
        if planes_of(self.format) == 1 {
            self.width as usize * self.format.bpp()
        } else {
            ceil_rshift(self.width, chroma(p)) as usize
        }
    }

    /// How many rows plane `p` has.
    pub fn rows(&self, p: usize) -> i32 {
        if planes_of(self.format) == 1 {
            self.height
        } else {
            ceil_rshift(self.height, chroma(p))
        }
    }

    /// Row `y` of plane `p`, with the crop and the flip applied.
    pub fn row(&self, p: usize, y: i32) -> &'a [u8] {
        let len = self.row_len(p);
        let y = if self.flip { self.rows(p) - 1 - y } else { y };
        let at = self.plane[p].origin + y as usize * self.plane[p].stride;

        &self.plane[p].data[at..at + len]
    }

    /// The same picture read bottom-first.
    pub fn flipped(mut self) -> Self {
        self.flip = !self.flip;
        self
    }

    /// The `w` by `h` window whose top-left corner is at `(x, y)`.
    ///
    /// The corner is in luma coordinates and is halved for the chroma planes,
    /// which is why a crop of a planar picture only lands where the caller
    /// expects on an even boundary — the same rule the C's pointer arithmetic
    /// followed.
    pub fn window(&self, x: i32, y: i32, w: i32, h: i32) -> Self {
        let mut out = *self;

        for p in 0..4 {
            let shift = if planes_of(self.format) == 1 {
                0
            } else {
                chroma(p)
            };
            let bpp = if planes_of(self.format) == 1 {
                self.format.bpp()
            } else {
                1
            };

            out.plane[p].origin += (y >> shift) as usize * self.plane[p].stride
                + (x >> shift) as usize * bpp;
        }
        out.width = w;
        out.height = h;
        out
    }
}

/// A picture the decoder may write.
pub struct FrameMut<'a> {
    pub plane: [PlaneMut<'a>; 4],
    pub width: i32,
    pub height: i32,
    pub format: Format,
}

impl<'a> FrameMut<'a> {
    /// A picture whose four planes the caller already holds.
    pub fn borrowed(
        plane: [PlaneMut<'a>; 4],
        width: i32,
        height: i32,
        format: Format,
    ) -> Self {
        FrameMut {
            plane,
            width,
            height,
            format,
        }
    }

    /// The four planes, each separately borrowed.
    ///
    /// They are four allocations, so destructuring the array is what lets a
    /// kernel write two of them while reading a third — which the chroma
    /// blend needs and no per-plane accessor can express.
    pub fn planes_mut(&mut self) -> &mut [PlaneMut<'a>; 4] {
        &mut self.plane
    }

    pub fn row_len(&self, p: usize) -> usize {
        if planes_of(self.format) == 1 {
            self.width as usize * self.format.bpp()
        } else {
            ceil_rshift(self.width, chroma(p)) as usize
        }
    }

    pub fn rows(&self, p: usize) -> i32 {
        if planes_of(self.format) == 1 {
            self.height
        } else {
            ceil_rshift(self.height, chroma(p))
        }
    }

    /// Row `y` of plane `p`.
    pub fn row(&mut self, p: usize, y: i32) -> &mut [u8] {
        let len = self.row_len(p);
        let at = self.plane[p].origin + y as usize * self.plane[p].stride;

        &mut self.plane[p].data[at..at + len]
    }

    /// Rows `y` of two planes at once, which the chroma kernels need.
    ///
    /// The planes must be given in order, because splitting the array at the
    /// second is what proves to the compiler that the two do not overlap.
    pub fn row_pair(&mut self, a: usize, b: usize, y: i32) -> (&mut [u8], &mut [u8]) {
        assert!(a < b, "planes must be asked for in order");

        let (len_a, len_b) = (self.row_len(a), self.row_len(b));
        let at_a = self.plane[a].origin + y as usize * self.plane[a].stride;
        let at_b = self.plane[b].origin + y as usize * self.plane[b].stride;
        let (lo, hi) = self.plane.split_at_mut(b);

        (
            &mut lo[a].data[at_a..at_a + len_a],
            &mut hi[0].data[at_b..at_b + len_b],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packed_buffer_has_one_plane_and_the_rest_are_empty() {
        let mut buf = Buffer::default();

        buf.alloc_packed(3, 2, 4, Format::Argb).unwrap();
        assert_eq!(buf.frame().row(0, 0).len(), 12);
        assert!(buf.frame().plane[1].is_empty());
        assert_eq!(buf.width, 3);
        assert_eq!(buf.height, 2);
    }

    #[test]
    fn a_planar_buffer_rounds_its_chroma_up() {
        let mut buf = Buffer::default();

        buf.alloc_planar(5, 3, true).unwrap();

        let f = buf.frame();

        assert_eq!(f.row_len(0), 5);
        assert_eq!(f.rows(0), 3);
        assert_eq!(f.row_len(1), 3);
        assert_eq!(f.rows(1), 2);
        assert_eq!(f.row_len(3), 5);
    }

    /// A flip is a reading order, not a rewrite: the same bytes come out in
    /// the opposite order and nothing has moved.
    #[test]
    fn flipping_reverses_the_rows_and_flipping_twice_restores_them() {
        let mut buf = Buffer::default();

        buf.alloc_packed(1, 3, 4, Format::Argb).unwrap();
        for y in 0..3 {
            buf.frame_mut().row(0, y)[0] = y as u8;
        }

        let f = buf.frame();

        assert_eq!(f.row(0, 0)[0], 0);
        assert_eq!(f.flipped().row(0, 0)[0], 2);
        assert_eq!(f.flipped().row(0, 2)[0], 0);
        assert_eq!(f.flipped().flipped().row(0, 0)[0], 0);
    }

    #[test]
    fn a_window_moves_the_origin_and_keeps_the_stride() {
        let mut buf = Buffer::default();

        buf.alloc_packed(4, 4, 4, Format::Argb).unwrap();
        buf.frame_mut().row(0, 2)[8] = 0x5a;

        let w = buf.frame().window(2, 2, 2, 2);

        assert_eq!(w.width, 2);
        assert_eq!(w.row(0, 0)[0], 0x5a);
    }

    /// The C's `image_free` left an image that could be allocated into again,
    /// and so must this.
    #[test]
    fn a_released_buffer_can_be_allocated_again() {
        let mut buf = Buffer::default();

        buf.alloc_argb(2, 2).unwrap();
        buf.release();
        assert!(buf.is_empty());
        buf.alloc_argb(4, 4).unwrap();
        assert_eq!(buf.frame().row(0, 3).len(), 16);
    }

    /// Reuse must not hand back a row of the previous frame's pixels.
    #[test]
    fn shrinking_and_growing_again_still_starts_from_zero() {
        let mut buf = Buffer::default();

        buf.alloc_argb(8, 8).unwrap();
        buf.frame_mut().row(0, 0)[0] = 0xff;
        buf.alloc_argb(2, 2).unwrap();
        assert_eq!(buf.frame().row(0, 0)[0], 0);
    }
}
