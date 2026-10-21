use std::ffi::{c_char, c_int, c_void, CStr};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::{alloc, mem, ptr, slice};

use wpd::driver::convert::{format_plane_dims, format_planes};
use wpd::driver::{Decoder, FORMAT_NONE};
use wpd::error::Error;
use wpd::handout::Handout;

use crate::container::{info_clear, WPDImageInfo};
use crate::frame::{
    frame_clear, frame_private_data, frame_set_plane, frame_set_private_data,
    frame_valid, write_frame, External, SinkInput, WPDFrame, WPDOutputPlane,
};
use crate::options::WPDDecoderOptions;
use crate::status::status;
pub use crate::status::{
    WPD_ERR_BITSTREAM, WPD_ERR_BUFFER_TOO_SMALL, WPD_ERR_INTERNAL, WPD_ERR_INVALID_ARG,
    WPD_ERR_NOT_WEBP, WPD_ERR_NO_MEMORY, WPD_ERR_TOO_LARGE, WPD_ERR_TRUNCATED,
    WPD_ERR_UNSUPPORTED, WPD_OK,
};

pub struct WPDDecoder<'a> {
    decoder: Decoder<'a>,
    poisoned: bool,
    planes: [WPDOutputPlane; 4],
    sink_input: Rc<SinkInput>,
}

impl WPDDecoder<'_> {
    pub(crate) fn new() -> Self {
        let mut decoder = Decoder::new();

        // Legacy callers may omit set_options entirely; their log callbacks
        // must stay on the calling thread until they opt into threading.
        decoder
            .set_core_options(wpd::options::Options {
                n_threads: 1,
                ..Default::default()
            })
            .expect("default C decoder options are valid");

        WPDDecoder {
            decoder,
            poisoned: false,
            planes: [WPDOutputPlane::empty(); 4],
            sink_input: Rc::default(),
        }
    }

    fn protect_input(&self) {
        let input = self.input_bytes();
        let start = input.as_ptr() as usize;
        self.sink_input.range.set((start, start + input.len()));
        self.sink_input.overlap.set(false);
    }

    fn output_failure(&mut self, message: &'static str, error: Error) -> Error {
        if self.sink_input.overlap.get() {
            self.fail("output overlaps input", Error::InvalidArgument)
        } else {
            self.fail(message, error)
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

/* Every wpd_decoder_* entry point null-checks the handle, runs under the
 * panic guard, and reports a c_int. Only the body differs. */
macro_rules! entry {
    (fn $name:ident($decoder:ident $(, $arg:ident: $ty:ty)* $(,)?) $body:block) => {
        entry!(@emit $name, *mut WPDDecoderRaw, as_mut, $decoder $(, $arg: $ty)* $body);
    };
    (fn $name:ident(const $decoder:ident $(, $arg:ident: $ty:ty)* $(,)?) $body:block) => {
        entry!(@emit $name, *const WPDDecoderRaw, cast_mut_as_mut, $decoder
               $(, $arg: $ty)* $body);
    };
    (@emit $name:ident, $ptr:ty, $reach:ident, $decoder:ident
     $(, $arg:ident: $ty:ty)* $body:block) => {
        /// # Safety
        ///
        /// `handle` must be null or a live decoder from `wpd_decoder_create`,
        /// which no other call or log callback may access while this one runs.
        /// Pointer arguments must meet the contract `wpd.h` gives them.
        #[no_mangle]
        pub unsafe extern "C" fn $name(handle: $ptr $(, $arg: $ty)*) -> c_int {
            // SAFETY: the caller passes null or a live handle and holds it
            // exclusively for this call (see `# Safety`), so a unique borrow is
            // sound; `wpd.h` requires that external synchronisation.
            match unsafe { $reach(handle) } {
                Some($decoder) => $decoder.guarded(WPD_ERR_INTERNAL, |$decoder| $body),
                None => WPD_ERR_INVALID_ARG,
            }
        }
    };
}

unsafe fn as_mut<'a>(handle: *mut WPDDecoderRaw) -> Option<&'a mut WPDDecoderRaw> {
    // SAFETY: the caller guarantees `handle` is null or a live, exclusively
    // held `Box` allocation from `wpd_decoder_create`.
    unsafe { handle.as_mut() }
}

unsafe fn cast_mut_as_mut<'a>(
    handle: *const WPDDecoderRaw,
) -> Option<&'a mut WPDDecoderRaw> {
    // SAFETY: as in `as_mut`. The C `const` only says the call does not
    // logically change the decoder; the allocation came from a `Box`, so it is
    // mutable, and `wpd.h` makes const queries exclusive like any other call.
    unsafe { handle.cast_mut().as_mut() }
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
    // SAFETY: `layout` has a non-zero size (checked above).
    let raw = unsafe { alloc::alloc(layout) }.cast::<T>();

    if raw.is_null() {
        return Err(Error::NoMemory);
    }
    // SAFETY: `raw` is a fresh, non-null allocation from the global allocator
    // with `T`'s layout, which is what `Box::from_raw` expects once written.
    unsafe {
        raw.write(value);
        Ok(Box::from_raw(raw))
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

/// # Safety
///
/// `decoder` must be null or a live decoder from `wpd_decoder_create`, not
/// freed before and not in use by any other call.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_free(decoder: *mut WPDDecoderRaw) {
    if !decoder.is_null() {
        // SAFETY: a non-null `decoder` is a `Box::into_raw` result from
        // `wpd_decoder_create` that the caller has not freed (see `# Safety`).
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
        || options.uvmode > 3
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
        // Rows are reached by pointer offset, which is only defined inside
        // an allocation, and no allocation outgrows isize.
        if plane.data.is_null() != (plane.stride == 0)
            || plane.size > isize::MAX as usize
        {
            return Err(decoder.fail("invalid output buffer", Error::InvalidArgument));
        }
    }
    if !decoder.has_sink() || decoder.planes != buffer.plane {
        let sink = try_box(External(buffer.plane, Rc::clone(&decoder.sink_input)))?;

        decoder.planes = buffer.plane;
        decoder.set_sink(Some(sink));
    }
    Ok(())
}

entry!(fn wpd_decoder_set_options(decoder, options: *const WPDDecoderOptions) {
    if options.is_null() {
        return status(decoder.fail("invalid decoder options", Error::InvalidArgument));
    };

    // Read only the caller's allocation; an older pointer cannot become a newer reference.
    // SAFETY: `options` is non-null (checked above) and, per `wpd.h`, points to
    // a caller-initialised struct that begins with `struct_size`.
    let size = unsafe { ptr::addr_of!((*options).struct_size).read() };
    if size < WPDDecoderOptions::v1() {
        return status(decoder.fail("invalid decoder options", Error::InvalidArgument));
    }
    // SAFETY: every field is an integer, for which zero is a valid value.
    let mut local: WPDDecoderOptions = unsafe { std::mem::zeroed() };
    // SAFETY: the caller declared `size` >= v1 bytes behind `options`, and
    // `local` is a distinct, full-sized struct.
    unsafe {
        ptr::copy_nonoverlapping(
            options.cast::<u8>(),
            ptr::addr_of_mut!(local).cast::<u8>(),
            WPDDecoderOptions::v1(),
        );
    }
    if size >= WPDDecoderOptions::v2() {
        // SAFETY: the declared `size` reaches past `n_threads`.
        local.n_threads = unsafe { ptr::addr_of!((*options).n_threads).read() };
    }
    if size >= WPDDecoderOptions::v3() {
        // SAFETY: the declared `size` reaches past `frame_size_limit`.
        local.frame_size_limit =
            unsafe { ptr::addr_of!((*options).frame_size_limit).read() };
    }
    reported(set_options(decoder, &local).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_set_animation_mode(decoder, mode: c_int) {
    reported(decoder.set_animation_mode(mode).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_set_output_format(decoder, format: c_int) {
    reported(decoder.set_output_format(format).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_set_output_buffer(decoder, buffer: *const WPDOutputBuffer) {
    // SAFETY: `buffer` is null or, per `wpd.h`, a full `WPDOutputBuffer` set
    // up by the caller; the plane memory it names is the caller's to lend.
    reported(unsafe { set_output_buffer(decoder, buffer.as_ref()) }.map(|()| WPD_OK))
});

unsafe fn lent<'a>(data: *const u8, size: usize) -> Option<&'a [u8]> {
    if size == 0 {
        return Some(&[]);
    }
    if data.is_null() || size > isize::MAX as usize {
        return None;
    }
    // SAFETY: `data` is non-null and `size` fits an isize (checked above); the
    // caller guarantees `size` readable bytes there, kept alive and unchanged
    // for as long as `'a` is used (for borrowed input, until reopen or free).
    Some(unsafe { slice::from_raw_parts(data, size) })
}

pub(crate) unsafe fn with_prefix<T>(
    p: *mut T,
    v1: usize,
    body: impl FnOnce(&mut T) -> Result<(), Error>,
) -> Result<(), Error> {
    debug_assert!(v1 <= mem::size_of::<T>());

    // SAFETY: the caller guarantees an all-zero `T` is a valid value.
    let mut local: T = unsafe { mem::zeroed() };

    // SAFETY: the caller guarantees `p` is readable for `v1` bytes, and
    // `v1` <= size_of::<T>() keeps the copy inside `local`.
    unsafe {
        ptr::copy_nonoverlapping(
            p.cast::<u8>(),
            ptr::addr_of_mut!(local).cast::<u8>(),
            v1,
        );
    }

    let ret = body(&mut local);

    // SAFETY: the caller guarantees `p` is writable for `v1` bytes; `local`
    // is a separate stack value, so the ranges cannot overlap.
    unsafe {
        ptr::copy_nonoverlapping(ptr::addr_of!(local).cast::<u8>(), p.cast::<u8>(), v1);
    }
    ret
}

entry!(fn wpd_decoder_open(decoder, data: *const u8, size: usize) {
    // SAFETY: the caller lends `size` readable bytes at `data`, per `wpd.h`.
    let Some(data) = (unsafe { lent(data, size) }) else {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    };
    reported(decoder.open(data).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_open_borrowed(decoder, data: *const u8, size: usize) {
    // SAFETY: the caller lends `size` readable bytes at `data`, per `wpd.h`.
    let Some(data) = (unsafe { lent(data, size) }) else {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    };
    reported(decoder.open_borrowed(data).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_open_stream(decoder) {
    reported(decoder.open_stream().map(|()| WPD_OK))
});

entry!(fn wpd_decoder_append(decoder, data: *const u8, size: usize) {
    // SAFETY: the caller lends `size` readable bytes at `data`, per `wpd.h`.
    let Some(data) = (unsafe { lent(data, size) }) else {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    };
    reported(decoder.append(data).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_update(decoder, data: *const u8, size: usize) {
    // SAFETY: the caller lends `size` readable bytes at `data`, per `wpd.h`.
    let Some(data) = (unsafe { lent(data, size) }) else {
        return status(decoder.fail("invalid input data", Error::InvalidArgument));
    };
    reported(decoder.update(data).map(|()| WPD_OK))
});

entry!(fn wpd_decoder_end_of_stream(decoder) {
    reported(decoder.end_of_stream().map(|()| WPD_OK))
});

entry!(fn wpd_decoder_get_info(const decoder, info: *mut WPDImageInfo) {
    if info.is_null() {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    // SAFETY: `info` is non-null (checked above) and, per `wpd.h`, points to a
    // caller-initialised struct that begins with `struct_size`.
    let size = unsafe { ptr::addr_of!((*info).struct_size).read() };

    if size < WPDImageInfo::v1() {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    // SAFETY: the caller declared at least v1 bytes behind `info` (checked
    // above); the struct is plain integers, so zeroed is a valid value.
    reported(
        unsafe { with_prefix(info, WPDImageInfo::v1(), |info| get_info(decoder, info)) }
            .map(|()| WPD_OK),
    )
});

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
    info.coding = crate::container::coding(got.coding);
    info.metadata = got.metadata;
    Ok(())
}

entry!(fn wpd_decoder_rewind(decoder) {
    reported(decoder.rewind().map(|()| WPD_OK))
});

entry!(fn wpd_decoder_frame_info(const decoder, index: c_int, info: *mut WPDFrameInfo) {
    if info.is_null() {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    // SAFETY: `info` is non-null (checked above) and, per `wpd.h`, points to a
    // caller-initialised struct that begins with `struct_size`.
    let size = unsafe { ptr::addr_of!((*info).struct_size).read() };

    if size < frame_info_v1() {
        return status(decoder.fail("invalid decoder state", Error::InvalidArgument));
    }
    // SAFETY: the caller declared at least v1 bytes behind `info` (checked
    // above); the struct is plain integers, so zeroed is a valid value.
    reported(
        unsafe {
            with_prefix(info, frame_info_v1(), |info| frame_info(decoder, index, info))
        }
        .map(|()| WPD_OK),
    )
});

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
    info.blend = c_int::from(entry.no_blend);
    info.has_alpha = c_int::from(entry.has_alpha);
    info.complete = c_int::from(entry.complete);
    Ok(())
}

entry!(fn wpd_decoder_metadata(
    const decoder,
    which: c_int,
    data: *mut *const u8,
    size: *mut usize,
) {
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

            // SAFETY: both out-pointers are non-null (checked above) and, per
            // `wpd.h`, point to writable storage of their types.
            unsafe {
                data.write(at);
                size.write(len);
            }
            WPD_OK
        }
    }
});

entry!(fn wpd_decoder_next_frame(decoder, frame: *mut WPDFrame) {
    // SAFETY: `frame` is null or a caller-owned `WPDFrame` set up with
    // `WPD_FRAME_INIT`, as `wpd.h` requires; `next_frame` checks the rest.
    reported(unsafe { next_frame(decoder, frame) }.map(c_int::from))
});

unsafe fn next_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
) -> Result<bool, Error> {
    // SAFETY: the caller guarantees `frame` is null or readable through its
    // `struct_size`.
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }

    decoder.protect_input();
    let ext = decoder.planes;
    let mut out = Handout::default();

    match decoder.next_picture(&mut out) {
        Ok(got) => {
            if got {
                // SAFETY: `frame` passed `frame_valid`, and `wpd.h` requires
                // its `private_data` to be null or set by this library.
                unsafe { wpd_frame_free(frame) };
                // SAFETY: `frame` passed `frame_valid` and is caller-writable.
                unsafe { write_frame(&out, &ext, frame) };
            }
            Ok(got)
        }
        Err((message, e)) => Err(decoder.output_failure(message, e)),
    }
}

entry!(fn wpd_decoder_partial_frame(
    decoder,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) {
    // SAFETY: `frame` is null or a caller-owned `WPDFrame` set up with
    // `WPD_FRAME_INIT`, and `rows_valid` is null or writable, per `wpd.h`.
    reported(unsafe { partial_frame(decoder, frame, rows_valid) }.map(|()| WPD_OK))
});

unsafe fn partial_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> Result<(), Error> {
    // SAFETY: the caller guarantees `frame` is null or readable through its
    // `struct_size`.
    if !unsafe { frame_valid(frame) } {
        return Err(decoder.fail("invalid frame", Error::InvalidArgument));
    }
    if let Err((message, e)) = decoder.require_open() {
        return Err(decoder.fail(message, e));
    }

    decoder.protect_input();
    let ext = decoder.planes;
    let mut out = Handout::default();
    let mut rows = 0;

    // SAFETY: `frame` passed `frame_valid`, and `wpd.h` requires its
    // `private_data` to be null or set by this library.
    unsafe { wpd_frame_free(frame) };

    let ret = match decoder.partial_picture(&mut out, &mut rows) {
        Ok(had_picture) => {
            if had_picture {
                // SAFETY: `frame` passed `frame_valid` and is caller-writable.
                unsafe { write_frame(&out, &ext, frame) };
            }
            Ok(())
        }
        Err((message, e)) => Err(decoder.output_failure(message, e)),
    };

    if !rows_valid.is_null() {
        // SAFETY: non-null (checked above) and writable, per the caller.
        unsafe { rows_valid.write(rows) };
    }
    ret
}

/// # Safety
///
/// `decoder` must be null or a live decoder from `wpd_decoder_create`, not in
/// use by any other call.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_status(decoder: *const WPDDecoderRaw) -> c_int {
    // SAFETY: `decoder` is null or live, and no other call mutates it now
    // (see `# Safety`).
    match unsafe { decoder.as_ref() } {
        Some(decoder) => decoder.status().map_or(WPD_OK, status),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `decoder` must be null or a live decoder from `wpd_decoder_create`, not in
/// use by any other call. The returned string is valid only until the next
/// call using that decoder.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_error(
    decoder: *const WPDDecoderRaw,
) -> *const c_char {
    // SAFETY: `decoder` is null or live, and no other call mutates it now
    // (see `# Safety`).
    match unsafe { decoder.as_ref() } {
        Some(decoder) if !decoder.error_raw().is_empty() => {
            decoder.error_raw().as_ptr().cast()
        }
        _ => c"unknown decoder error".as_ptr(),
    }
}

struct WPDFrameOwner {
    plane: [Vec<u8>; 4],
}

struct Owned(*mut WPDDecoderRaw);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `Owned` is only built around a non-null handle fresh from
        // `wpd_decoder_create`, which it alone frees, once.
        unsafe { wpd_decoder_free(self.0) };
    }
}

unsafe fn decode_once(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> Result<(Owned, c_int), c_int> {
    let decoder = wpd_decoder_create();

    if decoder.is_null() {
        return Err(WPD_ERR_NO_MEMORY);
    }
    let owned = Owned(decoder);
    let mut status = if options.is_null() {
        WPD_OK
    } else {
        // SAFETY: `decoder` is live and ours alone; `options` is non-null and
        // valid per the caller of `decode_once`.
        unsafe { wpd_decoder_set_options(decoder, options) }
    };

    if status == WPD_OK {
        // SAFETY: `decoder` is live and ours alone.
        status = unsafe { wpd_decoder_set_output_format(decoder, format) };
    }
    if status == WPD_OK && !buffer.is_null() {
        // SAFETY: `decoder` is live and ours alone; `buffer` is non-null and
        // valid per the caller of `decode_once`.
        status = unsafe { wpd_decoder_set_output_buffer(decoder, buffer) };
    }
    if status == WPD_OK {
        // SAFETY: the caller lends `size` bytes at `data`; the decoder borrowing
        // them is returned in `Owned`, which the caller drops before returning.
        status = unsafe { wpd_decoder_open_borrowed(decoder, data, size) };
    }
    let ret = if status == WPD_OK {
        // SAFETY: `decoder` is live and ours alone; the caller passes a
        // `frame` that passed `frame_valid`.
        unsafe { wpd_decoder_next_frame(decoder, frame) }
    } else {
        status
    };

    Ok((owned, ret))
}

/// # Safety
///
/// `data` must be readable for `size` bytes, `options` null or a valid
/// `WPDDecoderOptions`, `buffer` a valid `WPDOutputBuffer` whose planes are
/// writable for their `size` and do not overlap the input, and `frame` null or
/// a `WPDFrame` set up with `WPD_FRAME_INIT` whose `private_data` is null or
/// set by this library.
#[no_mangle]
pub unsafe extern "C" fn wpd_decode_into(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> c_int {
    crate::guard(WPD_ERR_INTERNAL, || {
        // SAFETY: `frame` is null or a caller-initialised `WPDFrame`.
        if data.is_null() || buffer.is_null() || !unsafe { frame_valid(frame) } {
            return WPD_ERR_INVALID_ARG;
        }
        // SAFETY: `frame` passed `frame_valid`.
        if !unsafe { frame_private_data(frame) }.is_null() {
            // SAFETY: `frame` passed `frame_valid`, and its `private_data` is
            // null or this library's, per `# Safety`.
            unsafe { wpd_frame_free(frame) };
        }
        // SAFETY: the pointers are non-null where required (checked above)
        // and otherwise valid per `# Safety`; `frame` passed `frame_valid`.
        let (_decoder, ret) =
            match unsafe { decode_once(data, size, format, options, buffer, frame) } {
                Ok(got) => got,
                Err(ret) => return ret,
            };

        match ret {
            0 => WPD_ERR_BITSTREAM,
            ret if ret < 0 => ret,
            _ => WPD_OK,
        }
    })
}

/// # Safety
///
/// `data` must be readable for `size` bytes, `options` null or a valid
/// `WPDDecoderOptions`, and `frame` null or a `WPDFrame` set up with
/// `WPD_FRAME_INIT` whose `private_data` is null or set by this library.
#[no_mangle]
pub unsafe extern "C" fn wpd_decode(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    frame: *mut WPDFrame,
) -> c_int {
    crate::guard(WPD_ERR_INTERNAL, || {
        // SAFETY: `frame` is null or a caller-initialised `WPDFrame`.
        if data.is_null() || !unsafe { frame_valid(frame) } {
            return WPD_ERR_INVALID_ARG;
        }
        // SAFETY: `frame` passed `frame_valid`.
        if !unsafe { frame_private_data(frame) }.is_null() {
            // SAFETY: `frame` passed `frame_valid`, and its `private_data` is
            // null or this library's, per `# Safety`.
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
        // SAFETY: `data` is non-null and lent per `# Safety`; `options` is null
        // or valid; `decoded` is a full, local `WPDFrame`.
        let (decoder, ret) = match unsafe {
            decode_once(data, size, format, options, ptr::null(), &mut decoded)
        } {
            Ok(got) => got,
            Err(ret) => return ret,
        };

        if ret <= 0 {
            return if ret < 0 { ret } else { WPD_ERR_BITSTREAM };
        }

        let owner = match try_box(WPDFrameOwner {
            plane: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        }) {
            Ok(owner) => owner,
            Err(e) => return status(e),
        };
        let planes = format_planes(decoded.format);

        // SAFETY: `frame` passed `frame_valid` and is caller-writable.
        unsafe { frame_clear(frame) };
        frame_copy(frame, &decoded);

        let owner = Box::into_raw(owner);

        // SAFETY: `frame` passed `frame_valid`.
        unsafe { frame_set_private_data(frame, owner.cast::<c_void>()) };

        // SAFETY: `owner` came from `Box::into_raw` just above; the copy in
        // `private_data` is not used until `wpd_frame_free`, after this
        // borrow's last use.
        let owner = unsafe { &mut *owner };
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
                // SAFETY: with no output buffer, `decoded` points into the
                // frame `decoder` owns and keeps alive until the `drop` below;
                // plane `p` holds `h` rows of at least `w` bytes `stride` apart,
                // and `data` is its first logical row even when flipped.
                let src =
                    unsafe { decoded.data[p].offset(y as isize * decoded.stride[p]) };

                owner.plane[p]
                    // SAFETY: `src` is row `y` of that plane, `w` bytes long.
                    .extend_from_slice(unsafe { slice::from_raw_parts(src, w) });
            }
            debug_assert_eq!(owner.plane[p].len(), bytes);
            // SAFETY: `frame` passed `frame_valid`, and `p` < `planes` <= 4.
            unsafe { frame_set_plane(frame, p, owner.plane[p].as_ptr(), w as isize) };
        }
        drop(decoder);

        if status != WPD_OK {
            // SAFETY: `frame` passed `frame_valid`, and its `private_data` is
            // the owner set above.
            unsafe { wpd_frame_free(frame) };
            return status;
        }
        WPD_OK
    })
}

fn frame_copy(dst: *mut WPDFrame, src: &WPDFrame) {
    let head = crate::frame::frame_head();
    // SAFETY: the only caller, `wpd_decode`, passes a `dst` that passed
    // `frame_valid`; `src` is a reference, so it is a full `WPDFrame`.
    let extent = unsafe { crate::frame::frame_extent(dst) }
        // SAFETY: as above.
        .min(unsafe { crate::frame::frame_extent(src) });

    // SAFETY: `extent` fits both frames, and `src` is a distinct local value
    // in `wpd_decode`, so the ranges do not overlap.
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
/// `frame` must be null or a `WPDFrame` set up with `WPD_FRAME_INIT` and not
/// modified since, so its `private_data` is null or set by this library.
#[no_mangle]
pub unsafe extern "C" fn wpd_frame_free(frame: *mut WPDFrame) {
    // SAFETY: `frame` is null or a caller-initialised `WPDFrame`.
    if !unsafe { frame_valid(frame) } {
        return;
    }
    // SAFETY: `frame` passed `frame_valid`.
    let owner = unsafe { frame_private_data(frame) };

    if !owner.is_null() {
        crate::guard((), || {
            // SAFETY: the only non-null `private_data` this library sets is a
            // `Box<WPDFrameOwner>` from `wpd_decode`, and `frame_clear` below
            // nulls it so it is freed once.
            drop(unsafe { Box::from_raw(owner.cast::<WPDFrameOwner>()) })
        });
    }
    // SAFETY: `frame` passed `frame_valid` and is caller-writable.
    unsafe { frame_clear(frame) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_frames_replace_one_shot_owned_frames() {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wpd-test-data/lossy.webp"
        ))
        .unwrap();
        let decoder = wpd_decoder_create();
        assert!(!decoder.is_null());
        // SAFETY: an all-zero `WPDFrame` is valid: it holds only integers and null
        // pointers.
        let mut frame: WPDFrame = unsafe { mem::zeroed() };
        frame.struct_size = mem::size_of::<WPDFrame>();
        for partial in [false, true] {
            assert_eq!(
                // SAFETY: `data` and `frame` are live; `frame.struct_size` is set.
                unsafe {
                    wpd_decode(data.as_ptr(), data.len(), 3, ptr::null(), &mut frame)
                },
                WPD_OK
            );
            assert!(!frame.private_data.is_null());
            assert_eq!(
                // SAFETY: `decoder` is live, and `data` outlives it.
                unsafe {
                    wpd_decoder_open_borrowed(decoder, data.as_ptr(), data.len())
                },
                WPD_OK
            );
            if partial {
                assert_eq!(
                    // SAFETY: `decoder` and `frame` are live; `rows_valid` may be null.
                    unsafe {
                        wpd_decoder_partial_frame(decoder, &mut frame, ptr::null_mut())
                    },
                    WPD_OK
                );
            } else {
                // SAFETY: `decoder` and `frame` are live.
                assert_eq!(unsafe { wpd_decoder_next_frame(decoder, &mut frame) }, 1);
            }
            assert!(frame.private_data.is_null());
        }
        // SAFETY: each is live and freed exactly once.
        unsafe {
            wpd_frame_free(&mut frame);
            wpd_decoder_free(decoder);
        }
    }

    #[test]
    fn borrowed_input_cannot_be_used_as_output() {
        let mut data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wpd-test-data/lossy.webp"
        ))
        .unwrap();
        let size = data.len();
        // SAFETY: an all-zero `WPDFrame` is valid: it holds only integers and null
        // pointers.
        let mut frame: WPDFrame = unsafe { mem::zeroed() };
        frame.struct_size = mem::size_of::<WPDFrame>();
        assert_eq!(
            // SAFETY: `data` covers `size` bytes and `frame.struct_size` is set.
            unsafe { wpd_decode(data.as_ptr(), size, 3, ptr::null(), &mut frame) },
            WPD_OK
        );
        let stride = frame.width as usize * 4;
        let bytes = stride * frame.height as usize;
        // SAFETY: `frame` holds only what `wpd_decode` put there.
        unsafe { wpd_frame_free(&mut frame) };
        data.resize(size.max(bytes), 0);
        let original = data.clone();
        let mut buffer = WPDOutputBuffer {
            struct_size: mem::size_of::<WPDOutputBuffer>(),
            plane: [WPDOutputPlane::empty(); 4],
        };
        buffer.plane[0] = WPDOutputPlane {
            data: data.as_mut_ptr(),
            size: bytes,
            stride: stride as isize,
        };
        assert_eq!(
            // SAFETY: every pointer names a live local covering its declared size; the
            // plane overlaps `data`, which the call must reject untouched.
            unsafe {
                wpd_decode_into(
                    data.as_ptr(),
                    size,
                    3,
                    ptr::null(),
                    &buffer,
                    &mut frame,
                )
            },
            WPD_ERR_INVALID_ARG
        );
        assert_eq!(data, original);
    }

    #[test]
    fn threading_requires_current_options_from_the_c_caller() {
        thread_local! {
            static ALPHA_ERRORS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }

        unsafe extern "C" fn log(_: *mut c_void, _: c_int, message: *const c_char) {
            // SAFETY: the library passes a NUL-terminated message valid for the call.
            if unsafe { CStr::from_ptr(message) }
                .to_bytes()
                .starts_with(b"ALPHA chunk carries ")
            {
                ALPHA_ERRORS.with(|n| n.set(n.get() + 1));
            }
        }

        let mut data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wpd-test-data/a_lossy.webp"
        ))
        .unwrap();
        let alpha = data.windows(4).position(|v| v == b"ALPH").unwrap() + 8;

        // Treat the compressed payload as raw alpha, producing a diagnostic
        // in the alpha job, which can run on a worker for this large image.
        data[alpha] = 0;
        let decoder = wpd_decoder_create();

        assert!(!decoder.is_null());
        // SAFETY: `log` only touches a thread-local, so any thread may call it, and
        // `opaque` is unused.
        unsafe { crate::compat::wpd_set_log_callback(Some(log), ptr::null_mut()) };
        let mut counts = Vec::new();
        for version in [
            None,
            Some(mem::size_of::<WPDDecoderOptions>()),
            Some(WPDDecoderOptions::v1()),
            Some(WPDDecoderOptions::v3()),
        ] {
            if let Some(size) = version {
                // SAFETY: every field is an integer, for which zero is a valid value.
                let mut options: WPDDecoderOptions = unsafe { mem::zeroed() };

                options.struct_size = size;
                options.n_threads = 2;
                assert_eq!(
                    // SAFETY: `decoder` and `options` are live; `struct_size` is set.
                    unsafe { wpd_decoder_set_options(decoder, &options) },
                    WPD_OK
                );
            }
            assert_eq!(
                // SAFETY: `decoder` is live; `data` outlives it.
                unsafe {
                    wpd_decoder_open_borrowed(decoder, data.as_ptr(), data.len())
                },
                WPD_OK
            );
            // SAFETY: an all-zero `WPDFrame` is valid: it holds only integers and null
            // pointers.
            let mut frame: WPDFrame = unsafe { mem::zeroed() };

            frame.struct_size = mem::size_of::<WPDFrame>();
            ALPHA_ERRORS.with(|n| n.set(0));
            assert_eq!(
                // SAFETY: `decoder` and `frame` are live.
                unsafe { wpd_decoder_next_frame(decoder, &mut frame) },
                WPD_ERR_BITSTREAM
            );
            counts.push(ALPHA_ERRORS.with(|n| n.get()));
        }
        // SAFETY: clearing the callback takes no pointers; `decoder` is freed once.
        unsafe {
            crate::compat::wpd_set_log_callback(None, ptr::null_mut());
            wpd_decoder_free(decoder);
        }
        let threaded = usize::from(!cfg!(feature = "threads"));

        assert_eq!(counts, [1, threaded, 1, threaded]);
    }

    #[repr(C, align(8))]
    struct Short<const N: usize> {
        bytes: [u8; N],
        canary: [u8; 32],
    }

    impl<const N: usize> Short<N> {
        fn new() -> Self {
            let mut this = Short {
                bytes: [0; N],
                canary: [0x5a; 32],
            };

            this.bytes[..mem::size_of::<usize>()].copy_from_slice(&N.to_ne_bytes());
            this
        }

        fn field(&self, at: usize) -> c_int {
            c_int::from_ne_bytes(self.bytes[at..at + 4].try_into().unwrap())
        }
    }

    #[test]
    fn a_caller_with_a_v1_info_struct_is_never_touched_past_it() {
        const IMAGE_V1: usize = mem::offset_of!(WPDImageInfo, metadata) + 4;
        const FRAME_V1: usize = mem::offset_of!(WPDFrameInfo, complete) + 4;

        assert!(IMAGE_V1 < mem::size_of::<WPDImageInfo>());
        assert!(FRAME_V1 < mem::size_of::<WPDFrameInfo>());

        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../wpd-test-data/anim_rgb.webp"
        ))
        .unwrap();
        let mut image = Short::<IMAGE_V1>::new();
        let mut frame = Short::<FRAME_V1>::new();
        let decoder = wpd_decoder_create();

        assert!(!decoder.is_null());
        // SAFETY: `data` covers its length, and `image` is aligned and holds the
        // `IMAGE_V1` bytes its `struct_size` declares.
        unsafe {
            assert_eq!(
                crate::container::wpd_get_info(
                    data.as_ptr(),
                    data.len(),
                    image.bytes.as_mut_ptr().cast()
                ),
                WPD_OK
            );
        }
        let width = image.field(mem::offset_of!(WPDImageInfo, width));
        let frames = image.field(mem::offset_of!(WPDImageInfo, frame_count));

        assert!(width > 0 && frames > 1);
        assert_eq!(image.canary, [0x5a; 32]);
        image = Short::new();

        // SAFETY: `decoder` is live, `data` outlives it, and `image` and `frame` are
        // aligned and hold the bytes their `struct_size` declares.
        unsafe {
            assert_eq!(
                wpd_decoder_open_borrowed(decoder, data.as_ptr(), data.len()),
                WPD_OK
            );
            assert_eq!(
                wpd_decoder_get_info(decoder, image.bytes.as_mut_ptr().cast()),
                WPD_OK
            );
            assert_eq!(
                wpd_decoder_frame_info(decoder, 1, frame.bytes.as_mut_ptr().cast()),
                WPD_OK
            );
        }
        assert_eq!(image.field(mem::offset_of!(WPDImageInfo, width)), width);
        assert!(frame.field(mem::offset_of!(WPDFrameInfo, width)) > 0);
        assert_eq!(image.canary, [0x5a; 32]);
        assert_eq!(frame.canary, [0x5a; 32]);

        let mut tiny = Short::<IMAGE_V1>::new();

        tiny.bytes[..mem::size_of::<usize>()]
            .copy_from_slice(&(IMAGE_V1 - 1).to_ne_bytes());
        // SAFETY: `tiny` is aligned and holds `IMAGE_V1` bytes, more than it declares;
        // `decoder` is live and freed once.
        unsafe {
            assert_eq!(
                wpd_decoder_get_info(decoder, tiny.bytes.as_mut_ptr().cast()),
                WPD_ERR_INVALID_ARG
            );
            wpd_decoder_free(decoder);
        }
        assert_eq!(tiny.field(mem::offset_of!(WPDImageInfo, width)), 0);
    }

    #[test]
    fn a_size_no_slice_can_describe_is_refused() {
        let decoder = wpd_decoder_create();
        let byte = 0u8;

        assert!(!decoder.is_null());
        // SAFETY: both sizes exceed isize::MAX and are refused before `&byte` is read;
        // `decoder` is live and freed once.
        unsafe {
            assert_eq!(
                wpd_decoder_open_borrowed(decoder, &byte, isize::MAX as usize + 1),
                WPD_ERR_INVALID_ARG
            );
            assert_eq!(
                wpd_decoder_append(decoder, &byte, usize::MAX),
                WPD_ERR_INVALID_ARG
            );
            wpd_decoder_free(decoder);
        }
    }

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
            // SAFETY: `decoder`, `frame` and `rows` are live locals.
            unsafe { wpd_decoder_partial_frame(decoder, &mut frame, &mut rows) };

        assert_eq!(status, WPD_ERR_INVALID_ARG);
        assert_eq!((frame.width, frame.height, frame.duration), (7, 9, 11));
        assert_eq!(rows, 5);
        // SAFETY: `decoder` is live and freed once.
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
