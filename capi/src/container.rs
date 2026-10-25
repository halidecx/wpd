use std::ffi::c_int;
use std::{mem, slice};

use wpd::container::{Coding, Info};

use crate::status::{status, WPD_ERR_INTERNAL, WPD_ERR_INVALID_ARG, WPD_OK};

#[repr(C)]
pub struct WPDImageInfo {
    pub struct_size: usize,
    pub width: c_int,
    pub height: c_int,
    pub has_alpha: c_int,
    pub is_animation: c_int,
    pub frame_count: c_int,
    pub loop_count: c_int,
    pub background_argb: u32,
    pub coding: c_int,
    pub metadata: c_int,
}

impl WPDImageInfo {
    pub(crate) fn v1() -> usize {
        mem::offset_of!(WPDImageInfo, metadata) + mem::size_of::<c_int>()
    }
}

pub(crate) fn coding(c: Coding) -> c_int {
    match c {
        Coding::Lossy => 1,
        Coding::Lossless => 2,
        /* `Coding` is non-exhaustive; WPD_CODING_UNKNOWN covers anything
         * the header has no value for yet. */
        _ => 0,
    }
}

fn fill_info(info: &mut WPDImageInfo, from: &Info) {
    info.width = from.width;
    info.height = from.height;
    info.has_alpha = c_int::from(from.has_alpha);
    info.is_animation = c_int::from(from.animation);
    info.frame_count = from.frame_count;
    info.loop_count = from.loop_count;
    info.background_argb = from.background_argb;
    info.coding = coding(from.coding);
    info.metadata = from.metadata;
}

pub(crate) fn info_clear(info: &mut WPDImageInfo) {
    fill_info(info, &Info::default());
    info.coding = coding(Coding::Unknown);
}

/// # Safety
///
/// `data` must be readable for `size` bytes. `info` must point to a writable
/// `WPDImageInfo` whose `struct_size` is set and no larger than the storage
/// behind it; only its first `WPDImageInfo::v1()` bytes are accessed.
#[no_mangle]
pub unsafe extern "C" fn wpd_get_info(
    data: *const u8,
    size: usize,
    info: *mut WPDImageInfo,
) -> c_int {
    if info.is_null() || data.is_null() || size > isize::MAX as usize {
        return WPD_ERR_INVALID_ARG;
    }
    // SAFETY: `info` is non-null (checked above) and, per the C contract,
    // points to a caller-initialised `WPDImageInfo`, which begins with
    // `struct_size`. Reading through `addr_of!` forms no reference to the rest.
    let declared = unsafe { std::ptr::addr_of!((*info).struct_size).read() };

    if declared < WPDImageInfo::v1() {
        return WPD_ERR_INVALID_ARG;
    }

    // SAFETY: `data` is non-null and `size` fits an isize (both checked above);
    // the caller guarantees `size` readable bytes there for this call, and
    // `buf` does not escape it.
    let buf = unsafe { slice::from_raw_parts(data, size) };

    crate::guard(WPD_ERR_INTERNAL, || {
        // SAFETY: `info` is non-null and the caller declared at least
        // `WPDImageInfo::v1()` bytes of it (checked above), which is all
        // `with_prefix` reads and writes. The struct is plain integers, so a
        // zeroed local is a valid value.
        let scanned = unsafe {
            crate::decoder::with_prefix(info, WPDImageInfo::v1(), |info| {
                info_clear(info);
                wpd::container::get_info(buf).map(|scanned| fill_info(info, &scanned))
            })
        };

        match scanned {
            Ok(()) => WPD_OK,
            Err(e) => status(e),
        }
    })
}
