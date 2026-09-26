use std::cell::RefCell;
use std::fmt::{self, Write};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
}

static SINK: OnceLock<fn(Level, &str)> = OnceLock::new();

thread_local! {
    /// Where this thread's messages go instead of the sink while `hold` runs.
    static HELD: RefCell<Option<Vec<(Level, String)>>> = const { RefCell::new(None) };
}

/// Messages held back by `hold`, to be passed on to the sink by `replay`.
#[derive(Default)]
pub(crate) struct Held(Vec<(Level, String)>);

impl Held {
    /// Passes the messages on, in the order they were logged.
    pub(crate) fn replay(self) {
        for (level, message) in self.0 {
            log(level, &message);
        }
    }
}

/// Runs `f`, holding back what it logs on this thread rather than passing it
/// to the sink. A decode that may still be running once the call that started
/// it has returned uses this, so that its messages reach the sink only from a
/// call, where the caller expects them.
pub(crate) fn hold<R>(f: impl FnOnce() -> R) -> (R, Held) {
    /* Put back on the way out, a panic included, so that nothing else this
     * thread logs goes astray. */
    struct Restore(Option<Vec<(Level, String)>>);

    impl Drop for Restore {
        fn drop(&mut self) {
            let outer = self.0.take();

            HELD.with(|held| *held.borrow_mut() = outer);
        }
    }

    let restore = Restore(HELD.with(|held| held.replace(Some(Vec::new()))));
    let out = f();
    let messages = HELD.with(|held| held.take()).unwrap_or_default();

    drop(restore);
    (out, Held(messages))
}

/// Adds `message` to the ones held back, or drops it if there is no memory
/// to keep it in.
fn keep(held: &mut Vec<(Level, String)>, level: Level, message: &str) {
    let mut copy = String::new();

    if held.try_reserve(1).is_ok() && copy.try_reserve_exact(message.len()).is_ok() {
        copy.push_str(message);
        held.push((level, copy));
    }
}

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

pub fn set_sink(sink: fn(Level, &str)) {
    let _ = SINK.set(sink);
}

pub fn log(level: Level, message: &str) {
    let Some(sink) = SINK.get() else {
        return;
    };
    let held = HELD.with(|held| match held.borrow_mut().as_mut() {
        Some(held) => {
            keep(held, level, message);
            true
        }
        None => false,
    });

    if !held {
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
