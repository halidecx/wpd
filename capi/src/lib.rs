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

use std::panic::{catch_unwind, AssertUnwindSafe};

pub mod compat;
pub mod container;
pub mod cpu;
pub mod decoder;
pub mod dsp;
pub mod frame;
pub mod options;
pub mod rescale;

/// Runs an entry point's body with a panic turned into `fallback` rather than
/// into the end of the caller's process.
///
/// A panic here is a defect: every geometry a bitstream can name is checked
/// before it indexes anything, and the fuzzers exist to keep that true. But
/// this library is linked into programs that decode what strangers send them,
/// and a defect that fails one call is a different thing from a defect that
/// takes the process down with it -- the second is the caller's outage and
/// they cannot even catch it. So the release profile no longer aborts on
/// panic and nothing unwinds out of `extern "C"`, which would be undefined
/// besides.
///
/// The entry points that only read a constant or a static string are not
/// wrapped: there is nothing in them to go wrong, and a `catch_unwind` that
/// can never fire says the opposite of what it means.
pub(crate) fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            wpd::log::error("internal error: the decoder panicked");
            fallback
        }
    }
}

#[cfg(test)]
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
