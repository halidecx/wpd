//! wpd — a fast WebP decoder.
//!
//! This crate is the decoder core, ported module by module from the C the
//! project started as; see `LOG.md` at the repository root.
//!
//! # Memory safety
//!
//! Without the `asm` feature the crate contains no `unsafe` at all, enforced by
//! `#![forbid(unsafe_code)]`. With `asm` enabled, `unsafe` is confined to the
//! [`asm`] module, which declares the hand-written assembly symbols and wraps
//! each one in a safe function that validates the slices first.

#![cfg_attr(not(feature = "asm"), forbid(unsafe_code))]
#![cfg_attr(feature = "asm", deny(unsafe_code))]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

#[cfg(feature = "asm")]
#[allow(unsafe_code)]
pub mod asm;

pub mod anim;
pub mod container;
pub mod cpu;
pub mod dsp;
pub mod error;
pub mod image;
pub mod input;
pub mod log;
pub mod rescale;
pub mod vp8;
pub mod vp8l;
