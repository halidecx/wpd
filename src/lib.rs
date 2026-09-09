#![cfg_attr(not(feature = "asm"), forbid(unsafe_code))]
#![cfg_attr(feature = "asm", deny(unsafe_code))]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(improper_ctypes)]
#![warn(clippy::all)]

#[cfg(feature = "asm")]
#[allow(unsafe_code)]
pub mod asm;

pub mod anim;
pub mod api;
pub mod bits;
pub mod blit;
pub mod compose;
pub mod container;
pub mod convert;
pub mod cpu;
pub mod driver;
pub mod dsp;
pub mod error;
pub mod handout;
pub mod image;
pub mod info;
pub mod input;
pub mod log;
pub mod options;
pub mod picture;
pub mod rescale;
pub mod task;
pub mod vp8;
pub mod vp8l;
