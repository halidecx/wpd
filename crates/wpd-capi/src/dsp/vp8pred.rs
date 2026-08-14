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
    use wpd::cpu::CpuFlags;

    extern "C" {
        fn ff_pred4x4_dc_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_horizontal_vp8_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vertical_left_vp8_8_ssse3(
            src: *mut u8,
            tr: *const u8,
            stride: isize,
        );
        fn ff_pred4x4_down_left_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_down_right_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_horizontal_down_8_sse2(
            src: *mut u8,
            tr: *const u8,
            stride: isize,
        );
        fn ff_pred4x4_horizontal_up_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_tm_vp8_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_tm_vp8_8_ssse3(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vertical_right_8_sse2(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vertical_vp8_8_sse2(src: *mut u8, tr: *const u8, stride: isize);

        fn ff_pred8x8_dc_vp8_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred8x8_top_dc_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred8x8_top_dc_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred8x8_left_dc_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred8x8_left_dc_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred8x8_horizontal_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred8x8_horizontal_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred8x8_tm_vp8_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred8x8_tm_vp8_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred8x8_vertical_8_sse2(src: *mut u8, stride: isize);

        fn ff_pred16x16_vertical_8_sse(src: *mut u8, stride: isize);
        fn ff_pred16x16_horizontal_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred16x16_horizontal_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred16x16_dc_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred16x16_dc_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred16x16_top_dc_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred16x16_top_dc_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred16x16_left_dc_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred16x16_left_dc_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred16x16_tm_vp8_8_sse2(src: *mut u8, stride: isize);
        fn ff_pred16x16_tm_vp8_8_ssse3(src: *mut u8, stride: isize);
        fn ff_pred16x16_tm_vp8_8_avx2(src: *mut u8, stride: isize);
    }

    pub fn init(p: &mut VP8PredContext) {
        let flags = wpd::cpu::flags();

        if flags.contains(CpuFlags::SSE) {
            p.pred16x16[VERT] = ff_pred16x16_vertical_8_sse;
        }

        if flags.contains(CpuFlags::SSE2) {
            p.pred4x4[DIAG_DOWN_LEFT] = ff_pred4x4_down_left_8_sse2;
            p.pred4x4[DIAG_DOWN_RIGHT] = ff_pred4x4_down_right_8_sse2;
            p.pred4x4[VERT_RIGHT] = ff_pred4x4_vertical_right_8_sse2;
            p.pred4x4[HOR_DOWN] = ff_pred4x4_horizontal_down_8_sse2;
            p.pred4x4[HOR_UP] = ff_pred4x4_horizontal_up_8_sse2;
            p.pred4x4[DC4] = ff_pred4x4_dc_8_sse2;
            p.pred4x4[TM4] = ff_pred4x4_tm_vp8_8_sse2;
            p.pred4x4[VERT4] = ff_pred4x4_vertical_vp8_8_sse2;
            p.pred4x4[HOR4] = ff_pred4x4_horizontal_vp8_8_sse2;

            p.pred8x8[DC] = ff_pred8x8_dc_vp8_8_sse2;
            p.pred8x8[HOR] = ff_pred8x8_horizontal_8_sse2;
            p.pred8x8[VERT] = ff_pred8x8_vertical_8_sse2;
            p.pred8x8[PLANE] = ff_pred8x8_tm_vp8_8_sse2;
            p.pred8x8[TOP_DC] = ff_pred8x8_top_dc_8_sse2;
            p.pred8x8[LEFT_DC] = ff_pred8x8_left_dc_8_sse2;

            p.pred16x16[HOR] = ff_pred16x16_horizontal_8_sse2;
            p.pred16x16[DC] = ff_pred16x16_dc_8_sse2;
            p.pred16x16[PLANE] = ff_pred16x16_tm_vp8_8_sse2;
            p.pred16x16[TOP_DC] = ff_pred16x16_top_dc_8_sse2;
            p.pred16x16[LEFT_DC] = ff_pred16x16_left_dc_8_sse2;
        }

        if flags.contains(CpuFlags::SSSE3) {
            p.pred4x4[TM4] = ff_pred4x4_tm_vp8_8_ssse3;
            p.pred4x4[VERT_LEFT] = ff_pred4x4_vertical_left_vp8_8_ssse3;

            p.pred8x8[HOR] = ff_pred8x8_horizontal_8_ssse3;
            p.pred8x8[PLANE] = ff_pred8x8_tm_vp8_8_ssse3;
            p.pred8x8[TOP_DC] = ff_pred8x8_top_dc_8_ssse3;
            p.pred8x8[LEFT_DC] = ff_pred8x8_left_dc_8_ssse3;

            p.pred16x16[PLANE] = ff_pred16x16_tm_vp8_8_ssse3;
            p.pred16x16[HOR] = ff_pred16x16_horizontal_8_ssse3;
            p.pred16x16[DC] = ff_pred16x16_dc_8_ssse3;
            p.pred16x16[TOP_DC] = ff_pred16x16_top_dc_8_ssse3;
            p.pred16x16[LEFT_DC] = ff_pred16x16_left_dc_8_ssse3;
        }

        if flags.contains(CpuFlags::AVX2) {
            p.pred16x16[PLANE] = ff_pred16x16_tm_vp8_8_avx2;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;
    use wpd::cpu::CpuFlags;

    extern "C" {
        fn ff_pred4x4_tm_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_dc_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vert_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_hor_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_down_left_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_down_right_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vert_left_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_vert_right_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_hor_up_neon(src: *mut u8, tr: *const u8, stride: isize);
        fn ff_pred4x4_hor_down_neon(src: *mut u8, tr: *const u8, stride: isize);

        fn ff_pred8x8_vert_neon(src: *mut u8, stride: isize);
        fn ff_pred8x8_dc_neon(src: *mut u8, stride: isize);
        fn ff_pred8x8_tm_neon(src: *mut u8, stride: isize);

        fn ff_pred16x16_vert_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_hor_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_dc_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_tm_neon(src: *mut u8, stride: isize);
    }

    pub fn init(p: &mut VP8PredContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        p.pred4x4[TM4] = ff_pred4x4_tm_neon;
        p.pred4x4[DC4] = ff_pred4x4_dc_neon;
        p.pred4x4[VERT4] = ff_pred4x4_vert_neon;
        p.pred4x4[HOR4] = ff_pred4x4_hor_neon;
        p.pred4x4[DIAG_DOWN_LEFT] = ff_pred4x4_down_left_neon;
        p.pred4x4[DIAG_DOWN_RIGHT] = ff_pred4x4_down_right_neon;
        p.pred4x4[VERT_LEFT] = ff_pred4x4_vert_left_neon;
        p.pred4x4[VERT_RIGHT] = ff_pred4x4_vert_right_neon;
        p.pred4x4[HOR_UP] = ff_pred4x4_hor_up_neon;
        p.pred4x4[HOR_DOWN] = ff_pred4x4_hor_down_neon;

        p.pred8x8[VERT] = ff_pred8x8_vert_neon;
        p.pred8x8[DC] = ff_pred8x8_dc_neon;
        p.pred8x8[PLANE] = ff_pred8x8_tm_neon;

        p.pred16x16[DC] = ff_pred16x16_dc_neon;
        p.pred16x16[VERT] = ff_pred16x16_vert_neon;
        p.pred16x16[HOR] = ff_pred16x16_hor_neon;
        p.pred16x16[PLANE] = ff_pred16x16_tm_neon;
    }
}

#[cfg(all(feature = "asm", target_arch = "arm"))]
mod asm {
    use super::*;
    use wpd::cpu::CpuFlags;

    extern "C" {
        fn ff_pred8x8_vert_neon(src: *mut u8, stride: isize);
        fn ff_pred8x8_hor_neon(src: *mut u8, stride: isize);
        fn ff_pred8x8_128_dc_neon(src: *mut u8, stride: isize);

        fn ff_pred16x16_dc_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_vert_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_hor_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_left_dc_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_top_dc_neon(src: *mut u8, stride: isize);
        fn ff_pred16x16_128_dc_neon(src: *mut u8, stride: isize);
    }

    pub fn init(p: &mut VP8PredContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        p.pred8x8[VERT] = ff_pred8x8_vert_neon;
        p.pred8x8[HOR] = ff_pred8x8_hor_neon;
        p.pred8x8[DC_128] = ff_pred8x8_128_dc_neon;

        p.pred16x16[DC] = ff_pred16x16_dc_neon;
        p.pred16x16[VERT] = ff_pred16x16_vert_neon;
        p.pred16x16[HOR] = ff_pred16x16_hor_neon;
        p.pred16x16[LEFT_DC] = ff_pred16x16_left_dc_neon;
        p.pred16x16[TOP_DC] = ff_pred16x16_top_dc_neon;
        p.pred16x16[DC_128] = ff_pred16x16_128_dc_neon;
    }
}

/// `VP8Pred4x4Mode`.
const VERT4: usize = 0;
const HOR4: usize = 1;
const DC4: usize = 2;
const DIAG_DOWN_LEFT: usize = 3;
const DIAG_DOWN_RIGHT: usize = 4;
const VERT_RIGHT: usize = 5;
const HOR_DOWN: usize = 6;
const VERT_LEFT: usize = 7;
const HOR_UP: usize = 8;
const TM4: usize = 9;

/// `VP8Pred8x8Mode`.
const DC: usize = 0;
const HOR: usize = 1;
const VERT: usize = 2;
const PLANE: usize = 3;
const LEFT_DC: usize = 4;
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
