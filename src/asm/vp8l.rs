/* Which macro families an arch reaches for varies; several are dead on
 * targets whose assembly does not cover this DSP at all. */
#![allow(unused_macros)]

use crate::cpu::CpuFlags;
use crate::dsp::vp8l::Vp8lDsp;
use std::ffi::c_int;

pub type PredAddRaw = unsafe extern "C" fn(*const u32, *const u32, c_int, *mut u32);
pub type MapColorRaw = unsafe extern "C" fn(*mut u8, *const u8, *const u32, c_int);
pub type ColorRowRaw = unsafe extern "C" fn(*mut u32, *const u32, c_int, u32);
pub type BlendRowRaw = unsafe extern "C" fn(*mut u8, *const u8, c_int);

pub use super::Raw;

/* The kind picks the signature alias and its argument list. */
macro_rules! raw_vp8l {
    ($m:ident, $i:ident, pred_add, $sym:literal) => {
        raw!(
            $m,
            $i,
            PredAddRaw,
            $sym,
            (*const u32, *const u32, c_int, *mut u32)
        );
    };
    ($m:ident, $i:ident, map_color, $sym:literal) => {
        raw!(
            $m,
            $i,
            MapColorRaw,
            $sym,
            (*mut u8, *const u8, *const u32, c_int)
        );
    };
    ($m:ident, $i:ident, color_row, $sym:literal) => {
        raw!(
            $m,
            $i,
            ColorRowRaw,
            $sym,
            (*mut u32, *const u32, c_int, u32)
        );
    };
    ($m:ident, $i:ident, blend_row, $sym:literal) => {
        raw!($m, $i, BlendRowRaw, $sym, (*mut u8, *const u8, c_int));
    };
}

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

fn color_row<T: Raw<Sig = ColorRowRaw>>(row: &mut [u32], mult: u32) {
    unsafe {
        let p = row.as_mut_ptr();

        (T::F)(p, p.cast_const(), row.len() as c_int, mult)
    }
}

fn blend_row<T: Raw<Sig = BlendRowRaw>>(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len()) / 4;

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

fn extract_green<T: Raw<Sig = BlendRowRaw>>(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len() / 4);

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

#[derive(Default)]
pub struct RawTable {
    pub pred_add: [Option<PredAddRaw>; 14],
    pub extract_green: Option<BlendRowRaw>,
    pub map_color32: Option<MapColorRaw>,
    pub blend_row_argb: Option<BlendRowRaw>,
    pub blend_row_argb_premult: Option<BlendRowRaw>,
    pub color_row: Option<ColorRowRaw>,
}

macro_rules! pred_slot {
    ($set:ident, 0) => {
        pred_add::<$set::Pred0, false, false, false>
    };
    ($set:ident, 1) => {
        pred_add::<$set::Pred1, false, true, false>
    };
    ($set:ident, 2) => {
        pred_add::<$set::Pred2, true, false, false>
    };
    ($set:ident, 3) => {
        pred_add::<$set::Pred3, true, false, false>
    };
    ($set:ident, 4) => {
        pred_add::<$set::Pred4, true, false, true>
    };
    ($set:ident, 5) => {
        pred_add::<$set::Pred5, true, true, false>
    };
    ($set:ident, 6) => {
        pred_add::<$set::Pred6, true, true, true>
    };
    ($set:ident, 7) => {
        pred_add::<$set::Pred7, true, true, false>
    };
    ($set:ident, 8) => {
        pred_add::<$set::Pred8, true, false, true>
    };
    ($set:ident, 9) => {
        pred_add::<$set::Pred9, true, false, false>
    };
    ($set:ident, 10) => {
        pred_add::<$set::Pred10, true, true, true>
    };
    ($set:ident, 11) => {
        pred_add::<$set::Pred11, true, true, true>
    };
    ($set:ident, 12) => {
        pred_add::<$set::Pred12, true, true, true>
    };
    ($set:ident, 13) => {
        pred_add::<$set::Pred13, true, true, true>
    };
}

macro_rules! pred_raw {
    ($set:ident, 0) => {
        <$set::Pred0 as Raw>::F
    };
    ($set:ident, 1) => {
        <$set::Pred1 as Raw>::F
    };
    ($set:ident, 2) => {
        <$set::Pred2 as Raw>::F
    };
    ($set:ident, 3) => {
        <$set::Pred3 as Raw>::F
    };
    ($set:ident, 4) => {
        <$set::Pred4 as Raw>::F
    };
    ($set:ident, 5) => {
        <$set::Pred5 as Raw>::F
    };
    ($set:ident, 6) => {
        <$set::Pred6 as Raw>::F
    };
    ($set:ident, 7) => {
        <$set::Pred7 as Raw>::F
    };
    ($set:ident, 8) => {
        <$set::Pred8 as Raw>::F
    };
    ($set:ident, 9) => {
        <$set::Pred9 as Raw>::F
    };
    ($set:ident, 10) => {
        <$set::Pred10 as Raw>::F
    };
    ($set:ident, 11) => {
        <$set::Pred11 as Raw>::F
    };
    ($set:ident, 12) => {
        <$set::Pred12 as Raw>::F
    };
    ($set:ident, 13) => {
        <$set::Pred13 as Raw>::F
    };
}

macro_rules! preds {
    ($($marker:ident, $inner:ident, $sym:literal;)*) => {
        $(raw_vp8l!($marker, $inner, pred_add, $sym);)*
    };
}

macro_rules! ladder {
    ($(
        $flag:ident {
            $( @preds $preds:ident [ $($idx:tt),* ]; )?
            $( $field:ident = $wrap:ident::<$marker:path>; )*
        }
    )*) => {
        pub fn init(dsp: &mut Vp8lDsp, flags: CpuFlags) {
            $(if flags.contains(CpuFlags::$flag) {
                $( $( dsp.pred_add[$idx] = pred_slot!($preds, $idx); )* )?
                $( dsp.$field = $wrap::<$marker>; )*
            })*
        }

        pub fn raw_table(flags: CpuFlags) -> RawTable {
            let mut t = RawTable::default();

            $(if flags.contains(CpuFlags::$flag) {
                $( $( t.pred_add[$idx] = Some(pred_raw!($preds, $idx)); )* )?
                $( t.$field = Some(<$marker as Raw>::F); )*
            })*
            t
        }
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;

    pub mod sse2 {
        use super::*;

        preds! {
            Pred5, pred5, "ff_pred_add_5_sse2";
            Pred6, pred6, "ff_pred_add_6_sse2";
            Pred7, pred7, "ff_pred_add_7_sse2";
            Pred10, pred10, "ff_pred_add_10_sse2";
            Pred12, pred12, "ff_pred_add_12_sse2";
        }
    }

    pub mod sse4 {
        use super::*;

        preds! {
            Pred13, pred13, "ff_pred_add_13_sse4";
        }
    }

    pub mod ssse3 {
        use super::*;

        raw_vp8l!(ColorRow, color_row, color_row, "ff_color_row_ssse3");
        raw_vp8l!(
            BlendPremult,
            blend_premult,
            blend_row,
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
            Pred8, pred8, "ff_pred_add_8_avx2";
            Pred9, pred9, "ff_pred_add_9_avx2";
            Pred11, pred11, "ff_pred_add_11_avx2";
        }

        raw_vp8l!(MapColor, map_color, map_color, "ff_map_color32_avx2");
        raw_vp8l!(ColorRow, color_row, color_row, "ff_color_row_avx2");
        raw_vp8l!(
            ExtractGreen,
            extract_green,
            blend_row,
            "ff_extract_green_avx2"
        );
        raw_vp8l!(Blend, blend, blend_row, "ff_blend_row_argb_avx2");
        raw_vp8l!(
            BlendPremult,
            blend_premult,
            blend_row,
            "ff_blend_row_argb_premult_avx2"
        );
    }

    ladder! {
        SSE2 {
            @preds sse2 [5, 6, 7, 10, 12];
        }
        SSE41 {
            @preds sse4 [13];
        }
        SSSE3 {
            color_row = color_row::<ssse3::ColorRow>;
            blend_row_argb_premult = blend_row::<ssse3::BlendPremult>;
        }
        AVX2 {
            @preds avx2 [0, 1, 2, 3, 4, 8, 9, 11];
            map_color32 = map_color32::<avx2::MapColor>;
            color_row = color_row::<avx2::ColorRow>;
            extract_green = extract_green::<avx2::ExtractGreen>;
            blend_row_argb = blend_row::<avx2::Blend>;
            blend_row_argb_premult = blend_row::<avx2::BlendPremult>;
        }
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

        raw_vp8l!(MapColor, map_color, map_color, "ff_map_color32_neon");
        raw_vp8l!(ColorRow, color_row, color_row, "ff_color_row_neon");
        raw_vp8l!(
            ExtractGreen,
            extract_green,
            blend_row,
            "ff_extract_green_neon"
        );
        raw_vp8l!(Blend, blend, blend_row, "ff_blend_row_argb_neon");
        raw_vp8l!(
            BlendPremult,
            blend_premult,
            blend_row,
            "ff_blend_row_argb_premult_neon"
        );
    }

    ladder! {
        NEON {
            @preds neon [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
            map_color32 = map_color32::<neon::MapColor>;
            color_row = color_row::<neon::ColorRow>;
            extract_green = extract_green::<neon::ExtractGreen>;
            blend_row_argb = blend_row::<neon::Blend>;
            blend_row_argb_premult = blend_row::<neon::BlendPremult>;
        }
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
