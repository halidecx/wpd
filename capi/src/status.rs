use std::ffi::c_int;

use wpd::error::Error;

pub const WPD_OK: c_int = 0;
pub const WPD_ERR_INVALID_ARG: c_int = -1;
pub const WPD_ERR_NOT_WEBP: c_int = -2;
pub const WPD_ERR_BITSTREAM: c_int = -3;
pub const WPD_ERR_TRUNCATED: c_int = -4;
pub const WPD_ERR_UNSUPPORTED: c_int = -5;
pub const WPD_ERR_NO_MEMORY: c_int = -6;
pub const WPD_ERR_TOO_LARGE: c_int = -7;
pub const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;
pub const WPD_ERR_INTERNAL: c_int = -9;

pub fn status(e: Error) -> c_int {
    match e {
        Error::InvalidArgument => WPD_ERR_INVALID_ARG,
        Error::InvalidData => WPD_ERR_BITSTREAM,
        Error::NoMemory => WPD_ERR_NO_MEMORY,
        Error::TooLarge => WPD_ERR_TOO_LARGE,
        Error::Truncated => WPD_ERR_TRUNCATED,
        Error::NotWebp => WPD_ERR_NOT_WEBP,
        Error::Unsupported => WPD_ERR_UNSUPPORTED,
        Error::BufferTooSmall => WPD_ERR_BUFFER_TOO_SMALL,
    }
}
