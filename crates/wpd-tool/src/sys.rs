//! Bindings for `include/wpd.h`.
//!
//! Hand-written rather than generated, so that the tool exercises the C ABI
//! the header promises exactly as an outside consumer would. The layouts here
//! are checked against the header at build time by
//! `tests/api.c`, which links the same library.

use std::ffi::{c_char, c_int, c_uint, c_void};

pub enum WPDDecoder {}

pub type WPDStatus = c_int;
pub const WPD_OK: WPDStatus = 0;

pub type WPDPixelFormat = c_int;
pub const WPD_PIX_FMT_NONE: WPDPixelFormat = -1;
pub const WPD_PIX_FMT_YUV420P: WPDPixelFormat = 0;
pub const WPD_PIX_FMT_YUVA420P: WPDPixelFormat = 1;
pub const WPD_PIX_FMT_ARGB: WPDPixelFormat = 2;
pub const WPD_PIX_FMT_RGBA: WPDPixelFormat = 3;
pub const WPD_PIX_FMT_BGRA: WPDPixelFormat = 4;
pub const WPD_PIX_FMT_RGB: WPDPixelFormat = 5;
pub const WPD_PIX_FMT_BGR: WPDPixelFormat = 6;
pub const WPD_PIX_FMT_ARGB_PRE: WPDPixelFormat = 7;
pub const WPD_PIX_FMT_RGBA_PRE: WPDPixelFormat = 8;
pub const WPD_PIX_FMT_BGRA_PRE: WPDPixelFormat = 9;
pub const WPD_PIX_FMT_RGB565: WPDPixelFormat = 10;
pub const WPD_PIX_FMT_RGBA4444: WPDPixelFormat = 11;
pub const WPD_PIX_FMT_RGBA4444_PRE: WPDPixelFormat = 12;
pub const WPD_PIX_FMT_BGR565: WPDPixelFormat = 13;
pub const WPD_PIX_FMT_BGRA4444: WPDPixelFormat = 14;
pub const WPD_PIX_FMT_BGRA4444_PRE: WPDPixelFormat = 15;

pub const WPD_CODING_LOSSLESS: c_int = 2;

pub const WPD_METADATA_ICCP: c_int = 1;
pub const WPD_METADATA_EXIF: c_int = 2;
pub const WPD_METADATA_XMP: c_int = 4;

pub const WPD_ANIM_SUBFRAME: c_int = 1;

#[repr(C)]
pub struct WPDFrame {
    pub struct_size: usize,
    pub data: [*const u8; 4],
    pub stride: [isize; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: WPDPixelFormat,
    pub duration: c_int,
    pub timestamp: i64,
    pub private_data: *mut c_void,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
}

impl Default for WPDFrame {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>(),
            data: [std::ptr::null(); 4],
            stride: [0; 4],
            width: 0,
            height: 0,
            format: WPD_PIX_FMT_NONE,
            duration: 0,
            timestamp: 0,
            private_data: std::ptr::null_mut(),
            pos_x: 0,
            pos_y: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
        }
    }
}

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

impl Default for WPDImageInfo {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>(),
            width: 0,
            height: 0,
            has_alpha: 0,
            is_animation: 0,
            frame_count: 0,
            loop_count: 0,
            background_argb: 0,
            coding: 0,
            metadata: 0,
        }
    }
}

#[repr(C)]
pub struct WPDFrameInfo {
    pub struct_size: usize,
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

impl Default for WPDFrameInfo {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>(),
            pos_x: 0,
            pos_y: 0,
            width: 0,
            height: 0,
            duration: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
            complete: 0,
        }
    }
}

extern "C" {
    pub fn wpd_version_string() -> *const c_char;
    pub fn wpd_get_info(
        data: *const u8,
        size: usize,
        info: *mut WPDImageInfo,
    ) -> WPDStatus;
    pub fn wpd_decoder_create() -> *mut WPDDecoder;
    pub fn wpd_decoder_free(decoder: *mut WPDDecoder);
    pub fn wpd_decoder_set_output_format(
        decoder: *mut WPDDecoder,
        format: WPDPixelFormat,
    ) -> WPDStatus;
    pub fn wpd_decoder_set_animation_mode(
        decoder: *mut WPDDecoder,
        mode: c_int,
    ) -> WPDStatus;
    pub fn wpd_decoder_open(
        decoder: *mut WPDDecoder,
        data: *const u8,
        size: usize,
    ) -> WPDStatus;
    pub fn wpd_decoder_open_stream(decoder: *mut WPDDecoder) -> WPDStatus;
    pub fn wpd_decoder_append(
        decoder: *mut WPDDecoder,
        data: *const u8,
        size: usize,
    ) -> WPDStatus;
    pub fn wpd_decoder_end_of_stream(decoder: *mut WPDDecoder) -> WPDStatus;
    pub fn wpd_decoder_get_info(
        decoder: *const WPDDecoder,
        info: *mut WPDImageInfo,
    ) -> WPDStatus;
    pub fn wpd_decoder_frame_info(
        decoder: *const WPDDecoder,
        index: c_int,
        info: *mut WPDFrameInfo,
    ) -> WPDStatus;
    pub fn wpd_decoder_rewind(decoder: *mut WPDDecoder) -> WPDStatus;
    pub fn wpd_decoder_metadata(
        decoder: *const WPDDecoder,
        which: c_int,
        data: *mut *const u8,
        size: *mut usize,
    ) -> WPDStatus;
    pub fn wpd_decoder_next_frame(
        decoder: *mut WPDDecoder,
        frame: *mut WPDFrame,
    ) -> c_int;
    pub fn wpd_decoder_partial_frame(
        decoder: *mut WPDDecoder,
        frame: *mut WPDFrame,
        rows: *mut c_int,
    ) -> WPDStatus;
    pub fn wpd_decoder_error(decoder: *const WPDDecoder) -> *const c_char;
    pub fn wpd_set_cpu_flags_mask(mask: c_uint);
}

/// Borrows a C string the library owns.
///
/// # Safety
///
/// `s` must be a live, NUL-terminated string.
pub unsafe fn cstr(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_string_lossy()
        .into_owned()
}
