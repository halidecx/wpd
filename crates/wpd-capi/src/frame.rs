//! `WPDFrame` and the caller's own output planes.
//!
//! Everything the C ABI's picture is made of that the decoder does not need to
//! know: the versioned struct a decode is reported through, and the planes a
//! caller may supply for the pixels to be written into. Both are pointers the
//! header promises something about and nothing on this side can check.

use std::ffi::{c_int, c_void};
use std::{mem, ptr, slice};

use wpd::handout::{Handout, Pixels, RowSink};
use wpd::image::external_plane_fits;
use wpd::picture::Frame;

const WPD_DISPOSE_BACKGROUND: c_int = 1;
const WPD_DISPOSE_NONE: c_int = 0;
const WPD_BLEND_ALPHA: c_int = 0;
const WPD_BLEND_NONE: c_int = 1;

/// `WPDOutputPlane` from `include/wpd.h`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WPDOutputPlane {
    pub data: *mut u8,
    pub size: usize,
    pub stride: isize,
}

impl WPDOutputPlane {
    pub(crate) fn empty() -> Self {
        WPDOutputPlane {
            data: ptr::null_mut(),
            size: 0,
            stride: 0,
        }
    }
}

/// The caller's own output planes, which the decoder writes into when
/// `wpd_decoder_set_output_buffer` has named them.
///
/// This is the one destination in the decoder that is neither its own memory
/// nor checked by the compiler: a plane is a pointer, a byte count and a
/// stride that may run backwards. [`wpd::image::external_plane_fits`] is asked
/// about the geometry before a single row is written, and nothing else here
/// takes the caller's word for anything.
pub struct External(pub [WPDOutputPlane; 4]);

impl RowSink for External {
    fn fits(&self, p: usize, row_len: usize, rows: i32) -> bool {
        let plane = &self.0[p];

        !plane.data.is_null()
            && plane.stride != 0
            && external_plane_fits(plane.size, plane.stride, row_len, rows)
    }

    fn row(&mut self, p: usize, y: i32, len: usize) -> &mut [u8] {
        let plane = &self.0[p];

        /* Sound because `fits` has agreed that the plane holds this row: the
        stride may be negative, so the offset is signed and the pointer walks
        backwards from the plane's first byte exactly as the C's did. */
        unsafe {
            slice::from_raw_parts_mut(plane.data.offset(y as isize * plane.stride), len)
        }
    }
}

/// `WPDFrame` from `include/wpd.h`.
#[repr(C)]
pub struct WPDFrame {
    pub struct_size: usize,
    pub data: [*const u8; 4],
    pub stride: [isize; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: c_int,
    pub duration: c_int,
    pub timestamp: i64,
    pub private_data: *mut c_void,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
}

impl WPDFrame {
    /// A zeroed frame of this build's revision, which is what the header's
    /// `WPD_FRAME_INIT` produces.
    pub(crate) fn zeroed() -> Self {
        WPDFrame {
            struct_size: mem::size_of::<WPDFrame>(),
            data: [ptr::null(); 4],
            stride: [0; 4],
            width: 0,
            height: 0,
            format: 0,
            duration: 0,
            timestamp: 0,
            private_data: ptr::null_mut(),
            pos_x: 0,
            pos_y: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
        }
    }
}

/// How far into `WPDFrame` the sub-frame placement fields start, which a
/// caller compiled against an older revision has not made room for.
fn has_alpha_extent() -> usize {
    mem::offset_of!(WPDFrame, has_alpha) + mem::size_of::<c_int>()
}

/// The oldest revision of `WPDFrame` this build accepts.
pub(crate) fn private_data_extent() -> usize {
    mem::offset_of!(WPDFrame, private_data) + mem::size_of::<*mut c_void>()
}

/// # Safety
///
/// `frame`, when not null, must point to a `WPDFrame` of at least its own
/// declared `struct_size` bytes.
pub(crate) unsafe fn frame_valid(frame: *const WPDFrame) -> bool {
    unsafe { frame.as_ref() }.is_some_and(|f| f.struct_size >= private_data_extent())
}

/// How much of the caller's frame this build may touch: the newest revision of
/// the struct it declares room for in full, capped at the newest this build
/// knows about. A size landing between two revisions rounds down to the older
/// one rather than writing part of a field pair the caller may not have.
///
/// # Safety
///
/// As [`frame_valid`], and the frame must not be null.
pub(crate) unsafe fn frame_extent(frame: *const WPDFrame) -> usize {
    if unsafe { (*frame).struct_size } >= has_alpha_extent() {
        has_alpha_extent()
    } else {
        private_data_extent()
    }
}

/// Zeroes everything past `struct_size`, which is the caller's and survives.
///
/// # Safety
///
/// As [`frame_extent`], and the frame must be writable.
pub(crate) unsafe fn frame_clear(frame: *mut WPDFrame) {
    let head = mem::size_of::<usize>();
    let extent = unsafe { frame_extent(frame) };

    unsafe { ptr::write_bytes(frame.cast::<u8>().add(head), 0, extent - head) };
}

/// Writes a finished handout into the caller's `WPDFrame`.
///
/// This is the only place the C ABI's shape is built, and the only place a
/// flip becomes the negative stride `include/wpd.h` promises: everywhere
/// inside the decoder a flip is a reading order.
///
/// # Safety
///
/// `frame` must point to a `WPDFrame` of at least its own declared
/// `struct_size` bytes.
pub(crate) unsafe fn write_frame(
    handout: &Handout<'_>,
    ext: &[WPDOutputPlane; 4],
    frame: *mut WPDFrame,
) {
    unsafe { frame_clear(frame) };

    let out = unsafe { &mut *frame };
    let planes = handout.planes();

    match &handout.pixels {
        Pixels::Own(img) => {
            for p in 0..planes {
                let (data, stride) = handout_plane(img, p);

                out.data[p] = data;
                out.stride[p] = stride;
            }
        }
        Pixels::Sink => {
            for (p, plane) in ext.iter().enumerate() {
                out.data[p] = if p < planes { plane.data } else { ptr::null() };
                out.stride[p] = if p < planes { plane.stride } else { 0 };
            }
        }
        Pixels::None => {}
    }
    out.width = handout.width;
    out.height = handout.height;
    out.format = handout.format as c_int;
    out.duration = handout.duration;
    out.timestamp = handout.timestamp;
    if unsafe { frame_extent(frame) } < has_alpha_extent() {
        return;
    }
    let out = unsafe { &mut *frame };

    out.pos_x = handout.pos_x;
    out.pos_y = handout.pos_y;
    out.dispose = if handout.dispose_to_background {
        WPD_DISPOSE_BACKGROUND
    } else {
        WPD_DISPOSE_NONE
    };
    out.blend = if handout.blend {
        WPD_BLEND_ALPHA
    } else {
        WPD_BLEND_NONE
    };
    out.has_alpha = c_int::from(handout.has_alpha);
}

/// The `(pointer, stride)` pair the C ABI hands plane `p` out as.
///
/// A flip is a reading order everywhere inside the decoder; here it becomes
/// the negative stride `include/wpd.h` promises, pointing at what is now the
/// first row.
fn handout_plane(img: &Frame<'_>, p: usize) -> (*const u8, isize) {
    if img.plane[p].is_empty() {
        return (ptr::null(), 0);
    }
    let stride = img.plane[p].stride() as isize;

    (
        img.row(p, 0).as_ptr(),
        if img.flip { -stride } else { stride },
    )
}
