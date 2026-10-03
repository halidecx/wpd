use std::ffi::c_int;

use super::count;
use std::slice;

use wpd::dsp::vp8l as k;

pub type PredAddFn = unsafe extern "C" fn(*const u32, *const u32, c_int, *mut u32);
pub type PredPairFn =
    unsafe extern "C" fn(*const u32, *const u32, c_int, *mut u32, *const u32, *mut u32);
pub type PredGreenFn = unsafe extern "C" fn(*const u32, *const u8, c_int, *mut u8);
pub type RowFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type MapColorFn = unsafe extern "C" fn(*mut u8, *const u8, *const u32, c_int);
pub type ColorRowFn = unsafe extern "C" fn(*mut u32, *const u32, c_int, u32);
pub type AddGreenFn = unsafe extern "C" fn(*mut u32, *const u32, c_int);
pub type ExpandAlphaFn = unsafe extern "C" fn(*mut u8, *const u8, *const u8, c_int);

pub const PRED_COUNT: usize = 14;

#[repr(C)]
pub struct WPDLosslessDSP {
    pub pred_add: [PredAddFn; PRED_COUNT],
    pub pred_add_pair: [PredPairFn; PRED_COUNT],
    pub extract_green: RowFn,
    pub map_color32: MapColorFn,
    pub blend_row_argb: RowFn,
    pub blend_row_argb_premult: RowFn,
    pub color_row: ColorRowFn,
    pub add_green: AddGreenFn,
    pub pred_green: [PredGreenFn; PRED_COUNT],
    pub expand_alpha_nibbles: ExpandAlphaFn,
}

unsafe extern "C" fn pred_add_0_c(
    inp: *const u32,
    _upper: *const u32,
    n: c_int,
    out: *mut u32,
) {
    debug_assert_eq!(inp, out.cast_const());
    let Some(n) = count(n) else {
        return;
    };
    unsafe { k::pred_add_0(slice::from_raw_parts_mut(out, n)) }
}

unsafe extern "C" fn pred_add_1_c(
    inp: *const u32,
    _upper: *const u32,
    n: c_int,
    out: *mut u32,
) {
    debug_assert_eq!(inp, out.cast_const());
    let Some(n) = count(n).filter(|&n| n != 0) else {
        return;
    };
    unsafe {
        let left = *out.sub(1);
        k::pred_add_1(slice::from_raw_parts_mut(out, n), left);
    }
}

macro_rules! pred_tramp {
    ($name:ident, $kernel:ident, $l:literal, $tl:literal, $tr:literal) => {
        unsafe extern "C" fn $name(
            inp: *const u32,
            upper: *const u32,
            n: c_int,
            out: *mut u32,
        ) {
            debug_assert_eq!(inp, out.cast_const());
            let Some(n) = count(n).filter(|&n| n != 0) else {
                return;
            };
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
unsafe extern "C" fn pred_add_4_c(
    inp: *const u32,
    upper: *const u32,
    n: c_int,
    out: *mut u32,
) {
    debug_assert_eq!(inp, out.cast_const());
    if count(n).is_none_or(|n| n == 0) {
        return;
    }
    unsafe { pred_add_2_c(inp, upper.sub(1), n, out) }
}
pred_tramp!(pred_add_5_c, pred_add_5, true, false, true);
pred_tramp!(pred_add_6_c, pred_add_6, true, true, false);
pred_tramp!(pred_add_7_c, pred_add_7, true, false, false);
pred_tramp!(pred_add_8_c, pred_add_8, false, true, false);
pred_tramp!(pred_add_9_c, pred_add_9, false, false, true);
pred_tramp!(pred_add_10_c, pred_add_10, true, true, true);
pred_tramp!(pred_add_11_c, pred_add_11, true, true, false);
pred_tramp!(pred_add_12_c, pred_add_12, true, true, false);
pred_tramp!(pred_add_13_c, pred_add_13, true, true, false);

/* The lower row after the upper, over its output. */
macro_rules! pair_tramp {
    ($name:ident, $single:ident) => {
        unsafe extern "C" fn $name(
            inp: *const u32,
            upper: *const u32,
            n: c_int,
            out: *mut u32,
            in_b: *const u32,
            out_b: *mut u32,
        ) {
            unsafe {
                $single(inp, upper, n, out);
                $single(in_b, out.cast_const(), n, out_b);
            }
        }
    };
}

pair_tramp!(pred_add_pair_0_c, pred_add_0_c);
pair_tramp!(pred_add_pair_1_c, pred_add_1_c);
pair_tramp!(pred_add_pair_2_c, pred_add_2_c);
pair_tramp!(pred_add_pair_3_c, pred_add_3_c);
pair_tramp!(pred_add_pair_4_c, pred_add_4_c);
pair_tramp!(pred_add_pair_5_c, pred_add_5_c);
pair_tramp!(pred_add_pair_6_c, pred_add_6_c);
pair_tramp!(pred_add_pair_7_c, pred_add_7_c);
pair_tramp!(pred_add_pair_8_c, pred_add_8_c);
pair_tramp!(pred_add_pair_9_c, pred_add_9_c);
pair_tramp!(pred_add_pair_10_c, pred_add_10_c);
pair_tramp!(pred_add_pair_11_c, pred_add_11_c);
pair_tramp!(pred_add_pair_12_c, pred_add_12_c);
pair_tramp!(pred_add_pair_13_c, pred_add_13_c);

/* The green predictors read out[-1] and upper[-1..=n]; mode 1 no upper. */
macro_rules! green_tramp {
    ($name:ident, $kernel:ident, $up:literal) => {
        unsafe extern "C" fn $name(
            res: *const u32,
            upper: *const u8,
            n: c_int,
            out: *mut u8,
        ) {
            let Some(n) = count(n) else {
                return;
            };
            unsafe {
                let above: &[u8] = if $up {
                    slice::from_raw_parts(upper.sub(1), n + 2)
                } else {
                    &[]
                };
                k::$kernel(
                    slice::from_raw_parts_mut(out.sub(1), n + 1),
                    above,
                    slice::from_raw_parts(res, n),
                );
            }
        }
    };
}

green_tramp!(pred_green_0_c, pred_green_0, true);
green_tramp!(pred_green_1_c, pred_green_1, false);
green_tramp!(pred_green_2_c, pred_green_2, true);
green_tramp!(pred_green_3_c, pred_green_3, true);
green_tramp!(pred_green_4_c, pred_green_4, true);
green_tramp!(pred_green_5_c, pred_green_5, true);
green_tramp!(pred_green_6_c, pred_green_6, true);
green_tramp!(pred_green_7_c, pred_green_7, true);
green_tramp!(pred_green_8_c, pred_green_8, true);
green_tramp!(pred_green_9_c, pred_green_9, true);
green_tramp!(pred_green_10_c, pred_green_10, true);
green_tramp!(pred_green_11_c, pred_green_11, true);
green_tramp!(pred_green_12_c, pred_green_12, true);
green_tramp!(pred_green_13_c, pred_green_13, true);

unsafe extern "C" fn add_green_c(dst: *mut u32, src: *const u32, n: c_int) {
    let Some(n) = count(n) else {
        return;
    };
    unsafe {
        if dst.cast_const() != src {
            std::ptr::copy(src, dst, n);
        }
        k::add_green(slice::from_raw_parts_mut(dst, n));
    }
}

unsafe extern "C" fn extract_green_c(dst: *mut u8, src: *const u8, n: c_int) {
    let Some(n) = count(n) else {
        return;
    };
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
    let Some(n) = count(n).map(|n| 4 * n) else {
        return;
    };
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

unsafe extern "C" fn expand_alpha_nibbles_c(
    dst: *mut u8,
    src: *const u8,
    palette: *const u8,
    n: c_int,
) {
    let Some(n) = count(n) else {
        return;
    };
    unsafe {
        k::expand_alpha_nibbles(
            slice::from_raw_parts_mut(dst, 16 * n),
            slice::from_raw_parts(src, 8 * n),
            &*palette.cast::<[u8; 16]>(),
        )
    }
}

unsafe extern "C" fn color_row_c(dst: *mut u32, src: *const u32, n: c_int, mult: u32) {
    let Some(n) = count(n) else {
        return;
    };
    unsafe {
        let row = slice::from_raw_parts_mut(dst, n);

        if dst.cast_const() != src {
            row.copy_from_slice(slice::from_raw_parts(src, n));
        }
        k::color_row(row, mult);
    }
}

unsafe extern "C" fn blend_row_argb_c(dst: *mut u8, src: *const u8, n: c_int) {
    let Some(n) = count(n).map(|n| 4 * n) else {
        return;
    };
    unsafe {
        k::blend_row_argb(
            slice::from_raw_parts_mut(dst, n),
            slice::from_raw_parts(src, n),
        )
    }
}

unsafe extern "C" fn blend_row_argb_premult_c(dst: *mut u8, src: *const u8, n: c_int) {
    let Some(n) = count(n).map(|n| 4 * n) else {
        return;
    };
    unsafe {
        k::blend_row_argb_premult(
            slice::from_raw_parts_mut(dst, n),
            slice::from_raw_parts(src, n),
        )
    }
}

#[cfg(all(
    feature = "asm",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
fn init_asm(dsp: &mut WPDLosslessDSP) {
    let t = wpd::asm::vp8l::raw_table(wpd::cpu::flags());

    for (slot, sel) in dsp.pred_add.iter_mut().zip(t.pred_add) {
        if let Some(v) = sel {
            *slot = v;
        }
    }
    for (slot, sel) in dsp.pred_add_pair.iter_mut().zip(t.pred_add_pair) {
        if let Some(v) = sel {
            *slot = v;
        }
    }
    for (slot, sel) in dsp.pred_green.iter_mut().zip(t.pred_green) {
        if let Some(v) = sel {
            *slot = v;
        }
    }
    if let Some(v) = t.extract_green {
        dsp.extract_green = v;
    }
    if let Some(v) = t.map_color32 {
        dsp.map_color32 = v;
    }
    if let Some(v) = t.blend_row_argb {
        dsp.blend_row_argb = v;
    }
    if let Some(v) = t.blend_row_argb_premult {
        dsp.blend_row_argb_premult = v;
    }
    if let Some(v) = t.color_row {
        dsp.color_row = v;
    }
    if let Some(v) = t.add_green {
        dsp.add_green = v;
    }
    if let Some(v) = t.expand_alpha_nibbles {
        dsp.expand_alpha_nibbles = v;
    }
}

impl WPDLosslessDSP {
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
            pred_add_pair: [
                pred_add_pair_0_c,
                pred_add_pair_1_c,
                pred_add_pair_2_c,
                pred_add_pair_3_c,
                pred_add_pair_4_c,
                pred_add_pair_5_c,
                pred_add_pair_6_c,
                pred_add_pair_7_c,
                pred_add_pair_8_c,
                pred_add_pair_9_c,
                pred_add_pair_10_c,
                pred_add_pair_11_c,
                pred_add_pair_12_c,
                pred_add_pair_13_c,
            ],
            extract_green: extract_green_c,
            map_color32: map_color32_c,
            blend_row_argb: blend_row_argb_c,
            blend_row_argb_premult: blend_row_argb_premult_c,
            color_row: color_row_c,
            add_green: add_green_c,
            pred_green: [
                pred_green_0_c,
                pred_green_1_c,
                pred_green_2_c,
                pred_green_3_c,
                pred_green_4_c,
                pred_green_5_c,
                pred_green_6_c,
                pred_green_7_c,
                pred_green_8_c,
                pred_green_9_c,
                pred_green_10_c,
                pred_green_11_c,
                pred_green_12_c,
                pred_green_13_c,
            ],
            expand_alpha_nibbles: expand_alpha_nibbles_c,
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
pub unsafe extern "C" fn wpd_vp8l_dsp_init(dsp: *mut WPDLosslessDSP) {
    unsafe { dsp.write(WPDLosslessDSP::new()) }
}
