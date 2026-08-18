//! The public decoder, as declared by `include/wpd.h`.
//!
//! Every entry point the header declares is here, and each is the same three
//! steps: check what only a raw pointer can get wrong, ask [`wpd::driver`],
//! and turn what comes back into a `WPDStatus` and a versioned struct.
//!
//! [`WPDDecoder`] is that decoder plus the one thing the ABI needs and it does
//! not: the planes a caller supplied. The decoder writes its rows through a
//! sink built over them and never learns what they are, but a `WPDFrame` has
//! to name them, because naming where the pixels went is what the header says
//! it does.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::ops::{Deref, DerefMut};
use std::{alloc, mem, ptr, slice};

use wpd::container::Coding;
use wpd::driver::convert::format_bpp;
use wpd::driver::{Decoder, FORMAT_NONE};
use wpd::error::Error;
use wpd::handout::Handout;
use wpd::image::Format;

use crate::container::{info_clear, WPDImageInfo};
use crate::frame::{
    frame_clear, frame_valid, write_frame, External, WPDFrame, WPDOutputPlane,
};
use crate::options::WPDDecoderOptions;

/// The decoder, with the lifetime of the file it was pointed at.
///
/// `wpd_decoder_open_borrowed` and `wpd_decoder_update` promise the caller's
/// bytes will outlive the decode; the C ABI cannot say so, so
/// [`WPDDecoderRaw`] is what crosses it and the promise is checked nowhere.
/// The safe API in [`wpd::api`] hands out a real `'a` instead.
pub struct WPDDecoder<'a> {
    decoder: Decoder<'a>,
    /// The planes `wpd_decoder_set_output_buffer` named, kept beside the sink
    /// built over them. Only the ABI needs these: a decode reports where its
    /// pixels went, and for a caller's own buffer that is the caller's own
    /// pointers, which the decoder itself never sees.
    planes: [WPDOutputPlane; 4],
}

impl WPDDecoder<'_> {
    pub(crate) fn new() -> Self {
        WPDDecoder {
            decoder: Decoder::new(),
            planes: [WPDOutputPlane::empty(); 4],
        }
    }
}

impl<'a> Deref for WPDDecoder<'a> {
    type Target = Decoder<'a>;

    fn deref(&self) -> &Decoder<'a> {
        &self.decoder
    }
}

impl<'a> DerefMut for WPDDecoder<'a> {
    fn deref_mut(&mut self) -> &mut Decoder<'a> {
        &mut self.decoder
    }
}

fn try_box<T>(value: T) -> Result<Box<T>, Error> {
    let layout = alloc::Layout::new::<T>();

    if layout.size() == 0 {
        return Ok(Box::new(value));
    }
    let raw = unsafe { alloc::alloc(layout) }.cast::<T>();

    if raw.is_null() {
        return Err(Error::NoMemory);
    }
    unsafe {
        raw.write(value);
        Ok(Box::from_raw(raw))
    }
}

pub const WPD_OK: c_int = 0;
pub const WPD_ERR_INVALID_ARG: c_int = -1;
pub const WPD_ERR_NOT_WEBP: c_int = -2;
pub const WPD_ERR_BITSTREAM: c_int = -3;
pub const WPD_ERR_TRUNCATED: c_int = -4;
pub const WPD_ERR_UNSUPPORTED: c_int = -5;
pub const WPD_ERR_NO_MEMORY: c_int = -6;
pub const WPD_ERR_TOO_LARGE: c_int = -7;
pub const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;

/// `WPDFrameInfo` from `include/wpd.h`.
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

/// `WPDOutputBuffer` from `include/wpd.h`.
#[repr(C)]
pub struct WPDOutputBuffer {
    pub struct_size: usize,
    pub plane: [WPDOutputPlane; 4],
}

fn frame_info_v1() -> usize {
    mem::offset_of!(WPDFrameInfo, complete) + mem::size_of::<c_int>()
}

fn output_buffer_v1() -> usize {
    mem::offset_of!(WPDOutputBuffer, plane) + mem::size_of::<[WPDOutputPlane; 4]>()
}

/// What the C ABI passes around, since a `*mut` cannot carry a lifetime.
pub type WPDDecoderRaw = WPDDecoder<'static>;

/// What the core's failures are called at the ABI.
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

/// What a driver call reports at the ABI: a status, and how many rows or
/// pictures it produced.
fn reported(ret: Result<c_int, Error>) -> c_int {
    match ret {
        Ok(n) => n,
        Err(e) => status(e),
    }
}

fn status_string(status: c_int) -> &'static CStr {
    match status {
        WPD_OK => c"success",
        WPD_ERR_INVALID_ARG => c"invalid argument",
        WPD_ERR_NOT_WEBP => c"not a WebP file",
        WPD_ERR_BITSTREAM => c"invalid bitstream",
        WPD_ERR_TRUNCATED => c"truncated file",
        WPD_ERR_UNSUPPORTED => c"unsupported feature",
        WPD_ERR_NO_MEMORY => c"out of memory",
        WPD_ERR_TOO_LARGE => c"image too large",
        WPD_ERR_BUFFER_TOO_SMALL => c"output buffer too small",
        _ => c"unknown error",
    }
}

#[no_mangle]
pub extern "C" fn wpd_status_string(status: c_int) -> *const c_char {
    status_string(status).as_ptr()
}

#[no_mangle]
pub extern "C" fn wpd_decoder_create() -> *mut WPDDecoderRaw {
    wpd::log::set_sink(crate::compat::forward_log);
    wpd::cpu::init();

    try_box(WPDDecoder::new()).map_or(ptr::null_mut(), Box::into_raw)
}

/// # Safety
///
/// `decoder` must come from [`wpd_decoder_create`] and not have been freed.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_free(decoder: *mut WPDDecoderRaw) {
    if !decoder.is_null() {
        drop(unsafe { Box::from_raw(decoder) });
    }
}

/// The versioned C struct: check what only its encoding can get wrong, then
/// hand the rest to [`WPDDecoder::set_core_options`].
fn set_options(
    decoder: &mut WPDDecoder<'_>,
    options: &WPDDecoderOptions,
) -> Result<(), Error> {
    let flag = |v: c_int| v == 0 || v == 1;

    if options.struct_size < WPDDecoderOptions::v1()
        || !flag(options.bypass_filtering)
        || !flag(options.no_fancy_upsampling)
        || !flag(options.use_cropping)
        || !flag(options.use_scaling)
        || !flag(options.flip)
    {
        return Err(decoder.fail("invalid decoder options", Error::InvalidArgument));
    }
    decoder.set_core_options(options.to_core())
}

/// # Safety
///
/// The buffer's planes must be as it declares them.
unsafe fn set_output_buffer(
    decoder: &mut WPDDecoder<'_>,
    buffer: Option<&WPDOutputBuffer>,
) -> Result<(), Error> {
    let Some(buffer) = buffer else {
        decoder.planes = [WPDOutputPlane::empty(); 4];
        decoder.set_sink(None);
        return Ok(());
    };

    if buffer.struct_size < output_buffer_v1()
        || buffer.plane[0].data.is_null()
        || buffer.plane[0].stride == 0
    {
        return Err(decoder.fail("invalid output buffer", Error::InvalidArgument));
    }
    for plane in &buffer.plane {
        if plane.data.is_null() != (plane.stride == 0) {
            return Err(decoder.fail("invalid output buffer", Error::InvalidArgument));
        }
    }
    /* The same planes named twice keeps the rows already converted, which is
    what lets a caller ask for a partial frame repeatedly without redoing the
    ones it has. */
    if !decoder.has_sink() || decoder.planes != buffer.plane {
        let sink = try_box(External(buffer.plane))?;

        decoder.planes = buffer.plane;
        decoder.set_sink(Some(sink));
    }
    Ok(())
}

/// # Safety
///
/// `options`, when not null, must point to a `WPDDecoderOptions` of at least
/// its own declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_options(
    decoder: *mut WPDDecoderRaw,
    options: *const WPDDecoderOptions,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return status(decoder.fail("invalid decoder options", Error::InvalidArgument));
    };

    reported(set_options(decoder, options).map(|()| WPD_OK))
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_animation_mode(
    decoder: *mut WPDDecoderRaw,
    mode: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(decoder.set_animation_mode(mode).map(|()| WPD_OK)),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_output_format(
    decoder: *mut WPDDecoderRaw,
    format: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(decoder.set_output_format(format).map(|()| WPD_OK)),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `buffer`, when not null, must point to a `WPDOutputBuffer` of at least its
/// own declared `struct_size` bytes, whose planes are as they were declared.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_output_buffer(
    decoder: *mut WPDDecoderRaw,
    buffer: *const WPDOutputBuffer,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    reported(unsafe { set_output_buffer(decoder, buffer.as_ref()) }.map(|()| WPD_OK))
}

/// The caller's bytes as a slice, with the one lifetime extension the C ABI
/// forces: `wpd_decoder_open_borrowed` and `wpd_decoder_update` promise the
/// memory outlives the decoder, and nothing on this side can check it.
///
/// # Safety
///
/// `data` must be readable for `size` bytes, and for the two borrowing entry
/// points must stay so until the decoder is reopened or freed.
unsafe fn lent<'a>(data: *const u8, size: usize) -> &'a [u8] {
    if data.is_null() || size == 0 {
        return &[];
    }
    unsafe { slice::from_raw_parts(data, size) }
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    }
    reported(decoder.open(unsafe { lent(data, size) }).map(|()| WPD_OK))
}

/// # Safety
///
/// `data` must be readable for `size` bytes and stay unchanged until the
/// decoder is reopened or freed, which is what the header asks of it.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open_borrowed(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    }
    reported(
        decoder
            .open_borrowed(unsafe { lent(data, size) })
            .map(|()| WPD_OK),
    )
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open_stream(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(decoder.open_stream().map(|()| WPD_OK)),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_append(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    }
    reported(decoder.append(unsafe { lent(data, size) }).map(|()| WPD_OK))
}

/// # Safety
///
/// `data` must be readable for `size` bytes and stay valid until the next
/// update or the decoder is freed.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_update(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    }
    reported(decoder.update(unsafe { lent(data, size) }).map(|()| WPD_OK))
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_end_of_stream(
    decoder: *mut WPDDecoderRaw,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(decoder.end_of_stream().map(|()| WPD_OK)),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `info`, when not null, must point to a `WPDImageInfo` of at least its own
/// declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_get_info(
    decoder: *const WPDDecoderRaw,
    info: *mut WPDImageInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(info) = (unsafe { info.as_mut() }) else {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    };

    reported(get_info(decoder, info).map(|()| WPD_OK))
}

/// Fills the caller's versioned struct in from what the decoder reports.
fn get_info(
    decoder: &mut WPDDecoder<'_>,
    info: &mut WPDImageInfo,
) -> Result<(), Error> {
    if info.struct_size < WPDImageInfo::v1() {
        return Err(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }

    let got = decoder.image_info()?;

    info_clear(info);
    info.width = got.width;
    info.height = got.height;
    info.has_alpha = c_int::from(got.has_alpha);
    info.is_animation = c_int::from(got.is_animation);
    info.frame_count = got.frame_count;
    info.loop_count = got.loop_count;
    info.background_argb = got.background_argb;
    info.coding = match got.coding {
        Coding::Unknown => 0,
        Coding::Lossy => 1,
        Coding::Lossless => 2,
    };
    info.metadata = got.metadata;
    Ok(())
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_rewind(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(decoder.rewind().map(|()| WPD_OK)),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `info`, when not null, must point to a `WPDFrameInfo` of at least its own
/// declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_frame_info(
    decoder: *const WPDDecoderRaw,
    index: c_int,
    info: *mut WPDFrameInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(info) = (unsafe { info.as_mut() }) else {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    };

    reported(frame_info(decoder, index, info).map(|()| WPD_OK))
}

/// Fills the caller's versioned struct in from what the decoder reports.
///
/// The struct is cleared once the state checks have passed and before the
/// frame is looked up, which is the order the C's did it in: asking for a
/// frame that is not there leaves a zeroed struct behind, not the last one.
fn frame_info(
    decoder: &mut WPDDecoder<'_>,
    index: c_int,
    info: &mut WPDFrameInfo,
) -> Result<(), Error> {
    if info.struct_size < frame_info_v1() {
        return Err(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    decoder.headers_ready()?;

    /* Everything past `struct_size`, which is the caller's, survives; the head
    is the size itself. */
    let size = info.struct_size;

    info.pos_x = 0;
    info.pos_y = 0;
    info.width = 0;
    info.height = 0;
    info.duration = 0;
    info.dispose = 0;
    info.blend = 0;
    info.has_alpha = 0;
    info.complete = 0;
    info.struct_size = size;

    let entry = decoder.frame_entry(index)?;

    info.pos_x = entry.pos_x;
    info.pos_y = entry.pos_y;
    info.width = entry.width;
    info.height = entry.height;
    info.duration = entry.duration;
    info.dispose = c_int::from(entry.dispose_to_background);
    info.blend = c_int::from(!entry.blend);
    info.has_alpha = c_int::from(entry.has_alpha);
    info.complete = c_int::from(entry.complete);
    Ok(())
}

/// # Safety
///
/// `data` and `size` must be writable.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_metadata(
    decoder: *const WPDDecoderRaw,
    which: c_int,
    data: *mut *const u8,
    size: *mut usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() || size.is_null() {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    match decoder.metadata(which) {
        Err(e) => status(e),
        Ok(found) => {
            let (at, len) = match found {
                Some(bytes) => (bytes.as_ptr(), bytes.len()),
                None => (ptr::null(), 0),
            };

            unsafe {
                data.write(at);
                size.write(len);
            }
            WPD_OK
        }
    }
}

/// # Safety
///
/// `frame` must point to a `WPDFrame` of at least its own declared
/// `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_next_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => {
            reported(unsafe { next_frame(decoder, frame) }.map(c_int::from))
        }
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// As [`wpd_decoder_next_frame`].
unsafe fn next_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
) -> Result<bool, Error> {
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }

    /* The handout borrows the decoder, so everything the shim needs from it
    besides the pixels is taken first, and a failure carries a message rather
    than setting one -- `set_error` wants the decoder back. */
    let ext = decoder.planes;
    let mut out = Handout::default();

    match decoder.next_picture(&mut out) {
        Ok(got) => {
            if got {
                unsafe { write_frame(&out, &ext, frame) };
            }
            Ok(got)
        }
        Err((message, e)) => Err(decoder.fail(message, e)),
    }
}

/// # Safety
///
/// `frame` must be as [`wpd_decoder_next_frame`] requires, and `rows_valid`,
/// when not null, writable.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_partial_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => reported(
            unsafe { partial_frame(decoder, frame, rows_valid) }.map(|()| WPD_OK),
        ),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// As [`wpd_decoder_partial_frame`].
unsafe fn partial_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> Result<(), Error> {
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }

    let ext = decoder.planes;
    let mut out = Handout::default();
    let mut rows = 0;

    unsafe { frame_clear(frame) };

    let ret = match decoder.partial_picture(&mut out, &mut rows) {
        Ok(had_picture) => {
            if had_picture {
                unsafe { write_frame(&out, &ext, frame) };
            }
            Ok(())
        }
        Err((message, e)) => Err(decoder.fail(message, e)),
    };

    if !rows_valid.is_null() {
        unsafe { rows_valid.write(rows) };
    }
    ret
}

/// # Safety
///
/// `decoder` must be live or null.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_status(decoder: *const WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_ref() } {
        Some(decoder) => decoder.status().map_or(WPD_OK, status),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// As [`wpd_decoder_status`]. The string belongs to the decoder and stays
/// valid until its next failure.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_error(
    decoder: *const WPDDecoderRaw,
) -> *const c_char {
    match unsafe { decoder.as_ref() } {
        Some(decoder) if decoder.error_raw()[0] != 0 => {
            decoder.error_raw().as_ptr().cast()
        }
        _ => c"unknown decoder error".as_ptr(),
    }
}

/// The planes `wpd_decode` copies out, which is what the frame's format says
/// it has.
fn frame_planes(format: c_int) -> usize {
    Format::from_raw(format).map_or(1, Format::nb_components)
}

/// The memory behind a frame `wpd_decode` handed out, released by
/// `wpd_frame_free`.
struct WPDFrameOwner {
    plane: [Vec<u8>; 4],
}

/// Runs a one-shot decode of `data` into `frame`, leaving the decoder for the
/// caller to take what it needs out of.
///
/// # Safety
///
/// As [`wpd_decode`].
unsafe fn decode_once(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> (*mut WPDDecoderRaw, c_int) {
    let decoder = wpd_decoder_create();

    if decoder.is_null() {
        return (ptr::null_mut(), WPD_ERR_NO_MEMORY);
    }
    let mut status = if options.is_null() {
        WPD_OK
    } else {
        unsafe { wpd_decoder_set_options(decoder, options) }
    };

    if status == WPD_OK {
        status = unsafe { wpd_decoder_set_output_format(decoder, format) };
    }
    if status == WPD_OK && !buffer.is_null() {
        status = unsafe { wpd_decoder_set_output_buffer(decoder, buffer) };
    }
    if status == WPD_OK {
        status = unsafe { wpd_decoder_open_borrowed(decoder, data, size) };
    }
    let ret = if status == WPD_OK {
        unsafe { wpd_decoder_next_frame(decoder, frame) }
    } else {
        status
    };

    (decoder, ret)
}

/// # Safety
///
/// `data` must be readable for `size` bytes, and `frame` must point to a
/// `WPDFrame` of at least its own declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decode_into(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> c_int {
    if data.is_null() || buffer.is_null() || !unsafe { frame_valid(frame) } {
        return WPD_ERR_INVALID_ARG;
    }
    if !unsafe { (*frame).private_data }.is_null() {
        unsafe { wpd_frame_free(frame) };
    }
    let (decoder, ret) =
        unsafe { decode_once(data, size, format, options, buffer, frame) };

    if decoder.is_null() {
        return ret;
    }
    unsafe { wpd_decoder_free(decoder) };

    match ret {
        0 => WPD_ERR_BITSTREAM,
        ret if ret < 0 => ret,
        _ => WPD_OK,
    }
}

/// # Safety
///
/// As [`wpd_decode_into`].
#[no_mangle]
pub unsafe extern "C" fn wpd_decode(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    frame: *mut WPDFrame,
) -> c_int {
    if data.is_null() || !unsafe { frame_valid(frame) } {
        return WPD_ERR_INVALID_ARG;
    }
    if !unsafe { (*frame).private_data }.is_null() {
        unsafe { wpd_frame_free(frame) };
    }
    let mut decoded = WPDFrame {
        struct_size: mem::size_of::<WPDFrame>(),
        data: [ptr::null(); 4],
        stride: [0; 4],
        width: 0,
        height: 0,
        format: FORMAT_NONE,
        duration: 0,
        timestamp: 0,
        private_data: ptr::null_mut(),
        pos_x: 0,
        pos_y: 0,
        dispose: 0,
        blend: 0,
        has_alpha: 0,
    };
    let (decoder, ret) =
        unsafe { decode_once(data, size, format, options, ptr::null(), &mut decoded) };

    if decoder.is_null() {
        return ret;
    }
    if ret <= 0 {
        unsafe { wpd_decoder_free(decoder) };
        return if ret < 0 { ret } else { WPD_ERR_BITSTREAM };
    }

    let owner = match try_box(WPDFrameOwner {
        plane: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    }) {
        Ok(owner) => owner,
        Err(e) => {
            unsafe { wpd_decoder_free(decoder) };
            return status(e);
        }
    };
    let planes = frame_planes(decoded.format);

    unsafe { frame_clear(frame) };
    frame_copy(frame, &decoded);

    let out = unsafe { &mut *frame };

    out.private_data = Box::into_raw(owner).cast::<c_void>();

    let owner = unsafe { &mut *out.private_data.cast::<WPDFrameOwner>() };
    let mut status = WPD_OK;

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = if planes == 1 {
            decoded.width as usize * format_bpp(decoded.format) as usize
        } else {
            wpd::image::ceil_rshift(decoded.width, shift) as usize
        };
        let h = wpd::image::ceil_rshift(decoded.height, shift) as usize;
        let Some(bytes) = w.checked_mul(h) else {
            status = WPD_ERR_TOO_LARGE;
            break;
        };

        if owner.plane[p].try_reserve_exact(bytes).is_err() {
            status = WPD_ERR_NO_MEMORY;
            break;
        }
        for y in 0..h {
            let src = unsafe { decoded.data[p].offset(y as isize * decoded.stride[p]) };

            owner.plane[p].extend_from_slice(unsafe { slice::from_raw_parts(src, w) });
        }
        debug_assert_eq!(owner.plane[p].len(), bytes);
        out.data[p] = owner.plane[p].as_ptr();
        out.stride[p] = w as isize;
    }
    unsafe { wpd_decoder_free(decoder) };

    if status != WPD_OK {
        unsafe { wpd_frame_free(frame) };
        return status;
    }
    WPD_OK
}

/// Copies past `struct_size` rather than assigning: the caller's frame may be
/// a newer, longer revision of the struct, and its own size has to survive.
fn frame_copy(dst: *mut WPDFrame, src: &WPDFrame) {
    let head = mem::size_of::<usize>();
    let extent = unsafe { crate::frame::frame_extent(dst) }
        .min(unsafe { crate::frame::frame_extent(src) });

    unsafe {
        ptr::copy_nonoverlapping(
            (src as *const WPDFrame).cast::<u8>().add(head),
            dst.cast::<u8>().add(head),
            extent - head,
        );
    }
}

/// # Safety
///
/// `frame`, when not null, must be one [`wpd_decode`] filled in, or a frame
/// that owns nothing.
#[no_mangle]
pub unsafe extern "C" fn wpd_frame_free(frame: *mut WPDFrame) {
    if !unsafe { frame_valid(frame) } {
        return;
    }
    let owner = unsafe { (*frame).private_data };

    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<WPDFrameOwner>()) });
    }
    unsafe { frame_clear(frame) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ABI's table of descriptions and the core's are written out
    /// separately, because one is NUL-terminated and the other is not. This is
    /// what says they still describe the same failures.
    #[test]
    fn every_core_failure_crosses_the_abi_under_its_own_name() {
        for e in [
            wpd::error::Error::InvalidArgument,
            wpd::error::Error::InvalidData,
            wpd::error::Error::NoMemory,
            wpd::error::Error::TooLarge,
            wpd::error::Error::Truncated,
            wpd::error::Error::NotWebp,
            wpd::error::Error::Unsupported,
            wpd::error::Error::BufferTooSmall,
        ] {
            let text = status_string(status(e)).to_str().unwrap();

            assert_eq!(text, e.message(), "{e:?}");
        }
    }
}
