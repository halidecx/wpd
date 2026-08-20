use std::ffi::{c_char, c_int, c_void, CStr};
use std::ops::{Deref, DerefMut};
use std::{alloc, mem, ptr, slice};

use wpd::container::Coding;
use wpd::driver::convert::{format_plane_dims, format_planes};
use wpd::driver::{Decoder, FORMAT_NONE};
use wpd::error::Error;
use wpd::handout::Handout;

use crate::container::{info_clear, WPDImageInfo};
use crate::frame::{
    frame_clear, frame_valid, write_frame, External, WPDFrame, WPDOutputPlane,
};
use crate::options::WPDDecoderOptions;

pub struct WPDDecoder<'a> {
    decoder: Decoder<'a>,
    poisoned: bool,
    planes: [WPDOutputPlane; 4],
}

impl WPDDecoder<'_> {
    pub(crate) fn new() -> Self {
        WPDDecoder {
            decoder: Decoder::new(),
            poisoned: false,
            planes: [WPDOutputPlane::empty(); 4],
        }
    }

    fn guarded<T>(&mut self, fallback: T, body: impl FnOnce(&mut Self) -> T) -> T {
        if self.poisoned {
            wpd::log::error("decoder used after an internal error");
            return fallback;
        }
        self.poisoned = true;

        match crate::guard(None, || Some(body(self))) {
            Some(value) => {
                self.poisoned = false;
                value
            }
            None => fallback,
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
pub const WPD_ERR_INTERNAL: c_int = -9;

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

pub type WPDDecoderRaw = WPDDecoder<'static>;

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
        WPD_ERR_INTERNAL => c"internal error",
        _ => c"unknown error",
    }
}

#[no_mangle]
pub extern "C" fn wpd_status_string(status: c_int) -> *const c_char {
    status_string(status).as_ptr()
}

#[no_mangle]
pub extern "C" fn wpd_decoder_create() -> *mut WPDDecoderRaw {
    crate::guard(ptr::null_mut(), || {
        wpd::log::set_sink(crate::compat::forward_log);
        wpd::cpu::init();

        try_box(WPDDecoder::new()).map_or(ptr::null_mut(), Box::into_raw)
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_free(decoder: *mut WPDDecoderRaw) {
    if !decoder.is_null() {
        crate::guard((), || drop(unsafe { Box::from_raw(decoder) }));
    }
}

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
    if !decoder.has_sink() || decoder.planes != buffer.plane {
        let sink = try_box(External(buffer.plane))?;

        decoder.planes = buffer.plane;
        decoder.set_sink(Some(sink));
    }
    Ok(())
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_set_options(
    decoder: *mut WPDDecoderRaw,
    options: *const WPDDecoderOptions,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        let Some(options) = (unsafe { options.as_ref() }) else {
            return status(
                decoder.fail("invalid decoder options", Error::InvalidArgument),
            );
        };

        reported(set_options(decoder, options).map(|()| WPD_OK))
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_set_animation_mode(
    decoder: *mut WPDDecoderRaw,
    mode: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(decoder.set_animation_mode(mode).map(|()| WPD_OK))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_set_output_format(
    decoder: *mut WPDDecoderRaw,
    format: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(decoder.set_output_format(format).map(|()| WPD_OK))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_set_output_buffer(
    decoder: *mut WPDDecoderRaw,
    buffer: *const WPDOutputBuffer,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        reported(
            unsafe { set_output_buffer(decoder, buffer.as_ref()) }.map(|()| WPD_OK),
        )
    })
}

unsafe fn lent<'a>(data: *const u8, size: usize) -> &'a [u8] {
    if data.is_null() || size == 0 {
        return &[];
    }
    unsafe { slice::from_raw_parts(data, size) }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_open(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        if data.is_null() {
            return status(decoder.fail("invalid input data", Error::InvalidArgument));
        }
        reported(decoder.open(unsafe { lent(data, size) }).map(|()| WPD_OK))
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_open_borrowed(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        if data.is_null() {
            return status(decoder.fail("invalid input data", Error::InvalidArgument));
        }
        reported(
            decoder
                .open_borrowed(unsafe { lent(data, size) })
                .map(|()| WPD_OK),
        )
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_open_stream(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(decoder.open_stream().map(|()| WPD_OK))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_append(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        if data.is_null() {
            return status(decoder.fail("invalid input data", Error::InvalidArgument));
        }
        reported(decoder.append(unsafe { lent(data, size) }).map(|()| WPD_OK))
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_update(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        if data.is_null() {
            return status(decoder.fail("invalid input data", Error::InvalidArgument));
        }
        reported(decoder.update(unsafe { lent(data, size) }).map(|()| WPD_OK))
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_end_of_stream(
    decoder: *mut WPDDecoderRaw,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(decoder.end_of_stream().map(|()| WPD_OK))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_get_info(
    decoder: *const WPDDecoderRaw,
    info: *mut WPDImageInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        let Some(info) = (unsafe { info.as_mut() }) else {
            return status(
                decoder.fail("invalid decoder state", Error::InvalidArgument),
            );
        };

        reported(get_info(decoder, info).map(|()| WPD_OK))
    })
}

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_rewind(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(decoder.rewind().map(|()| WPD_OK))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_frame_info(
    decoder: *const WPDDecoderRaw,
    index: c_int,
    info: *mut WPDFrameInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        let Some(info) = (unsafe { info.as_mut() }) else {
            return status(
                decoder.fail("invalid decoder state", Error::InvalidArgument),
            );
        };

        reported(frame_info(decoder, index, info).map(|()| WPD_OK))
    })
}

fn frame_info(
    decoder: &mut WPDDecoder<'_>,
    index: c_int,
    info: &mut WPDFrameInfo,
) -> Result<(), Error> {
    if info.struct_size < frame_info_v1() {
        return Err(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    decoder.headers_ready()?;

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_metadata(
    decoder: *const WPDDecoderRaw,
    which: c_int,
    data: *mut *const u8,
    size: *mut usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
        if data.is_null() || size.is_null() {
            return status(
                decoder.fail("invalid decoder state", Error::InvalidArgument),
            );
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
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_next_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(unsafe { next_frame(decoder, frame) }.map(c_int::from))
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

unsafe fn next_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
) -> Result<bool, Error> {
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_partial_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.guarded(WPD_ERR_INTERNAL, |decoder| {
            reported(
                unsafe { partial_frame(decoder, frame, rows_valid) }.map(|()| WPD_OK),
            )
        }),
        None => WPD_ERR_INVALID_ARG,
    }
}

unsafe fn partial_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> Result<(), Error> {
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }
    if let Err((message, e)) = decoder.require_open() {
        return Err(decoder.fail(message, e));
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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decoder_status(decoder: *const WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_ref() } {
        Some(decoder) => decoder.status().map_or(WPD_OK, status),
        None => WPD_ERR_INVALID_ARG,
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
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

struct WPDFrameOwner {
    plane: [Vec<u8>; 4],
}

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decode_into(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> c_int {
    crate::guard(WPD_ERR_INTERNAL, || {
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
    })
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_decode(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    frame: *mut WPDFrame,
) -> c_int {
    crate::guard(WPD_ERR_INTERNAL, || {
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
        let (decoder, ret) = unsafe {
            decode_once(data, size, format, options, ptr::null(), &mut decoded)
        };

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
        let planes = format_planes(decoded.format);

        unsafe { frame_clear(frame) };
        frame_copy(frame, &decoded);

        let out = unsafe { &mut *frame };

        out.private_data = Box::into_raw(owner).cast::<c_void>();

        let owner = unsafe { &mut *out.private_data.cast::<WPDFrameOwner>() };
        let mut status = WPD_OK;

        for p in 0..planes {
            let (w, h) =
                format_plane_dims(decoded.format, p, decoded.width, decoded.height);
            let Some(bytes) = w.checked_mul(h) else {
                status = WPD_ERR_TOO_LARGE;
                break;
            };

            if owner.plane[p].try_reserve_exact(bytes).is_err() {
                status = WPD_ERR_NO_MEMORY;
                break;
            }
            for y in 0..h {
                let src =
                    unsafe { decoded.data[p].offset(y as isize * decoded.stride[p]) };

                owner.plane[p]
                    .extend_from_slice(unsafe { slice::from_raw_parts(src, w) });
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
    })
}

fn frame_copy(dst: *mut WPDFrame, src: &WPDFrame) {
    let head = crate::frame::frame_head();
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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_frame_free(frame: *mut WPDFrame) {
    if !unsafe { frame_valid(frame) } {
        return;
    }
    let owner = unsafe { (*frame).private_data };

    if !owner.is_null() {
        crate::guard((), || {
            drop(unsafe { Box::from_raw(owner.cast::<WPDFrameOwner>()) })
        });
    }
    unsafe { frame_clear(frame) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(panic = "abort"))]
    fn a_panicked_decoder_is_poisoned_and_stays_that_way() {
        let mut decoder = WPDDecoder::new();

        assert_eq!(decoder.guarded(WPD_ERR_INTERNAL, |_| WPD_OK), WPD_OK);
        assert!(!decoder.poisoned);

        assert_eq!(
            decoder.guarded(WPD_ERR_INTERNAL, |_| panic!("mid-decode")),
            WPD_ERR_INTERNAL
        );
        assert!(decoder.poisoned);

        assert_eq!(
            decoder.guarded(WPD_ERR_INTERNAL, |_| WPD_OK),
            WPD_ERR_INTERNAL
        );
    }

    #[test]
    fn a_rejected_partial_frame_leaves_the_caller_s_frame_alone() {
        let decoder = wpd_decoder_create();
        let mut frame = WPDFrame {
            struct_size: mem::size_of::<WPDFrame>(),
            data: [ptr::null(); 4],
            stride: [0; 4],
            width: 7,
            height: 9,
            format: 3,
            duration: 11,
            timestamp: 13,
            private_data: ptr::null_mut(),
            pos_x: 0,
            pos_y: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
        };
        let mut rows = 5;

        assert!(!decoder.is_null());

        let status =
            unsafe { wpd_decoder_partial_frame(decoder, &mut frame, &mut rows) };

        assert_eq!(status, WPD_ERR_INVALID_ARG);
        assert_eq!((frame.width, frame.height, frame.duration), (7, 9, 11));
        assert_eq!(rows, 5);
        unsafe { wpd_decoder_free(decoder) };
    }

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
