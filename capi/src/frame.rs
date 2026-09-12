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
    !frame.is_null()
        && unsafe { ptr::addr_of!((*frame).struct_size).read() }
            >= private_data_extent()
}

pub(crate) unsafe fn frame_extent(frame: *const WPDFrame) -> usize {
    if unsafe { ptr::addr_of!((*frame).struct_size).read() } >= has_alpha_extent() {
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

    let planes = handout.planes();

    match &handout.pixels {
        Pixels::Own(img) => {
            for p in 0..planes {
                let (data, stride) = handout_plane(img, p);

                unsafe {
                    ptr::addr_of_mut!((*frame).data)
                        .cast::<*const u8>()
                        .add(p)
                        .write(data);
                    ptr::addr_of_mut!((*frame).stride)
                        .cast::<isize>()
                        .add(p)
                        .write(stride);
                }
            }
        }
        Pixels::Sink => {
            for (p, plane) in ext.iter().enumerate() {
                unsafe {
                    ptr::addr_of_mut!((*frame).data)
                        .cast::<*const u8>()
                        .add(p)
                        .write(if p < planes { plane.data } else { ptr::null() });
                    ptr::addr_of_mut!((*frame).stride)
                        .cast::<isize>()
                        .add(p)
                        .write(if p < planes { plane.stride } else { 0 });
                }
            }
        }
        Pixels::None => {}
    }
    unsafe {
        ptr::addr_of_mut!((*frame).width).write(handout.width);
        ptr::addr_of_mut!((*frame).height).write(handout.height);
        ptr::addr_of_mut!((*frame).format).write(handout.format as c_int);
        ptr::addr_of_mut!((*frame).duration).write(handout.duration);
        ptr::addr_of_mut!((*frame).timestamp).write(handout.timestamp);
    }
    if unsafe { frame_extent(frame) } < has_alpha_extent() {
        return;
    }
    unsafe {
        ptr::addr_of_mut!((*frame).pos_x).write(handout.pos_x);
        ptr::addr_of_mut!((*frame).pos_y).write(handout.pos_y);
        ptr::addr_of_mut!((*frame).dispose).write(if handout.dispose_to_background {
            WPD_DISPOSE_BACKGROUND
        } else {
            WPD_DISPOSE_NONE
        });
        ptr::addr_of_mut!((*frame).blend).write(if handout.no_blend {
            WPD_BLEND_NONE
        } else {
            WPD_BLEND_ALPHA
        });
        ptr::addr_of_mut!((*frame).has_alpha).write(c_int::from(handout.has_alpha));
    }
}

pub(crate) unsafe fn frame_private_data(frame: *const WPDFrame) -> *mut c_void {
    unsafe { ptr::addr_of!((*frame).private_data).read() }
}

pub(crate) unsafe fn frame_set_private_data(frame: *mut WPDFrame, owner: *mut c_void) {
    unsafe { ptr::addr_of_mut!((*frame).private_data).write(owner) };
}

pub(crate) unsafe fn frame_set_plane(
    frame: *mut WPDFrame,
    p: usize,
    data: *const u8,
    stride: isize,
) {
    unsafe {
        ptr::addr_of_mut!((*frame).data)
            .cast::<*const u8>()
            .add(p)
            .write(data);
        ptr::addr_of_mut!((*frame).stride)
            .cast::<isize>()
            .add(p)
            .write(stride);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct LegacyFrame {
        struct_size: usize,
        data: [*const u8; 4],
        stride: [isize; 4],
        width: c_int,
        height: c_int,
        format: c_int,
        duration: c_int,
        timestamp: i64,
        private_data: *mut c_void,
    }

    #[test]
    fn legacy_frame_storage_is_only_accessed_through_its_extent() {
        assert_eq!(mem::size_of::<LegacyFrame>(), private_data_extent());
        let mut frame = LegacyFrame {
            struct_size: mem::size_of::<LegacyFrame>(),
            data: [std::ptr::NonNull::<u8>::dangling().as_ptr().cast_const(); 4],
            stride: [7; 4],
            width: 11,
            height: 13,
            format: 2,
            duration: 17,
            timestamp: 19,
            private_data: ptr::null_mut(),
        };
        let frame = (&mut frame as *mut LegacyFrame).cast::<WPDFrame>();

        assert!(unsafe { frame_valid(frame) });
        let handout = Handout {
            width: 23,
            height: 29,
            duration: 31,
            timestamp: 37,
            ..Handout::default()
        };
        unsafe { write_frame(&handout, &[WPDOutputPlane::empty(); 4], frame) };
        assert_eq!(unsafe { ptr::addr_of!((*frame).width).read() }, 23);
        assert_eq!(unsafe { ptr::addr_of!((*frame).height).read() }, 29);
        assert!(unsafe { frame_private_data(frame) }.is_null());
    }
}
