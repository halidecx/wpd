use std::ffi::{c_char, c_int, c_uint, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::{mem, ptr};

use wpd::log::Level;

pub type WPDLogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char);

const MESSAGE_MAX: usize = 511;

static LOG_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LOG_OPAQUE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_set_log_callback(
    callback: Option<WPDLogCallback>,
    opaque: *mut c_void,
) {
    let callback = callback.map_or(ptr::null_mut(), |f| f as usize as *mut c_void);

    LOG_OPAQUE.store(opaque, Ordering::Release);
    LOG_CALLBACK.store(callback, Ordering::Release);
}

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
    if bytes.contains(&0) {
        return;
    }
    let mut text = [0; MESSAGE_MAX + 1];

    text[..bytes.len()].copy_from_slice(bytes);
    let level = match level {
        Level::Error => 0,
        Level::Warning => 1,
    };

    unsafe { callback(opaque, level, text.as_ptr().cast()) };
}
