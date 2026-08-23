use crate::error::{Error, Result};
use crate::image::{ceil_rshift, plane_size, Format};

fn chroma(p: usize) -> u32 {
    u32::from(p == 1 || p == 2)
}

#[derive(Default)]
pub struct Plane {
    data: Vec<u8>,
    stride: usize,
}

impl Plane {
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

#[derive(Default)]
pub struct Buffer {
    plane: [Plane; 4],
    pub width: i32,
    pub height: i32,
    pub format: Option<Format>,
    pub chroma_full: bool,
    pub premultiplied: bool,
}

impl Buffer {
    pub fn is_empty(&self) -> bool {
        self.plane[0].data.is_empty()
    }

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
        self.chroma_full = false;
        Ok(())
    }

    pub fn alloc_argb(&mut self, w: i32, h: i32) -> Result<()> {
        self.alloc_packed(w, h, 4, Format::Argb)
    }

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
        self.chroma_full = !subsample;
        Ok(())
    }

    pub fn drop_plane(&mut self, p: usize) {
        self.plane[p].release();
    }

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

    pub fn frame_mut(&mut self) -> FrameMut<'_> {
        let (width, height) = (self.width, self.height);
        let format = self.format.unwrap_or(Format::Argb);
        let chroma_full = self.chroma_full;
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
            chroma_full,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PlaneRef<'a> {
    data: &'a [u8],
    stride: usize,
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

    pub fn row(&self, y: i32, from: usize, len: usize) -> &'a [u8] {
        let at = self.origin + y as usize * self.stride + from;

        &self.data[at..at + len]
    }
}

pub struct PlaneMut<'a> {
    data: &'a mut [u8],
    stride: usize,
    origin: usize,
    /// The picture row `data` begins at. A band cut out of a plane keeps the
    /// row numbers it had in the whole plane, so nothing walking the picture
    /// in absolute rows has to know it is looking at a piece of one.
    first: i32,
}

impl<'a> PlaneMut<'a> {
    fn new(plane: &'a mut Plane) -> Self {
        PlaneMut {
            data: &mut plane.data,
            stride: plane.stride,
            origin: 0,
            first: 0,
        }
    }

    pub fn borrowed(data: &'a mut [u8], stride: usize) -> Self {
        PlaneMut {
            data,
            stride,
            origin: 0,
            first: 0,
        }
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    #[inline(always)]
    fn at(&self, y: i32, from: usize) -> usize {
        self.origin + (y - self.first) as usize * self.stride + from
    }

    pub fn row(&self, y: i32, from: usize, len: usize) -> &[u8] {
        let at = self.at(y, from);

        &self.data[at..at + len]
    }

    pub fn row_mut(&mut self, y: i32, from: usize, len: usize) -> &mut [u8] {
        let at = self.at(y, from);

        &mut self.data[at..at + len]
    }

    pub fn row_pair_mut(
        &mut self,
        a: i32,
        b: i32,
        from: usize,
        len: usize,
    ) -> (&mut [u8], &mut [u8]) {
        assert!(a < b, "rows must be asked for in order");

        let at_a = self.at(a, from);
        let at_b = self.at(b, from);
        let (lo, hi) = self.data.split_at_mut(at_b);

        (&mut lo[at_a..at_a + len], &mut hi[..len])
    }

    /// Borrows this plane as one that can be handed on or split.
    pub fn reborrow(&mut self) -> PlaneMut<'_> {
        PlaneMut {
            data: self.data,
            stride: self.stride,
            origin: self.origin,
            first: self.first,
        }
    }

    /// Cuts the plane in two at row `y`. Both halves answer to the row numbers
    /// they had here, so two threads can write disjoint bands of one picture
    /// without either of them counting rows differently.
    pub fn split_rows_at(self, y: i32) -> (Self, Self) {
        let at = self.at(y, 0);
        let (lo, hi) = self.data.split_at_mut(at);

        (
            PlaneMut {
                data: lo,
                stride: self.stride,
                origin: self.origin,
                first: self.first,
            },
            PlaneMut {
                data: hi,
                stride: self.stride,
                origin: 0,
                first: y,
            },
        )
    }
}

#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub plane: [PlaneRef<'a>; 4],
    pub width: i32,
    pub height: i32,
    pub format: Format,
    pub chroma_full: bool,
    pub premultiplied: bool,
    pub flip: bool,
}

impl<'a> Frame<'a> {
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

    fn shift(&self, p: usize) -> u32 {
        if self.format.nb_components() == 1 || self.chroma_full {
            0
        } else {
            chroma(p)
        }
    }

    pub fn row_len(&self, p: usize) -> usize {
        if self.format.nb_components() == 1 {
            self.width as usize * self.format.bpp()
        } else {
            ceil_rshift(self.width, self.shift(p)) as usize
        }
    }

    pub fn rows(&self, p: usize) -> i32 {
        if self.format.nb_components() == 1 {
            self.height
        } else {
            ceil_rshift(self.height, self.shift(p))
        }
    }

    pub fn row(&self, p: usize, y: i32) -> &'a [u8] {
        let len = self.row_len(p);
        let y = if self.flip { self.rows(p) - 1 - y } else { y };
        let at = self.plane[p].origin + y as usize * self.plane[p].stride;

        &self.plane[p].data[at..at + len]
    }

    pub fn flipped(mut self) -> Self {
        self.flip = !self.flip;
        self
    }

    pub fn window(&self, x: i32, y: i32, w: i32, h: i32) -> Self {
        let mut out = *self;

        for p in 0..4 {
            let shift = self.shift(p);
            let bpp = if self.format.nb_components() == 1 {
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

pub struct FrameMut<'a> {
    pub plane: [PlaneMut<'a>; 4],
    pub width: i32,
    pub height: i32,
    pub format: Format,
    pub chroma_full: bool,
}

impl<'a> FrameMut<'a> {
    pub fn borrowed(
        plane: [PlaneMut<'a>; 4],
        width: i32,
        height: i32,
        format: Format,
        chroma_full: bool,
    ) -> Self {
        FrameMut {
            plane,
            width,
            height,
            format,
            chroma_full,
        }
    }

    fn shift(&self, p: usize) -> u32 {
        if self.format.nb_components() == 1 || self.chroma_full {
            0
        } else {
            chroma(p)
        }
    }

    pub fn planes_mut(&mut self) -> &mut [PlaneMut<'a>; 4] {
        &mut self.plane
    }

    pub fn row_len(&self, p: usize) -> usize {
        if self.format.nb_components() == 1 {
            self.width as usize * self.format.bpp()
        } else {
            ceil_rshift(self.width, self.shift(p)) as usize
        }
    }

    pub fn rows(&self, p: usize) -> i32 {
        if self.format.nb_components() == 1 {
            self.height
        } else {
            ceil_rshift(self.height, self.shift(p))
        }
    }

    pub fn row(&mut self, p: usize, y: i32) -> &mut [u8] {
        let len = self.row_len(p);
        let at = self.plane[p].origin + y as usize * self.plane[p].stride;

        &mut self.plane[p].data[at..at + len]
    }

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

    #[test]
    fn a_released_buffer_can_be_allocated_again() {
        let mut buf = Buffer::default();

        buf.alloc_argb(2, 2).unwrap();
        buf.release();
        assert!(buf.is_empty());
        buf.alloc_argb(4, 4).unwrap();
        assert_eq!(buf.frame().row(0, 3).len(), 16);
    }

    #[test]
    fn shrinking_and_growing_again_still_starts_from_zero() {
        let mut buf = Buffer::default();

        buf.alloc_argb(8, 8).unwrap();
        buf.frame_mut().row(0, 0)[0] = 0xff;
        buf.alloc_argb(2, 2).unwrap();
        assert_eq!(buf.frame().row(0, 0)[0], 0);
    }
}
