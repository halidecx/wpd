use std::ffi::{c_int, c_void};
use std::{mem, ptr, slice};

use wpd::handout::{Handout, Pixels, RowSink};
use wpd::image::external_plane_fits;
use wpd::picture::Frame;

const WPD_DISPOSE_BACKGROUND: c_int = 1;
const WPD_DISPOSE_NONE: c_int = 0;
const WPD_BLEND_ALPHA: c_int = 0;
const WPD_BLEND_NONE: c_int = 1;

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

        unsafe {
            slice::from_raw_parts_mut(plane.data.offset(y as isize * plane.stride), len)
        }
    }
}

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

fn has_alpha_extent() -> usize {
    mem::offset_of!(WPDFrame, has_alpha) + mem::size_of::<c_int>()
}

pub(crate) fn private_data_extent() -> usize {
    mem::offset_of!(WPDFrame, private_data) + mem::size_of::<*mut c_void>()
}

pub(crate) fn frame_head() -> usize {
    mem::offset_of!(WPDFrame, data)
}

pub(crate) unsafe fn frame_valid(frame: *const WPDFrame) -> bool {
    unsafe { frame.as_ref() }.is_some_and(|f| f.struct_size >= private_data_extent())
}

pub(crate) unsafe fn frame_extent(frame: *const WPDFrame) -> usize {
    if unsafe { (*frame).struct_size } >= has_alpha_extent() {
        has_alpha_extent()
    } else {
        private_data_extent()
    }
}

pub(crate) unsafe fn frame_clear(frame: *mut WPDFrame) {
    let head = frame_head();
    let extent = unsafe { frame_extent(frame) };

    unsafe { ptr::write_bytes(frame.cast::<u8>().add(head), 0, extent - head) };
}

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
