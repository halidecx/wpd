use std::ffi::c_int;
use std::mem;

use wpd::options::Options;

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
    /// Takes the tail padding the v1 struct had, so that every field after it
    /// begins past the size a v1 caller can present. See `V1_SIZE`.
    pub reserved: c_int,
    pub n_threads: c_int,
}

/// What `sizeof` gave the v1 struct: it ended at `flip`, and padded out to the
/// alignment this struct still has.
const V1_SIZE: usize =
    WPDDecoderOptions::v1().next_multiple_of(mem::align_of::<WPDDecoderOptions>());

/// A field appended into the v1 struct's tail padding would extend no further
/// than `sizeof` already reached, and `struct_size` could not see it. Every
/// version gate below has to sit past that.
const _: () = assert!(V1_SIZE < WPDDecoderOptions::v2());

impl WPDDecoderOptions {
    pub(crate) const fn v1() -> usize {
        mem::offset_of!(WPDDecoderOptions, flip) + mem::size_of::<c_int>()
    }

    const fn v2() -> usize {
        mem::offset_of!(WPDDecoderOptions, n_threads) + mem::size_of::<c_int>()
    }

    /// Legacy callers retain serial decoding and serial log callbacks.
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
            n_threads: if self.struct_size >= Self::v2() {
                self.n_threads
            } else {
                1
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_options_keep_callbacks_on_the_calling_thread() {
        let mut options: WPDDecoderOptions = unsafe { mem::zeroed() };

        options.struct_size = V1_SIZE;
        options.n_threads = 8;
        assert_eq!(options.to_core().n_threads, 1);
        options.struct_size = mem::size_of::<WPDDecoderOptions>();
        assert_eq!(options.to_core().n_threads, 8);
    }
}
