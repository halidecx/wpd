//! What a decode can go wrong with, and how it can stop short.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    InvalidData,
    NoMemory,
    TooLarge,
    /// The input stops inside something the decoder was still reading.
    Truncated,
    /// The input is not a WebP file at all, rather than a damaged one.
    NotWebp,
}

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
