//! Diagnostics on the way out of the decoder.
//!
//! The core has no idea how a consumer wants to report anything, so it hands
//! messages to a sink installed once at startup. `wpd-capi` installs one that
//! forwards to the `WPDLogCallback` the public header documents; a pure-Rust
//! consumer can install its own.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
}

static SINK: OnceLock<fn(Level, &str)> = OnceLock::new();

/// Installs the sink. Only the first call has any effect, which is what makes
/// this safe to read from any thread without a lock.
pub fn set_sink(sink: fn(Level, &str)) {
    let _ = SINK.set(sink);
}

pub fn log(level: Level, message: &str) {
    if let Some(sink) = SINK.get() {
        sink(level, message);
    }
}

pub fn error(message: &str) {
    log(Level::Error, message);
}

pub fn warning(message: &str) {
    log(Level::Warning, message);
}
