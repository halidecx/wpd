//! Diagnostics on the way out of the decoder.
//!
//! The core has no idea how a consumer wants to report anything, so it hands
//! messages to a sink installed once at startup. `wpd-capi` installs one that
//! forwards to the `WPDLogCallback` the public header documents; a pure-Rust
//! consumer can install its own.

use std::fmt::{self, Write};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
}

static SINK: OnceLock<fn(Level, &str)> = OnceLock::new();

struct Message {
    bytes: [u8; 512],
    len: usize,
}

impl Write for Message {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let available = self.bytes.len() - self.len;
        let mut len = text.len().min(available);

        while !text.is_char_boundary(len) {
            len -= 1;
        }
        self.bytes[self.len..self.len + len].copy_from_slice(&text.as_bytes()[..len]);
        self.len += len;
        Ok(())
    }
}

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

pub fn error_args(args: fmt::Arguments<'_>) {
    log_args(Level::Error, args);
}

pub fn warning_args(args: fmt::Arguments<'_>) {
    log_args(Level::Warning, args);
}

fn log_args(level: Level, args: fmt::Arguments<'_>) {
    let mut message = Message {
        bytes: [0; 512],
        len: 0,
    };

    let _ = message.write_fmt(args);
    if let Ok(message) = std::str::from_utf8(&message.bytes[..message.len]) {
        log(level, message);
    }
}
