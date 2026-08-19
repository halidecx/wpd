//! The hand-written assembly boundary.
//!
//! This is the only module in the core crate permitted to use `unsafe`. Every
//! assembly routine is declared here and exposed as a safe function that
//! validates its slices before the call, so nothing outside this module needs
//! to reason about raw pointers.
//!
//! Building without the `asm` feature removes the module entirely, which is
//! what lets the crate compile under `#![forbid(unsafe_code)]`.
//!
//! # Dispatch tables
//!
//! Each architecture is described by one `ladder!` invocation: a list of the
//! kernels a CPU tier adds, from which both of the tables that tier feeds are
//! generated. The decoder's table holds the safe wrapper and the C ABI's holds
//! the bare symbol — checkasm has to time the assembly and not a wrapper —
//! which is the whole reason there are two. What they have to agree on is
//! which slots a tier fills, and nothing compares them: checkasm exercises
//! only the raw table and the decode tests only the safe one, so a kernel
//! added to one list and forgotten in the other passes every gate as a silent
//! regression. One list is what makes that unrepresentable.

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;
