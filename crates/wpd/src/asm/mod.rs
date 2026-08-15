//! The hand-written assembly boundary.
//!
//! This is the only module in the core crate permitted to use `unsafe`. Every
//! assembly routine is declared here and exposed as a safe function that
//! validates its slices before the call, so nothing outside this module needs
//! to reason about raw pointers.
//!
//! Building without the `asm` feature removes the module entirely, which is
//! what lets the crate compile under `#![forbid(unsafe_code)]`.

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;
