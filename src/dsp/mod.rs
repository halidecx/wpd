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

/// Saturates a signed intermediate back into a sample.
///
/// This is `v.clamp(0, 255)`, which `const fn` cannot call, written as the two
/// separate bounds it is. Keep the two steps sequential: folding them into one
/// `else if` chain costs the lossy scalar kernels a few percent, because the
/// branchy form is what reaches the back end rather than a pair of selects.
#[inline(always)]
pub(crate) const fn clip_uint8(v: i32) -> u8 {
    let lo = if v < 0 { 0 } else { v };

    (if lo > 255 { 255 } else { lo }) as u8
}
