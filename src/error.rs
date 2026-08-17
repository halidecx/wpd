//! What a decode can go wrong with, and how it can stop short.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    /// What the caller asked for is not something that can be asked.
    InvalidArgument,
    InvalidData,
    NoMemory,
    TooLarge,
    /// The input stops inside something the decoder was still reading.
    Truncated,
    /// The input is not a WebP file at all, rather than a damaged one.
    NotWebp,
    /// The file is well-formed but uses something this decoder does not do.
    Unsupported,
    /// A destination the caller supplied has no room for what was decoded.
    BufferTooSmall,
}

impl Error {
    /// A one-line description, which is what `wpd_status_string` hands out for
    /// the status this failure crosses the C ABI as.
    pub fn message(self) -> &'static str {
        match self {
            Error::InvalidArgument => "invalid argument",
            Error::InvalidData => "invalid bitstream",
            Error::NoMemory => "out of memory",
            Error::TooLarge => "image too large",
            Error::Truncated => "truncated file",
            Error::NotWebp => "not a WebP file",
            Error::Unsupported => "unsupported feature",
            Error::BufferTooSmall => "output buffer too small",
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

/// What a decode call stopped on: either the work asked for is finished, or
/// the chunk ran out part way and the caller should append more and resume.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Status {
    Done,
    NeedMore,
}

pub type Result<T> = core::result::Result<T, Error>;

/// The limit `wpd_check_image_size` puts on a picture, in either direction.
pub const MAX_DIMENSION: i32 = 16384;

pub fn check_image_size(width: i32, height: i32) -> Result<()> {
    if width <= 0 || height <= 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::TooLarge);
    }
    Ok(())
}
