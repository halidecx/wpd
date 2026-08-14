//! C ABI for the lossless frame decoder, as declared by `src/vp8l.h`.
//!
//! Two pieces of caller-owned memory cross here and neither one enters the
//! core. The chunk is rebuilt as a slice per call, as in [`crate::vp8`]. The
//! alpha plane an ALPH chunk can be decoded straight into is a pointer the C
//! hands over with `vp8l_set_alpha_dst` and takes back afterwards; the shim
//! keeps it as a `(pointer, stride)` pair and turns it into a `&mut [u8]` only
//! for the duration of the decode call that uses it.
//!
//! The pictures the C reads back are views: a `WebPImage` whose `data[0]`
//! points into memory the decoder owns and whose `alloc` fields are null, so
//! nothing on that side will try to free it. That is what `image_view` did.

use std::ffi::{c_int, c_uint};
use std::{ptr, slice};

use wpd::error::Status;
use wpd::vp8l::{AlphaDst, Decoder, Picture, Target};

use crate::vp8::{forward_log, status, WPD_ERROR_INVALID_DATA};

pub const VP8L_NEED_MORE: c_int = 1;

const WPD_PIX_FMT_ARGB: c_int = 2;

/// `WebPImage` from `src/image.h`. Only the ARGB fields are ever filled in.
#[repr(C)]
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

impl WebPImage {
    fn empty() -> Self {
        Self {
            chroma_full: 0,
            premultiplied: 0,
            data: [ptr::null_mut(); 4],
            alloc: [ptr::null_mut(); 4],
            alloc_size: [0; 4],
            linesize: [0; 4],
            width: 0,
            height: 0,
            format: 0,
        }
    }
}

/// The decoder plus the alpha plane it was last pointed at.
pub struct Context {
    decoder: Decoder,
    alpha_dst: *mut u8,
    alpha_stride: c_int,
}

/// # Safety
///
/// `ctx` must point to a live `Context` from [`vp8l_alloc`].
unsafe fn context<'a>(ctx: *mut Context) -> Option<&'a mut Context> {
    unsafe { ctx.as_mut() }
}

/// Writes a view of `pic` into `out`, on the terms `image_view` set: the caller
/// may read it but must not free it.
fn view(out: *mut WebPImage, pic: &Picture) {
    if out.is_null() {
        return;
    }
    let mut img = WebPImage::empty();

    if pic.width > 0 && !pic.data.is_empty() {
        img.data[0] = pic.data.as_ptr().cast::<u8>().cast_mut();
        img.linesize[0] = (pic.stride * 4) as c_int;
        img.width = pic.width;
        img.height = pic.height;
        img.format = WPD_PIX_FMT_ARGB;
    }
    unsafe { out.write(img) };
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
unsafe fn chunk<'a>(data: *const u8, size: c_uint) -> &'a [u8] {
    if data.is_null() {
        return &[];
    }
    unsafe { slice::from_raw_parts(data, size as usize) }
}

#[no_mangle]
pub extern "C" fn vp8l_alloc() -> *mut Context {
    wpd::log::set_sink(forward_log);
    wpd::cpu::init();

    Box::into_raw(Box::new(Context {
        decoder: Decoder::new(),
        alpha_dst: ptr::null_mut(),
        alpha_stride: 0,
    }))
}

/// # Safety
///
/// `ctx` must point to a writable pointer to a live [`Context`], or to null.
#[no_mangle]
pub unsafe extern "C" fn vp8l_free(ctx: *mut *mut Context) {
    unsafe {
        let p = *ctx;

        if !p.is_null() {
            drop(Box::from_raw(p));
            *ctx = ptr::null_mut();
        }
    }
}

/// # Safety
///
/// `ctx` must point to a live [`Context`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_reset(ctx: *mut Context) {
    if let Some(c) = unsafe { context(ctx) } {
        c.decoder.reset();
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_release(ctx: *mut Context) {
    if let Some(c) = unsafe { context(ctx) } {
        c.decoder.release();
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_set_canvas(
    ctx: *mut Context,
    width: c_int,
    height: c_int,
) {
    if let Some(c) = unsafe { context(ctx) } {
        c.decoder.set_canvas(width, height);
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_width(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c.decoder.width)
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_height(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c.decoder.height)
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_has_alpha(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c_int::from(c.decoder.has_alpha))
}

/// # Safety
///
/// `dst` must be readable and writable for `stride` bytes on each of the
/// canvas's rows until the pointer is replaced, which is what the C decoder
/// required of it too.
#[no_mangle]
pub unsafe extern "C" fn vp8l_set_alpha_dst(
    ctx: *mut Context,
    dst: *mut u8,
    stride: c_int,
) {
    if let Some(c) = unsafe { context(ctx) } {
        c.alpha_dst = dst;
        c.alpha_stride = stride;
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_alpha_dst_used(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c_int::from(c.decoder.alpha_dst_used()))
}

/// # Safety
///
/// `data` must be readable for `size` bytes and `out`, when not null, writable.
#[no_mangle]
pub unsafe extern "C" fn vp8l_decode_frame(
    ctx: *mut Context,
    target: c_int,
    out: *mut WebPImage,
    data: *const u8,
    size: c_uint,
    is_alpha_chunk: c_int,
) -> c_int {
    let Some(c) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };
    let target = if target == 1 {
        Target::Alpha
    } else {
        Target::Argb
    };
    let buf = unsafe { chunk(data, size) };
    let stride = c.alpha_stride.max(0) as usize;
    let rows = c.decoder.height.max(0) as usize;
    let alpha = (!c.alpha_dst.is_null() && stride > 0 && rows > 0).then(|| AlphaDst {
        data: unsafe { slice::from_raw_parts_mut(c.alpha_dst, stride * rows) },
        stride,
    });

    let ret = c
        .decoder
        .decode_frame(target, buf, is_alpha_chunk != 0, alpha);

    view(out, c.decoder.picture(target));

    match ret {
        Ok(()) => 0,
        Err(e) => status(e),
    }
}

/// # Safety
///
/// `payload` must be readable for `avail` bytes.
#[no_mangle]
pub unsafe extern "C" fn vp8l_still_step(
    ctx: *mut Context,
    payload: *const u8,
    avail: c_uint,
    size: c_uint,
    complete: c_int,
) -> c_int {
    let Some(c) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };
    let buf = unsafe { chunk(payload, avail) };

    match c.decoder.still_step(buf, size as usize, complete != 0) {
        Ok(Status::Done) => 1,
        Ok(Status::NeedMore) => 0,
        Err(e) => status(e),
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_still_peek(ctx: *mut Context) -> c_int {
    let Some(c) = (unsafe { context(ctx) }) else {
        return WPD_ERROR_INVALID_DATA;
    };

    match c.decoder.still_peek() {
        Ok(()) => 0,
        Err(e) => status(e),
    }
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_still_active(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c_int::from(c.decoder.still_active()))
}

/// # Safety
///
/// As [`vp8l_reset`].
#[no_mangle]
pub unsafe extern "C" fn vp8l_still_rows_out(ctx: *const Context) -> c_int {
    unsafe { ctx.as_ref() }.map_or(0, |c| c.decoder.still_rows_out())
}

/// # Safety
///
/// `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn vp8l_still_frame(ctx: *const Context, out: *mut WebPImage) {
    let Some(c) = (unsafe { ctx.as_ref() }) else {
        return;
    };

    if let Some(pic) = c.decoder.still_picture() {
        view(out, pic);
    }
}
