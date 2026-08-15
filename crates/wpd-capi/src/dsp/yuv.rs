//! C ABI for the YUV DSP table and its row drivers, as declared by
//! `src/yuvdsp.h`.
//!
//! The assembly entries are the raw symbols, so `checkasm --bench` measures
//! the assembly and nothing else. The fallbacks are trampolines that rebuild
//! slices for the safe kernels in [`wpd::dsp::yuv`].
//!
//! The row drivers live here rather than in the core because they still take
//! the caller's `(pointer, stride)` pairs and because they dispatch through
//! the table. They move up with the rest of the image pipeline in Phase 5.

use std::ffi::c_int;
use std::ptr;
use std::slice;

use wpd::dsp::yuv as k;
use wpd::dsp::yuv::{
    bpp, LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_NB, LAYOUT_RGB, LAYOUT_RGBA,
    UPSAMPLE_BLOCK,
};

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

/// One output row pair of the fancy upsampler.
///
/// The kernel reads `len` luma samples, `(len + 1) / 2` chroma samples and
/// writes `len` pixels, which is what the block entry point plus the scalar
/// head and tail add up to.
///
/// # Safety
///
/// The rows must hold those extents. A null `bottom_y` means the row pair is
/// folded onto itself, in which case `bottom_dst` is null too.
#[allow(clippy::too_many_arguments)]
unsafe fn upsample_row<const L: usize>(
    dsp: &WPDYUVDSP,
    top_y: *const u8,
    bottom_y: *const u8,
    top_u: *const u8,
    top_v: *const u8,
    cur_u: *const u8,
    cur_v: *const u8,
    top_dst: *mut u8,
    bottom_dst: *mut u8,
    len: usize,
) {
    let bpp = bpp(L);
    let last_pair = (len - 1) >> 1;
    let blocks = if len >= UPSAMPLE_BLOCK + 2 {
        (len - 2) / UPSAMPLE_BLOCK
    } else {
        0
    };
    let done = blocks * (UPSAMPLE_BLOCK / 2);
    let chroma = last_pair + 1;

    unsafe {
        k::upsample_edge::<L>(
            *top_y,
            (!bottom_y.is_null()).then(|| *bottom_y),
            *top_u,
            *top_v,
            *cur_u,
            *cur_v,
            slice::from_raw_parts_mut(top_dst, bpp),
            (!bottom_dst.is_null()).then(|| slice::from_raw_parts_mut(bottom_dst, bpp)),
        );

        if blocks != 0 {
            (dsp.upsample_block[L])(
                top_y.add(1),
                if bottom_y.is_null() {
                    ptr::null()
                } else {
                    bottom_y.add(1)
                },
                top_u,
                top_v,
                cur_u,
                cur_v,
                top_dst.add(bpp),
                if bottom_dst.is_null() {
                    ptr::null_mut()
                } else {
                    bottom_dst.add(bpp)
                },
                blocks as c_int,
            );
        }

        k::upsample_pairs::<L>(
            slice::from_raw_parts(top_y, len),
            (!bottom_y.is_null()).then(|| slice::from_raw_parts(bottom_y, len)),
            slice::from_raw_parts(top_u, chroma),
            slice::from_raw_parts(top_v, chroma),
            slice::from_raw_parts(cur_u, chroma),
            slice::from_raw_parts(cur_v, chroma),
            slice::from_raw_parts_mut(top_dst, bpp * len),
            (!bottom_dst.is_null())
                .then(|| slice::from_raw_parts_mut(bottom_dst, bpp * len)),
            done + 1,
            last_pair,
            2 * done + 1,
        );

        if len % 2 == 0 {
            let tail = bpp * (len - 1);

            k::upsample_edge::<L>(
                *top_y.add(len - 1),
                (!bottom_y.is_null()).then(|| *bottom_y.add(len - 1)),
                *top_u.add(last_pair),
                *top_v.add(last_pair),
                *cur_u.add(last_pair),
                *cur_v.add(last_pair),
                slice::from_raw_parts_mut(top_dst.add(tail), bpp),
                (!bottom_dst.is_null())
                    .then(|| slice::from_raw_parts_mut(bottom_dst.add(tail), bpp)),
            );
        }
    }
}

/// The pair index the upsampler must restart from to rewrite `row_start`.
const fn first_pair(row_start: usize) -> usize {
    if row_start != 0 {
        row_start.div_ceil(2)
    } else {
        1
    }
}

/// The first row `wpd_yuv420_to_packed_rows` actually writes.
const fn first_row(row_start: usize) -> usize {
    if row_start != 0 {
        2 * first_pair(row_start) - 1
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn yuv420_to_packed<const L: usize>(
    dsp: &WPDYUVDSP,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    width: usize,
    height: usize,
    row_start: usize,
    row_end: usize,
) {
    unsafe {
        if row_start == 0 {
            upsample_row::<L>(
                dsp,
                y,
                ptr::null(),
                u,
                v,
                u,
                v,
                dst,
                ptr::null_mut(),
                width,
            );
        }

        let mut j = first_pair(row_start);

        while 2 * j < row_end {
            let top_u = u.offset((j - 1) as isize * uv_stride);
            let top_v = v.offset((j - 1) as isize * uv_stride);
            let top = dst.offset((2 * j - 1) as isize * dst_stride);

            upsample_row::<L>(
                dsp,
                y.offset((2 * j - 1) as isize * y_stride),
                y.offset(2 * j as isize * y_stride),
                top_u,
                top_v,
                top_u.offset(uv_stride),
                top_v.offset(uv_stride),
                top,
                top.offset(dst_stride),
                width,
            );
            j += 1;
        }

        if height % 2 == 0 && row_end == height {
            let off = (height.div_ceil(2) - 1) as isize * uv_stride;

            upsample_row::<L>(
                dsp,
                y.offset((height - 1) as isize * y_stride),
                ptr::null(),
                u.offset(off),
                v.offset(off),
                u.offset(off),
                v.offset(off),
                dst.offset((height - 1) as isize * dst_stride),
                ptr::null_mut(),
                width,
            );
        }
    }
}

/// Converts rows `[row_start, row_end)` and returns the first row written.
///
/// # Safety
///
/// The planes must hold `height` rows of `width` pixels at the given strides,
/// and `dst` the same in the packed layout.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv420_to_packed_rows(
    dsp: *const WPDYUVDSP,
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

    let dsp = unsafe { &*dsp };
    let layout = layout as usize;
    let first = first_row(row_start as usize);
    let (w, h) = (width as usize, height as usize);
    let (start, end) = (row_start as usize, row_end as usize);

    macro_rules! run {
        ($l:expr) => {
            unsafe {
                yuv420_to_packed::<$l>(
                    dsp, dst, dst_stride, y, y_stride, u, v, uv_stride, w, h, start,
                    end,
                )
            }
        };
    }

    match layout {
        LAYOUT_RGBA => run!(LAYOUT_RGBA),
        LAYOUT_BGRA => run!(LAYOUT_BGRA),
        LAYOUT_RGB => run!(LAYOUT_RGB),
        LAYOUT_BGR => run!(LAYOUT_BGR),
        _ => run!(LAYOUT_ARGB),
    }

    if a.is_null() || layout == LAYOUT_RGB || layout == LAYOUT_BGR {
        return first as c_int;
    }

    let dispatch = if layout == LAYOUT_ARGB {
        dsp.dispatch_alpha_first
    } else {
        dsp.dispatch_alpha_last
    };

    for j in first..end {
        unsafe {
            dispatch(
                dst.offset(j as isize * dst_stride),
                a.offset(j as isize * a_stride),
                width,
            )
        };
    }
    first as c_int
}

/// # Safety
///
/// As [`wpd_yuv420_to_packed_rows`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv420_to_packed(
    dsp: *const WPDYUVDSP,
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
            dsp, layout, dst, dst_stride, y, y_stride, u, v, uv_stride, a, a_stride,
            width, height, 0, height,
        )
    };
}

/// Point sampling, which libwebp uses when fancy upsampling is turned off.
/// Every output row stands alone here, so `[row_start, row_end)` may be cut
/// anywhere.
///
/// # Safety
///
/// As [`wpd_yuv420_to_packed_rows`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv420_to_packed_simple(
    dsp: *const WPDYUVDSP,
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
    row_start: c_int,
    row_end: c_int,
) {
    let dsp = unsafe { &*dsp };
    let layout = layout as usize;
    let w = width as usize;
    let chroma = w.div_ceil(2);

    for j in row_start..row_end {
        let out = unsafe { dst.offset(j as isize * dst_stride) };

        unsafe {
            let row = slice::from_raw_parts_mut(out, bpp(layout) * w);
            let yy = slice::from_raw_parts(y.offset(j as isize * y_stride), w);
            let uu =
                slice::from_raw_parts(u.offset((j >> 1) as isize * uv_stride), chroma);
            let vv =
                slice::from_raw_parts(v.offset((j >> 1) as isize * uv_stride), chroma);

            match layout {
                LAYOUT_RGBA => k::yuv420_row::<LAYOUT_RGBA>(row, yy, uu, vv),
                LAYOUT_BGRA => k::yuv420_row::<LAYOUT_BGRA>(row, yy, uu, vv),
                LAYOUT_RGB => k::yuv420_row::<LAYOUT_RGB>(row, yy, uu, vv),
                LAYOUT_BGR => k::yuv420_row::<LAYOUT_BGR>(row, yy, uu, vv),
                _ => k::yuv420_row::<LAYOUT_ARGB>(row, yy, uu, vv),
            }
        }

        if !a.is_null() && layout != LAYOUT_RGB && layout != LAYOUT_BGR {
            let dispatch = if layout == LAYOUT_ARGB {
                dsp.dispatch_alpha_first
            } else {
                dsp.dispatch_alpha_last
            };

            unsafe { dispatch(out, a.offset(j as isize * a_stride), width) };
        }
    }
}

/// Point conversion from full-resolution planes, which is what libwebp uses
/// once the rescaler has brought chroma up to the output size.
///
/// # Safety
///
/// The planes must hold `height` rows of `width` samples at the given strides.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_yuv444_to_packed(
    layout: c_int,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    width: c_int,
    height: c_int,
) {
    let layout = layout as usize;
    let w = width as usize;

    for j in 0..height {
        unsafe {
            let row = slice::from_raw_parts_mut(
                dst.offset(j as isize * dst_stride),
                bpp(layout) * w,
            );
            let yy = slice::from_raw_parts(y.offset(j as isize * y_stride), w);
            let uu = slice::from_raw_parts(u.offset(j as isize * uv_stride), w);
            let vv = slice::from_raw_parts(v.offset(j as isize * uv_stride), w);

            match layout {
                LAYOUT_RGBA => k::yuv444_row::<LAYOUT_RGBA>(row, yy, uu, vv),
                LAYOUT_BGRA => k::yuv444_row::<LAYOUT_BGRA>(row, yy, uu, vv),
                LAYOUT_RGB => k::yuv444_row::<LAYOUT_RGB>(row, yy, uu, vv),
                LAYOUT_BGR => k::yuv444_row::<LAYOUT_BGR>(row, yy, uu, vv),
                _ => k::yuv444_row::<LAYOUT_ARGB>(row, yy, uu, vv),
            }
        }
    }
}

/// Converts rows `[row_start, row_end)` of a packed ARGB image to planar
/// 4:2:0. A null `a` means no alpha plane, and chroma is then averaged without
/// weighting it, which is what libwebp does for its YUV colorspace.
///
/// # Safety
///
/// `argb` must hold `row_end` rows of `width` pixels, and the planes their
/// matching extents.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_argb_to_yuva(
    dsp: *const WPDYUVDSP,
    y: *mut u8,
    y_stride: isize,
    u: *mut u8,
    v: *mut u8,
    uv_stride: isize,
    a: *mut u8,
    a_stride: isize,
    argb: *const u8,
    argb_stride: isize,
    width: c_int,
    row_start: c_int,
    row_end: c_int,
) {
    let dsp = unsafe { &*dsp };
    let weight_alpha = c_int::from(!a.is_null());
    let mut row = row_start;

    while row + 1 < row_end {
        unsafe {
            let src = argb.offset(row as isize * argb_stride);
            let chroma = (row >> 1) as isize * uv_stride;

            (dsp.argb_to_y)(y.offset(row as isize * y_stride), src, width);
            (dsp.argb_to_y)(
                y.offset((row + 1) as isize * y_stride),
                src.offset(argb_stride),
                width,
            );
            (dsp.argb_to_uv)(
                u.offset(chroma),
                v.offset(chroma),
                src,
                argb_stride,
                width,
                weight_alpha,
            );
        }
        row += 2;
    }
    if row < row_end {
        unsafe {
            let src = argb.offset(row as isize * argb_stride);
            let chroma = (row >> 1) as isize * uv_stride;

            (dsp.argb_to_y)(y.offset(row as isize * y_stride), src, width);
            (dsp.argb_to_uv)(
                u.offset(chroma),
                v.offset(chroma),
                src,
                0,
                width,
                weight_alpha,
            );
        }
    }
    if a.is_null() {
        return;
    }
    for row in row_start..row_end {
        unsafe {
            k::extract_alpha(
                slice::from_raw_parts_mut(
                    a.offset(row as isize * a_stride),
                    width as usize,
                ),
                slice::from_raw_parts(
                    argb.offset(row as isize * argb_stride),
                    4 * width as usize,
                ),
            )
        };
    }
}

/// # Safety
///
/// As [`wpd_argb_to_yuva`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_argb_to_yuv444(
    dsp: *const WPDYUVDSP,
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
    let dsp = unsafe { &*dsp };

    for row in 0..height {
        unsafe {
            (dsp.argb_to_yuv444)(
                y.offset(row as isize * y_stride),
                u.offset(row as isize * uv_stride),
                v.offset(row as isize * uv_stride),
                argb.offset(row as isize * argb_stride),
                width,
            )
        };
    }
}
