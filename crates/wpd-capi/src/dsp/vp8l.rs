//! C ABI for the lossless DSP table, as declared by `src/vp8l_dsp.h`.
//!
//! The assembly entries are the raw symbols, so `checkasm --bench` measures
//! the assembly and nothing else. The fallback entries are thin trampolines
//! that rebuild slices from the caller's pointers and hand them to the safe
//! kernels in [`wpd::dsp::vp8l`]; that cost lands on checkasm's reference
//! side, where it affects neither correctness nor the benchmark.

use std::ffi::c_int;
use std::slice;

use wpd::dsp::vp8l as k;

pub type PredAddFn = unsafe extern "C" fn(*const u32, *const u32, c_int, *mut u32);
pub type RowFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type MapColorFn = unsafe extern "C" fn(*mut u8, *const u8, *const u32, c_int);

pub const PRED_COUNT: usize = 14;

#[repr(C)]
pub struct WPDLosslessDSP {
    pub pred_add: [PredAddFn; PRED_COUNT],
    pub extract_green: RowFn,
    pub map_color32: MapColorFn,
    pub blend_row_argb: RowFn,
    pub blend_row_argb_premult: RowFn,
}

unsafe extern "C" fn pred_add_0_c(
    inp: *const u32,
    _upper: *const u32,
    n: c_int,
    out: *mut u32,
) {
    debug_assert_eq!(inp, out.cast_const());
    unsafe { k::pred_add_0(slice::from_raw_parts_mut(out, n as usize)) }
}

unsafe extern "C" fn pred_add_1_c(
    inp: *const u32,
    _upper: *const u32,
    n: c_int,
    out: *mut u32,
) {
    debug_assert_eq!(inp, out.cast_const());
    let n = n as usize;
    if n == 0 {
        return;
    }
    unsafe {
        let left = *out.sub(1);
        k::pred_add_1(slice::from_raw_parts_mut(out, n), left);
    }
}

/// Wraps a kernel that reads `upper`. The three flags say which out-of-row
/// neighbours the predictor actually needs, because reading one it does not —
/// `upper[-1]` on the first row of the image, say — would step outside the
/// buffer.
macro_rules! pred_tramp {
    ($name:ident, $kernel:ident, $l:literal, $tl:literal, $tr:literal) => {
        unsafe extern "C" fn $name(
            inp: *const u32,
            upper: *const u32,
            n: c_int,
            out: *mut u32,
        ) {
            debug_assert_eq!(inp, out.cast_const());
            let n = n as usize;
            if n == 0 {
                return;
            }
            unsafe {
                let left = if $l { *out.sub(1) } else { 0 };
                let top_left = if $tl { *upper.sub(1) } else { 0 };
                k::$kernel(
                    slice::from_raw_parts_mut(out, n),
                    slice::from_raw_parts(upper, n + $tr as usize),
                    left,
                    top_left,
                );
            }
        }
    };
}

pred_tramp!(pred_add_2_c, pred_add_2, false, false, false);
pred_tramp!(pred_add_3_c, pred_add_3, false, false, true);
pred_tramp!(pred_add_4_c, pred_add_4, false, true, false);
pred_tramp!(pred_add_5_c, pred_add_5, true, false, true);
pred_tramp!(pred_add_6_c, pred_add_6, true, true, false);
pred_tramp!(pred_add_7_c, pred_add_7, true, false, false);
pred_tramp!(pred_add_8_c, pred_add_8, false, true, false);
pred_tramp!(pred_add_9_c, pred_add_9, false, false, true);
pred_tramp!(pred_add_10_c, pred_add_10, true, true, true);
pred_tramp!(pred_add_11_c, pred_add_11, true, true, false);
pred_tramp!(pred_add_12_c, pred_add_12, true, true, false);
pred_tramp!(pred_add_13_c, pred_add_13, true, true, false);

unsafe extern "C" fn extract_green_c(dst: *mut u8, src: *const u8, n: c_int) {
    let n = n as usize;
    unsafe {
        k::extract_green(
            slice::from_raw_parts_mut(dst, n),
            slice::from_raw_parts(src, 4 * n),
        )
    }
}

unsafe extern "C" fn map_color32_c(
    dst: *mut u8,
    src: *const u8,
    palette: *const u32,
    n: c_int,
) {
    let n = 4 * n as usize;
    unsafe {
        let palette = slice::from_raw_parts(palette, 256);
        if dst.cast_const() == src {
            k::map_color32_inplace(slice::from_raw_parts_mut(dst, n), palette);
        } else {
            k::map_color32(
                slice::from_raw_parts_mut(dst, n),
                slice::from_raw_parts(src, n),
                palette,
            );
        }
    }
}

unsafe extern "C" fn blend_row_argb_c(dst: *mut u8, src: *const u8, n: c_int) {
    let n = 4 * n as usize;
    unsafe {
        k::blend_row_argb(
            slice::from_raw_parts_mut(dst, n),
            slice::from_raw_parts(src, n),
        )
    }
}

unsafe extern "C" fn blend_row_argb_premult_c(dst: *mut u8, src: *const u8, n: c_int) {
    let n = 4 * n as usize;
    unsafe {
        k::blend_row_argb_premult(
            slice::from_raw_parts_mut(dst, n),
            slice::from_raw_parts(src, n),
        )
    }
}

#[cfg(all(feature = "asm", any(target_arch = "x86", target_arch = "x86_64")))]
mod asm {
    use super::*;

    extern "C" {
        pub fn ff_pred_add_0_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_1_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_2_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_3_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_4_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_5_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_6_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_7_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_8_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_9_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_10_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_11_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_12_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_13_avx2(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_extract_green_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_map_color32_avx2(
            dst: *mut u8,
            src: *const u8,
            palette: *const u32,
            n: c_int,
        );
        pub fn ff_blend_row_argb_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_blend_row_argb_premult_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_blend_row_argb_premult_avx2(dst: *mut u8, src: *const u8, n: c_int);
    }

    pub fn init(dsp: &mut WPDLosslessDSP) {
        let flags = wpd::cpu::flags();

        if flags.contains(wpd::cpu::CpuFlags::SSSE3) {
            dsp.blend_row_argb_premult = ff_blend_row_argb_premult_ssse3;
        }

        if flags.contains(wpd::cpu::CpuFlags::AVX2) {
            dsp.pred_add = [
                ff_pred_add_0_avx2,
                ff_pred_add_1_avx2,
                ff_pred_add_2_avx2,
                ff_pred_add_3_avx2,
                ff_pred_add_4_avx2,
                ff_pred_add_5_avx2,
                ff_pred_add_6_avx2,
                ff_pred_add_7_avx2,
                ff_pred_add_8_avx2,
                ff_pred_add_9_avx2,
                ff_pred_add_10_avx2,
                ff_pred_add_11_avx2,
                ff_pred_add_12_avx2,
                ff_pred_add_13_avx2,
            ];
            dsp.extract_green = ff_extract_green_avx2;
            dsp.map_color32 = ff_map_color32_avx2;
            dsp.blend_row_argb = ff_blend_row_argb_avx2;
            dsp.blend_row_argb_premult = ff_blend_row_argb_premult_avx2;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;

    extern "C" {
        pub fn ff_pred_add_0_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_1_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_2_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_3_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_4_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_5_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_6_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_7_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_8_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_9_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_10_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_11_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_12_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_pred_add_13_neon(a: *const u32, b: *const u32, n: c_int, o: *mut u32);
        pub fn ff_extract_green_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_map_color32_neon(
            dst: *mut u8,
            src: *const u8,
            palette: *const u32,
            n: c_int,
        );
        pub fn ff_blend_row_argb_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_blend_row_argb_premult_neon(dst: *mut u8, src: *const u8, n: c_int);
    }

    pub fn init(dsp: &mut WPDLosslessDSP) {
        if !wpd::cpu::flags().contains(wpd::cpu::CpuFlags::NEON) {
            return;
        }
        dsp.pred_add = [
            ff_pred_add_0_neon,
            ff_pred_add_1_neon,
            ff_pred_add_2_neon,
            ff_pred_add_3_neon,
            ff_pred_add_4_neon,
            ff_pred_add_5_neon,
            ff_pred_add_6_neon,
            ff_pred_add_7_neon,
            ff_pred_add_8_neon,
            ff_pred_add_9_neon,
            ff_pred_add_10_neon,
            ff_pred_add_11_neon,
            ff_pred_add_12_neon,
            ff_pred_add_13_neon,
        ];
        dsp.extract_green = ff_extract_green_neon;
        dsp.map_color32 = ff_map_color32_neon;
        dsp.blend_row_argb = ff_blend_row_argb_neon;
        dsp.blend_row_argb_premult = ff_blend_row_argb_premult_neon;
    }
}

impl WPDLosslessDSP {
    /// The best implementation the running CPU allows.
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = WPDLosslessDSP {
            pred_add: [
                pred_add_0_c,
                pred_add_1_c,
                pred_add_2_c,
                pred_add_3_c,
                pred_add_4_c,
                pred_add_5_c,
                pred_add_6_c,
                pred_add_7_c,
                pred_add_8_c,
                pred_add_9_c,
                pred_add_10_c,
                pred_add_11_c,
                pred_add_12_c,
                pred_add_13_c,
            ],
            extract_green: extract_green_c,
            map_color32: map_color32_c,
            blend_row_argb: blend_row_argb_c,
            blend_row_argb_premult: blend_row_argb_premult_c,
        };

        #[cfg(all(
            feature = "asm",
            any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
        ))]
        asm::init(&mut table);

        table
    }
}

/// Fills in `dsp` with the best implementation the running CPU allows.
///
/// # Safety
///
/// `dsp` must point to a writable, aligned `WPDLosslessDSP`.
#[no_mangle]
pub unsafe extern "C" fn wpd_vp8l_dsp_init(dsp: *mut WPDLosslessDSP) {
    unsafe { dsp.write(WPDLosslessDSP::new()) }
}
