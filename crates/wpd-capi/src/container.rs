//! C ABI for the RIFF container scanner, as declared by `src/container.h`,
//! plus the one public entry point that is nothing but a scan.
//!
//! The scanner never keeps the caller's memory: `scan_headers` is handed a
//! window and a stream offset, and everything it remembers between calls is an
//! offset into the stream, so the buffer on the C side is free to move, grow
//! or lose its head.
//!
//! `WPDImageInfo` is versioned by `struct_size`, and the caller's copy may be
//! a longer revision than this build knows about. Nothing here writes the
//! struct whole: the fields are assigned one at a time, so the tail padding a
//! future field would occupy is left alone, which is what `WPD_FIELD_END` was
//! guarding in the C.

use std::ffi::c_int;
use std::{mem, ptr, slice};

use wpd::container::{Blend, Coding, Dispose, Info, Raw, Scan, METADATA_NB};
use wpd::error::Error;

use crate::vp8::forward_log;

const WPD_OK: c_int = 0;
const WPD_ERR_INVALID_ARG: c_int = -1;
const WPD_ERR_NOT_WEBP: c_int = -2;
const WPD_ERR_BITSTREAM: c_int = -3;
const WPD_ERR_TRUNCATED: c_int = -4;
const WPD_ERR_NO_MEMORY: c_int = -6;
const WPD_ERR_TOO_LARGE: c_int = -7;

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

/// `FrameEntry` from `src/container.h`.
#[repr(C)]
pub struct CFrameEntry {
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub width: c_int,
    pub height: c_int,
    pub duration: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
    pub complete: c_int,
}

/// `ScanInfo` from `src/container.h`.
#[repr(C)]
pub struct CScanInfo {
    pub end: usize,
    pub width: c_int,
    pub height: c_int,
    pub has_alpha: c_int,
    pub image_has_alpha: c_int,
    pub animation: c_int,
    pub images: c_int,
    pub frame_count: c_int,
    pub loop_count: c_int,
    pub background_argb: u32,
    pub coding: c_int,
    pub truncated: c_int,
    pub metadata: c_int,
    pub meta_offset: [usize; METADATA_NB],
    pub meta_size: [u32; METADATA_NB],
    pub raw_kind: c_int,
    pub raw_image_offset: usize,
    pub raw_image_size: usize,
    pub raw_alpha_offset: usize,
    pub raw_alpha_size: usize,
}

fn status(e: Error) -> c_int {
    match e {
        Error::InvalidData => WPD_ERR_BITSTREAM,
        Error::NoMemory => WPD_ERR_NO_MEMORY,
        Error::TooLarge => WPD_ERR_TOO_LARGE,
        Error::Truncated => WPD_ERR_TRUNCATED,
        Error::NotWebp => WPD_ERR_NOT_WEBP,
    }
}

fn coding(c: Coding) -> c_int {
    match c {
        Coding::Unknown => 0,
        Coding::Lossy => 1,
        Coding::Lossless => 2,
    }
}

/// The three shapes, numbered as the C's `raw_kind` was: zero for a file that
/// has a RIFF wrapper after all.
fn raw_kind(raw: Raw) -> c_int {
    match raw {
        Raw::No => 0,
        Raw::Lossless => 1,
        Raw::Lossy => 2,
        Raw::AlphaAndLossy => 3,
    }
}

/// How much of a caller's `WPDImageInfo` this build may touch: through the
/// last field it knows about, and no further.
fn image_info_v1() -> usize {
    mem::offset_of!(WPDImageInfo, metadata) + mem::size_of::<c_int>()
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

#[no_mangle]
pub extern "C" fn scan_alloc() -> *mut Scan {
    wpd::log::set_sink(forward_log);

    Box::into_raw(Box::new(Scan::new()))
}

/// # Safety
///
/// `hs` must point to a writable pointer to a live [`Scan`], or to null.
#[no_mangle]
pub unsafe extern "C" fn scan_free(hs: *mut *mut Scan) {
    unsafe {
        let p = *hs;

        if !p.is_null() {
            drop(Box::from_raw(p));
            *hs = ptr::null_mut();
        }
    }
}

/// # Safety
///
/// `hs` must point to a live [`Scan`].
#[no_mangle]
pub unsafe extern "C" fn scan_reset(hs: *mut Scan) {
    if let Some(scan) = unsafe { hs.as_mut() } {
        scan.reset();
    }
}

/// # Safety
///
/// `data` must be readable for `size - base` bytes, which is the window of the
/// stream that is currently buffered.
#[no_mangle]
pub unsafe extern "C" fn scan_headers(
    hs: *mut Scan,
    data: *const u8,
    base: usize,
    size: usize,
    partial: c_int,
    collect_frames: c_int,
) -> c_int {
    let Some(scan) = (unsafe { hs.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let buf = if data.is_null() {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(data, size.saturating_sub(base)) }
    };

    match scan.headers(buf, base, partial != 0, collect_frames != 0) {
        Ok(()) => WPD_OK,
        Err(e) => status(e),
    }
}

/// # Safety
///
/// `hs` must point to a live [`Scan`] and `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn scan_info(hs: *const Scan, out: *mut CScanInfo) {
    let (Some(scan), false) = (unsafe { hs.as_ref() }, out.is_null()) else {
        return;
    };
    let info = scan.info();
    let mut c = CScanInfo {
        end: info.end,
        width: info.width,
        height: info.height,
        has_alpha: c_int::from(info.has_alpha),
        image_has_alpha: c_int::from(info.image_has_alpha),
        animation: c_int::from(info.animation),
        images: info.images,
        frame_count: info.frame_count,
        loop_count: info.loop_count,
        background_argb: info.background_argb,
        coding: coding(info.coding),
        truncated: c_int::from(info.truncated),
        metadata: info.metadata,
        meta_offset: [0; METADATA_NB],
        meta_size: [0; METADATA_NB],
        raw_kind: raw_kind(info.raw),
        raw_image_offset: info.raw_image_offset,
        raw_image_size: info.raw_image_size,
        raw_alpha_offset: info.raw_alpha_offset,
        raw_alpha_size: info.raw_alpha_size,
    };

    c.meta_offset.copy_from_slice(&info.meta_offset);
    c.meta_size.copy_from_slice(&info.meta_size);
    unsafe { out.write(c) };
}

/// # Safety
///
/// `hs` must point to a live [`Scan`] and `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn scan_frame(
    hs: *const Scan,
    index: c_int,
    out: *mut CFrameEntry,
) -> c_int {
    let (Some(scan), false) = (unsafe { hs.as_ref() }, out.is_null()) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(frame) = scan.frame(index) else {
        return 0;
    };

    unsafe {
        out.write(CFrameEntry {
            pos_x: frame.pos_x,
            pos_y: frame.pos_y,
            width: frame.width,
            height: frame.height,
            duration: frame.duration,
            dispose: match frame.dispose {
                Dispose::None => 0,
                Dispose::Background => 1,
            },
            blend: match frame.blend {
                Blend::Alpha => 0,
                Blend::None => 1,
            },
            has_alpha: c_int::from(frame.has_alpha),
            complete: c_int::from(frame.complete),
        })
    };
    1
}

/// # Safety
///
/// `info`, when not null, must point to a `WPDImageInfo` of at least its own
/// declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn info_valid(info: *const WPDImageInfo) -> c_int {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return 0;
    };

    c_int::from(info.struct_size >= image_info_v1())
}

/// # Safety
///
/// As [`info_valid`], and the struct must be writable.
#[no_mangle]
pub unsafe extern "C" fn info_clear(info: *mut WPDImageInfo) {
    let Some(info) = (unsafe { info.as_mut() }) else {
        return;
    };

    fill_info(info, &Info::default());
    info.coding = coding(Coding::Unknown);
}

/// # Safety
///
/// `data` must be readable for `size` bytes, and `info` writable as
/// [`info_clear`] requires.
#[no_mangle]
pub unsafe extern "C" fn wpd_get_info(
    data: *const u8,
    size: usize,
    info: *mut WPDImageInfo,
) -> c_int {
    if data.is_null() || unsafe { info_valid(info) } == 0 {
        return WPD_ERR_INVALID_ARG;
    }
    unsafe { info_clear(info) };

    let buf = unsafe { slice::from_raw_parts(data, size) };

    match wpd::container::get_info(buf) {
        Ok(scanned) => {
            fill_info(unsafe { &mut *info }, &scanned);
            WPD_OK
        }
        Err(e) => status(e),
    }
}
