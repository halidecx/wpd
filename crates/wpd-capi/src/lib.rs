//! The C ABI for wpd, as declared by `include/wpd.h`.
//!
//! This crate owns everything that touches caller-supplied raw pointers. The
//! core [`wpd`] crate never stores borrowed memory: this shim keeps the
//! `(pointer, length, stride)` triples the C API hands over and rebuilds the
//! slices per call, which is what confines the lifetime hazard the C ABI
//! cannot express to one place.
//!
//! While the port is in progress the entry points are still implemented in C,
//! compiled by this crate's build script. They move up into safe Rust module
//! by module; see `LOG.md`.

#![deny(unsafe_op_in_unsafe_fn)]

mod cpu;
mod dsp;
mod rescale;
