/* Which macro families an arch reaches for varies; several are dead on
 * targets whose assembly does not cover this DSP at all. */
#![allow(unused_macros)]

use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::yuv::{bpp, UpsampleDst, UpsampleSrc, YuvDsp, UPSAMPLE_BLOCK};

pub(crate) use super::Raw;

pub type UpsampleBlockRaw = unsafe extern "C" fn(
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *mut u8,
    *mut u8,
    c_int,
);
pub type RowRaw = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type YuvRowRaw =
    unsafe extern "C" fn(*mut u8, *const u8, *const u8, *const u8, c_int);
pub type PremultiplyRaw = unsafe extern "C" fn(*mut u8, c_int, c_int);
pub type Premultiply4444Raw = unsafe extern "C" fn(*mut u8, c_int);
pub type ArgbToYuv444Raw =
    unsafe extern "C" fn(*mut u8, *mut u8, *mut u8, *const u8, c_int);
pub type ArgbToUvRaw =
    unsafe extern "C" fn(*mut u8, *mut u8, *const u8, isize, c_int, c_int);
/* Forward direction only; the inverse divides per pixel and stays scalar. */
pub type MultiplyRowRaw = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type MultiplyArgbRaw = unsafe extern "C" fn(*mut u8, c_int);

macro_rules! raw_upsample {
    ($marker:ident, $inner:ident, $sym:literal) => {
        raw!(
            $marker,
            $inner,
            UpsampleBlockRaw,
            $sym,
            (
                *const u8,
                *const u8,
                *const u8,
                *const u8,
                *const u8,
                *const u8,
                *mut u8,
                *mut u8,
                c_int
            )
        );
    };
}

macro_rules! raw_row {
    ($marker:ident, $inner:ident, $sym:literal) => {
        raw!($marker, $inner, RowRaw, $sym, (*mut u8, *const u8, c_int));
    };
}

fn upsample_block<T: Raw<Sig = UpsampleBlockRaw>, const L: usize>(
    src: &UpsampleSrc<'_>,
    dst: &mut UpsampleDst<'_>,
    blocks: usize,
) {
    let last = blocks * (UPSAMPLE_BLOCK / 2);
    let pixels = 2 * last;

    assert!(src.top_y.len() >= pixels, "short luma row");
    assert!(
        src.top_u.len() > last
            && src.top_v.len() > last
            && src.cur_u.len() > last
            && src.cur_v.len() > last,
        "short chroma row"
    );
    assert!(dst.top.len() >= bpp(L) * pixels, "short output row");
    assert_eq!(
        src.bottom_y.is_some(),
        dst.bottom.is_some(),
        "a bottom luma row needs a bottom output row"
    );
    if let (Some(y), Some(d)) = (src.bottom_y, dst.bottom.as_deref()) {
        assert!(y.len() >= pixels, "short luma row");
        assert!(d.len() >= bpp(L) * pixels, "short output row");
    }
    unsafe {
        (T::F)(
            src.top_y.as_ptr(),
            src.bottom_y.map_or(std::ptr::null(), <[u8]>::as_ptr),
            src.top_u.as_ptr(),
            src.top_v.as_ptr(),
            src.cur_u.as_ptr(),
            src.cur_v.as_ptr(),
            dst.top.as_mut_ptr(),
            dst.bottom
                .as_deref_mut()
                .map_or(std::ptr::null_mut(), <[u8]>::as_mut_ptr),
            blocks as c_int,
        );
    }
}

fn yuv444_row<T: Raw<Sig = YuvRowRaw>, const L: usize>(
    dst: &mut [u8],
    y: &[u8],
    u: &[u8],
    v: &[u8],
) {
    let n = (dst.len() / bpp(L)).min(y.len()).min(u.len()).min(v.len());

    unsafe {
        (T::F)(
            dst.as_mut_ptr(),
            y.as_ptr(),
            u.as_ptr(),
            v.as_ptr(),
            n as c_int,
        )
    }
}

fn yuv420_row<T: Raw<Sig = YuvRowRaw>, const L: usize>(
    dst: &mut [u8],
    y: &[u8],
    u: &[u8],
    v: &[u8],
) {
    let n = (dst.len() / bpp(L))
        .min(y.len())
        .min(2 * u.len())
        .min(2 * v.len());

    unsafe {
        (T::F)(
            dst.as_mut_ptr(),
            y.as_ptr(),
            u.as_ptr(),
            v.as_ptr(),
            n as c_int,
        )
    }
}

fn dispatch_alpha<T: Raw<Sig = RowRaw>>(dst: &mut [u8], src: &[u8]) {
    let n = (dst.len() / 4).min(src.len());

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

fn pack_row<T: Raw<Sig = RowRaw>, const BPP: usize>(dst: &mut [u8], src: &[u8]) {
    let n = (dst.len() / BPP).min(src.len() / 4);

    unsafe { (T::F)(dst.as_mut_ptr(), src.as_ptr(), n as c_int) }
}

fn premultiply_row<T: Raw<Sig = PremultiplyRaw>>(rgba: &mut [u8], alpha_first: bool) {
    let n = rgba.len() / 4;

    unsafe { (T::F)(rgba.as_mut_ptr(), c_int::from(alpha_first), n as c_int) }
}

fn premultiply_row_4444<T: Raw<Sig = Premultiply4444Raw>>(row: &mut [u8]) {
    let n = row.len() / 2;

    unsafe { (T::F)(row.as_mut_ptr(), n as c_int) }
}

fn multiply_row<T: Raw<Sig = MultiplyRowRaw>>(
    plane: &mut [u8],
    alpha: &[u8],
    inverse: bool,
) {
    if inverse {
        return crate::dsp::yuv::multiply_row(plane, alpha, true);
    }

    let n = plane.len().min(alpha.len());

    unsafe { (T::F)(plane.as_mut_ptr(), alpha.as_ptr(), n as c_int) }
}

fn premultiply_argb_row<T: Raw<Sig = MultiplyArgbRaw>>(argb: &mut [u8], inverse: bool) {
    if inverse {
        return crate::dsp::yuv::premultiply_argb_row(argb, true);
    }
    unsafe { (T::F)(argb.as_mut_ptr(), (argb.len() / 4) as c_int) }
}

fn argb_to_y<T: Raw<Sig = RowRaw>>(y: &mut [u8], argb: &[u8]) {
    let n = y.len().min(argb.len() / 4);

    unsafe { (T::F)(y.as_mut_ptr(), argb.as_ptr(), n as c_int) }
}

fn argb_to_yuv444<T: Raw<Sig = ArgbToYuv444Raw>>(
    y: &mut [u8],
    u: &mut [u8],
    v: &mut [u8],
    argb: &[u8],
) {
    let n = y.len().min(u.len()).min(v.len()).min(argb.len() / 4);

    unsafe {
        (T::F)(
            y.as_mut_ptr(),
            u.as_mut_ptr(),
            v.as_mut_ptr(),
            argb.as_ptr(),
            n as c_int,
        )
    }
}

fn argb_to_uv<T: Raw<Sig = ArgbToUvRaw>>(
    u: &mut [u8],
    v: &mut [u8],
    argb: &[u8],
    stride: usize,
    n: usize,
    weight_alpha: bool,
) {
    assert!(argb.len() >= 4 * n + stride, "short pixel row pair");
    assert!(
        u.len() >= n.div_ceil(2) && v.len() >= n.div_ceil(2),
        "short chroma row"
    );
    unsafe {
        (T::F)(
            u.as_mut_ptr(),
            v.as_mut_ptr(),
            argb.as_ptr(),
            stride as isize,
            n as c_int,
            c_int::from(weight_alpha),
        )
    }
}

#[derive(Default)]
pub struct RawTable {
    pub upsample_block: Option<[UpsampleBlockRaw; 5]>,
    pub upsample_rgb: Option<UpsampleBlockRaw>,
    pub upsample_bgr: Option<UpsampleBlockRaw>,
    pub yuv444_row: Option<[YuvRowRaw; 5]>,
    pub yuv420_row: Option<[YuvRowRaw; 5]>,
    pub yuv444_row_rgb: Option<[YuvRowRaw; 2]>,
    pub yuv420_row_rgb: Option<[YuvRowRaw; 2]>,
    pub dispatch_alpha_first: Option<RowRaw>,
    pub dispatch_alpha_last: Option<RowRaw>,
    pub packers: Option<[RowRaw; 8]>,
    pub premultiply_row: Option<PremultiplyRaw>,
    pub premultiply_row_4444: Option<Premultiply4444Raw>,
    pub premultiply_row_4444_swap: Option<Premultiply4444Raw>,
    pub multiply_row: Option<MultiplyRowRaw>,
    pub premultiply_argb_row: Option<MultiplyArgbRaw>,
    pub argb_to_y: Option<RowRaw>,
    pub argb_to_yuv444: Option<ArgbToYuv444Raw>,
    pub argb_to_uv: Option<ArgbToUvRaw>,
}

macro_rules! raw_packers {
    ($set:ident) => {
        [
            $set::PackRgba::F,
            $set::PackBgra::F,
            $set::PackRgb::F,
            $set::PackBgr::F,
            $set::PackRgb565::F,
            $set::PackRgba4444::F,
            $set::PackBgr565::F,
            $set::PackBgra4444::F,
        ]
    };
}

macro_rules! raw_upsample_table {
    ($set:ident) => {
        [
            $set::UpsampleArgb::F,
            $set::UpsampleRgba::F,
            $set::UpsampleBgra::F,
            $set::UpsampleRgb::F,
            $set::UpsampleBgr::F,
        ]
    };
}

macro_rules! raw_yuv_row {
    ($marker:ident, $inner:ident, $sym:literal) => {
        raw!(
            $marker,
            $inner,
            YuvRowRaw,
            $sym,
            (*mut u8, *const u8, *const u8, *const u8, c_int)
        );
    };
}

macro_rules! yuv_row_syms {
    ($argb444:literal, $rgba444:literal, $bgra444:literal, $rgb444:literal,
     $bgr444:literal, $argb420:literal, $rgba420:literal, $bgra420:literal,
     $rgb420:literal, $bgr420:literal) => {
        raw_yuv_row!(Yuv444Argb, yuv444_argb, $argb444);
        raw_yuv_row!(Yuv444Rgba, yuv444_rgba, $rgba444);
        raw_yuv_row!(Yuv444Bgra, yuv444_bgra, $bgra444);
        raw_yuv_row!(Yuv420Argb, yuv420_argb, $argb420);
        raw_yuv_row!(Yuv420Rgba, yuv420_rgba, $rgba420);
        raw_yuv_row!(Yuv420Bgra, yuv420_bgra, $bgra420);
        yuv_row_rgb_syms!($rgb444, $bgr444, $rgb420, $bgr420);
    };
}

macro_rules! yuv_row_rgb_syms {
    ($rgb444:literal, $bgr444:literal, $rgb420:literal, $bgr420:literal) => {
        raw_yuv_row!(Yuv444Rgb, yuv444_rgb, $rgb444);
        raw_yuv_row!(Yuv444Bgr, yuv444_bgr, $bgr444);
        raw_yuv_row!(Yuv420Rgb, yuv420_rgb, $rgb420);
        raw_yuv_row!(Yuv420Bgr, yuv420_bgr, $bgr420);
    };
}

macro_rules! yuv_rows {
    ($dsp:ident, $set:ident) => {
        $dsp.yuv444_row = [
            yuv444_row::<$set::Yuv444Argb, LAYOUT_ARGB>,
            yuv444_row::<$set::Yuv444Rgba, LAYOUT_RGBA>,
            yuv444_row::<$set::Yuv444Bgra, LAYOUT_BGRA>,
            yuv444_row::<$set::Yuv444Rgb, LAYOUT_RGB>,
            yuv444_row::<$set::Yuv444Bgr, LAYOUT_BGR>,
        ];
        $dsp.yuv420_row = [
            yuv420_row::<$set::Yuv420Argb, LAYOUT_ARGB>,
            yuv420_row::<$set::Yuv420Rgba, LAYOUT_RGBA>,
            yuv420_row::<$set::Yuv420Bgra, LAYOUT_BGRA>,
            yuv420_row::<$set::Yuv420Rgb, LAYOUT_RGB>,
            yuv420_row::<$set::Yuv420Bgr, LAYOUT_BGR>,
        ];
    };
}

macro_rules! yuv_rows_rgb {
    ($dsp:ident, $set:ident) => {
        $dsp.yuv444_row[LAYOUT_RGB] = yuv444_row::<$set::Yuv444Rgb, LAYOUT_RGB>;
        $dsp.yuv444_row[LAYOUT_BGR] = yuv444_row::<$set::Yuv444Bgr, LAYOUT_BGR>;
        $dsp.yuv420_row[LAYOUT_RGB] = yuv420_row::<$set::Yuv420Rgb, LAYOUT_RGB>;
        $dsp.yuv420_row[LAYOUT_BGR] = yuv420_row::<$set::Yuv420Bgr, LAYOUT_BGR>;
    };
}

macro_rules! packers {
    ($dsp:ident, $set:ident) => {
        $dsp.pack_rgba = pack_row::<$set::PackRgba, 4>;
        $dsp.pack_bgra = pack_row::<$set::PackBgra, 4>;
        $dsp.pack_rgb = pack_row::<$set::PackRgb, 3>;
        $dsp.pack_bgr = pack_row::<$set::PackBgr, 3>;
        $dsp.pack_rgb565 = pack_row::<$set::PackRgb565, 2>;
        $dsp.pack_rgba4444 = pack_row::<$set::PackRgba4444, 2>;
        $dsp.pack_bgr565 = pack_row::<$set::PackBgr565, 2>;
        $dsp.pack_bgra4444 = pack_row::<$set::PackBgra4444, 2>;
    };
}

macro_rules! pack_syms {
    ($rgba:literal, $bgra:literal, $rgb:literal, $bgr:literal,
     $rgb565:literal, $rgba4444:literal, $bgr565:literal, $bgra4444:literal) => {
        raw_row!(PackRgba, pack_rgba, $rgba);
        raw_row!(PackBgra, pack_bgra, $bgra);
        raw_row!(PackRgb, pack_rgb, $rgb);
        raw_row!(PackBgr, pack_bgr, $bgr);
        raw_row!(PackRgb565, pack_rgb565, $rgb565);
        raw_row!(PackRgba4444, pack_rgba4444, $rgba4444);
        raw_row!(PackBgr565, pack_bgr565, $bgr565);
        raw_row!(PackBgra4444, pack_bgra4444, $bgra4444);
    };
}

macro_rules! premultiply_syms {
    ($row:literal) => {
        raw!(
            Premultiply,
            premultiply,
            PremultiplyRaw,
            $row,
            (*mut u8, c_int, c_int)
        );
    };
}

macro_rules! multiply_syms {
    ($row:literal, $argb:literal) => {
        raw!(
            MultiplyRow,
            multiply_row,
            MultiplyRowRaw,
            $row,
            (*mut u8, *const u8, c_int)
        );
        raw!(
            MultiplyArgb,
            multiply_argb,
            MultiplyArgbRaw,
            $argb,
            (*mut u8, c_int)
        );
    };
}

macro_rules! premultiply_4444_syms {
    ($p4444:literal, $swap:literal) => {
        raw!(
            Premultiply4444,
            premultiply_4444,
            Premultiply4444Raw,
            $p4444,
            (*mut u8, c_int)
        );
        raw!(
            Premultiply4444Swap,
            premultiply_4444_swap,
            Premultiply4444Raw,
            $swap,
            (*mut u8, c_int)
        );
    };
}

macro_rules! upsample_table {
    ($dsp:ident, $set:ident) => {
        $dsp.upsample_block = [
            upsample_block::<$set::UpsampleArgb, LAYOUT_ARGB>,
            upsample_block::<$set::UpsampleRgba, LAYOUT_RGBA>,
            upsample_block::<$set::UpsampleBgra, LAYOUT_BGRA>,
            upsample_block::<$set::UpsampleRgb, LAYOUT_RGB>,
            upsample_block::<$set::UpsampleBgr, LAYOUT_BGR>,
        ];
    };
}

macro_rules! upsample_syms {
    ($argb:literal, $rgba:literal, $bgra:literal, $rgb:literal, $bgr:literal) => {
        raw_upsample!(UpsampleArgb, upsample_argb, $argb);
        raw_upsample!(UpsampleRgba, upsample_rgba, $rgba);
        raw_upsample!(UpsampleBgra, upsample_bgra, $bgra);
        raw_upsample!(UpsampleRgb, upsample_rgb, $rgb);
        raw_upsample!(UpsampleBgr, upsample_bgr, $bgr);
    };
}

macro_rules! ladder {
    ($(
        $(#[$attr:meta])*
        $($flag:ident)|+ {
            $( @upsample $up:ident; )?
            $( @upsample_rgb $up_rgb:ident; )?
            $( @upsample_bgr $up_bgr:ident; )?
            $( @packers $packers:ident; )?
            $( @rows $rows:ident; )?
            $( @rows_rgb $rows_rgb:ident; )?
            $( $field:ident = $wrap:ident::<$marker:path>; )*
        }
    )*) => {
        pub fn init(dsp: &mut YuvDsp, flags: CpuFlags) {
            $(
                $(#[$attr])*
                if flags.contains(CpuFlags::NONE$(.union(CpuFlags::$flag))+) {
                    $( upsample_table!(dsp, $up); )?
                    $( dsp.upsample_block[LAYOUT_RGB] =
                        upsample_block::<$up_rgb::UpsampleRgb, LAYOUT_RGB>; )?
                    $( dsp.upsample_block[LAYOUT_BGR] =
                        upsample_block::<$up_bgr::UpsampleBgr, LAYOUT_BGR>; )?
                    $( packers!(dsp, $packers); )?
                    $( yuv_rows!(dsp, $rows); )?
                    $( yuv_rows_rgb!(dsp, $rows_rgb); )?
                    $( dsp.$field = $wrap::<$marker>; )*
                }
            )*
        }

        pub fn raw_table(flags: CpuFlags) -> RawTable {
            let mut t = RawTable::default();

            $(
                $(#[$attr])*
                if flags.contains(CpuFlags::NONE$(.union(CpuFlags::$flag))+) {
                    $(
                        t.upsample_block = Some(raw_upsample_table!($up));
                        t.upsample_rgb = None;
                        t.upsample_bgr = None;
                    )?
                    $( t.upsample_rgb = Some($up_rgb::UpsampleRgb::F); )?
                    $( t.upsample_bgr = Some($up_bgr::UpsampleBgr::F); )?
                    $( t.packers = Some(raw_packers!($packers)); )?
                    $(
                        t.yuv444_row = Some([
                            $rows::Yuv444Argb::F,
                            $rows::Yuv444Rgba::F,
                            $rows::Yuv444Bgra::F,
                            $rows::Yuv444Rgb::F,
                            $rows::Yuv444Bgr::F,
                        ]);
                        t.yuv420_row = Some([
                            $rows::Yuv420Argb::F,
                            $rows::Yuv420Rgba::F,
                            $rows::Yuv420Bgra::F,
                            $rows::Yuv420Rgb::F,
                            $rows::Yuv420Bgr::F,
                        ]);
                        t.yuv444_row_rgb = None;
                        t.yuv420_row_rgb = None;
                    )?
                    $(
                        t.yuv444_row_rgb =
                            Some([$rows_rgb::Yuv444Rgb::F, $rows_rgb::Yuv444Bgr::F]);
                        t.yuv420_row_rgb =
                            Some([$rows_rgb::Yuv420Rgb::F, $rows_rgb::Yuv420Bgr::F]);
                    )?
                    $( t.$field = Some(<$marker as Raw>::F); )*
                }
            )*
            t
        }
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;
    use crate::dsp::yuv::{
        LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_RGB, LAYOUT_RGBA,
    };

    pub mod sse2 {
        use super::*;

        raw_row!(
            DispatchFirst,
            dispatch_first,
            "wpd_dispatch_alpha_first_sse2"
        );
        raw_row!(DispatchLast, dispatch_last, "wpd_dispatch_alpha_last_sse2");
        premultiply_4444_syms!(
            "wpd_premultiply_row_4444_sse2",
            "wpd_premultiply_row_4444_swap_sse2"
        );
        multiply_syms!("wpd_multiply_row_sse2", "wpd_premultiply_argb_row_sse2");

        #[cfg(target_arch = "x86_64")]
        upsample_syms!(
            "wpd_upsample_block_argb_sse2",
            "wpd_upsample_block_rgba_sse2",
            "wpd_upsample_block_bgra_sse2",
            "wpd_upsample_block_rgb_sse2",
            "wpd_upsample_block_bgr_sse2"
        );
        #[cfg(target_arch = "x86_64")]
        yuv_row_syms!(
            "wpd_yuv444_row_argb_sse2",
            "wpd_yuv444_row_rgba_sse2",
            "wpd_yuv444_row_bgra_sse2",
            "wpd_yuv444_row_rgb_sse2",
            "wpd_yuv444_row_bgr_sse2",
            "wpd_yuv420_row_argb_sse2",
            "wpd_yuv420_row_rgba_sse2",
            "wpd_yuv420_row_bgra_sse2",
            "wpd_yuv420_row_rgb_sse2",
            "wpd_yuv420_row_bgr_sse2"
        );
    }

    pub mod ssse3 {
        use super::*;

        pack_syms!(
            "wpd_pack_rgba_ssse3",
            "wpd_pack_bgra_ssse3",
            "wpd_pack_rgb_ssse3",
            "wpd_pack_bgr_ssse3",
            "wpd_pack_rgb565_ssse3",
            "wpd_pack_rgba4444_ssse3",
            "wpd_pack_bgr565_ssse3",
            "wpd_pack_bgra4444_ssse3"
        );
        premultiply_syms!("wpd_premultiply_row_ssse3");
        raw_row!(ArgbToY, argb_to_y, "wpd_argb_to_y_ssse3");

        #[cfg(target_arch = "x86_64")]
        raw_upsample!(UpsampleRgb, upsample_rgb, "wpd_upsample_block_rgb_ssse3");
        #[cfg(target_arch = "x86_64")]
        raw_upsample!(UpsampleBgr, upsample_bgr, "wpd_upsample_block_bgr_ssse3");
        #[cfg(target_arch = "x86_64")]
        yuv_row_rgb_syms!(
            "wpd_yuv444_row_rgb_ssse3",
            "wpd_yuv444_row_bgr_ssse3",
            "wpd_yuv420_row_rgb_ssse3",
            "wpd_yuv420_row_bgr_ssse3"
        );
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "wpd_argb_to_yuv444_ssse3",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    pub mod avx2 {
        use super::*;

        raw_row!(
            DispatchFirst,
            dispatch_first,
            "wpd_dispatch_alpha_first_avx2"
        );
        raw_row!(DispatchLast, dispatch_last, "wpd_dispatch_alpha_last_avx2");
        pack_syms!(
            "wpd_pack_rgba_avx2",
            "wpd_pack_bgra_avx2",
            "wpd_pack_rgb_avx2",
            "wpd_pack_bgr_avx2",
            "wpd_pack_rgb565_avx2",
            "wpd_pack_rgba4444_avx2",
            "wpd_pack_bgr565_avx2",
            "wpd_pack_bgra4444_avx2"
        );
        premultiply_syms!("wpd_premultiply_row_avx2");
        premultiply_4444_syms!(
            "wpd_premultiply_row_4444_avx2",
            "wpd_premultiply_row_4444_swap_avx2"
        );
        multiply_syms!("wpd_multiply_row_avx2", "wpd_premultiply_argb_row_avx2");
        raw_row!(ArgbToY, argb_to_y, "wpd_argb_to_y_avx2");

        #[cfg(target_arch = "x86_64")]
        upsample_syms!(
            "wpd_upsample_block_argb_avx2",
            "wpd_upsample_block_rgba_avx2",
            "wpd_upsample_block_bgra_avx2",
            "wpd_upsample_block_rgb_avx2",
            "wpd_upsample_block_bgr_avx2"
        );
        #[cfg(target_arch = "x86_64")]
        yuv_row_syms!(
            "wpd_yuv444_row_argb_avx2",
            "wpd_yuv444_row_rgba_avx2",
            "wpd_yuv444_row_bgra_avx2",
            "wpd_yuv444_row_rgb_avx2",
            "wpd_yuv444_row_bgr_avx2",
            "wpd_yuv420_row_argb_avx2",
            "wpd_yuv420_row_rgba_avx2",
            "wpd_yuv420_row_bgra_avx2",
            "wpd_yuv420_row_rgb_avx2",
            "wpd_yuv420_row_bgr_avx2"
        );
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "wpd_argb_to_yuv444_avx2",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToUv,
            argb_to_uv,
            ArgbToUvRaw,
            "wpd_argb_to_uv_avx2",
            (*mut u8, *mut u8, *const u8, isize, c_int, c_int)
        );
    }

    ladder! {
        #[cfg(target_arch = "x86_64")]
        SSE2 {
            @upsample sse2;
            @rows sse2;
        }
        SSE2 {
            dispatch_alpha_first = dispatch_alpha::<sse2::DispatchFirst>;
            dispatch_alpha_last = dispatch_alpha::<sse2::DispatchLast>;
            premultiply_row_4444 = premultiply_row_4444::<sse2::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<sse2::Premultiply4444Swap>;
            multiply_row = multiply_row::<sse2::MultiplyRow>;
            premultiply_argb_row = premultiply_argb_row::<sse2::MultiplyArgb>;
        }
        #[cfg(target_arch = "x86_64")]
        SSSE3 {
            @upsample_rgb ssse3;
            @upsample_bgr ssse3;
            @rows_rgb ssse3;

            argb_to_yuv444 = argb_to_yuv444::<ssse3::ArgbToYuv444>;
        }
        SSSE3 {
            @packers ssse3;

            premultiply_row = premultiply_row::<ssse3::Premultiply>;
            argb_to_y = argb_to_y::<ssse3::ArgbToY>;
        }
        #[cfg(target_arch = "x86_64")]
        AVX2 {
            @upsample avx2;
            @rows avx2;

            argb_to_yuv444 = argb_to_yuv444::<avx2::ArgbToYuv444>;
            argb_to_uv = argb_to_uv::<avx2::ArgbToUv>;
        }
        AVX2 {
            @packers avx2;

            premultiply_row = premultiply_row::<avx2::Premultiply>;
            premultiply_row_4444 = premultiply_row_4444::<avx2::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<avx2::Premultiply4444Swap>;
            multiply_row = multiply_row::<avx2::MultiplyRow>;
            premultiply_argb_row = premultiply_argb_row::<avx2::MultiplyArgb>;
            dispatch_alpha_first = dispatch_alpha::<avx2::DispatchFirst>;
            dispatch_alpha_last = dispatch_alpha::<avx2::DispatchLast>;
            argb_to_y = argb_to_y::<avx2::ArgbToY>;
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;
    use crate::dsp::yuv::{
        LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_RGB, LAYOUT_RGBA,
    };

    pub mod neon {
        use super::*;

        upsample_syms!(
            "wpd_upsample_block_argb_neon",
            "wpd_upsample_block_rgba_neon",
            "wpd_upsample_block_bgra_neon",
            "wpd_upsample_block_rgb_neon",
            "wpd_upsample_block_bgr_neon"
        );
        yuv_row_syms!(
            "wpd_yuv444_row_argb_neon",
            "wpd_yuv444_row_rgba_neon",
            "wpd_yuv444_row_bgra_neon",
            "wpd_yuv444_row_rgb_neon",
            "wpd_yuv444_row_bgr_neon",
            "wpd_yuv420_row_argb_neon",
            "wpd_yuv420_row_rgba_neon",
            "wpd_yuv420_row_bgra_neon",
            "wpd_yuv420_row_rgb_neon",
            "wpd_yuv420_row_bgr_neon"
        );
        pack_syms!(
            "wpd_pack_rgba_neon",
            "wpd_pack_bgra_neon",
            "wpd_pack_rgb_neon",
            "wpd_pack_bgr_neon",
            "wpd_pack_rgb565_neon",
            "wpd_pack_rgba4444_neon",
            "wpd_pack_bgr565_neon",
            "wpd_pack_bgra4444_neon"
        );
        premultiply_syms!("wpd_premultiply_row_neon");
        premultiply_4444_syms!(
            "wpd_premultiply_row_4444_neon",
            "wpd_premultiply_row_4444_swap_neon"
        );
        multiply_syms!("wpd_multiply_row_neon", "wpd_premultiply_argb_row_neon");
        raw_row!(
            DispatchFirst,
            dispatch_first,
            "wpd_dispatch_alpha_first_neon"
        );
        raw_row!(DispatchLast, dispatch_last, "wpd_dispatch_alpha_last_neon");
        raw_row!(ArgbToY, argb_to_y, "wpd_argb_to_y_neon");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "wpd_argb_to_yuv444_neon",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
        raw!(
            ArgbToUv,
            argb_to_uv,
            ArgbToUvRaw,
            "wpd_argb_to_uv_neon",
            (*mut u8, *mut u8, *const u8, isize, c_int, c_int)
        );
    }

    #[cfg(wpd_asm_dotprod)]
    pub mod dotprod {
        use super::*;

        raw_row!(ArgbToY, argb_to_y, "wpd_argb_to_y_neon_dotprod");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "wpd_argb_to_yuv444_neon_dotprod",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    #[cfg(wpd_asm_i8mm)]
    pub mod i8mm {
        use super::*;

        raw_row!(ArgbToY, argb_to_y, "wpd_argb_to_y_neon_i8mm");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "wpd_argb_to_yuv444_neon_i8mm",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    ladder! {
        NEON {
            @upsample neon;
            @packers neon;
            @rows neon;

            premultiply_row = premultiply_row::<neon::Premultiply>;
            premultiply_row_4444 = premultiply_row_4444::<neon::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<neon::Premultiply4444Swap>;
            multiply_row = multiply_row::<neon::MultiplyRow>;
            premultiply_argb_row = premultiply_argb_row::<neon::MultiplyArgb>;
            dispatch_alpha_first = dispatch_alpha::<neon::DispatchFirst>;
            dispatch_alpha_last = dispatch_alpha::<neon::DispatchLast>;
            argb_to_y = argb_to_y::<neon::ArgbToY>;
            argb_to_yuv444 = argb_to_yuv444::<neon::ArgbToYuv444>;
            argb_to_uv = argb_to_uv::<neon::ArgbToUv>;
        }
        #[cfg(wpd_asm_dotprod)]
        NEON | DOTPROD {
            argb_to_y = argb_to_y::<dotprod::ArgbToY>;
            argb_to_yuv444 = argb_to_yuv444::<dotprod::ArgbToYuv444>;
        }
        #[cfg(wpd_asm_i8mm)]
        NEON | I8MM {
            argb_to_y = argb_to_y::<i8mm::ArgbToY>;
            argb_to_yuv444 = argb_to_yuv444::<i8mm::ArgbToYuv444>;
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
mod arch {
    use super::*;

    pub fn init(_dsp: &mut YuvDsp, _flags: CpuFlags) {}

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
