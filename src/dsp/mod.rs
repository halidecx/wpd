//! Scalar DSP kernels.
//!
//! Each submodule holds the safe Rust fallbacks for one of the decoder's DSP
//! tables. They are the reference implementations: the hand-written assembly
//! replaces them at runtime where the CPU allows, and checkasm compares the
//! two.

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;

/// Saturates a signed intermediate back into a sample. Spelled out rather than
/// with `clamp`, which is not usable from the `const fn` colour kernels.
#[inline(always)]
pub(crate) const fn clip_uint8(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}
