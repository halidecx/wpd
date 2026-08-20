use std::slice;

use wpd::dsp::vp8pred as k;

pub type Pred4x4Fn = unsafe extern "C" fn(*mut u8, *const u8, isize);
pub type PredFn = unsafe extern "C" fn(*mut u8, isize);

pub const PRED4X4_COUNT: usize = 10;
pub const PRED8X8_COUNT: usize = 7;

#[repr(C)]
pub struct VP8PredContext {
    pub pred4x4: [Pred4x4Fn; PRED4X4_COUNT],
    pub pred8x8: [PredFn; PRED8X8_COUNT],
    pub pred16x16: [PredFn; PRED8X8_COUNT],
}

macro_rules! pred4x4_entry {
    ($name:ident, $k:expr, $rows:expr, $cols:expr) => {
        unsafe extern "C" fn $name(src: *mut u8, _tr: *const u8, stride: isize) {
            let s = stride as usize;
            let o = $rows * s + $cols;
            let buf = unsafe { slice::from_raw_parts_mut(src.sub(o), o + 3 * s + 4) };

            $k(buf, o, s);
        }
    };
}

macro_rules! pred4x4_tr_entry {
    ($name:ident, $k:expr, $rows:expr, $cols:expr) => {
        unsafe extern "C" fn $name(src: *mut u8, tr: *const u8, stride: isize) {
            let s = stride as usize;
            let o = $rows * s + $cols;

            unsafe {
                let buf = slice::from_raw_parts_mut(src.sub(o), o + 3 * s + 4);

                $k(buf, o, s, &*tr.cast::<[u8; 4]>());
            }
        }
    };
}

macro_rules! pred_entry {
    ($name:ident, $k:expr, $n:expr, $rows:expr, $cols:expr) => {
        unsafe extern "C" fn $name(src: *mut u8, stride: isize) {
            let s = stride as usize;
            let o = $rows * s + $cols;
            let buf =
                unsafe { slice::from_raw_parts_mut(src.sub(o), o + ($n - 1) * s + $n) };

            $k(buf, o, s);
        }
    };
}

pred4x4_tr_entry!(vertical_c, k::pred4x4_vertical, 1, 1);
pred4x4_entry!(horizontal_c, k::pred4x4_horizontal, 1, 1);
pred4x4_entry!(dc_c, k::pred4x4_dc, 1, 0);
pred4x4_tr_entry!(down_left_c, k::pred4x4_down_left, 1, 0);
pred4x4_entry!(down_right_c, k::pred4x4_down_right, 1, 1);
pred4x4_entry!(vertical_right_c, k::pred4x4_vertical_right, 1, 1);
pred4x4_entry!(horizontal_down_c, k::pred4x4_horizontal_down, 1, 1);
pred4x4_tr_entry!(vertical_left_c, k::pred4x4_vertical_left, 1, 0);
pred4x4_entry!(horizontal_up_c, k::pred4x4_horizontal_up, 0, 1);
pred4x4_entry!(tm4x4_c, k::pred_tm::<4>, 1, 1);

pred_entry!(dc8_c, k::pred_dc::<8>, 8, 1, 0);
pred_entry!(horizontal8_c, k::pred_horizontal::<8>, 8, 0, 1);
pred_entry!(vertical8_c, k::pred_vertical::<8>, 8, 1, 0);
pred_entry!(tm8_c, k::pred_tm::<8>, 8, 1, 1);
pred_entry!(left_dc8_c, k::pred_left_dc::<8>, 8, 0, 1);
pred_entry!(top_dc8_c, k::pred_top_dc::<8>, 8, 1, 0);
pred_entry!(dc128_8_c, k::pred_dc128::<8>, 8, 0, 0);

pred_entry!(dc16_c, k::pred_dc::<16>, 16, 1, 0);
pred_entry!(horizontal16_c, k::pred_horizontal::<16>, 16, 0, 1);
pred_entry!(vertical16_c, k::pred_vertical::<16>, 16, 1, 0);
pred_entry!(tm16_c, k::pred_tm::<16>, 16, 1, 1);
pred_entry!(left_dc16_c, k::pred_left_dc::<16>, 16, 0, 1);
pred_entry!(top_dc16_c, k::pred_top_dc::<16>, 16, 1, 0);
pred_entry!(dc128_16_c, k::pred_dc128::<16>, 16, 0, 0);

#[cfg(all(
    feature = "asm",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm"
    )
))]
fn init_asm(p: &mut VP8PredContext) {
    let t = wpd::asm::vp8pred::raw_table(wpd::cpu::flags());

    for (slot, entry) in p.pred4x4.iter_mut().zip(t.pred4x4) {
        if let Some(f) = entry {
            *slot = f;
        }
    }
    for (slot, entry) in p.pred8x8.iter_mut().zip(t.pred8x8) {
        if let Some(f) = entry {
            *slot = f;
        }
    }
    for (slot, entry) in p.pred16x16.iter_mut().zip(t.pred16x16) {
        if let Some(f) = entry {
            *slot = f;
        }
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn ff_vp8_pred_init(p: *mut VP8PredContext) {
    #[allow(unused_mut)]
    let mut table = VP8PredContext {
        pred4x4: [
            vertical_c,
            horizontal_c,
            dc_c,
            down_left_c,
            down_right_c,
            vertical_right_c,
            horizontal_down_c,
            vertical_left_c,
            horizontal_up_c,
            tm4x4_c,
        ],
        pred8x8: [
            dc8_c,
            horizontal8_c,
            vertical8_c,
            tm8_c,
            left_dc8_c,
            top_dc8_c,
            dc128_8_c,
        ],
        pred16x16: [
            dc16_c,
            horizontal16_c,
            vertical16_c,
            tm16_c,
            left_dc16_c,
            top_dc16_c,
            dc128_16_c,
        ],
    };

    #[cfg(all(
        feature = "asm",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm"
        )
    ))]
    init_asm(&mut table);

    unsafe { p.write(table) }
}
