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
    let Some((dst_extent, src_extent, work_len, channels)) = (|| {
        let channels = usize::try_from(num_channels).ok().filter(|&n| n != 0)?;
        let row = |width: c_int| {
            usize::try_from(width)
                .ok()
                .filter(|&n| n != 0)?
                .checked_mul(channels)
        };
        let extent = |stride: c_int, width: c_int, height: c_int| {
            let stride = usize::try_from(stride).ok()?;
            let height = usize::try_from(height).ok().filter(|&n| n != 0)?;
            let row = row(width)?;

            if stride < row {
                return None;
            }
            (height - 1)
                .checked_mul(stride)?
                .checked_add(row)
                .filter(|&n| n <= isize::MAX as usize)
        };
        let dst_extent = extent(dst_stride, dst_width, dst_height)?;
        let src_extent = extent(src_stride, src_width, src_height)?;
        let work_len = row(dst_width)?.checked_mul(2)?;

        if work_len.checked_mul(std::mem::size_of::<u32>())? > isize::MAX as usize {
            return None;
        }
        Some((dst_extent, src_extent, work_len, channels))
    })() else {
        return;
    };
    if dst.is_null() || src.is_null() || work.is_null() {
        return;
    }

    crate::guard((), || unsafe {
        let mut out = PlaneMut::borrowed(
            slice::from_raw_parts_mut(dst, dst_extent),
            dst_stride as usize,
        );
        let inp = PlaneRef::borrowed(
            slice::from_raw_parts(src, src_extent),
            src_stride as usize,
        );
        let work = slice::from_raw_parts_mut(work, work_len);

        wpd::rescale::rescale_plane(
            &wpd::dsp::rescale::RescaleDsp::new(),
            work,
            &mut out,
            dst_width,
            dst_height,
            &inp,
            src_width,
            src_height,
            channels,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_rescale_geometry_returns_before_touching_pointers() {
        for (w, h, stride, channels) in [
            (0, 1, 1, 1),
            (1, 0, 1, 1),
            (-1, 1, 1, 1),
            (2, 1, 1, 1),
            (1, 1, 1, 0),
            (1, 1, 1, -1),
        ] {
            unsafe {
                wpd_rescale_plane(
                    std::ptr::null_mut(),
                    stride,
                    w,
                    h,
                    std::ptr::null(),
                    stride,
                    w,
                    h,
                    channels,
                    std::ptr::null_mut(),
                );
            }
        }
    }
}
