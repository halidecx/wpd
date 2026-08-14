//! C ABI for plane allocation, as declared by `src/image.h`.
//!
//! `WebPImage` stays a plain C struct because the rest of the decoder makes
//! crop and flip views of it by adding to `data[p]` and negating `linesize[p]`,
//! which no owning Rust type can express. What moves here is the ownership:
//! every byte an image holds is allocated and released on this side, so the
//! `(alloc, alloc_size)` pair is the one description of the block and the size
//! arithmetic that a damaged header drives is checked.

use std::alloc::{self, Layout};
use std::ffi::c_int;
use std::mem::size_of;
use std::{ptr, slice};

use wpd::error::Error;
use wpd::image::{plane_size, Format};

use crate::vp8::{WPD_ENOMEM, WPD_ERROR_TOO_LARGE};

/// What `malloc` guarantees on the platforms wpd builds for, which is what the
/// C allocation this replaces was aligned to.
const ALIGN: usize = 16;

/// `WebPImage` from `src/image.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WebPImage {
    pub chroma_full: c_int,
    pub premultiplied: c_int,
    pub data: [*mut u8; 4],
    pub alloc: [*mut u8; 4],
    pub alloc_size: [usize; 4],
    pub linesize: [c_int; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: c_int,
}

/// `RescaleScratch` from `src/image.h`.
#[repr(C)]
pub struct RescaleScratch {
    pub work: *mut u32,
    pub work_size: usize,
    pub row: *mut u8,
    pub row_size: usize,
}

fn layout(size: usize) -> Layout {
    Layout::from_size_align(size, ALIGN)
        .expect("a plane size that already fits in a usize")
}

fn status(e: Error) -> c_int {
    match e {
        Error::NoMemory => WPD_ENOMEM,
        _ => WPD_ERROR_TOO_LARGE,
    }
}

/// Releases plane `p`, leaving the rest of the image as it was.
///
/// # Safety
///
/// `img` must point to a live `WebPImage` whose `alloc[p]` came from here.
unsafe fn drop_plane(img: &mut WebPImage, p: usize) {
    if !img.alloc[p].is_null() {
        unsafe { alloc::dealloc(img.alloc[p], layout(img.alloc_size[p])) };
    }
    img.alloc[p] = ptr::null_mut();
    img.alloc_size[p] = 0;
    img.data[p] = ptr::null_mut();
    img.linesize[p] = 0;
}

/// Zeroed room for `size` bytes in plane `p`, reusing the block that is there
/// when it is already big enough.
///
/// Growing releases the old block first, which clears the plane's geometry:
/// the caller sets `data` and `linesize` after this returns, never before.
///
/// Reuse clears only the `size` bytes asked for, not the whole block: a plane
/// that shrank between frames keeps its allocation, and the tail past the new
/// image is never read.
unsafe fn alloc_plane(img: &mut WebPImage, p: usize, size: usize) -> *mut u8 {
    if !img.alloc[p].is_null() && img.alloc_size[p] >= size {
        unsafe { ptr::write_bytes(img.alloc[p], 0, size) };
        return img.alloc[p];
    }
    unsafe { drop_plane(img, p) };

    let fresh = unsafe { alloc::alloc_zeroed(layout(size)) };

    if !fresh.is_null() {
        img.alloc[p] = fresh;
        img.alloc_size[p] = size;
    }
    fresh
}

/// # Safety
///
/// `img` must point to a live `WebPImage`.
#[no_mangle]
pub unsafe extern "C" fn image_free(img: *mut WebPImage) {
    let Some(img) = (unsafe { img.as_mut() }) else {
        return;
    };

    for p in 0..4 {
        unsafe { drop_plane(img, p) };
    }
    img.chroma_full = 0;
    img.premultiplied = 0;
    img.width = 0;
    img.height = 0;
    img.format = 0;
}

/// # Safety
///
/// As [`image_free`], and `p` must be below four.
#[no_mangle]
pub unsafe extern "C" fn image_drop_plane(img: *mut WebPImage, p: c_int) {
    let Some(img) = (unsafe { img.as_mut() }) else {
        return;
    };

    if let Ok(p) = usize::try_from(p) {
        if p < 4 {
            unsafe { drop_plane(img, p) };
        }
    }
}

/// # Safety
///
/// As [`image_free`].
#[no_mangle]
pub unsafe extern "C" fn image_alloc_packed(
    img: *mut WebPImage,
    w: c_int,
    h: c_int,
    bpp: c_int,
    format: c_int,
) -> c_int {
    let Some(img) = (unsafe { img.as_mut() }) else {
        return WPD_ERROR_TOO_LARGE;
    };
    let Ok(bpp) = usize::try_from(bpp) else {
        return WPD_ERROR_TOO_LARGE;
    };
    let size = match plane_size(w, h, bpp) {
        Ok(size) => size,
        Err(e) => return status(e),
    };

    for p in 1..4 {
        unsafe { drop_plane(img, p) };
    }
    img.data[0] = unsafe { alloc_plane(img, 0, size) };
    if img.data[0].is_null() {
        return WPD_ENOMEM;
    }
    img.linesize[0] = (w as usize * bpp) as c_int;
    img.width = w;
    img.height = h;
    img.format = format;
    0
}

/// # Safety
///
/// As [`image_free`].
#[no_mangle]
pub unsafe extern "C" fn image_alloc_argb(
    img: *mut WebPImage,
    w: c_int,
    h: c_int,
) -> c_int {
    unsafe { image_alloc_packed(img, w, h, 4, Format::Argb as c_int) }
}

/// Four full-resolution planes, which is what the rescaler fills when it
/// brings chroma up to the output size.
///
/// # Safety
///
/// As [`image_free`].
#[no_mangle]
pub unsafe extern "C" fn image_alloc_yuv444(
    img: *mut WebPImage,
    w: c_int,
    h: c_int,
) -> c_int {
    unsafe { alloc_planar(img, w, h, false) }
}

/// # Safety
///
/// As [`image_free`].
#[no_mangle]
pub unsafe extern "C" fn image_alloc_yuva(
    img: *mut WebPImage,
    w: c_int,
    h: c_int,
) -> c_int {
    unsafe { alloc_planar(img, w, h, true) }
}

unsafe fn alloc_planar(
    img: *mut WebPImage,
    w: c_int,
    h: c_int,
    subsample: bool,
) -> c_int {
    let Some(img) = (unsafe { img.as_mut() }) else {
        return WPD_ERROR_TOO_LARGE;
    };

    if w <= 0 || h <= 0 {
        return WPD_ERROR_TOO_LARGE;
    }
    for p in 0..4 {
        let chroma = subsample && (p == 1 || p == 2);
        let pw = if chroma { (w + 1) / 2 } else { w };
        let ph = if chroma { (h + 1) / 2 } else { h };
        let size = match plane_size(pw, ph, 1) {
            Ok(size) => size,
            Err(e) => {
                unsafe { image_free(img) };
                return status(e);
            }
        };

        img.data[p] = unsafe { alloc_plane(img, p, size) };
        if img.data[p].is_null() {
            unsafe { image_free(img) };
            return WPD_ENOMEM;
        }
        img.linesize[p] = pw;
    }
    img.width = w;
    img.height = h;
    img.format = Format::Yuva420p as c_int;
    0
}

/// # Safety
///
/// `scratch` must point to a live `RescaleScratch` allocated from here.
#[no_mangle]
pub unsafe extern "C" fn image_scratch_free(scratch: *mut RescaleScratch) {
    let Some(scratch) = (unsafe { scratch.as_mut() }) else {
        return;
    };

    if !scratch.work.is_null() {
        let bytes = scratch.work_size * size_of::<u32>();

        unsafe { alloc::dealloc(scratch.work.cast(), layout(bytes)) };
    }
    if !scratch.row.is_null() {
        unsafe { alloc::dealloc(scratch.row, layout(scratch.row_size)) };
    }
    scratch.work = ptr::null_mut();
    scratch.work_size = 0;
    scratch.row = ptr::null_mut();
    scratch.row_size = 0;
}

/// Grows the scratch to fit, discarding what was there: nothing in it lives
/// across a call, so there is no reason to copy it forward.
///
/// # Safety
///
/// As [`image_scratch_free`].
#[no_mangle]
pub unsafe extern "C" fn image_scratch_grow(
    scratch: *mut RescaleScratch,
    dst_width: c_int,
    src_width: c_int,
    channels: c_int,
) -> c_int {
    let Some(scratch) = (unsafe { scratch.as_mut() }) else {
        return WPD_ENOMEM;
    };
    let (Ok(dst_width), Ok(src_width), Ok(channels)) = (
        usize::try_from(dst_width),
        usize::try_from(src_width),
        usize::try_from(channels),
    ) else {
        return WPD_ERROR_TOO_LARGE;
    };
    let Some(need) = dst_width
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(2))
    else {
        return WPD_ERROR_TOO_LARGE;
    };
    let Some(row) = src_width.checked_mul(channels) else {
        return WPD_ERROR_TOO_LARGE;
    };

    if scratch.work_size < need {
        let Some(bytes) = need.checked_mul(size_of::<u32>()) else {
            return WPD_ERROR_TOO_LARGE;
        };
        let grown = unsafe { alloc::alloc(layout(bytes)) };

        if grown.is_null() {
            return WPD_ENOMEM;
        }
        if !scratch.work.is_null() {
            let had = scratch.work_size * size_of::<u32>();

            unsafe { alloc::dealloc(scratch.work.cast(), layout(had)) };
        }
        scratch.work = grown.cast();
        scratch.work_size = need;
    }
    if scratch.row_size < row {
        let grown = unsafe { alloc::alloc(layout(row)) };

        if grown.is_null() {
            return WPD_ENOMEM;
        }
        if !scratch.row.is_null() {
            unsafe { alloc::dealloc(scratch.row, layout(scratch.row_size)) };
        }
        scratch.row = grown;
        scratch.row_size = row;
    }
    0
}

impl WebPImage {
    pub(crate) fn format(&self) -> Option<Format> {
        Format::from_raw(self.format)
    }

    /// One row of plane `p`, `len` bytes of it.
    ///
    /// # Safety
    ///
    /// The plane must hold `len` bytes at row `y`, which the geometry the
    /// allocator recorded guarantees for any `y` below the plane's height.
    pub(crate) unsafe fn row(&self, p: usize, y: i32, len: usize) -> &[u8] {
        let at = self.data[p].wrapping_offset(y as isize * self.linesize[p] as isize);

        unsafe { slice::from_raw_parts(at, len) }
    }

    /// # Safety
    ///
    /// As [`WebPImage::row`].
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn row_mut(&self, p: usize, y: i32, len: usize) -> &mut [u8] {
        let at = self.data[p].wrapping_offset(y as isize * self.linesize[p] as isize);

        unsafe { slice::from_raw_parts_mut(at, len) }
    }
}
