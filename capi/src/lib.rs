#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

#[cfg(not(panic = "abort"))]
use std::panic::{catch_unwind, AssertUnwindSafe};

pub mod compat;
pub mod container;
pub mod cpu;
pub mod decoder;
pub mod dsp;
pub mod frame;
pub mod options;
pub mod rescale;

#[cfg(not(panic = "abort"))]
pub(crate) fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            wpd::log::error("internal error: the decoder panicked");
            fallback
        }
    }
}

#[cfg(panic = "abort")]
pub(crate) fn guard<T>(_fallback: T, body: impl FnOnce() -> T) -> T {
    body()
}

#[cfg(all(test, not(panic = "abort")))]
mod tests {
    #[test]
    fn a_panic_becomes_the_fallback_rather_than_the_end_of_the_process() {
        assert_eq!(super::guard(-9, || 7), 7);
        assert_eq!(
            super::guard(-9, || panic!("from inside an entry point")),
            -9
        );
    }
}
