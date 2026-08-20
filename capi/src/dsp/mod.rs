use std::ffi::c_int;

fn count(n: c_int) -> Option<usize> {
    usize::try_from(n).ok()
}

pub mod filters;
pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;
