//! C ABI DSP tables.
//!
//! One module per table in `src/*_dsp.h`, each `#[repr(C)]` and laid out
//! field for field like the C header so that `tests/checkasm/*.c` and the
//! not-yet-ported C both keep working unchanged.

use std::ffi::c_int;

/// The table's counts come in signed, and the assembly entries run a loop that
/// falls straight through when the count is not positive. A cast would instead
/// turn a negative one into a slice length no allocation can back, so the
/// trampolines answer it the same way the assembly does: with nothing.
fn count(n: c_int) -> Option<usize> {
    usize::try_from(n).ok()
}

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;
