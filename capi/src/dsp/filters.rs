use std::ffi::c_int;
use std::slice;

use super::count;
use wpd::dsp::filters as k;

/* A null prev marks the top row, which is left-predicted whatever the mode. */
pub type UnfilterFn = unsafe extern "C" fn(*const u8, *mut u8, c_int);

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
pub struct WPDFILTERSDSP {
    pub horizontal_unfilter: UnfilterFn,
    pub vertical_unfilter: UnfilterFn,
    pub gradient_unfilter: UnfilterFn,
}

macro_rules! unfilter_tramp {
    ($name:ident, $k:path) => {
        unsafe extern "C" fn $name(prev: *const u8, row: *mut u8, width: c_int) {
            let Some(width) = count(width) else {
                return;
            };

            unsafe {
                $k(
                    (!prev.is_null()).then(|| slice::from_raw_parts(prev, width)),
                    slice::from_raw_parts_mut(row, width),
                )
            }
        }
    };
}

unfilter_tramp!(horizontal_unfilter_c, k::horizontal_unfilter);
unfilter_tramp!(vertical_unfilter_c, k::vertical_unfilter);
unfilter_tramp!(gradient_unfilter_c, k::gradient_unfilter);

#[cfg(all(
    feature = "asm",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
fn init_asm(dsp: &mut WPDFILTERSDSP) {
    let t = wpd::asm::filters::raw_table(wpd::cpu::flags());

    if let Some(v) = t.horizontal_unfilter {
        dsp.horizontal_unfilter = v;
    }
    if let Some(v) = t.vertical_unfilter {
        dsp.vertical_unfilter = v;
    }
    if let Some(v) = t.gradient_unfilter {
        dsp.gradient_unfilter = v;
    }
}

impl WPDFILTERSDSP {
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = WPDFILTERSDSP {
            horizontal_unfilter: horizontal_unfilter_c,
            vertical_unfilter: vertical_unfilter_c,
            gradient_unfilter: gradient_unfilter_c,
        };

        #[cfg(all(
            feature = "asm",
            any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
        ))]
        init_asm(&mut table);

        table
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_filters_dsp_init(dsp: *mut WPDFILTERSDSP) {
    unsafe { dsp.write(WPDFILTERSDSP::new()) }
}
