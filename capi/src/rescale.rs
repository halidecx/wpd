use std::ffi::c_int;
use std::slice;

use wpd::picture::{PlaneMut, PlaneRef};

use crate::frame::plane_extent;

fn disjoint(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> bool {
    let (a, b) = (a as usize, b as usize);

    a.saturating_add(a_len) <= b || b.saturating_add(b_len) <= a
}

/// # Safety
///
/// For a valid geometry, `dst` must be writable for
/// `(dst_height - 1) * dst_stride + dst_width * num_channels` bytes, `src`
/// readable for the same expression over the source arguments, and `work`
/// writable for `2 * dst_width * num_channels` `u32`s, all for the duration of
/// the call. Invalid geometry, null, misaligned `work`, or overlapping buffers
/// make the call return without touching any of them.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
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
            plane_extent(stride as isize, usize::try_from(height).ok()?, row(width)?)
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
    if work as usize % std::mem::align_of::<u32>() != 0 {
        return;
    }

    let work_bytes = work_len * std::mem::size_of::<u32>();

    if !disjoint(dst, dst_extent, src, src_extent)
        || !disjoint(dst, dst_extent, work.cast(), work_bytes)
        || !disjoint(src, src_extent, work.cast(), work_bytes)
    {
        return;
    }

    // SAFETY: all three pointers are non-null and `work` is aligned (checked
    // above). The caller guarantees `dst_extent`, `src_extent` and `work_len`
    // valid elements behind them, each extent fits an isize, and the three
    // ranges were checked disjoint, so the `&mut` slices alias nothing.
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
            // SAFETY: every geometry here is invalid, so the call returns
            // before touching the null pointers.
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

    #[test]
    fn overlapping_or_misaligned_rescale_buffers_are_left_alone() {
        let mut dst = [0xaau8; 64];
        let mut src = [7u8; 64];
        let mut work = [0u32; 64];
        let mut misaligned = [0u8; 256];
        let work_ptr = work.as_mut_ptr();
        // SAFETY: offset 1 stays inside the 256-byte `misaligned` array.
        let odd = unsafe { misaligned.as_mut_ptr().add(1) }.cast::<u32>();

        // SAFETY: each call names live local arrays covering the declared
        // extents; the overlapping or misaligned ones are rejected unused.
        unsafe {
            wpd_rescale_plane(
                src.as_mut_ptr(),
                8,
                4,
                4,
                src.as_ptr(),
                8,
                8,
                8,
                1,
                work_ptr,
            );
            wpd_rescale_plane(
                dst.as_mut_ptr(),
                8,
                4,
                4,
                src.as_ptr(),
                8,
                8,
                8,
                1,
                dst.as_mut_ptr().cast(),
            );
            wpd_rescale_plane(dst.as_mut_ptr(), 8, 4, 4, src.as_ptr(), 8, 8, 8, 1, odd);
        }
        assert!(dst.iter().all(|&b| b == 0xaa));
        assert!(src.iter().all(|&b| b == 7));
        assert!(misaligned.iter().all(|&b| b == 0));

        // SAFETY: `dst` (64 bytes) covers 3 * 8 + 4, `src` (64) covers
        // 7 * 8 + 8, `work` holds 64 >= 2 * 4 `u32`s, and the three are disjoint.
        unsafe {
            wpd_rescale_plane(
                dst.as_mut_ptr(),
                8,
                4,
                4,
                src.as_ptr(),
                8,
                8,
                8,
                1,
                work_ptr,
            );
        }
        for row in dst[..32].chunks(8) {
            assert!(row[..4].iter().all(|&b| b == 7));
        }
    }
}
