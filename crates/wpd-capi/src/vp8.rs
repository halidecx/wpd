//! C ABI for the lossy frame decoder, as declared by `src/vp8.h`.
//!
//! The chunk stays on this side of the boundary. `vp8_decode_rows` takes no
//! buffer — the C kept one in `VP8Context` and re-pointed it on every
//! streaming append — so the shim remembers the pointer and length it was last
//! given and rebuilds a slice per call. The core never holds a borrow it did
//! not receive as an argument, which is what lets the decoder be safe code.

use std::ffi::{c_char, c_int, c_void, CString};
use std::{ptr, slice};

use wpd::log::Level;
use wpd::vp8::{Decoder, Error, Status};

pub const VP8_NEED_MORE: c_int = 1;

pub(crate) const WPD_ERROR_INVALID_DATA: c_int = -1094995529;
pub(crate) const WPD_ERROR_TOO_LARGE: c_int = -558319938;
pub(crate) const WPD_ENOMEM: c_int = -12;

#[repr(C)]
pub struct WpdFrame {
    pub data: [*mut u8; 3],
    pub allocation: [*mut u8; 3],
    pub linesize: [c_int; 3],
}

#[repr(C)]
pub struct WpdCodecContext {
    pub priv_data: *mut c_void,
    pub width: c_int,
    pub height: c_int,
    pub bypass_filtering: c_int,
}

#[repr(C)]
pub struct WpdPacket {
    pub data: *const u8,
    pub size: c_int,
}

extern "C" {
    fn wpd_log(context: *mut c_void, level: c_int, format: *const c_char, ...);
}

pub(crate) fn forward_log(level: Level, message: &str) {
    let Ok(message) = CString::new(message) else {
        return;
    };
    let level = match level {
        Level::Error => 0,
        Level::Warning => 1,
    };

    unsafe { wpd_log(ptr::null_mut(), level, c"%s".as_ptr(), message.as_ptr()) };
}

pub(crate) fn status(e: Error) -> c_int {
    match e {
        Error::InvalidData => WPD_ERROR_INVALID_DATA,
        Error::NoMemory => WPD_ENOMEM,
        Error::TooLarge => WPD_ERROR_TOO_LARGE,
    }
}

/// The decoder plus the chunk it was last pointed at.
struct Context {
    decoder: Decoder,
    chunk: *const u8,
    chunk_len: usize,
}

/// The chunk as the core wants it: `slice::from_raw_parts` with a null guard,
/// and the same unbounded lifetime, which is the hazard this crate exists to
/// contain.
///
/// # Safety
///
/// The caller must not have freed or moved the buffer since the last
/// `vp8_decode_frame_init` or `vp8_decode_extend`, which is what the C decoder
/// required of it too.
unsafe fn chunk_slice<'a>(chunk: *const u8, len: usize) -> &'a [u8] {
    if chunk.is_null() {
        return &[];
    }
    unsafe { slice::from_raw_parts(chunk, len) }
}

/// # Safety
///
/// `ctx` must point to a live `WpdCodecContext`.
unsafe fn context<'a>(ctx: *mut WpdCodecContext) -> Option<&'a mut Context> {
    unsafe { (*ctx).priv_data.cast::<Context>().as_mut() }
}

/// Fills `frame` in with the decoder's current picture.
fn export(decoder: &mut Decoder, frame: *mut WpdFrame) {
    if frame.is_null() {
        return;
    }
    let mut out = WpdFrame {
        data: [ptr::null_mut(); 3],
        allocation: [ptr::null_mut(); 3],
        linesize: [0; 3],
    };

    for (i, plane) in decoder.picture.planes.iter_mut().enumerate() {
        if plane.data.is_empty() {
            continue;
        }
        let origin = plane.origin;

        out.data[i] = unsafe { plane.data.as_mut_ptr().add(origin) };
        out.linesize[i] = plane.stride as c_int;
    }
    unsafe { frame.write(out) };
}

/// # Safety
///
/// `ctx` must point to a live `WpdCodecContext` with no decoder attached.
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_init(ctx: *mut WpdCodecContext) -> c_int {
    wpd::log::set_sink(forward_log);
    wpd::cpu::init();

    let context = Box::new(Context {
        decoder: Decoder::new(),
        chunk: ptr::null(),
        chunk_len: 0,
    });

    unsafe { (*ctx).priv_data = Box::into_raw(context).cast() };
    0
}

/// # Safety
///
/// `ctx` must point to a live `WpdCodecContext`.
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_free(ctx: *mut WpdCodecContext) -> c_int {
    unsafe {
        let p = (*ctx).priv_data.cast::<Context>();

        if !p.is_null() {
            drop(Box::from_raw(p));
            (*ctx).priv_data = ptr::null_mut();
        }
    }
    0
}

/// # Safety
///
/// `chunk` must be readable for `avail` bytes and stay valid until the next
/// call that replaces it.
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_frame_init(
    ctx: *mut WpdCodecContext,
    chunk: *const u8,
    avail: c_int,
    size: c_int,
) -> c_int {
    let Some(context) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };

    context.chunk = chunk;
    context.chunk_len = avail.max(0) as usize;
    context.decoder.bypass_filtering = unsafe { (*ctx).bypass_filtering } != 0;

    let buf = unsafe { chunk_slice(context.chunk, context.chunk_len) };

    match context
        .decoder
        .frame_init(buf, avail.max(0) as usize, size.max(0) as usize)
    {
        Err(e) => status(e),
        Ok(Status::NeedMore) => VP8_NEED_MORE,
        Ok(Status::Done) => {
            unsafe {
                (*ctx).width = context.decoder.width;
                (*ctx).height = context.decoder.height;
            }
            0
        }
    }
}

/// # Safety
///
/// As [`vp8_decode_frame_init`].
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_extend(
    ctx: *mut WpdCodecContext,
    chunk: *const u8,
    avail: c_int,
) {
    let Some(context) = (unsafe { context(ctx) }) else {
        return;
    };

    context.chunk = chunk;
    context.chunk_len = avail.max(0) as usize;

    let buf = unsafe { chunk_slice(context.chunk, context.chunk_len) };

    context.decoder.extend(buf, avail.max(0) as usize);
}

/// # Safety
///
/// `frame` must point to a writable `WpdFrame`.
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_rows(
    ctx: *mut WpdCodecContext,
    frame: *mut WpdFrame,
) -> c_int {
    let Some(context) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };
    let buf = unsafe { chunk_slice(context.chunk, context.chunk_len) };
    let ret = context.decoder.decode_rows(buf);

    export(&mut context.decoder, frame);

    match ret {
        Err(e) => status(e),
        Ok(Status::NeedMore) => VP8_NEED_MORE,
        Ok(Status::Done) => 0,
    }
}

/// # Safety
///
/// `ctx` must point to a live `WpdCodecContext`.
#[no_mangle]
pub unsafe extern "C" fn vp8_rows_finalized(ctx: *const WpdCodecContext) -> c_int {
    let p = unsafe { (*ctx).priv_data.cast::<Context>() };

    match unsafe { p.as_ref() } {
        Some(context) => context.decoder.rows_finalized(),
        None => 0,
    }
}

/// # Safety
///
/// `packet` must describe a complete frame and `frame` must be writable.
#[no_mangle]
pub unsafe extern "C" fn vp8_decode_frame(
    ctx: *mut WpdCodecContext,
    frame: *mut WpdFrame,
    packet: *mut WpdPacket,
) -> c_int {
    let Some(context) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };
    let (data, size) = unsafe { ((*packet).data, (*packet).size.max(0) as usize) };

    context.chunk = data;
    context.chunk_len = size;
    context.decoder.bypass_filtering = unsafe { (*ctx).bypass_filtering } != 0;

    let buf = unsafe { chunk_slice(context.chunk, context.chunk_len) };

    if let Err(e) = context.decoder.decode_frame(buf) {
        return status(e);
    }
    unsafe {
        (*ctx).width = context.decoder.width;
        (*ctx).height = context.decoder.height;
    }
    export(&mut context.decoder, frame);
    size as c_int
}

/// # Safety
///
/// `frame` must point to a writable `WpdFrame`.
#[no_mangle]
pub unsafe extern "C" fn vp8_current_frame(
    ctx: *const WpdCodecContext,
    frame: *mut WpdFrame,
) {
    let p = unsafe { (*ctx).priv_data.cast::<Context>() };

    if let Some(context) = unsafe { p.as_mut() } {
        export(&mut context.decoder, frame);
    }
}
