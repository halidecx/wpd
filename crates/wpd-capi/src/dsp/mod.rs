//! C ABI DSP tables.
//!
//! One module per table in `src/*_dsp.h`, each `#[repr(C)]` and laid out
//! field for field like the C header so that `tests/checkasm/*.c` and the
//! not-yet-ported C both keep working unchanged.

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
