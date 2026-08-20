use crate::cpu::CpuFlags;
use crate::dsp::vp8pred::*;

pub type PredRaw = unsafe extern "C" fn(*mut u8, isize);
pub type Pred4x4Raw = unsafe extern "C" fn(*mut u8, *const u8, isize);

pub use super::vp8::Raw;

pub struct RawTable {
    pub pred4x4: [Option<Pred4x4Raw>; PRED4X4_COUNT],
    pub pred8x8: [Option<PredRaw>; PRED8X8_COUNT],
    pub pred16x16: [Option<PredRaw>; PRED8X8_COUNT],
}

impl Default for RawTable {
    fn default() -> Self {
        RawTable {
            pred4x4: [None; PRED4X4_COUNT],
            pred8x8: [None; PRED8X8_COUNT],
            pred16x16: [None; PRED8X8_COUNT],
        }
    }
}

macro_rules! raw_pred {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *mut u8, _: isize);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = PredRaw;
            const F: PredRaw = $inner;
        }
    };
}

macro_rules! raw_pred4x4 {
    ($marker:ident, $inner:ident, $sym:literal) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner(_: *mut u8, _: *const u8, _: isize);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = Pred4x4Raw;
            const F: Pred4x4Raw = $inner;
        }
    };
}

#[inline(always)]
fn check(p: &[u8], o: usize, s: usize, n: usize) {
    assert!(o > s && p.len() >= o + (n - 1) * s + n, "plane too small");
}

fn pred<T: Raw<Sig = PredRaw>, const N: usize>(p: &mut [u8], o: usize, s: usize) {
    check(p, o, s, N);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize) }
}

fn pred4x4<T: Raw<Sig = Pred4x4Raw>>(p: &mut [u8], o: usize, s: usize, tr: &[u8; 4]) {
    check(p, o, s, 4);
    unsafe { (T::F)(p.as_mut_ptr().add(o), tr.as_ptr(), s as isize) }
}

#[allow(unused_macros)]
macro_rules! ladder {
    ($(
        $flag:ident {
            $( $table:ident[$idx:ident] = $wrap:ident::<$marker:path $(, $n:tt)?>; )*
        }
    )*) => {
        pub fn init(p: &mut Vp8Pred, flags: CpuFlags) {
            $(if flags.contains(CpuFlags::$flag) {
                $( p.$table[$idx] = $wrap::<$marker $(, $n)?>; )*
            })*
        }

        pub fn raw_table(flags: CpuFlags) -> RawTable {
            let mut t = RawTable::default();

            $(if flags.contains(CpuFlags::$flag) {
                $( t.$table[$idx] = Some(<$marker as Raw>::F); )*
            })*
            t
        }
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;

    pub mod sse {
        use super::*;

        raw_pred!(Vert16, vert16, "ff_pred16x16_vertical_8_sse");
    }

    pub mod sse2 {
        use super::*;

        raw_pred4x4!(Dc4, dc4, "ff_pred4x4_dc_8_sse2");
        raw_pred4x4!(Hor4, hor4, "ff_pred4x4_horizontal_vp8_8_sse2");
        raw_pred4x4!(Vert4, vert4, "ff_pred4x4_vertical_vp8_8_sse2");
        raw_pred4x4!(DownLeft4, down_left4, "ff_pred4x4_down_left_8_sse2");
        raw_pred4x4!(DownRight4, down_right4, "ff_pred4x4_down_right_8_sse2");
        raw_pred4x4!(VertRight4, vert_right4, "ff_pred4x4_vertical_right_8_sse2");
        raw_pred4x4!(HorDown4, hor_down4, "ff_pred4x4_horizontal_down_8_sse2");
        raw_pred4x4!(HorUp4, hor_up4, "ff_pred4x4_horizontal_up_8_sse2");
        raw_pred4x4!(Tm4, tm4, "ff_pred4x4_tm_vp8_8_sse2");

        raw_pred!(Dc8, dc8, "ff_pred8x8_dc_vp8_8_sse2");
        raw_pred!(TopDc8, top_dc8, "ff_pred8x8_top_dc_8_sse2");
        raw_pred!(LeftDc8, left_dc8, "ff_pred8x8_left_dc_8_sse2");
        raw_pred!(Hor8, hor8, "ff_pred8x8_horizontal_8_sse2");
        raw_pred!(Tm8, tm8, "ff_pred8x8_tm_vp8_8_sse2");
        raw_pred!(Vert8, vert8, "ff_pred8x8_vertical_8_sse2");

        raw_pred!(Hor16, hor16, "ff_pred16x16_horizontal_8_sse2");
        raw_pred!(Dc16, dc16, "ff_pred16x16_dc_8_sse2");
        raw_pred!(TopDc16, top_dc16, "ff_pred16x16_top_dc_8_sse2");
        raw_pred!(LeftDc16, left_dc16, "ff_pred16x16_left_dc_8_sse2");
        raw_pred!(Tm16, tm16, "ff_pred16x16_tm_vp8_8_sse2");
    }

    pub mod ssse3 {
        use super::*;

        raw_pred4x4!(Tm4, tm4, "ff_pred4x4_tm_vp8_8_ssse3");
        raw_pred4x4!(
            VertLeft4,
            vert_left4,
            "ff_pred4x4_vertical_left_vp8_8_ssse3"
        );

        raw_pred!(TopDc8, top_dc8, "ff_pred8x8_top_dc_8_ssse3");
        raw_pred!(LeftDc8, left_dc8, "ff_pred8x8_left_dc_8_ssse3");
        raw_pred!(Hor8, hor8, "ff_pred8x8_horizontal_8_ssse3");
        raw_pred!(Tm8, tm8, "ff_pred8x8_tm_vp8_8_ssse3");

        raw_pred!(Hor16, hor16, "ff_pred16x16_horizontal_8_ssse3");
        raw_pred!(Dc16, dc16, "ff_pred16x16_dc_8_ssse3");
        raw_pred!(TopDc16, top_dc16, "ff_pred16x16_top_dc_8_ssse3");
        raw_pred!(LeftDc16, left_dc16, "ff_pred16x16_left_dc_8_ssse3");
        raw_pred!(Tm16, tm16, "ff_pred16x16_tm_vp8_8_ssse3");
    }

    pub mod avx2 {
        use super::*;

        raw_pred!(Tm16, tm16, "ff_pred16x16_tm_vp8_8_avx2");
    }

    ladder! {
        SSE {
            pred16x16[VERT_PRED8X8] = pred::<sse::Vert16, 16>;
        }
        SSE2 {
            pred4x4[DIAG_DOWN_LEFT_PRED] = pred4x4::<sse2::DownLeft4>;
            pred4x4[DIAG_DOWN_RIGHT_PRED] = pred4x4::<sse2::DownRight4>;
            pred4x4[VERT_RIGHT_PRED] = pred4x4::<sse2::VertRight4>;
            pred4x4[HOR_DOWN_PRED] = pred4x4::<sse2::HorDown4>;
            pred4x4[HOR_UP_PRED] = pred4x4::<sse2::HorUp4>;
            pred4x4[DC_PRED] = pred4x4::<sse2::Dc4>;
            pred4x4[TM_VP8_PRED] = pred4x4::<sse2::Tm4>;
            pred4x4[VERT_PRED] = pred4x4::<sse2::Vert4>;
            pred4x4[HOR_PRED] = pred4x4::<sse2::Hor4>;

            pred8x8[DC_PRED8X8] = pred::<sse2::Dc8, 8>;
            pred8x8[HOR_PRED8X8] = pred::<sse2::Hor8, 8>;
            pred8x8[VERT_PRED8X8] = pred::<sse2::Vert8, 8>;
            pred8x8[PLANE_PRED8X8] = pred::<sse2::Tm8, 8>;
            pred8x8[TOP_DC_PRED8X8] = pred::<sse2::TopDc8, 8>;
            pred8x8[LEFT_DC_PRED8X8] = pred::<sse2::LeftDc8, 8>;

            pred16x16[HOR_PRED8X8] = pred::<sse2::Hor16, 16>;
            pred16x16[DC_PRED8X8] = pred::<sse2::Dc16, 16>;
            pred16x16[PLANE_PRED8X8] = pred::<sse2::Tm16, 16>;
            pred16x16[TOP_DC_PRED8X8] = pred::<sse2::TopDc16, 16>;
            pred16x16[LEFT_DC_PRED8X8] = pred::<sse2::LeftDc16, 16>;
        }
        SSSE3 {
            pred4x4[TM_VP8_PRED] = pred4x4::<ssse3::Tm4>;
            pred4x4[VERT_LEFT_PRED] = pred4x4::<ssse3::VertLeft4>;

            pred8x8[HOR_PRED8X8] = pred::<ssse3::Hor8, 8>;
            pred8x8[PLANE_PRED8X8] = pred::<ssse3::Tm8, 8>;
            pred8x8[TOP_DC_PRED8X8] = pred::<ssse3::TopDc8, 8>;
            pred8x8[LEFT_DC_PRED8X8] = pred::<ssse3::LeftDc8, 8>;

            pred16x16[PLANE_PRED8X8] = pred::<ssse3::Tm16, 16>;
            pred16x16[HOR_PRED8X8] = pred::<ssse3::Hor16, 16>;
            pred16x16[DC_PRED8X8] = pred::<ssse3::Dc16, 16>;
            pred16x16[TOP_DC_PRED8X8] = pred::<ssse3::TopDc16, 16>;
            pred16x16[LEFT_DC_PRED8X8] = pred::<ssse3::LeftDc16, 16>;
        }
        AVX2 {
            pred16x16[PLANE_PRED8X8] = pred::<avx2::Tm16, 16>;
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;

    pub mod neon {
        use super::*;

        raw_pred4x4!(Tm4, tm4, "ff_pred4x4_tm_neon");
        raw_pred4x4!(Dc4, dc4, "ff_pred4x4_dc_neon");
        raw_pred4x4!(Vert4, vert4, "ff_pred4x4_vert_neon");
        raw_pred4x4!(Hor4, hor4, "ff_pred4x4_hor_neon");
        raw_pred4x4!(DownLeft4, down_left4, "ff_pred4x4_down_left_neon");
        raw_pred4x4!(DownRight4, down_right4, "ff_pred4x4_down_right_neon");
        raw_pred4x4!(VertLeft4, vert_left4, "ff_pred4x4_vert_left_neon");
        raw_pred4x4!(VertRight4, vert_right4, "ff_pred4x4_vert_right_neon");
        raw_pred4x4!(HorUp4, hor_up4, "ff_pred4x4_hor_up_neon");
        raw_pred4x4!(HorDown4, hor_down4, "ff_pred4x4_hor_down_neon");

        raw_pred!(Vert8, vert8, "ff_pred8x8_vert_neon");
        raw_pred!(Hor8, hor8, "ff_pred8x8_hor_neon");
        raw_pred!(Dc8, dc8, "ff_pred8x8_dc_neon");
        raw_pred!(Tm8, tm8, "ff_pred8x8_tm_neon");
        raw_pred!(TopDc8, top_dc8, "ff_pred8x8_top_dc_neon");
        raw_pred!(LeftDc8, left_dc8, "ff_pred8x8_left_dc_neon");

        raw_pred!(Vert16, vert16, "ff_pred16x16_vert_neon");
        raw_pred!(Hor16, hor16, "ff_pred16x16_hor_neon");
        raw_pred!(Dc16, dc16, "ff_pred16x16_dc_neon");
        raw_pred!(Tm16, tm16, "ff_pred16x16_tm_neon");
        raw_pred!(TopDc16, top_dc16, "ff_pred16x16_top_dc_neon");
        raw_pred!(LeftDc16, left_dc16, "ff_pred16x16_left_dc_neon");
    }

    ladder! {
        NEON {
            pred4x4[TM_VP8_PRED] = pred4x4::<neon::Tm4>;
            pred4x4[DC_PRED] = pred4x4::<neon::Dc4>;
            pred4x4[VERT_PRED] = pred4x4::<neon::Vert4>;
            pred4x4[HOR_PRED] = pred4x4::<neon::Hor4>;
            pred4x4[DIAG_DOWN_LEFT_PRED] = pred4x4::<neon::DownLeft4>;
            pred4x4[DIAG_DOWN_RIGHT_PRED] = pred4x4::<neon::DownRight4>;
            pred4x4[VERT_LEFT_PRED] = pred4x4::<neon::VertLeft4>;
            pred4x4[VERT_RIGHT_PRED] = pred4x4::<neon::VertRight4>;
            pred4x4[HOR_UP_PRED] = pred4x4::<neon::HorUp4>;
            pred4x4[HOR_DOWN_PRED] = pred4x4::<neon::HorDown4>;

            pred8x8[VERT_PRED8X8] = pred::<neon::Vert8, 8>;
            pred8x8[HOR_PRED8X8] = pred::<neon::Hor8, 8>;
            pred8x8[DC_PRED8X8] = pred::<neon::Dc8, 8>;
            pred8x8[PLANE_PRED8X8] = pred::<neon::Tm8, 8>;
            pred8x8[TOP_DC_PRED8X8] = pred::<neon::TopDc8, 8>;
            pred8x8[LEFT_DC_PRED8X8] = pred::<neon::LeftDc8, 8>;

            pred16x16[DC_PRED8X8] = pred::<neon::Dc16, 16>;
            pred16x16[VERT_PRED8X8] = pred::<neon::Vert16, 16>;
            pred16x16[HOR_PRED8X8] = pred::<neon::Hor16, 16>;
            pred16x16[PLANE_PRED8X8] = pred::<neon::Tm16, 16>;
            pred16x16[TOP_DC_PRED8X8] = pred::<neon::TopDc16, 16>;
            pred16x16[LEFT_DC_PRED8X8] = pred::<neon::LeftDc16, 16>;
        }
    }
}

#[cfg(target_arch = "arm")]
mod arch {
    use super::*;

    pub mod neon {
        use super::*;

        raw_pred!(Vert8, vert8, "ff_pred8x8_vert_neon");
        raw_pred!(Hor8, hor8, "ff_pred8x8_hor_neon");
        raw_pred!(Dc128_8, dc128_8, "ff_pred8x8_128_dc_neon");

        raw_pred!(Dc16, dc16, "ff_pred16x16_dc_neon");
        raw_pred!(Vert16, vert16, "ff_pred16x16_vert_neon");
        raw_pred!(Hor16, hor16, "ff_pred16x16_hor_neon");
        raw_pred!(LeftDc16, left_dc16, "ff_pred16x16_left_dc_neon");
        raw_pred!(TopDc16, top_dc16, "ff_pred16x16_top_dc_neon");
        raw_pred!(Dc128_16, dc128_16, "ff_pred16x16_128_dc_neon");
    }

    ladder! {
        NEON {
            pred8x8[VERT_PRED8X8] = pred::<neon::Vert8, 8>;
            pred8x8[HOR_PRED8X8] = pred::<neon::Hor8, 8>;
            pred8x8[DC_128_PRED8X8] = pred::<neon::Dc128_8, 8>;

            pred16x16[DC_PRED8X8] = pred::<neon::Dc16, 16>;
            pred16x16[VERT_PRED8X8] = pred::<neon::Vert16, 16>;
            pred16x16[HOR_PRED8X8] = pred::<neon::Hor16, 16>;
            pred16x16[LEFT_DC_PRED8X8] = pred::<neon::LeftDc16, 16>;
            pred16x16[TOP_DC_PRED8X8] = pred::<neon::TopDc16, 16>;
            pred16x16[DC_128_PRED8X8] = pred::<neon::Dc128_16, 16>;
        }
    }
}

#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm"
)))]
mod arch {
    use super::*;

    pub fn init(_p: &mut Vp8Pred, _flags: CpuFlags) {}

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
