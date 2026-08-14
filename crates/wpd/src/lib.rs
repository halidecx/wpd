//! wpd — a fast WebP decoder.
//!
//! This crate is the decoder core. It is being ported from C module by module;
//! see `LOG.md` at the repository root for the current state.
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

pub mod cpu;
