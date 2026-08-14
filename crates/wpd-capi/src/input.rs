//! C ABI for the input buffer, as declared by `src/input.h`.
//!
//! The geometry is [`wpd::input`]; what is here is the memory. A buffer is in
//! one of two states: it owns a growing allocation that a stream is appended
//! to, or it points at memory the caller lent and owns nothing. The second is
//! the whole reason this lives on this side of the boundary — a borrowed
//! buffer has a lifetime the C ABI cannot express, so it is kept as a raw
//! pointer and rebuilt into a slice per call, never stored as a borrow.

use std::alloc::{self, Layout};
use std::ffi::c_int;
use std::{ptr, slice};

use wpd::error::Error;
use wpd::image::FILE_PADDING;
use wpd::input::{compact, grow_to, Window};

const WPD_OK: c_int = 0;
const WPD_ERR_NO_MEMORY: c_int = -6;
const WPD_ERR_TOO_LARGE: c_int = -7;

/// What `malloc` guarantees, which is what the C allocation this replaces had.
const ALIGN: usize = 16;

fn layout(size: usize) -> Layout {
    Layout::from_size_align(size, ALIGN)
        .expect("a capacity that already fits in a usize")
}

pub struct InputBuffer {
    /// Where the buffered bytes start. Equal to `alloc` unless the caller lent
    /// its own memory, in which case nothing here owns them.
    at: *const u8,
    alloc: *mut u8,
    capacity: usize,
    window: Window,
    borrowed: bool,
}

impl InputBuffer {
    /// The owned allocation as a slice of its capacity.
    ///
    /// # Safety
    ///
    /// There must be an allocation, which every caller checks by having just
    /// grown it.
    unsafe fn owned(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.alloc, self.capacity) }
    }

    /// Grows the allocation to take `size` more bytes.
    fn reserve(&mut self, size: usize) -> c_int {
        let grown = match grow_to(self.capacity, self.window.buffered(), size) {
            Ok(None) => return WPD_OK,
            Ok(Some(grown)) => grown,
            Err(Error::TooLarge) => return WPD_ERR_TOO_LARGE,
            Err(_) => return WPD_ERR_NO_MEMORY,
        };
        let fresh = unsafe {
            if self.alloc.is_null() {
                alloc::alloc(layout(grown))
            } else {
                alloc::realloc(self.alloc, layout(self.capacity), grown)
            }
        };

        if fresh.is_null() {
            return WPD_ERR_NO_MEMORY;
        }
        self.alloc = fresh;
        self.at = fresh;
        self.capacity = grown;
        WPD_OK
    }
}

#[no_mangle]
pub extern "C" fn input_alloc() -> *mut InputBuffer {
    Box::into_raw(Box::new(InputBuffer {
        at: ptr::null(),
        alloc: ptr::null_mut(),
        capacity: 0,
        window: Window::default(),
        borrowed: false,
    }))
}

/// # Safety
///
/// `input` must point to a writable pointer to a live [`InputBuffer`], or to
/// null.
#[no_mangle]
pub unsafe extern "C" fn input_free(input: *mut *mut InputBuffer) {
    unsafe {
        let p = *input;

        if p.is_null() {
            return;
        }
        let mut boxed = Box::from_raw(p);

        if !boxed.alloc.is_null() {
            alloc::dealloc(boxed.alloc, layout(boxed.capacity));
            boxed.alloc = ptr::null_mut();
        }
        drop(boxed);
        *input = ptr::null_mut();
    }
}

/// Forgets the input, keeping the allocation for the next file.
///
/// # Safety
///
/// `input` must point to a live [`InputBuffer`].
#[no_mangle]
pub unsafe extern "C" fn input_reset(input: *mut InputBuffer) {
    let input = unsafe { &mut *input };

    input.at = input.alloc;
    input.window = Window::default();
    input.borrowed = false;
}

/// # Safety
///
/// `input` must be live and `data` readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn input_own(
    input: *mut InputBuffer,
    data: *const u8,
    size: usize,
) -> c_int {
    let input = unsafe { &mut *input };

    input.at = input.alloc;
    input.window = Window::default();
    input.borrowed = false;

    let ret = input.reserve(size);

    if ret != WPD_OK {
        return ret;
    }
    let src = unsafe { slice::from_raw_parts(data, size) };
    let dst = unsafe { input.owned() };

    dst[..size].copy_from_slice(src);
    dst[size..size + FILE_PADDING].fill(0);
    input.at = input.alloc;
    input.window.size = size;
    WPD_OK
}

/// # Safety
///
/// `data` must stay readable for `size` bytes and unmoved until the next call
/// that replaces it, which is what the C API asks of a borrowed input.
#[no_mangle]
pub unsafe extern "C" fn input_borrow(
    input: *mut InputBuffer,
    data: *const u8,
    size: usize,
) {
    let input = unsafe { &mut *input };

    input.at = data;
    input.window = Window { size, discarded: 0 };
    input.borrowed = true;
}

/// # Safety
///
/// As [`input_own`].
#[no_mangle]
pub unsafe extern "C" fn input_append(
    input: *mut InputBuffer,
    data: *const u8,
    size: usize,
) -> c_int {
    let input = unsafe { &mut *input };
    let ret = input.reserve(size);

    if ret != WPD_OK {
        return ret;
    }
    let at = input.window.buffered();
    let src = unsafe { slice::from_raw_parts(data, size) };
    let dst = unsafe { input.owned() };

    dst[at..at + size].copy_from_slice(src);
    dst[at + size..at + size + FILE_PADDING].fill(0);
    input.window.size += size;
    WPD_OK
}

/// Drops everything before `keep`, if there is enough of it to be worth the
/// move. A borrowed buffer keeps every byte, since holding them costs nothing.
///
/// # Safety
///
/// `input` must point to a live [`InputBuffer`].
#[no_mangle]
pub unsafe extern "C" fn input_compact(input: *mut InputBuffer, keep: usize) {
    let input = unsafe { &mut *input };

    if input.borrowed {
        return;
    }
    let Some(moved) = compact(input.window, keep) else {
        return;
    };
    let from = keep - input.window.discarded;

    unsafe { input.owned() }.copy_within(from..from + moved, 0);
    input.at = input.alloc;
    input.window.discarded = keep;
}

/// # Safety
///
/// `input` must point to a live [`InputBuffer`].
#[no_mangle]
pub unsafe extern "C" fn input_size(input: *const InputBuffer) -> usize {
    unsafe { (*input).window.size }
}

/// # Safety
///
/// As [`input_size`].
#[no_mangle]
pub unsafe extern "C" fn input_discarded(input: *const InputBuffer) -> usize {
    unsafe { (*input).window.discarded }
}

/// # Safety
///
/// As [`input_size`].
#[no_mangle]
pub unsafe extern "C" fn input_buffered(input: *const InputBuffer) -> usize {
    unsafe { (*input).window.buffered() }
}

/// Where `offset` sits in the buffer.
///
/// # Safety
///
/// As [`input_size`]. An offset outside the window has no byte to point at;
/// the pointer this returns for one is past the buffer, exactly as the C's
/// arithmetic was, and every caller has already checked the offset against
/// what the scan reached.
#[no_mangle]
pub unsafe extern "C" fn input_at(
    input: *const InputBuffer,
    offset: usize,
) -> *const u8 {
    let input = unsafe { &*input };

    input
        .at
        .wrapping_add(offset.wrapping_sub(input.window.discarded))
}
