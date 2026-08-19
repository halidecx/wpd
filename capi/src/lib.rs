//! The C ABI for wpd, as declared by `include/wpd.h`.
//!
//! This crate owns everything that touches caller-supplied raw pointers. The
//! core [`wpd`] crate never stores borrowed memory: this shim keeps the
//! `(pointer, length, stride)` triples the C API hands over and rebuilds the
//! slices per call, which is what confines the lifetime hazard the C ABI
//! cannot express to one place.
//!
//! No decoding happens here. Every entry point `include/wpd.h` declares is
//! the same three steps -- check what only a raw pointer can get wrong, ask
//! [`wpd::driver`], and fill in a versioned struct -- and the safe Rust API is
//! [`wpd::api`], which does not go through this crate at all. See `LOG.md`.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

pub mod compat;
pub mod container;
pub mod cpu;
pub mod decoder;
pub mod dsp;
pub mod frame;
pub mod options;
pub mod rescale;
