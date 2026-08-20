use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::yuv::{bpp, UpsampleDst, UpsampleSrc, YuvDsp, UPSAMPLE_BLOCK};

pub use super::vp8::Raw;

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
pub type PremultiplyRaw = unsafe extern "C" fn(*mut u8, c_int, c_int);
pub type Premultiply4444Raw = unsafe extern "C" fn(*mut u8, c_int);
pub type ArgbToYuv444Raw =
    unsafe extern "C" fn(*mut u8, *mut u8, *mut u8, *const u8, c_int);
pub type ArgbToUvRaw =
    unsafe extern "C" fn(*mut u8, *mut u8, *const u8, isize, c_int, c_int);

macro_rules! raw {
    ($marker:ident, $inner:ident, $sig:ty, $sym:literal, ($($arg:ty),*)) => {
        extern "C" {
            #[link_name = $sym]
            fn $inner($(_: $arg),*);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = $sig;
            const F: $sig = $inner;
        }
    };
}

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
    pub dispatch_alpha_first: Option<RowRaw>,
    pub dispatch_alpha_last: Option<RowRaw>,
    pub packers: Option<[RowRaw; 8]>,
    pub premultiply_row: Option<PremultiplyRaw>,
    pub premultiply_row_4444: Option<Premultiply4444Raw>,
    pub premultiply_row_4444_swap: Option<Premultiply4444Raw>,
    pub argb_to_y: Option<RowRaw>,
    pub argb_to_yuv444: Option<ArgbToYuv444Raw>,
    pub argb_to_uv: Option<ArgbToUvRaw>,
}

#[allow(unused_macros)]
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

#[allow(unused_macros)]
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

#[allow(unused_macros)]
macro_rules! ladder {
    ($(
        $(#[$attr:meta])*
        $($flag:ident)|+ {
            $( @upsample $up:ident; )?
            $( @upsample_rgb $up_rgb:ident; )?
            $( @upsample_bgr $up_bgr:ident; )?
            $( @packers $packers:ident; )?
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
            "ff_dispatch_alpha_first_sse2"
        );
        raw_row!(DispatchLast, dispatch_last, "ff_dispatch_alpha_last_sse2");
        premultiply_4444_syms!(
            "ff_premultiply_row_4444_sse2",
            "ff_premultiply_row_4444_swap_sse2"
        );

        #[cfg(target_arch = "x86_64")]
        upsample_syms!(
            "ff_upsample_block_argb_sse2",
            "ff_upsample_block_rgba_sse2",
            "ff_upsample_block_bgra_sse2",
            "ff_upsample_block_rgb_sse2",
            "ff_upsample_block_bgr_sse2"
        );
    }

    pub mod ssse3 {
        use super::*;

        pack_syms!(
            "ff_pack_rgba_ssse3",
            "ff_pack_bgra_ssse3",
            "ff_pack_rgb_ssse3",
            "ff_pack_bgr_ssse3",
            "ff_pack_rgb565_ssse3",
            "ff_pack_rgba4444_ssse3",
            "ff_pack_bgr565_ssse3",
            "ff_pack_bgra4444_ssse3"
        );
        premultiply_syms!("ff_premultiply_row_ssse3");
        raw_row!(ArgbToY, argb_to_y, "ff_argb_to_y_ssse3");

        #[cfg(target_arch = "x86_64")]
        raw_upsample!(UpsampleRgb, upsample_rgb, "ff_upsample_block_rgb_ssse3");
        #[cfg(target_arch = "x86_64")]
        raw_upsample!(UpsampleBgr, upsample_bgr, "ff_upsample_block_bgr_ssse3");
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "ff_argb_to_yuv444_ssse3",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    pub mod avx2 {
        use super::*;

        raw_row!(
            DispatchFirst,
            dispatch_first,
            "ff_dispatch_alpha_first_avx2"
        );
        raw_row!(DispatchLast, dispatch_last, "ff_dispatch_alpha_last_avx2");
        pack_syms!(
            "ff_pack_rgba_avx2",
            "ff_pack_bgra_avx2",
            "ff_pack_rgb_avx2",
            "ff_pack_bgr_avx2",
            "ff_pack_rgb565_avx2",
            "ff_pack_rgba4444_avx2",
            "ff_pack_bgr565_avx2",
            "ff_pack_bgra4444_avx2"
        );
        premultiply_syms!("ff_premultiply_row_avx2");
        premultiply_4444_syms!(
            "ff_premultiply_row_4444_avx2",
            "ff_premultiply_row_4444_swap_avx2"
        );
        raw_row!(ArgbToY, argb_to_y, "ff_argb_to_y_avx2");

        #[cfg(target_arch = "x86_64")]
        upsample_syms!(
            "ff_upsample_block_argb_avx2",
            "ff_upsample_block_rgba_avx2",
            "ff_upsample_block_bgra_avx2",
            "ff_upsample_block_rgb_avx2",
            "ff_upsample_block_bgr_avx2"
        );
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "ff_argb_to_yuv444_avx2",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
        #[cfg(target_arch = "x86_64")]
        raw!(
            ArgbToUv,
            argb_to_uv,
            ArgbToUvRaw,
            "ff_argb_to_uv_avx2",
            (*mut u8, *mut u8, *const u8, isize, c_int, c_int)
        );
    }

    ladder! {
        #[cfg(target_arch = "x86_64")]
        SSE2 {
            @upsample sse2;
        }
        SSE2 {
            dispatch_alpha_first = dispatch_alpha::<sse2::DispatchFirst>;
            dispatch_alpha_last = dispatch_alpha::<sse2::DispatchLast>;
            premultiply_row_4444 = premultiply_row_4444::<sse2::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<sse2::Premultiply4444Swap>;
        }
        #[cfg(target_arch = "x86_64")]
        SSSE3 {
            @upsample_rgb ssse3;
            @upsample_bgr ssse3;

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

            argb_to_yuv444 = argb_to_yuv444::<avx2::ArgbToYuv444>;
            argb_to_uv = argb_to_uv::<avx2::ArgbToUv>;
        }
        AVX2 {
            @packers avx2;

            premultiply_row = premultiply_row::<avx2::Premultiply>;
            premultiply_row_4444 = premultiply_row_4444::<avx2::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<avx2::Premultiply4444Swap>;
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
            "ff_upsample_block_argb_neon",
            "ff_upsample_block_rgba_neon",
            "ff_upsample_block_bgra_neon",
            "ff_upsample_block_rgb_neon",
            "ff_upsample_block_bgr_neon"
        );
        pack_syms!(
            "ff_pack_rgba_neon",
            "ff_pack_bgra_neon",
            "ff_pack_rgb_neon",
            "ff_pack_bgr_neon",
            "ff_pack_rgb565_neon",
            "ff_pack_rgba4444_neon",
            "ff_pack_bgr565_neon",
            "ff_pack_bgra4444_neon"
        );
        premultiply_syms!("ff_premultiply_row_neon");
        premultiply_4444_syms!(
            "ff_premultiply_row_4444_neon",
            "ff_premultiply_row_4444_swap_neon"
        );
        raw_row!(
            DispatchFirst,
            dispatch_first,
            "ff_dispatch_alpha_first_neon"
        );
        raw_row!(DispatchLast, dispatch_last, "ff_dispatch_alpha_last_neon");
        raw_row!(ArgbToY, argb_to_y, "ff_argb_to_y_neon");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "ff_argb_to_yuv444_neon",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
        raw!(
            ArgbToUv,
            argb_to_uv,
            ArgbToUvRaw,
            "ff_argb_to_uv_neon",
            (*mut u8, *mut u8, *const u8, isize, c_int, c_int)
        );
    }

    #[cfg(wpd_asm_dotprod)]
    pub mod dotprod {
        use super::*;

        raw_row!(ArgbToY, argb_to_y, "ff_argb_to_y_neon_dotprod");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "ff_argb_to_yuv444_neon_dotprod",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    #[cfg(wpd_asm_i8mm)]
    pub mod i8mm {
        use super::*;

        raw_row!(ArgbToY, argb_to_y, "ff_argb_to_y_neon_i8mm");
        raw!(
            ArgbToYuv444,
            argb_to_yuv444,
            ArgbToYuv444Raw,
            "ff_argb_to_yuv444_neon_i8mm",
            (*mut u8, *mut u8, *mut u8, *const u8, c_int)
        );
    }

    ladder! {
        NEON {
            @upsample neon;
            @packers neon;

            premultiply_row = premultiply_row::<neon::Premultiply>;
            premultiply_row_4444 = premultiply_row_4444::<neon::Premultiply4444>;
            premultiply_row_4444_swap =
                premultiply_row_4444::<neon::Premultiply4444Swap>;
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
