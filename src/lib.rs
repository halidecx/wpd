//! A fast WebP decoder.
//!
//! [`api`] is the public interface: a [`Decoder`](api::Decoder) that decodes
//! still and animated WebP into packed RGB(A) or planar YUV(A), with optional
//! cropping, scaling, flipping, streaming input, and threads. The other modules
//! are internals shared with the C API, checkasm, the fuzzers, and the tools;
//! they are hidden from the documentation and carry no stability promise.
//!
//! ```no_run
//! use wpd::api::{Decoder, Format, Options};
//!
//! # fn main() -> wpd::api::Result<()> {
//! # let data: Vec<u8> = Vec::new();
//! let mut decoder = Decoder::new();
//! decoder.set_format(Format::Rgba)?;
//! decoder.set_options(Options { scale: Some((320, 0)), ..Default::default() })?;
//! decoder.open(&data)?;
//! while let Some(frame) = decoder.next_frame()? {
//!     for row in frame.rows_of(0) {
//!         let _ = row;
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "asm"), forbid(unsafe_code))]
#![cfg_attr(feature = "asm", deny(unsafe_code))]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(improper_ctypes)]
#![warn(clippy::all)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]
#![warn(missing_docs)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::wildcard_imports)]

#[cfg(feature = "asm")]
#[allow(unsafe_code)]
#[doc(hidden)]
pub mod asm;

#[doc(hidden)]
pub mod anim;
pub mod api;
#[doc(hidden)]
pub mod bits;
#[doc(hidden)]
pub mod blit;
#[doc(hidden)]
pub mod compose;
#[doc(hidden)]
pub mod container;
#[doc(hidden)]
pub mod convert;
#[doc(hidden)]
pub mod cpu;
#[doc(hidden)]
pub mod driver;
#[doc(hidden)]
pub mod dsp;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod handout;
#[doc(hidden)]
pub mod image;
#[doc(hidden)]
pub mod info;
#[doc(hidden)]
pub mod input;
#[doc(hidden)]
pub mod log;
#[doc(hidden)]
pub mod options;
#[doc(hidden)]
pub mod picture;
#[doc(hidden)]
pub mod rescale;
#[doc(hidden)]
pub mod task;
#[doc(hidden)]
pub mod vp8;
#[doc(hidden)]
pub mod vp8l;
