//! The lossless DSP assembly.
//!
//! Laid out as [`super::vp8`]: the symbols are declared once and exported raw
//! for the C ABI table, and the core crate gets safe wrappers.
//!
//! # Regions
//!
//! A predictor writes `n` pixels at `out` and reads `n` at `up`, plus, for the
//! ones whose fallback wrapper asks for them, `out[-1]` and `up[-1]`. The three
//! that use the top-right neighbour read one past the end of the row above,
//! which is the first pixel of the row being written when the picture is
//! contiguous, and a slot the caller has filled in when it is not. Either way
//! the read lands inside the picture, so the check is the same for all of them:
//! `up + n` must be a valid index.

use crate::cpu::CpuFlags;
use crate::dsp::vp8l::Vp8lDsp;
use std::ffi::c_int;

pub type PredAddRaw = unsafe extern "C" fn(*const u32, *const u32, c_int, *mut u32);
pub type MapColorRaw = unsafe extern "C" fn(*mut u8, *const u8, *const u32, c_int);
pub type ColorRowRaw = unsafe extern "C" fn(*mut u32, *const u32, c_int, u32);
pub type BlendRowRaw = unsafe extern "C" fn(*mut u8, *const u8, c_int);

pub use super::vp8::Raw;

macro_rules! raw_pred_add {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *const u32, _: *const u32, _: c_int, _: *mut u32);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = PredAddRaw;
            const F: PredAddRaw = $inner;
        }
    };
}

macro_rules! raw_map_color {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *mut u8, _: *const u8, _: *const u32, _: c_int);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = MapColorRaw;
            const F: MapColorRaw = $inner;
        }
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
macro_rules! raw_color_row {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *mut u32, _: *const u32, _: c_int, _: u32);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = ColorRowRaw;
            const F: ColorRowRaw = $inner;
        }
    };
}

macro_rules! raw_blend_row {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *mut u8, _: *const u8, _: c_int);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = BlendRowRaw;
            const F: BlendRowRaw = $inner;
        }
    };
}

/// `UP` is false for the two predictors that run on the first row of a picture,
/// where there is no row above and the caller passes no offset for one. `LEFT`
/// and `TL` say which out-of-row neighbours the kernel reads, the same split the
/// scalar table encodes: a predictor that reads one has to be given a row it can
/// read it from.
fn pred_add<
    T: Raw<Sig = PredAddRaw>,
    const UP: bool,
    const LEFT: bool,
    const TL: bool,
>(
    plane: &mut [u32],
    out: usize,
    up: usize,
    n: usize,
) {
    assert!(plane.len() >= out + n, "picture too small");
    if UP {
        assert!(up < out && plane.len() > up + n, "picture too small");
    }
    if LEFT {
        assert!(out >= 1, "no left neighbour");
    }
    if TL {
        assert!(UP && up >= 1, "no top-left neighbour");
    }
    unsafe {
        let base = plane.as_mut_ptr();

        (T::F)(
            base.add(out).cast_const(),
            if UP { base.add(up).cast_const() } else { base },
            n as c_int,
            base.add(out),
        )
    }
}

fn map_color32<T: Raw<Sig = MapColorRaw>>(row: &mut [u32], palette: &[u32]) {
    assert!(palette.len() >= 256, "short palette");
    unsafe {
        let p = row.as_mut_ptr().cast::<u8>();

        (T::F)(p, p.cast_const(), palette.as_ptr(), row.len() as c_int)
    }
}

/// The kernel is in place at every call site, and the length is the count, so
/// there is no region to check beyond what the slice already says.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn color_row<T: Raw<Sig = ColorRowRaw>>(row: &mut [u32], mult: u32) {
    unsafe {
        let p = row.as_mut_ptr();

        (T::F)(p, p.cast_const(), row.len() as c_int, mult)
    }
}

/// The kernel reads and writes whole pixels, so the shorter of the two rows
/// bounds the run — which is what the scalar `zip` does too.
fn blend_row<T: Raw<Sig = BlendRowRaw>>(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len()) / 4;

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

/// Green comes out one byte per four, so the destination bounds the run.
fn extract_green<T: Raw<Sig = BlendRowRaw>>(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len() / 4);

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

/// What the running CPU offers the C ABI table, slot by slot: `None` leaves
/// the caller's fallback in place. The C table holds the raw symbols, so it
/// cannot share the safe wrappers above — but it shares the selection, which
/// is the part that has to agree with what the decoder actually runs.
#[derive(Default)]
pub struct RawTable {
    pub pred_add: Option<[PredAddRaw; 14]>,
    pub extract_green: Option<BlendRowRaw>,
    pub map_color32: Option<MapColorRaw>,
    pub blend_row_argb: Option<BlendRowRaw>,
    pub blend_row_argb_premult: Option<BlendRowRaw>,
    pub color_row: Option<ColorRowRaw>,
}

/// The fourteen predictors of one instruction set, in table order. The flags
/// after each marker are `UP`, `LEFT`, `TL`, and they match the `l`/`tl` pair
/// the scalar table passes to its own kernels.
macro_rules! pred_table {
    ($set:ident) => {
        [
            pred_add::<$set::Pred0, false, false, false>,
            pred_add::<$set::Pred1, false, true, false>,
            pred_add::<$set::Pred2, true, false, false>,
            pred_add::<$set::Pred3, true, false, false>,
            pred_add::<$set::Pred4, true, false, true>,
            pred_add::<$set::Pred5, true, true, false>,
            pred_add::<$set::Pred6, true, true, true>,
            pred_add::<$set::Pred7, true, true, false>,
            pred_add::<$set::Pred8, true, false, true>,
            pred_add::<$set::Pred9, true, false, false>,
            pred_add::<$set::Pred10, true, true, true>,
            pred_add::<$set::Pred11, true, true, true>,
            pred_add::<$set::Pred12, true, true, true>,
            pred_add::<$set::Pred13, true, true, true>,
        ]
    };
}

/// The same fourteen, unwrapped, for the C ABI table.
#[allow(unused_macros)]
macro_rules! raw_pred_table {
    ($set:ident) => {
        [
            $set::Pred0::F,
            $set::Pred1::F,
            $set::Pred2::F,
            $set::Pred3::F,
            $set::Pred4::F,
            $set::Pred5::F,
            $set::Pred6::F,
            $set::Pred7::F,
            $set::Pred8::F,
            $set::Pred9::F,
            $set::Pred10::F,
            $set::Pred11::F,
            $set::Pred12::F,
            $set::Pred13::F,
        ]
    };
}

macro_rules! preds {
    ($($marker:ident, $inner:ident, $sym:literal;)*) => {
        $(raw_pred_add!($marker, $inner, $sym);)*
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;

    pub mod ssse3 {
        use super::*;

        raw_color_row!(ColorRow, color_row, "ff_color_row_ssse3");
        raw_blend_row!(
            BlendPremult,
            blend_premult,
            "ff_blend_row_argb_premult_ssse3"
        );
    }

    pub mod avx2 {
        use super::*;

        preds! {
            Pred0, pred0, "ff_pred_add_0_avx2";
            Pred1, pred1, "ff_pred_add_1_avx2";
            Pred2, pred2, "ff_pred_add_2_avx2";
            Pred3, pred3, "ff_pred_add_3_avx2";
            Pred4, pred4, "ff_pred_add_4_avx2";
            Pred5, pred5, "ff_pred_add_5_avx2";
            Pred6, pred6, "ff_pred_add_6_avx2";
            Pred7, pred7, "ff_pred_add_7_avx2";
            Pred8, pred8, "ff_pred_add_8_avx2";
            Pred9, pred9, "ff_pred_add_9_avx2";
            Pred10, pred10, "ff_pred_add_10_avx2";
            Pred11, pred11, "ff_pred_add_11_avx2";
            Pred12, pred12, "ff_pred_add_12_avx2";
            Pred13, pred13, "ff_pred_add_13_avx2";
        }

        raw_map_color!(MapColor, map_color, "ff_map_color32_avx2");
        raw_color_row!(ColorRow, color_row, "ff_color_row_avx2");
        raw_blend_row!(ExtractGreen, extract_green, "ff_extract_green_avx2");
        raw_blend_row!(Blend, blend, "ff_blend_row_argb_avx2");
        raw_blend_row!(
            BlendPremult,
            blend_premult,
            "ff_blend_row_argb_premult_avx2"
        );
    }

    pub fn init(dsp: &mut Vp8lDsp, flags: CpuFlags) {
        if flags.contains(CpuFlags::SSSE3) {
            dsp.color_row = color_row::<ssse3::ColorRow>;
            dsp.blend_row_argb_premult = blend_row::<ssse3::BlendPremult>;
        }
        if flags.contains(CpuFlags::AVX2) {
            dsp.pred_add = pred_table!(avx2);
            dsp.map_color32 = map_color32::<avx2::MapColor>;
            dsp.color_row = color_row::<avx2::ColorRow>;
            dsp.extract_green = extract_green::<avx2::ExtractGreen>;
            dsp.blend_row_argb = blend_row::<avx2::Blend>;
            dsp.blend_row_argb_premult = blend_row::<avx2::BlendPremult>;
        }
    }

    pub fn raw_table(flags: CpuFlags) -> RawTable {
        let mut t = RawTable::default();

        if flags.contains(CpuFlags::SSSE3) {
            t.color_row = Some(ssse3::ColorRow::F);
            t.blend_row_argb_premult = Some(ssse3::BlendPremult::F);
        }
        if flags.contains(CpuFlags::AVX2) {
            t.pred_add = Some(raw_pred_table!(avx2));
            t.map_color32 = Some(avx2::MapColor::F);
            t.color_row = Some(avx2::ColorRow::F);
            t.extract_green = Some(avx2::ExtractGreen::F);
            t.blend_row_argb = Some(avx2::Blend::F);
            t.blend_row_argb_premult = Some(avx2::BlendPremult::F);
        }
        t
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;

    pub mod neon {
        use super::*;

        preds! {
            Pred0, pred0, "ff_pred_add_0_neon";
            Pred1, pred1, "ff_pred_add_1_neon";
            Pred2, pred2, "ff_pred_add_2_neon";
            Pred3, pred3, "ff_pred_add_3_neon";
            Pred4, pred4, "ff_pred_add_4_neon";
            Pred5, pred5, "ff_pred_add_5_neon";
            Pred6, pred6, "ff_pred_add_6_neon";
            Pred7, pred7, "ff_pred_add_7_neon";
            Pred8, pred8, "ff_pred_add_8_neon";
            Pred9, pred9, "ff_pred_add_9_neon";
            Pred10, pred10, "ff_pred_add_10_neon";
            Pred11, pred11, "ff_pred_add_11_neon";
            Pred12, pred12, "ff_pred_add_12_neon";
            Pred13, pred13, "ff_pred_add_13_neon";
        }

        raw_map_color!(MapColor, map_color, "ff_map_color32_neon");
        raw_blend_row!(ExtractGreen, extract_green, "ff_extract_green_neon");
        raw_blend_row!(Blend, blend, "ff_blend_row_argb_neon");
        raw_blend_row!(
            BlendPremult,
            blend_premult,
            "ff_blend_row_argb_premult_neon"
        );
    }

    pub fn init(dsp: &mut Vp8lDsp, flags: CpuFlags) {
        if !flags.contains(CpuFlags::NEON) {
            return;
        }
        dsp.pred_add = pred_table!(neon);
        dsp.map_color32 = map_color32::<neon::MapColor>;
        dsp.extract_green = extract_green::<neon::ExtractGreen>;
        dsp.blend_row_argb = blend_row::<neon::Blend>;
        dsp.blend_row_argb_premult = blend_row::<neon::BlendPremult>;
    }

    pub fn raw_table(flags: CpuFlags) -> RawTable {
        let mut t = RawTable::default();

        if !flags.contains(CpuFlags::NEON) {
            return t;
        }
        t.pred_add = Some(raw_pred_table!(neon));
        t.map_color32 = Some(neon::MapColor::F);
        t.extract_green = Some(neon::ExtractGreen::F);
        t.blend_row_argb = Some(neon::Blend::F);
        t.blend_row_argb_premult = Some(neon::BlendPremult::F);
        t
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
mod arch {
    use super::*;

    pub fn init(_dsp: &mut Vp8lDsp, _flags: CpuFlags) {}

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
