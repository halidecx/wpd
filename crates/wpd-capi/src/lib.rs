//! The C ABI for wpd, as declared by `include/wpd.h`.
//!
//! This crate owns everything that touches caller-supplied raw pointers. The
//! core [`wpd`] crate never stores borrowed memory: this shim keeps the
//! `(pointer, length, stride)` triples the C API hands over and rebuilds the
//! slices per call, which is what confines the lifetime hazard the C ABI
//! cannot express to one place.
//!
//! Every entry point `include/wpd.h` declares is implemented here. The port is
//! complete: no decoder logic is compiled from C any more, and what is left of
//! the build script is the target probing and the x86 assembly's constant
//! tables. See `LOG.md`.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

pub mod anim;
pub mod api;
pub mod compat;
pub mod container;
pub mod convert;
pub mod cpu;
pub mod decoder;
pub mod dsp;
pub mod export;
pub mod lossy;
pub mod rescale;
