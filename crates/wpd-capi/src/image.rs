//! `WebPImage`, the borrowed picture the two codecs hand back.
//!
//! Nothing allocates here any more: plane memory the decoder owns is
//! [`wpd::picture::Buffer`], and what is left of this type is the description
//! of memory a codec owns — three VP8 planes plus the alpha plane beside them,
//! or the lossless decoder's ARGB canvas. It is still a `(pointer, stride)`
//! set because that is what the codecs report, and [`WebPImage::frame`] is
//! where it becomes a slice.

use std::ffi::c_int;
use std::{ptr, slice};

use wpd::image::Format;
use wpd::picture::{Frame, FrameMut, PlaneMut, PlaneRef};

/// A picture the decoder may read but does not own.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WebPImage {
    pub chroma_full: c_int,
    pub premultiplied: c_int,
    pub data: [*mut u8; 4],
    pub linesize: [c_int; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: c_int,
}

impl WebPImage {
    /// How many bytes of plane `p` the picture's geometry covers.
    fn extent(&self, p: usize) -> usize {
        let planar = matches!(
            self.format(),
            Some(Format::Yuv420p) | Some(Format::Yuva420p)
        );
        let shift = u32::from(planar && self.chroma_full == 0 && (p == 1 || p == 2));
        let row = if planar {
            wpd::image::ceil_rshift(self.width, shift) as usize
        } else {
            self.width as usize * self.format().map_or(4, Format::bpp)
        };
        let rows = wpd::image::ceil_rshift(self.height, shift) as usize;

        if rows == 0 || row == 0 {
            return 0;
        }
        (rows - 1) * self.linesize[p].unsigned_abs() as usize + row
    }

    /// A safe view of the picture.
    ///
    /// # Safety
    ///
    /// The planes must be as the codec reported them. A flipped image has no
    /// view: a negative `linesize` is something the C ABI produces on the way
    /// out, never something the decoder reads through.
    pub(crate) unsafe fn frame(&self) -> Frame<'_> {
        let plane = core::array::from_fn(|p| {
            if self.data[p].is_null() || self.linesize[p] <= 0 {
                return PlaneRef::borrowed(&[], 0);
            }
            let bytes = unsafe { slice::from_raw_parts(self.data[p], self.extent(p)) };

            PlaneRef::borrowed(bytes, self.linesize[p] as usize)
        });

        let mut frame = Frame::borrowed(
            plane,
            self.width,
            self.height,
            self.format().unwrap_or(Format::Argb),
        );

        frame.chroma_full = self.chroma_full != 0;
        frame.premultiplied = self.premultiplied != 0;
        frame
    }

    /// As [`WebPImage::frame`], writable.
    ///
    /// # Safety
    ///
    /// As [`WebPImage::frame`], and nothing else may be looking at the planes.
    pub(crate) unsafe fn frame_mut(&mut self) -> FrameMut<'_> {
        let (width, height) = (self.width, self.height);
        let format = self.format().unwrap_or(Format::Argb);
        let chroma_full = self.chroma_full != 0;
        let extent: [usize; 4] = core::array::from_fn(|p| self.extent(p));
        let plane = core::array::from_fn(|p| {
            if self.data[p].is_null() || self.linesize[p] <= 0 {
                return PlaneMut::borrowed(&mut [], 0);
            }
            let bytes = unsafe { slice::from_raw_parts_mut(self.data[p], extent[p]) };

            PlaneMut::borrowed(bytes, self.linesize[p] as usize)
        });

        FrameMut::borrowed(plane, width, height, format, chroma_full)
    }

    /// An image that describes nothing, which is what a zeroed C struct was.
    pub(crate) fn empty() -> Self {
        WebPImage {
            chroma_full: 0,
            premultiplied: 0,
            data: [ptr::null_mut(); 4],
            linesize: [0; 4],
            width: 0,
            height: 0,
            format: 0,
        }
    }

    pub(crate) fn format(&self) -> Option<Format> {
        Format::from_raw(self.format)
    }
}
