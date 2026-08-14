//! Scalar DSP kernels.
//!
//! Each submodule holds the safe Rust fallbacks for one of the decoder's DSP
//! tables. They are the reference implementations: the hand-written assembly
//! replaces them at runtime where the CPU allows, and checkasm compares the
//! two.

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
