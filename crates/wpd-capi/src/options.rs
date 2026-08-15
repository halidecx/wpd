//! `WPDDecoderOptions`.
//!
//! The versioned C struct a caller asks for cropping, scaling and flipping
//! with, and the one thing it does that the decoder's own options cannot: say
//! a field is present with a `use_` flag beside it rather than with an
//! `Option`.

use std::ffi::c_int;
use std::mem;

use wpd::options::Options;

/// `WPDDecoderOptions` from `include/wpd.h`.
#[repr(C)]
pub struct WPDDecoderOptions {
    pub struct_size: usize,
    pub bypass_filtering: c_int,
    pub no_fancy_upsampling: c_int,
    pub use_cropping: c_int,
    pub crop_left: c_int,
    pub crop_top: c_int,
    pub crop_width: c_int,
    pub crop_height: c_int,
    pub use_scaling: c_int,
    pub scaled_width: c_int,
    pub scaled_height: c_int,
    pub flip: c_int,
}

impl WPDDecoderOptions {
    /// The oldest revision this build accepts, and equally how much of a
    /// caller's struct it reads.
    pub(crate) fn v1() -> usize {
        mem::offset_of!(WPDDecoderOptions, flip) + mem::size_of::<c_int>()
    }

    /// The versioned C struct as the decoder's own options.
    ///
    /// Read field by field rather than copied whole: the caller's struct may
    /// be a shorter revision than this build's, and its `struct_size` is not
    /// ours to keep. The `use_` flags become the `Option`s they were standing
    /// in for.
    pub(crate) fn to_core(&self) -> Options {
        Options {
            bypass_filtering: self.bypass_filtering != 0,
            no_fancy_upsampling: self.no_fancy_upsampling != 0,
            crop: (self.use_cropping != 0).then_some((
                self.crop_left,
                self.crop_top,
                self.crop_width,
                self.crop_height,
            )),
            scale: (self.use_scaling != 0)
                .then_some((self.scaled_width, self.scaled_height)),
            flip: self.flip != 0,
        }
    }
}
