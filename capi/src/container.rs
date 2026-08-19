//! `WPDImageInfo`, and the one public entry point that is nothing but a scan.
//!
//! The struct is versioned by `struct_size`, and the caller's copy may be a
//! longer revision than this build knows about. Nothing here writes it whole:
//! the fields are assigned one at a time, so the tail padding a future field
//! would occupy is left alone, which is what `WPD_FIELD_END` was guarding in
//! the C.

use std::ffi::c_int;
use std::{mem, slice};

use wpd::container::{Coding, Info};
use wpd::error::Error;

const WPD_OK: c_int = 0;
const WPD_ERR_INVALID_ARG: c_int = -1;
const WPD_ERR_NOT_WEBP: c_int = -2;
const WPD_ERR_BITSTREAM: c_int = -3;
const WPD_ERR_TRUNCATED: c_int = -4;
const WPD_ERR_UNSUPPORTED: c_int = -5;
const WPD_ERR_NO_MEMORY: c_int = -6;
const WPD_ERR_TOO_LARGE: c_int = -7;
const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;

/// `WPDImageInfo` from `include/wpd.h`.
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
    /// How much of a caller's struct this build may touch: through the last
    /// field it knows about, and no further.
    pub(crate) fn v1() -> usize {
        mem::offset_of!(WPDImageInfo, metadata) + mem::size_of::<c_int>()
    }
}

pub(crate) fn status(e: Error) -> c_int {
    match e {
        Error::InvalidArgument => WPD_ERR_INVALID_ARG,
        Error::InvalidData => WPD_ERR_BITSTREAM,
        Error::NoMemory => WPD_ERR_NO_MEMORY,
        Error::TooLarge => WPD_ERR_TOO_LARGE,
        Error::Truncated => WPD_ERR_TRUNCATED,
        Error::NotWebp => WPD_ERR_NOT_WEBP,
        Error::Unsupported => WPD_ERR_UNSUPPORTED,
        Error::BufferTooSmall => WPD_ERR_BUFFER_TOO_SMALL,
    }
}

fn coding(c: Coding) -> c_int {
    match c {
        Coding::Unknown => 0,
        Coding::Lossy => 1,
        Coding::Lossless => 2,
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

/// Zeroes everything this build knows about, leaving the caller's
/// `struct_size` and any field a newer revision added.
pub(crate) fn info_clear(info: &mut WPDImageInfo) {
    fill_info(info, &Info::default());
    info.coding = coding(Coding::Unknown);
}

/// # Safety
///
/// `data` must be readable for `size` bytes, and `info` writable as
/// `info_clear` requires.
#[no_mangle]
pub unsafe extern "C" fn wpd_get_info(
    data: *const u8,
    size: usize,
    info: *mut WPDImageInfo,
) -> c_int {
    let Some(info) = (unsafe { info.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() || info.struct_size < WPDImageInfo::v1() {
        return WPD_ERR_INVALID_ARG;
    }
    info_clear(info);

    let buf = unsafe { slice::from_raw_parts(data, size) };

    match wpd::container::get_info(buf) {
        Ok(scanned) => {
            fill_info(info, &scanned);
            WPD_OK
        }
        Err(e) => status(e),
    }
}
