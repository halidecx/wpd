use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::filters::FilterDsp;

pub(crate) use super::Raw;

/* A null previous row marks the top row; the assembly left-predicts it. */
pub type UnfilterRaw = unsafe extern "C" fn(*const u8, *mut u8, c_int);

macro_rules! raw_unfilter {
    ($marker:ident, $inner:ident, $sym:literal) => {
        raw!(
            $marker,
            $inner,
            UnfilterRaw,
            $sym,
            (*const u8, *mut u8, c_int)
        );
    };
}

fn unfilter<T: Raw<Sig = UnfilterRaw>>(prev: Option<&[u8]>, row: &mut [u8]) {
    if let Some(prev) = prev {
        assert!(prev.len() >= row.len(), "short previous row");
    }
    unsafe {
        (T::F)(
            prev.map_or(std::ptr::null(), <[u8]>::as_ptr),
            row.as_mut_ptr(),
            row.len() as c_int,
        )
    }
}

#[derive(Default)]
pub struct RawTable {
    pub horizontal_unfilter: Option<UnfilterRaw>,
    pub vertical_unfilter: Option<UnfilterRaw>,
    pub gradient_unfilter: Option<UnfilterRaw>,
}

macro_rules! ladder {
    ($(
        $flag:ident {
            $( $field:ident = $wrap:ident::<$marker:path>; )*
        }
    )*) => {
        pub fn init(dsp: &mut FilterDsp, flags: CpuFlags) {
            $(
                if flags.contains(CpuFlags::$flag) {
                    $( dsp.$field = $wrap::<$marker>; )*
                }
            )*
        }

        pub fn raw_table(flags: CpuFlags) -> RawTable {
            let mut t = RawTable::default();

            $(
                if flags.contains(CpuFlags::$flag) {
                    $( t.$field = Some(<$marker as Raw>::F); )*
                }
            )*
            t
        }
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;

    pub mod sse2 {
        use super::*;

        raw_unfilter!(Horizontal, horizontal, "ff_horizontal_unfilter_sse2");
        raw_unfilter!(Vertical, vertical, "ff_vertical_unfilter_sse2");
        raw_unfilter!(Gradient, gradient, "ff_gradient_unfilter_sse2");
    }

    pub mod avx2 {
        use super::*;

        raw_unfilter!(Vertical, vertical, "ff_vertical_unfilter_avx2");
        raw_unfilter!(Gradient, gradient, "ff_gradient_unfilter_avx2");
    }

    ladder! {
        SSE2 {
            horizontal_unfilter = unfilter::<sse2::Horizontal>;
            vertical_unfilter = unfilter::<sse2::Vertical>;
            gradient_unfilter = unfilter::<sse2::Gradient>;
        }
        AVX2 {
            vertical_unfilter = unfilter::<avx2::Vertical>;
            gradient_unfilter = unfilter::<avx2::Gradient>;
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
mod arch {
    use super::*;

    pub fn init(_dsp: &mut FilterDsp, _flags: CpuFlags) {}

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
