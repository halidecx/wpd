//! The two things a consumer asks of the library rather than of a decode:
//! which version it is, and where its diagnostics should go.
//!
//! The core hands messages to a sink ([`wpd::log`]); what is here is the sink
//! the C ABI wants, which forwards to a `WPDLogCallback` installed once at
//! startup. The callback and its opaque pointer are stored atomically because
//! the header only promises that installing one before decoding is safe, not
//! that no other thread is decoding while it happens.

use std::ffi::{c_char, c_int, c_uint, c_void, CString};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::{mem, ptr};

use wpd::log::Level;

/// `WPDLogCallback` from `include/wpd.h`.
pub type WPDLogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char);

/// As long a message as the C's stack buffer held, so a consumer that was
/// relying on the truncation still sees it.
const MESSAGE_MAX: usize = 511;

static LOG_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LOG_OPAQUE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The version the package declares, which `include/wpd.h` is checked against
/// at configure time, so neither can drift from the other unnoticed.
const fn parse_version(s: &str) -> [u32; 3] {
    let bytes = s.as_bytes();
    let mut parts = [0u32; 3];
    let mut at = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'.' {
            at += 1;
        } else {
            parts[at] = parts[at] * 10 + (bytes[i] - b'0') as u32;
        }
        i += 1;
    }
    parts
}

const VERSION: [u32; 3] = parse_version(env!("CARGO_PKG_VERSION"));

#[no_mangle]
pub extern "C" fn wpd_version() -> c_uint {
    (VERSION[0] << 16) | (VERSION[1] << 8) | VERSION[2]
}

#[no_mangle]
pub extern "C" fn wpd_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Installs the process-global diagnostic callback. A null one disables
/// logging.
///
/// # Safety
///
/// `callback`, when not null, must stay callable for as long as the library
/// may log, which is what the header asks of it.
#[no_mangle]
pub unsafe extern "C" fn wpd_set_log_callback(
    callback: Option<WPDLogCallback>,
    opaque: *mut c_void,
) {
    let callback = callback.map_or(ptr::null_mut(), |f| f as usize as *mut c_void);

    LOG_OPAQUE.store(opaque, Ordering::Release);
    LOG_CALLBACK.store(callback, Ordering::Release);
}

/// The sink [`wpd::log`] is given, which is where every message the decoder
/// raises leaves the library.
///
/// A trailing newline is dropped and the message truncated, both of which the
/// C did on its way through `vsnprintf`.
pub(crate) fn forward_log(level: Level, message: &str) {
    let installed = LOG_CALLBACK.load(Ordering::Acquire);

    if installed.is_null() {
        return;
    }
    let callback: WPDLogCallback = unsafe { mem::transmute(installed) };
    let opaque = LOG_OPAQUE.load(Ordering::Relaxed);
    let mut bytes = message.as_bytes();

    if bytes.len() > MESSAGE_MAX {
        bytes = &bytes[..MESSAGE_MAX];
    }
    while let [head @ .., b'\n'] = bytes {
        bytes = head;
    }
    let Ok(text) = CString::new(bytes) else {
        return;
    };
    let level = match level {
        Level::Error => 0,
        Level::Warning => 1,
    };

    unsafe { callback(opaque, level, text.as_ptr()) };
}
