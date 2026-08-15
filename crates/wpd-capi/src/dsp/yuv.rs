//! C ABI for the YUV DSP table and its row drivers, as declared by
//! `src/yuvdsp.h`.
//!
//! The assembly entries are the raw symbols, so `checkasm --bench` measures
//! the assembly and nothing else. The fallbacks are trampolines that rebuild
//! slices for the safe kernels in [`wpd::dsp::yuv`].
//!
//! The row drivers moved to [`wpd::convert`]; what is left of them here are
//! three entry points the C harnesses in `tests/` and the tool still call.

use std::ffi::c_int;
use std::slice;

use wpd::convert::YuvPlanes;
use wpd::dsp::yuv as k;
use wpd::dsp::yuv::{
    bpp, YuvDsp, LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_NB, LAYOUT_RGB,
    LAYOUT_RGBA, UPSAMPLE_BLOCK,
};
use wpd::picture::{PlaneMut, PlaneRef};

pub type UpsampleBlockFn = unsafe extern "C" fn(
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
pub type DispatchAlphaFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type PackRowFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type PremultiplyRowFn = unsafe extern "C" fn(*mut u8, c_int, c_int);
pub type Premultiply4444Fn = unsafe extern "C" fn(*mut u8, c_int);
pub type ArgbToYFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type ArgbToYuv444Fn =
    unsafe extern "C" fn(*mut u8, *mut u8, *mut u8, *const u8, c_int);
pub type ArgbToUvFn =
    unsafe extern "C" fn(*mut u8, *mut u8, *const u8, isize, c_int, c_int);

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
pub struct WPDYUVDSP {
    pub upsample_block: [UpsampleBlockFn; LAYOUT_NB],
    pub dispatch_alpha_first: DispatchAlphaFn,
    pub dispatch_alpha_last: DispatchAlphaFn,
    pub pack_rgba: PackRowFn,
    pub pack_bgra: PackRowFn,
    pub pack_rgb: PackRowFn,
    pub pack_bgr: PackRowFn,
    pub pack_rgb565: PackRowFn,
    pub pack_rgba4444: PackRowFn,
    pub pack_bgr565: PackRowFn,
    pub pack_bgra4444: PackRowFn,
    pub premultiply_row: PremultiplyRowFn,
    pub premultiply_row_4444: Premultiply4444Fn,
    pub premultiply_row_4444_swap: Premultiply4444Fn,
    pub argb_to_y: ArgbToYFn,
    pub argb_to_yuv444: ArgbToYuv444Fn,
    pub argb_to_uv: ArgbToUvFn,
}

/// Builds the row an upsample block entry point reads, given the pair count.
///
/// For `n` blocks the kernel walks pairs `1..=16n`, so it touches luma and
/// output pixels `0..32n` and chroma `0..=16n`.
macro_rules! upsample_block_tramp {
    ($name:ident, $layout:expr) => {
        unsafe extern "C" fn $name(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            num_blocks: c_int,
        ) {
            let last = num_blocks as usize * (UPSAMPLE_BLOCK / 2);
            let pixels = 2 * last;
            let bpp = bpp($layout);

            unsafe {
                k::upsample_pairs::<$layout>(
                    slice::from_raw_parts(top_y, pixels),
                    (!bottom_y.is_null())
                        .then(|| slice::from_raw_parts(bottom_y, pixels)),
                    slice::from_raw_parts(top_u, last + 1),
                    slice::from_raw_parts(top_v, last + 1),
                    slice::from_raw_parts(cur_u, last + 1),
                    slice::from_raw_parts(cur_v, last + 1),
                    slice::from_raw_parts_mut(top_dst, bpp * pixels),
                    (!bottom_dst.is_null())
                        .then(|| slice::from_raw_parts_mut(bottom_dst, bpp * pixels)),
                    1,
                    last,
                    0,
                )
            }
        }
    };
}

upsample_block_tramp!(upsample_block_argb_c, LAYOUT_ARGB);
upsample_block_tramp!(upsample_block_rgba_c, LAYOUT_RGBA);
upsample_block_tramp!(upsample_block_bgra_c, LAYOUT_BGRA);
upsample_block_tramp!(upsample_block_rgb_c, LAYOUT_RGB);
upsample_block_tramp!(upsample_block_bgr_c, LAYOUT_BGR);

unsafe extern "C" fn dispatch_alpha_first_c(dst: *mut u8, src: *const u8, n: c_int) {
    let n = n as usize;

    unsafe {
        k::dispatch_alpha_first(
            slice::from_raw_parts_mut(dst, 4 * n),
            slice::from_raw_parts(src, n),
        )
    }
}

unsafe extern "C" fn dispatch_alpha_last_c(dst: *mut u8, src: *const u8, n: c_int) {
    let n = n as usize;

    unsafe {
        k::dispatch_alpha_last(
            slice::from_raw_parts_mut(dst, 4 * n),
            slice::from_raw_parts(src, n),
        )
    }
}

macro_rules! pack_tramp {
    ($name:ident, $kernel:ident, $bpp:literal) => {
        unsafe extern "C" fn $name(dst: *mut u8, src: *const u8, n: c_int) {
            let n = n as usize;

            unsafe {
                k::$kernel(
                    slice::from_raw_parts_mut(dst, $bpp * n),
                    slice::from_raw_parts(src, 4 * n),
                )
            }
        }
    };
}

pack_tramp!(pack_rgba_c, pack_rgba, 4);
pack_tramp!(pack_bgra_c, pack_bgra, 4);
pack_tramp!(pack_rgb_c, pack_rgb, 3);
pack_tramp!(pack_bgr_c, pack_bgr, 3);
pack_tramp!(pack_rgb565_c, pack_rgb565, 2);
pack_tramp!(pack_bgr565_c, pack_bgr565, 2);
pack_tramp!(pack_rgba4444_c, pack_rgba4444, 2);
pack_tramp!(pack_bgra4444_c, pack_bgra4444, 2);

unsafe extern "C" fn premultiply_row_c(rgba: *mut u8, alpha_first: c_int, n: c_int) {
    let row = unsafe { slice::from_raw_parts_mut(rgba, 4 * n as usize) };

    k::premultiply_row(row, alpha_first != 0);
}

unsafe extern "C" fn premultiply_row_4444_c(rgba4444: *mut u8, n: c_int) {
    let row = unsafe { slice::from_raw_parts_mut(rgba4444, 2 * n as usize) };

    k::premultiply_row_4444(row, false);
}

unsafe extern "C" fn premultiply_row_4444_swap_c(bgra4444: *mut u8, n: c_int) {
    let row = unsafe { slice::from_raw_parts_mut(bgra4444, 2 * n as usize) };

    k::premultiply_row_4444(row, true);
}

unsafe extern "C" fn argb_to_y_c(y: *mut u8, argb: *const u8, n: c_int) {
    let n = n as usize;

    unsafe {
        k::argb_to_y(
            slice::from_raw_parts_mut(y, n),
            slice::from_raw_parts(argb, 4 * n),
        )
    }
}

unsafe extern "C" fn argb_to_yuv444_c(
    y: *mut u8,
    u: *mut u8,
    v: *mut u8,
    argb: *const u8,
    n: c_int,
) {
    let n = n as usize;

    unsafe {
        k::argb_to_yuv444(
            slice::from_raw_parts_mut(y, n),
            slice::from_raw_parts_mut(u, n),
            slice::from_raw_parts_mut(v, n),
            slice::from_raw_parts(argb, 4 * n),
        )
    }
}

unsafe extern "C" fn argb_to_uv_c(
    u: *mut u8,
    v: *mut u8,
    argb: *const u8,
    argb_stride: isize,
    n: c_int,
    weight_alpha: c_int,
) {
    let n = n as usize;
    let stride = argb_stride as usize;
    let chroma = n.div_ceil(2);

    unsafe {
        k::argb_to_uv(
            slice::from_raw_parts_mut(u, chroma),
            slice::from_raw_parts_mut(v, chroma),
            slice::from_raw_parts(argb, 4 * n + stride),
            stride,
            n,
            weight_alpha != 0,
        )
    }
}

#[cfg(all(feature = "asm", any(target_arch = "x86", target_arch = "x86_64")))]
mod asm {
    use super::*;

    #[cfg(target_arch = "x86_64")]
    mod wide {
        use super::*;

        macro_rules! upsample_syms {
            ($($rust:ident = $sym:literal,)*) => {
                extern "C" {
                    $(#[link_name = $sym]
                      pub fn $rust(
                          top_y: *const u8,
                          bottom_y: *const u8,
                          top_u: *const u8,
                          top_v: *const u8,
                          cur_u: *const u8,
                          cur_v: *const u8,
                          top_dst: *mut u8,
                          bottom_dst: *mut u8,
                          num_blocks: c_int,
                      );)*
                }
            };
        }

        upsample_syms! {
            argb_sse2 = "ff_upsample_block_argb_sse2",
            rgba_sse2 = "ff_upsample_block_rgba_sse2",
            bgra_sse2 = "ff_upsample_block_bgra_sse2",
            rgb_sse2 = "ff_upsample_block_rgb_sse2",
            bgr_sse2 = "ff_upsample_block_bgr_sse2",
            rgb_ssse3 = "ff_upsample_block_rgb_ssse3",
            bgr_ssse3 = "ff_upsample_block_bgr_ssse3",
            argb_avx2 = "ff_upsample_block_argb_avx2",
            rgba_avx2 = "ff_upsample_block_rgba_avx2",
            bgra_avx2 = "ff_upsample_block_bgra_avx2",
            rgb_avx2 = "ff_upsample_block_rgb_avx2",
            bgr_avx2 = "ff_upsample_block_bgr_avx2",
        }

        extern "C" {
            pub fn ff_argb_to_yuv444_ssse3(
                y: *mut u8,
                u: *mut u8,
                v: *mut u8,
                argb: *const u8,
                n: c_int,
            );
            pub fn ff_argb_to_yuv444_avx2(
                y: *mut u8,
                u: *mut u8,
                v: *mut u8,
                argb: *const u8,
                n: c_int,
            );
            pub fn ff_argb_to_uv_avx2(
                u: *mut u8,
                v: *mut u8,
                argb: *const u8,
                argb_stride: isize,
                n: c_int,
                weight_alpha: c_int,
            );
        }
    }

    extern "C" {
        pub fn ff_dispatch_alpha_first_sse2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_dispatch_alpha_last_sse2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_dispatch_alpha_first_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_dispatch_alpha_last_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb565_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba4444_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr565_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra4444_ssse3(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb565_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba4444_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr565_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra4444_avx2(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_premultiply_row_ssse3(rgba: *mut u8, alpha_first: c_int, n: c_int);
        pub fn ff_premultiply_row_avx2(rgba: *mut u8, alpha_first: c_int, n: c_int);
        pub fn ff_premultiply_row_4444_ssse3(rgba4444: *mut u8, n: c_int);
        pub fn ff_premultiply_row_4444_avx2(rgba4444: *mut u8, n: c_int);
        pub fn ff_premultiply_row_4444_swap_ssse3(bgra4444: *mut u8, n: c_int);
        pub fn ff_premultiply_row_4444_swap_avx2(bgra4444: *mut u8, n: c_int);
        pub fn ff_argb_to_y_ssse3(y: *mut u8, argb: *const u8, n: c_int);
        pub fn ff_argb_to_y_avx2(y: *mut u8, argb: *const u8, n: c_int);
    }

    pub fn init(dsp: &mut WPDYUVDSP) {
        let flags = wpd::cpu::flags();

        if flags.contains(wpd::cpu::CpuFlags::SSE2) {
            #[cfg(target_arch = "x86_64")]
            {
                dsp.upsample_block = [
                    wide::argb_sse2,
                    wide::rgba_sse2,
                    wide::bgra_sse2,
                    wide::rgb_sse2,
                    wide::bgr_sse2,
                ];
            }
            dsp.dispatch_alpha_first = ff_dispatch_alpha_first_sse2;
            dsp.dispatch_alpha_last = ff_dispatch_alpha_last_sse2;
        }

        if flags.contains(wpd::cpu::CpuFlags::SSSE3) {
            #[cfg(target_arch = "x86_64")]
            {
                dsp.upsample_block[LAYOUT_RGB] = wide::rgb_ssse3;
                dsp.upsample_block[LAYOUT_BGR] = wide::bgr_ssse3;
                dsp.argb_to_yuv444 = wide::ff_argb_to_yuv444_ssse3;
            }
            dsp.pack_rgba = ff_pack_rgba_ssse3;
            dsp.pack_bgra = ff_pack_bgra_ssse3;
            dsp.pack_rgb = ff_pack_rgb_ssse3;
            dsp.pack_bgr = ff_pack_bgr_ssse3;
            dsp.pack_rgb565 = ff_pack_rgb565_ssse3;
            dsp.pack_rgba4444 = ff_pack_rgba4444_ssse3;
            dsp.pack_bgr565 = ff_pack_bgr565_ssse3;
            dsp.pack_bgra4444 = ff_pack_bgra4444_ssse3;
            dsp.premultiply_row = ff_premultiply_row_ssse3;
            dsp.premultiply_row_4444 = ff_premultiply_row_4444_ssse3;
            dsp.premultiply_row_4444_swap = ff_premultiply_row_4444_swap_ssse3;
            dsp.argb_to_y = ff_argb_to_y_ssse3;
        }

        if flags.contains(wpd::cpu::CpuFlags::AVX2) {
            #[cfg(target_arch = "x86_64")]
            {
                dsp.upsample_block = [
                    wide::argb_avx2,
                    wide::rgba_avx2,
                    wide::bgra_avx2,
                    wide::rgb_avx2,
                    wide::bgr_avx2,
                ];
                dsp.argb_to_yuv444 = wide::ff_argb_to_yuv444_avx2;
                dsp.argb_to_uv = wide::ff_argb_to_uv_avx2;
            }
            dsp.dispatch_alpha_first = ff_dispatch_alpha_first_avx2;
            dsp.dispatch_alpha_last = ff_dispatch_alpha_last_avx2;
            dsp.pack_rgba = ff_pack_rgba_avx2;
            dsp.pack_bgra = ff_pack_bgra_avx2;
            dsp.pack_rgb = ff_pack_rgb_avx2;
            dsp.pack_bgr = ff_pack_bgr_avx2;
            dsp.pack_rgb565 = ff_pack_rgb565_avx2;
            dsp.pack_rgba4444 = ff_pack_rgba4444_avx2;
            dsp.pack_bgr565 = ff_pack_bgr565_avx2;
            dsp.pack_bgra4444 = ff_pack_bgra4444_avx2;
            dsp.premultiply_row = ff_premultiply_row_avx2;
            dsp.premultiply_row_4444 = ff_premultiply_row_4444_avx2;
            dsp.premultiply_row_4444_swap = ff_premultiply_row_4444_swap_avx2;
            dsp.argb_to_y = ff_argb_to_y_avx2;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;

    extern "C" {
        pub fn ff_upsample_block_argb_neon(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            blocks: c_int,
        );
        pub fn ff_upsample_block_rgba_neon(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            blocks: c_int,
        );
        pub fn ff_upsample_block_bgra_neon(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            blocks: c_int,
        );
        pub fn ff_upsample_block_rgb_neon(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            blocks: c_int,
        );
        pub fn ff_upsample_block_bgr_neon(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            blocks: c_int,
        );
        pub fn ff_dispatch_alpha_first_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_dispatch_alpha_last_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgb565_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_rgba4444_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgr565_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_pack_bgra4444_neon(dst: *mut u8, src: *const u8, n: c_int);
        pub fn ff_premultiply_row_neon(rgba: *mut u8, alpha_first: c_int, n: c_int);
        pub fn ff_premultiply_row_4444_neon(rgba4444: *mut u8, n: c_int);
        pub fn ff_premultiply_row_4444_swap_neon(bgra4444: *mut u8, n: c_int);
        pub fn ff_argb_to_y_neon(y: *mut u8, argb: *const u8, n: c_int);
        pub fn ff_argb_to_yuv444_neon(
            y: *mut u8,
            u: *mut u8,
            v: *mut u8,
            argb: *const u8,
            n: c_int,
        );
        pub fn ff_argb_to_uv_neon(
            u: *mut u8,
            v: *mut u8,
            argb: *const u8,
            argb_stride: isize,
            n: c_int,
            weight_alpha: c_int,
        );
    }

    #[cfg(wpd_asm_dotprod)]
    extern "C" {
        pub fn ff_argb_to_y_neon_dotprod(y: *mut u8, argb: *const u8, n: c_int);
        pub fn ff_argb_to_yuv444_neon_dotprod(
            y: *mut u8,
            u: *mut u8,
            v: *mut u8,
            argb: *const u8,
            n: c_int,
        );
    }

    #[cfg(wpd_asm_i8mm)]
    extern "C" {
        pub fn ff_argb_to_y_neon_i8mm(y: *mut u8, argb: *const u8, n: c_int);
        pub fn ff_argb_to_yuv444_neon_i8mm(
            y: *mut u8,
            u: *mut u8,
            v: *mut u8,
            argb: *const u8,
            n: c_int,
        );
    }

    pub fn init(dsp: &mut WPDYUVDSP) {
        let flags = wpd::cpu::flags();

        if !flags.contains(wpd::cpu::CpuFlags::NEON) {
            return;
        }
        dsp.upsample_block = [
            ff_upsample_block_argb_neon,
            ff_upsample_block_rgba_neon,
            ff_upsample_block_bgra_neon,
            ff_upsample_block_rgb_neon,
            ff_upsample_block_bgr_neon,
        ];
        dsp.dispatch_alpha_first = ff_dispatch_alpha_first_neon;
        dsp.dispatch_alpha_last = ff_dispatch_alpha_last_neon;
        dsp.pack_rgba = ff_pack_rgba_neon;
        dsp.pack_bgra = ff_pack_bgra_neon;
        dsp.pack_rgb = ff_pack_rgb_neon;
        dsp.pack_bgr = ff_pack_bgr_neon;
        dsp.pack_rgb565 = ff_pack_rgb565_neon;
        dsp.pack_rgba4444 = ff_pack_rgba4444_neon;
        dsp.pack_bgr565 = ff_pack_bgr565_neon;
        dsp.pack_bgra4444 = ff_pack_bgra4444_neon;
        dsp.premultiply_row = ff_premultiply_row_neon;
        dsp.premultiply_row_4444 = ff_premultiply_row_4444_neon;
        dsp.premultiply_row_4444_swap = ff_premultiply_row_4444_swap_neon;
        dsp.argb_to_y = ff_argb_to_y_neon;
        dsp.argb_to_yuv444 = ff_argb_to_yuv444_neon;
        dsp.argb_to_uv = ff_argb_to_uv_neon;

        #[cfg(wpd_asm_dotprod)]
        if flags.contains(wpd::cpu::CpuFlags::DOTPROD) {
            dsp.argb_to_y = ff_argb_to_y_neon_dotprod;
            dsp.argb_to_yuv444 = ff_argb_to_yuv444_neon_dotprod;
        }
        #[cfg(wpd_asm_i8mm)]
        if flags.contains(wpd::cpu::CpuFlags::I8MM) {
            dsp.argb_to_y = ff_argb_to_y_neon_i8mm;
            dsp.argb_to_yuv444 = ff_argb_to_yuv444_neon_i8mm;
        }
    }
}

impl WPDYUVDSP {
    /// The best implementation the running CPU allows.
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = WPDYUVDSP {
            upsample_block: [
                upsample_block_argb_c,
                upsample_block_rgba_c,
                upsample_block_bgra_c,
                upsample_block_rgb_c,
                upsample_block_bgr_c,
            ],
            dispatch_alpha_first: dispatch_alpha_first_c,
            dispatch_alpha_last: dispatch_alpha_last_c,
            pack_rgba: pack_rgba_c,
            pack_bgra: pack_bgra_c,
            pack_rgb: pack_rgb_c,
            pack_bgr: pack_bgr_c,
            pack_rgb565: pack_rgb565_c,
            pack_rgba4444: pack_rgba4444_c,
            pack_bgr565: pack_bgr565_c,
            pack_bgra4444: pack_bgra4444_c,
            premultiply_row: premultiply_row_c,
            premultiply_row_4444: premultiply_row_4444_c,
            premultiply_row_4444_swap: premultiply_row_4444_swap_c,
            argb_to_y: argb_to_y_c,
            argb_to_yuv444: argb_to_yuv444_c,
            argb_to_uv: argb_to_uv_c,
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
/// `dsp` must point to a writable, aligned `WPDYUVDSP`.
#[no_mangle]
pub unsafe extern "C" fn wpd_yuv_dsp_init(dsp: *mut WPDYUVDSP) {
    unsafe { dsp.write(WPDYUVDSP::new()) }
}

/// Fancy-upsamples rows `[row_start, row_end)`, returning the first row
/// written.
///
/// This entry point and its two siblings exist for the harnesses in `tests/`;
/// the decoder calls [`wpd::convert`] directly. They take no table, because
/// the one the core builds from the current CPU flags is the one under test —
/// `checkasm` sets those flags before it asks for either.
///
/// # Safety
///
/// The planes must hold `height` rows of `width` samples at the given strides,
/// and `dst` the same in the packed layout.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv420_to_packed_rows(
    layout: c_int,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    a: *const u8,
    a_stride: isize,
    width: c_int,
    height: c_int,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    if width <= 0 || height <= 0 || row_start >= row_end {
        return row_start;
    }

    let layout = layout as usize;
    let (w, h) = (width as usize, height as usize);
    let rows = |stride: isize, n: usize, len: usize| (n - 1) * stride as usize + len;

    unsafe {
        let mut out = PlaneMut::borrowed(
            slice::from_raw_parts_mut(dst, rows(dst_stride, h, bpp(layout) * w)),
            dst_stride as usize,
        );
        let plane = |p: *const u8, stride: isize, n: usize, len: usize| {
            PlaneRef::borrowed(
                slice::from_raw_parts(p, rows(stride, n, len)),
                stride as usize,
            )
        };
        let src = YuvPlanes {
            y: plane(y, y_stride, h, w),
            u: plane(u, uv_stride, h.div_ceil(2), w.div_ceil(2)),
            v: plane(v, uv_stride, h.div_ceil(2), w.div_ceil(2)),
            a: (!a.is_null()).then(|| plane(a, a_stride, h, w)),
        };

        wpd::convert::yuv420_to_packed_rows(
            &YuvDsp::new(),
            layout,
            &mut out,
            &src,
            w,
            h,
            row_start as usize,
            row_end as usize,
        ) as c_int
    }
}

/// # Safety
///
/// As [`wpd_yuv420_to_packed_rows`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv420_to_packed(
    layout: c_int,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    a: *const u8,
    a_stride: isize,
    width: c_int,
    height: c_int,
) {
    unsafe {
        wpd_yuv420_to_packed_rows(
            layout, dst, dst_stride, y, y_stride, u, v, uv_stride, a, a_stride, width,
            height, 0, height,
        )
    };
}

/// # Safety
///
/// `argb` must hold `height` rows of `width` pixels, and the three planes the
/// same at their strides.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_argb_to_yuv444(
    y: *mut u8,
    y_stride: isize,
    u: *mut u8,
    v: *mut u8,
    uv_stride: isize,
    argb: *const u8,
    argb_stride: isize,
    width: c_int,
    height: c_int,
) {
    let (w, h) = (width as usize, height as usize);
    let extent = |stride: isize, len: usize| (h - 1) * stride as usize + len;

    unsafe {
        let mut planes = [
            PlaneMut::borrowed(
                slice::from_raw_parts_mut(y, extent(y_stride, w)),
                y_stride as usize,
            ),
            PlaneMut::borrowed(
                slice::from_raw_parts_mut(u, extent(uv_stride, w)),
                uv_stride as usize,
            ),
            PlaneMut::borrowed(
                slice::from_raw_parts_mut(v, extent(uv_stride, w)),
                uv_stride as usize,
            ),
            PlaneMut::borrowed(&mut [], 0),
        ];
        let src = PlaneRef::borrowed(
            slice::from_raw_parts(argb, extent(argb_stride, 4 * w)),
            argb_stride as usize,
        );

        wpd::convert::argb_to_yuv444(&YuvDsp::new(), &mut planes, &src, w, height);
    }
}
