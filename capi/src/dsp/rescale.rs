use std::ffi::c_int;
use std::slice;

use super::count;
use wpd::dsp::rescale as k;
use wpd::dsp::rescale::{ExportExpand, ExportShrink, Import};

pub type ImportRowFn =
    unsafe extern "C" fn(*mut u32, *const u8, c_int, c_int, c_int, u32, u32, u32);
pub type ExportExpandRowFn =
    unsafe extern "C" fn(*mut u8, *const u32, *const u32, c_int, c_int, u32, u32);
pub type ExportShrinkRowFn =
    unsafe extern "C" fn(*mut u8, *mut u32, *const u32, c_int, c_int, u32, u32);

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
pub struct WPDRESCALEDSP {
    pub import_row_expand: ImportRowFn,
    pub import_row_shrink: ImportRowFn,
    pub export_row_expand: ExportExpandRowFn,
    pub export_row_shrink: ExportShrinkRowFn,
}

macro_rules! import_tramp {
    ($name:ident, $f:path) => {
        unsafe extern "C" fn $name(
            frow: *mut u32,
            src: *const u8,
            dst_width: c_int,
            src_width: c_int,
            num_channels: c_int,
            x_add: u32,
            x_sub: u32,
            fx_scale: u32,
        ) {
            let (Some(dw), Some(sw), Some(ch)) =
                (count(dst_width), count(src_width), count(num_channels))
            else {
                return;
            };
            /* An empty source row has nothing to resample from. An empty
             * destination one is the kernels' own business: guarding it
             * here would leave their entry tests ungated. */
            if sw == 0 || ch == 0 {
                return;
            }
            let p = Import {
                num_channels: ch,
                src_width: sw,
                dst_width: dw,
                x_add,
                x_sub,
                fx_scale,
            };

            unsafe {
                $f(
                    slice::from_raw_parts_mut(frow, dw * ch),
                    slice::from_raw_parts(src, sw * ch),
                    p,
                )
            }
        }
    };
}

/* Only the row a kernel actually reads becomes a slice: the other pointer
 * is free to be null, as it is on the pass that ignores it. */
macro_rules! export_expand_tramp {
    ($name:ident, $f:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            irow: *const u32,
            frow: *const u32,
            width: c_int,
            y_accum: c_int,
            y_sub: u32,
            fy_scale: u32,
        ) {
            let Some(n) = count(width) else {
                return;
            };
            let p = ExportExpand {
                y_accum,
                y_sub,
                fy_scale,
            };

            unsafe {
                $f(
                    slice::from_raw_parts_mut(dst, n),
                    match p.blend() {
                        Some(_) => slice::from_raw_parts(irow, n),
                        None => &[],
                    },
                    slice::from_raw_parts(frow, n),
                    p,
                )
            }
        }
    };
}

macro_rules! export_shrink_tramp {
    ($name:ident, $f:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            irow: *mut u32,
            frow: *const u32,
            width: c_int,
            y_accum: c_int,
            fy_scale: u32,
            fxy_scale: u32,
        ) {
            let Some(n) = count(width) else {
                return;
            };
            let p = ExportShrink {
                y_accum,
                fy_scale,
                fxy_scale,
            };

            unsafe {
                $f(
                    slice::from_raw_parts_mut(dst, n),
                    slice::from_raw_parts_mut(irow, n),
                    match p.yscale() {
                        0 => &[],
                        _ => slice::from_raw_parts(frow, n),
                    },
                    p,
                )
            }
        }
    };
}

import_tramp!(import_row_expand_c, k::import_row_expand);
import_tramp!(import_row_shrink_c, k::import_row_shrink);
export_expand_tramp!(export_row_expand_c, k::export_row_expand);
export_shrink_tramp!(export_row_shrink_c, k::export_row_shrink);

#[cfg(all(feature = "asm", target_arch = "x86_64"))]
mod asm {
    use super::*;
    use wpd::asm::rescale::Kernel;

    import_tramp!(
        import_row_expand_sse2_c,
        wpd::asm::rescale::import_row_expand_sse2
    );
    import_tramp!(
        import_row_shrink_sse2_c,
        wpd::asm::rescale::import_row_shrink_sse2
    );
    export_expand_tramp!(
        export_row_expand_sse2_c,
        wpd::asm::rescale::export_row_expand_sse2
    );
    export_shrink_tramp!(
        export_row_shrink_sse2_c,
        wpd::asm::rescale::export_row_shrink_sse2
    );
    export_expand_tramp!(
        export_row_expand_avx2_c,
        wpd::asm::rescale::export_row_expand_avx2
    );
    export_shrink_tramp!(
        export_row_shrink_avx2_c,
        wpd::asm::rescale::export_row_shrink_avx2
    );

    pub fn init(dsp: &mut WPDRESCALEDSP) {
        let s = wpd::asm::rescale::selection(wpd::cpu::flags());

        dsp.import_row_expand = match s.import_row_expand {
            Kernel::Sse2 => import_row_expand_sse2_c,
            _ => dsp.import_row_expand,
        };
        dsp.import_row_shrink = match s.import_row_shrink {
            Kernel::Sse2 => import_row_shrink_sse2_c,
            _ => dsp.import_row_shrink,
        };
        dsp.export_row_expand = match s.export_row_expand {
            Kernel::Sse2 => export_row_expand_sse2_c,
            Kernel::Avx2 => export_row_expand_avx2_c,
            _ => dsp.export_row_expand,
        };
        dsp.export_row_shrink = match s.export_row_shrink {
            Kernel::Sse2 => export_row_shrink_sse2_c,
            Kernel::Avx2 => export_row_shrink_avx2_c,
            _ => dsp.export_row_shrink,
        };
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;
    use wpd::asm::rescale::Kernel;

    import_tramp!(
        import_row_expand_neon_c,
        wpd::asm::rescale::import_row_expand_neon
    );
    import_tramp!(
        import_row_shrink_neon_c,
        wpd::asm::rescale::import_row_shrink_neon
    );
    export_expand_tramp!(
        export_row_expand_neon_c,
        wpd::asm::rescale::export_row_expand_neon
    );
    export_shrink_tramp!(
        export_row_shrink_neon_c,
        wpd::asm::rescale::export_row_shrink_neon
    );

    pub fn init(dsp: &mut WPDRESCALEDSP) {
        let s = wpd::asm::rescale::selection(wpd::cpu::flags());

        dsp.import_row_expand = match s.import_row_expand {
            Kernel::Neon => import_row_expand_neon_c,
            _ => dsp.import_row_expand,
        };
        dsp.import_row_shrink = match s.import_row_shrink {
            Kernel::Neon => import_row_shrink_neon_c,
            _ => dsp.import_row_shrink,
        };
        dsp.export_row_expand = match s.export_row_expand {
            Kernel::Neon => export_row_expand_neon_c,
            _ => dsp.export_row_expand,
        };
        dsp.export_row_shrink = match s.export_row_shrink {
            Kernel::Neon => export_row_shrink_neon_c,
            _ => dsp.export_row_shrink,
        };
    }
}

impl WPDRESCALEDSP {
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = WPDRESCALEDSP {
            import_row_expand: import_row_expand_c,
            import_row_shrink: import_row_shrink_c,
            export_row_expand: export_row_expand_c,
            export_row_shrink: export_row_shrink_c,
        };

        #[cfg(all(
            feature = "asm",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        asm::init(&mut table);

        table
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_rescale_dsp_init(dsp: *mut WPDRESCALEDSP) {
    unsafe { dsp.write(WPDRESCALEDSP::new()) }
}
