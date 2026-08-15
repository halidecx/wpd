//! C ABI for the intra prediction table, as declared by `src/vp8pred.h`.

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

/// Wraps a 4x4 predictor. `$rows` and `$cols` say how far back of the block
/// the mode reads, in whole rows and in bytes; that is exactly the region the
/// kernel is handed.
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

/// The same, for the three modes that also read four samples above and to the
/// right of the block, which live outside that region.
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

#[cfg(all(feature = "asm", any(target_arch = "x86", target_arch = "x86_64")))]
mod asm {
    use super::*;
    use wpd::asm::vp8pred::{avx2, sse, sse2, ssse3, Raw};
    use wpd::cpu::CpuFlags;

    pub fn init(p: &mut VP8PredContext) {
        let flags = wpd::cpu::flags();

        if flags.contains(CpuFlags::SSE) {
            p.pred16x16[VERT] = sse::Vert16::F;
        }

        if flags.contains(CpuFlags::SSE2) {
            p.pred4x4[DIAG_DOWN_LEFT] = sse2::DownLeft4::F;
            p.pred4x4[DIAG_DOWN_RIGHT] = sse2::DownRight4::F;
            p.pred4x4[VERT_RIGHT] = sse2::VertRight4::F;
            p.pred4x4[HOR_DOWN] = sse2::HorDown4::F;
            p.pred4x4[HOR_UP] = sse2::HorUp4::F;
            p.pred4x4[DC4] = sse2::Dc4::F;
            p.pred4x4[TM4] = sse2::Tm4::F;
            p.pred4x4[VERT4] = sse2::Vert4::F;
            p.pred4x4[HOR4] = sse2::Hor4::F;

            p.pred8x8[DC] = sse2::Dc8::F;
            p.pred8x8[HOR] = sse2::Hor8::F;
            p.pred8x8[VERT] = sse2::Vert8::F;
            p.pred8x8[PLANE] = sse2::Tm8::F;
            p.pred8x8[TOP_DC] = sse2::TopDc8::F;
            p.pred8x8[LEFT_DC] = sse2::LeftDc8::F;

            p.pred16x16[HOR] = sse2::Hor16::F;
            p.pred16x16[DC] = sse2::Dc16::F;
            p.pred16x16[PLANE] = sse2::Tm16::F;
            p.pred16x16[TOP_DC] = sse2::TopDc16::F;
            p.pred16x16[LEFT_DC] = sse2::LeftDc16::F;
        }

        if flags.contains(CpuFlags::SSSE3) {
            p.pred4x4[TM4] = ssse3::Tm4::F;
            p.pred4x4[VERT_LEFT] = ssse3::VertLeft4::F;

            p.pred8x8[HOR] = ssse3::Hor8::F;
            p.pred8x8[PLANE] = ssse3::Tm8::F;
            p.pred8x8[TOP_DC] = ssse3::TopDc8::F;
            p.pred8x8[LEFT_DC] = ssse3::LeftDc8::F;

            p.pred16x16[PLANE] = ssse3::Tm16::F;
            p.pred16x16[HOR] = ssse3::Hor16::F;
            p.pred16x16[DC] = ssse3::Dc16::F;
            p.pred16x16[TOP_DC] = ssse3::TopDc16::F;
            p.pred16x16[LEFT_DC] = ssse3::LeftDc16::F;
        }

        if flags.contains(CpuFlags::AVX2) {
            p.pred16x16[PLANE] = avx2::Tm16::F;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;
    use wpd::asm::vp8pred::{neon, Raw};
    use wpd::cpu::CpuFlags;

    pub fn init(p: &mut VP8PredContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        p.pred4x4[TM4] = neon::Tm4::F;
        p.pred4x4[DC4] = neon::Dc4::F;
        p.pred4x4[VERT4] = neon::Vert4::F;
        p.pred4x4[HOR4] = neon::Hor4::F;
        p.pred4x4[DIAG_DOWN_LEFT] = neon::DownLeft4::F;
        p.pred4x4[DIAG_DOWN_RIGHT] = neon::DownRight4::F;
        p.pred4x4[VERT_LEFT] = neon::VertLeft4::F;
        p.pred4x4[VERT_RIGHT] = neon::VertRight4::F;
        p.pred4x4[HOR_UP] = neon::HorUp4::F;
        p.pred4x4[HOR_DOWN] = neon::HorDown4::F;

        p.pred8x8[VERT] = neon::Vert8::F;
        p.pred8x8[DC] = neon::Dc8::F;
        p.pred8x8[PLANE] = neon::Tm8::F;

        p.pred16x16[DC] = neon::Dc16::F;
        p.pred16x16[VERT] = neon::Vert16::F;
        p.pred16x16[HOR] = neon::Hor16::F;
        p.pred16x16[PLANE] = neon::Tm16::F;
    }
}

#[cfg(all(feature = "asm", target_arch = "arm"))]
mod asm {
    use super::*;
    use wpd::asm::vp8pred::{neon, Raw};
    use wpd::cpu::CpuFlags;

    pub fn init(p: &mut VP8PredContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        p.pred8x8[VERT] = neon::Vert8::F;
        p.pred8x8[HOR] = neon::Hor8::F;
        p.pred8x8[DC_128] = neon::Dc128_8::F;

        p.pred16x16[DC] = neon::Dc16::F;
        p.pred16x16[VERT] = neon::Vert16::F;
        p.pred16x16[HOR] = neon::Hor16::F;
        p.pred16x16[LEFT_DC] = neon::LeftDc16::F;
        p.pred16x16[TOP_DC] = neon::TopDc16::F;
        p.pred16x16[DC_128] = neon::Dc128_16::F;
    }
}

/* The mode indices are the table's layout, so they are stated whether or not
this build has an assembly entry that names one. */
#[allow(dead_code)]
/// `VP8Pred4x4Mode`.
const VERT4: usize = 0;
#[allow(dead_code)]
const HOR4: usize = 1;
#[allow(dead_code)]
const DC4: usize = 2;
#[allow(dead_code)]
const DIAG_DOWN_LEFT: usize = 3;
#[allow(dead_code)]
const DIAG_DOWN_RIGHT: usize = 4;
#[allow(dead_code)]
const VERT_RIGHT: usize = 5;
#[allow(dead_code)]
const HOR_DOWN: usize = 6;
#[allow(dead_code)]
const VERT_LEFT: usize = 7;
#[allow(dead_code)]
const HOR_UP: usize = 8;
#[allow(dead_code)]
const TM4: usize = 9;

/// `VP8Pred8x8Mode`.
#[allow(dead_code)]
const DC: usize = 0;
#[allow(dead_code)]
const HOR: usize = 1;
#[allow(dead_code)]
const VERT: usize = 2;
#[allow(dead_code)]
const PLANE: usize = 3;
#[allow(dead_code)]
const LEFT_DC: usize = 4;
#[allow(dead_code)]
const TOP_DC: usize = 5;
#[allow(dead_code)]
const DC_128: usize = 6;

/// Fills in `p` with the best implementation the running CPU allows.
///
/// # Safety
///
/// `p` must point to a writable, aligned `VP8PredContext`.
#[no_mangle]
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
    asm::init(&mut table);

    unsafe { p.write(table) }
}
