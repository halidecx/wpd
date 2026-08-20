use std::ffi::c_int;
use std::slice;

use wpd::picture::{PlaneMut, PlaneRef};

#[no_mangle]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_rescale_plane(
    dst: *mut u8,
    dst_stride: c_int,
    dst_width: c_int,
    dst_height: c_int,
    src: *const u8,
    src_stride: c_int,
    src_width: c_int,
    src_height: c_int,
    num_channels: c_int,
    work: *mut u32,
) {
    let channels = num_channels as usize;
    let extent = |stride: c_int, width: c_int, height: c_int| {
        (height as usize - 1) * stride as usize + width as usize * channels
    };

    crate::guard((), || unsafe {
        let mut out = PlaneMut::borrowed(
            slice::from_raw_parts_mut(dst, extent(dst_stride, dst_width, dst_height)),
            dst_stride as usize,
        );
        let inp = PlaneRef::borrowed(
            slice::from_raw_parts(src, extent(src_stride, src_width, src_height)),
            src_stride as usize,
        );
        let work = slice::from_raw_parts_mut(work, 2 * channels * dst_width as usize);

        wpd::rescale::rescale_plane(
            work, &mut out, dst_width, dst_height, &inp, src_width, src_height,
            channels,
        );
    });
}
