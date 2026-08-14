//! C ABI for the lossy DSP table, as declared by `src/vp8dsp.h`.
//!
//! Each instruction set gets a module whose function names are the same, with
//! `#[link_name]` pointing at that variant's symbol. The macroblock-edge
//! filters are then composed once per module, as the `VP8_*_LOOP_FILTER*_MB`
//! macros in `src/vp8dsp.h` do for the C.

use std::ffi::c_int;
use std::slice;

use wpd::dsp::vp8 as k;

pub type WhtFn = unsafe extern "C" fn(*mut [[i16; 16]; 4], *mut i16);
pub type IdctFn = unsafe extern "C" fn(*mut u8, *mut i16, isize);
pub type Idct4Fn = unsafe extern "C" fn(*mut u8, *mut [i16; 16], isize);
pub type LfFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int);
pub type LfUvFn = unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int);
pub type LfMbFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfUvMbFn =
    unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfSimpleFn = unsafe extern "C" fn(*mut u8, isize, c_int);
pub type LfSimpleMbFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int);

#[repr(C)]
pub struct VP8DSPContext {
    pub vp8_luma_dc_wht: WhtFn,
    pub vp8_luma_dc_wht_dc: WhtFn,
    pub vp8_idct_add: IdctFn,
    pub vp8_idct_dc_add: IdctFn,
    pub vp8_idct_dc_add4y: Idct4Fn,
    pub vp8_idct_dc_add4uv: Idct4Fn,

    pub vp8_v_loop_filter16y: LfFn,
    pub vp8_h_loop_filter16y: LfFn,
    pub vp8_v_loop_filter8uv: LfUvFn,
    pub vp8_h_loop_filter8uv: LfUvFn,

    pub vp8_v_loop_filter16y_inner: LfFn,
    pub vp8_h_loop_filter16y_inner: LfFn,
    pub vp8_v_loop_filter8uv_inner: LfUvFn,
    pub vp8_h_loop_filter8uv_inner: LfUvFn,

    pub vp8_h_loop_filter16y_mb: LfMbFn,
    pub vp8_h_loop_filter8uv_mb: LfUvMbFn,
    pub vp8_v_loop_filter16y_mb: LfMbFn,
    pub vp8_v_loop_filter8uv_mb: LfUvMbFn,

    pub vp8_v_loop_filter_simple: LfSimpleFn,
    pub vp8_h_loop_filter_simple: LfSimpleFn,

    pub vp8_h_loop_filter_simple_mb: LfSimpleMbFn,
    pub vp8_v_loop_filter_simple_mb: LfSimpleMbFn,
}

macro_rules! h_simple_mb {
    ($name:ident, $single:expr) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        ) {
            let f = $single;

            unsafe {
                f(dst, stride, mbedge_lim);
                f(dst.add(4), stride, bedge_lim);
                f(dst.add(8), stride, bedge_lim);
                f(dst.add(12), stride, bedge_lim);
            }
        }
    };
}

macro_rules! v_simple_mb {
    ($name:ident, $single:expr) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        ) {
            let f = $single;

            unsafe {
                f(dst, stride, mbedge_lim);
                f(dst.offset(4 * stride), stride, bedge_lim);
                f(dst.offset(8 * stride), stride, bedge_lim);
                f(dst.offset(12 * stride), stride, bedge_lim);
            }
        }
    };
}

macro_rules! h_mb {
    ($name:ident, $mbedge:expr, $inner:expr) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            let (edge, inner) = ($mbedge, $inner);

            unsafe {
                edge(dst, stride, mbedge_e, flim_i, hev);
                inner(dst.add(4), stride, bedge_e, flim_i, hev);
                inner(dst.add(8), stride, bedge_e, flim_i, hev);
                inner(dst.add(12), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! v_mb {
    ($name:ident, $mbedge:expr, $inner:expr) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            let (edge, inner) = ($mbedge, $inner);

            unsafe {
                edge(dst, stride, mbedge_e, flim_i, hev);
                inner(dst.offset(4 * stride), stride, bedge_e, flim_i, hev);
                inner(dst.offset(8 * stride), stride, bedge_e, flim_i, hev);
                inner(dst.offset(12 * stride), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! h_uv_mb {
    ($name:ident, $mbedge:expr, $inner:expr) => {
        unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            let (edge, inner) = ($mbedge, $inner);

            unsafe {
                edge(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
                inner(dst_u.add(4), dst_v.add(4), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! v_uv_mb {
    ($name:ident, $mbedge:expr, $inner:expr) => {
        unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            let (edge, inner) = ($mbedge, $inner);

            unsafe {
                edge(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
                inner(
                    dst_u.offset(4 * stride),
                    dst_v.offset(4 * stride),
                    stride,
                    bedge_e,
                    flim_i,
                    hev,
                );
            }
        }
    };
}

/// # Safety
///
/// `dst` must be the edge sample of a plane with four rows above it and four
/// below at `stride`, and `size` columns to its right. That is what every
/// caller of the C prototype already guarantees.
unsafe fn lf_v<const SIZE: usize, const INNER: bool>(
    dst: *mut u8,
    stride: isize,
    flim_e: c_int,
    flim_i: c_int,
    hev: c_int,
) {
    let s = stride as usize;
    let buf =
        unsafe { slice::from_raw_parts_mut(dst.offset(-4 * stride), 7 * s + SIZE) };

    k::loop_filter::<SIZE, true, INNER>(buf, s, flim_e, flim_i, hev);
}

/// # Safety
///
/// As [`lf_v`], transposed: four samples either side of `dst` in each of
/// `size` rows.
unsafe fn lf_h<const SIZE: usize, const INNER: bool>(
    dst: *mut u8,
    stride: isize,
    flim_e: c_int,
    flim_i: c_int,
    hev: c_int,
) {
    let s = stride as usize;
    let buf = unsafe { slice::from_raw_parts_mut(dst.sub(4), (SIZE - 1) * s + 8) };

    k::loop_filter::<SIZE, false, INNER>(buf, s, flim_e, flim_i, hev);
}

macro_rules! lf_entry {
    ($name:ident, $driver:ident, $inner:literal) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            flim_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe { $driver::<16, $inner>(dst, stride, flim_e, flim_i, hev) }
        }
    };
}

macro_rules! lf_uv_entry {
    ($name:ident, $driver:ident, $inner:literal) => {
        unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            flim_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe {
                $driver::<8, $inner>(dst_u, stride, flim_e, flim_i, hev);
                $driver::<8, $inner>(dst_v, stride, flim_e, flim_i, hev);
            }
        }
    };
}

lf_entry!(v_loop_filter16y_c, lf_v, false);
lf_entry!(h_loop_filter16y_c, lf_h, false);
lf_entry!(v_loop_filter16y_inner_c, lf_v, true);
lf_entry!(h_loop_filter16y_inner_c, lf_h, true);
lf_uv_entry!(v_loop_filter8uv_c, lf_v, false);
lf_uv_entry!(h_loop_filter8uv_c, lf_h, false);
lf_uv_entry!(v_loop_filter8uv_inner_c, lf_v, true);
lf_uv_entry!(h_loop_filter8uv_inner_c, lf_h, true);

unsafe extern "C" fn v_loop_filter_simple_c(dst: *mut u8, stride: isize, flim: c_int) {
    let s = stride as usize;
    let buf = unsafe { slice::from_raw_parts_mut(dst.offset(-2 * stride), 3 * s + 16) };

    k::loop_filter_simple::<true>(buf, s, flim);
}

unsafe extern "C" fn h_loop_filter_simple_c(dst: *mut u8, stride: isize, flim: c_int) {
    let s = stride as usize;
    let buf = unsafe { slice::from_raw_parts_mut(dst.sub(2), 15 * s + 4) };

    k::loop_filter_simple::<false>(buf, s, flim);
}

h_mb!(
    h_loop_filter16y_mb_c,
    h_loop_filter16y_c,
    h_loop_filter16y_inner_c
);
v_mb!(
    v_loop_filter16y_mb_c,
    v_loop_filter16y_c,
    v_loop_filter16y_inner_c
);
h_uv_mb!(
    h_loop_filter8uv_mb_c,
    h_loop_filter8uv_c,
    h_loop_filter8uv_inner_c
);
v_uv_mb!(
    v_loop_filter8uv_mb_c,
    v_loop_filter8uv_c,
    v_loop_filter8uv_inner_c
);
h_simple_mb!(h_loop_filter_simple_mb_c, h_loop_filter_simple_c);
v_simple_mb!(v_loop_filter_simple_mb_c, v_loop_filter_simple_c);

unsafe extern "C" fn luma_dc_wht_c(block: *mut [[i16; 16]; 4], dc: *mut i16) {
    unsafe {
        k::luma_dc_wht(
            &mut *block.cast::<[[i16; 16]; 16]>(),
            &mut *dc.cast::<[i16; 16]>(),
        )
    }
}

unsafe extern "C" fn luma_dc_wht_dc_c(block: *mut [[i16; 16]; 4], dc: *mut i16) {
    unsafe {
        k::luma_dc_wht_dc(
            &mut *block.cast::<[[i16; 16]; 16]>(),
            &mut *dc.cast::<[i16; 16]>(),
        )
    }
}

unsafe extern "C" fn idct_add_c(dst: *mut u8, block: *mut i16, stride: isize) {
    let s = stride as usize;

    unsafe {
        k::idct_add(
            slice::from_raw_parts_mut(dst, 3 * s + 4),
            s,
            &mut *block.cast::<[i16; 16]>(),
        )
    }
}

unsafe extern "C" fn idct_dc_add_c(dst: *mut u8, block: *mut i16, stride: isize) {
    let s = stride as usize;

    unsafe {
        k::idct_dc_add(
            slice::from_raw_parts_mut(dst, 3 * s + 4),
            s,
            &mut *block.cast::<[i16; 16]>(),
        )
    }
}

unsafe extern "C" fn idct_dc_add4y_c(
    dst: *mut u8,
    block: *mut [i16; 16],
    stride: isize,
) {
    for i in 0..4 {
        unsafe { idct_dc_add_c(dst.add(4 * i), block.add(i).cast(), stride) }
    }
}

unsafe extern "C" fn idct_dc_add4uv_c(
    dst: *mut u8,
    block: *mut [i16; 16],
    stride: isize,
) {
    for i in 0..4 {
        unsafe {
            let dst = dst.offset(4 * stride * (i as isize / 2)).add(4 * (i % 2));
            idct_dc_add_c(dst, block.add(i).cast(), stride);
        }
    }
}

#[cfg(all(feature = "asm", any(target_arch = "x86", target_arch = "x86_64")))]
mod asm {
    use super::*;
    use wpd::asm::vp8::{avx2, sse2, sse2_idct, sse4, ssse3, Raw};
    use wpd::cpu::CpuFlags;

    h_simple_mb!(h_simple_mb_sse2, sse2::HSimple::F);
    v_simple_mb!(v_simple_mb_sse2, sse2::VSimple::F);
    h_mb!(h16_mb_sse2, sse2::H16::F, sse2::H16Inner::F);
    v_mb!(v16_mb_sse2, sse2::V16::F, sse2::V16Inner::F);
    h_uv_mb!(h8uv_mb_sse2, sse2::H8uv::F, sse2::H8uvInner::F);
    v_uv_mb!(v8uv_mb_sse2, sse2::V8uv::F, sse2::V8uvInner::F);

    h_simple_mb!(h_simple_mb_ssse3, ssse3::HSimple::F);
    v_simple_mb!(v_simple_mb_ssse3, ssse3::VSimple::F);
    h_mb!(h16_mb_ssse3, ssse3::H16::F, ssse3::H16Inner::F);
    v_mb!(v16_mb_ssse3, ssse3::V16::F, ssse3::V16Inner::F);
    h_uv_mb!(h8uv_mb_ssse3, ssse3::H8uv::F, ssse3::H8uvInner::F);
    v_uv_mb!(v8uv_mb_ssse3, ssse3::V8uv::F, ssse3::V8uvInner::F);

    pub fn init(c: &mut VP8DSPContext) {
        let flags = wpd::cpu::flags();

        if flags.contains(CpuFlags::SSE2) {
            c.vp8_idct_add = sse2_idct::Add::F;
            c.vp8_luma_dc_wht = sse2_idct::Wht::F;
            c.vp8_idct_dc_add = sse2_idct::DcAdd::F;
            c.vp8_idct_dc_add4y = sse2_idct::DcAdd4y::F;
            c.vp8_idct_dc_add4uv = sse2_idct::DcAdd4uv::F;

            c.vp8_v_loop_filter_simple = sse2::VSimple::F;
            c.vp8_h_loop_filter_simple = sse2::HSimple::F;
            c.vp8_v_loop_filter_simple_mb = v_simple_mb_sse2;
            c.vp8_h_loop_filter_simple_mb = h_simple_mb_sse2;

            c.vp8_v_loop_filter16y_inner = sse2::V16Inner::F;
            c.vp8_h_loop_filter16y_inner = sse2::H16Inner::F;
            c.vp8_v_loop_filter8uv_inner = sse2::V8uvInner::F;
            c.vp8_h_loop_filter8uv_inner = sse2::H8uvInner::F;

            c.vp8_v_loop_filter16y = sse2::V16::F;
            c.vp8_h_loop_filter16y = sse2::H16::F;
            c.vp8_v_loop_filter8uv = sse2::V8uv::F;
            c.vp8_h_loop_filter8uv = sse2::H8uv::F;

            c.vp8_h_loop_filter16y_mb = h16_mb_sse2;
            c.vp8_h_loop_filter8uv_mb = h8uv_mb_sse2;
            c.vp8_v_loop_filter16y_mb = v16_mb_sse2;
            c.vp8_v_loop_filter8uv_mb = v8uv_mb_sse2;
        }

        if flags.contains(CpuFlags::SSSE3) {
            c.vp8_v_loop_filter_simple = ssse3::VSimple::F;
            c.vp8_h_loop_filter_simple = ssse3::HSimple::F;
            c.vp8_v_loop_filter_simple_mb = v_simple_mb_ssse3;
            c.vp8_h_loop_filter_simple_mb = h_simple_mb_ssse3;

            c.vp8_v_loop_filter16y_inner = ssse3::V16Inner::F;
            c.vp8_h_loop_filter16y_inner = ssse3::H16Inner::F;
            c.vp8_v_loop_filter8uv_inner = ssse3::V8uvInner::F;
            c.vp8_h_loop_filter8uv_inner = ssse3::H8uvInner::F;

            c.vp8_v_loop_filter16y = ssse3::V16::F;
            c.vp8_h_loop_filter16y = ssse3::H16::F;
            c.vp8_v_loop_filter8uv = ssse3::V8uv::F;
            c.vp8_h_loop_filter8uv = ssse3::H8uv::F;

            c.vp8_h_loop_filter16y_mb = h16_mb_ssse3;
            c.vp8_h_loop_filter8uv_mb = h8uv_mb_ssse3;
            c.vp8_v_loop_filter16y_mb = v16_mb_ssse3;
            c.vp8_v_loop_filter8uv_mb = v8uv_mb_ssse3;
        }

        if flags.contains(CpuFlags::SSE41) {
            c.vp8_idct_dc_add = sse4::DcAdd::F;
            c.vp8_luma_dc_wht = sse4::Wht::F;
        }

        if flags.contains(CpuFlags::AVX2) {
            c.vp8_v_loop_filter8uv_inner = avx2::V8uvInner::F;
            c.vp8_v_loop_filter_simple_mb = avx2::VSimpleMb::F;
            c.vp8_h_loop_filter_simple_mb = avx2::HSimpleMb::F;
            c.vp8_h_loop_filter16y_mb = wpd::asm::vp8::h16_mb_avx2;
            c.vp8_h_loop_filter8uv_mb = wpd::asm::vp8::h8uv_mb_avx2;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;
    use wpd::asm::vp8::{fused, neon, neon_idct, Raw};
    use wpd::cpu::CpuFlags;

    v_simple_mb!(v_simple_mb_neon, neon::VSimple::F);
    v_mb!(v16_mb_neon, neon::V16::F, neon::V16Inner::F);
    v_uv_mb!(v8uv_mb_neon, neon::V8uv::F, neon::V8uvInner::F);

    pub fn init(c: &mut VP8DSPContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        c.vp8_luma_dc_wht = neon_idct::Wht::F;
        c.vp8_idct_add = neon_idct::Add::F;
        c.vp8_idct_dc_add = neon_idct::DcAdd::F;
        c.vp8_idct_dc_add4y = neon_idct::DcAdd4y::F;
        c.vp8_idct_dc_add4uv = neon_idct::DcAdd4uv::F;

        c.vp8_v_loop_filter16y = neon::V16::F;
        c.vp8_h_loop_filter16y = neon::H16::F;
        c.vp8_v_loop_filter8uv = neon::V8uv::F;
        c.vp8_h_loop_filter8uv = neon::H8uv::F;

        c.vp8_v_loop_filter16y_inner = neon::V16Inner::F;
        c.vp8_h_loop_filter16y_inner = neon::H16Inner::F;
        c.vp8_v_loop_filter8uv_inner = neon::V8uvInner::F;
        c.vp8_h_loop_filter8uv_inner = neon::H8uvInner::F;

        c.vp8_h_loop_filter16y_mb = fused::H16Mb::F;
        c.vp8_h_loop_filter8uv_mb = fused::H8uvMb::F;
        c.vp8_v_loop_filter16y_mb = v16_mb_neon;
        c.vp8_v_loop_filter8uv_mb = v8uv_mb_neon;

        c.vp8_v_loop_filter_simple = neon::VSimple::F;
        c.vp8_h_loop_filter_simple = neon::HSimple::F;
        c.vp8_h_loop_filter_simple_mb = fused::HSimpleMb::F;
        c.vp8_v_loop_filter_simple_mb = v_simple_mb_neon;
    }
}

#[cfg(all(feature = "asm", target_arch = "arm"))]
mod asm {
    use super::*;
    use wpd::asm::vp8::{neon, neon_idct, Raw};
    use wpd::cpu::CpuFlags;

    h_simple_mb!(h_simple_mb_neon, neon::HSimple::F);
    v_simple_mb!(v_simple_mb_neon, neon::VSimple::F);
    h_mb!(h16_mb_neon, neon::H16::F, neon::H16Inner::F);
    v_mb!(v16_mb_neon, neon::V16::F, neon::V16Inner::F);
    h_uv_mb!(h8uv_mb_neon, neon::H8uv::F, neon::H8uvInner::F);
    v_uv_mb!(v8uv_mb_neon, neon::V8uv::F, neon::V8uvInner::F);

    fn init_neon(c: &mut VP8DSPContext) {
        c.vp8_luma_dc_wht = neon_idct::Wht::F;
        c.vp8_idct_add = neon_idct::Add::F;
        c.vp8_idct_dc_add = neon_idct::DcAdd::F;
        c.vp8_idct_dc_add4y = neon_idct::DcAdd4y::F;
        c.vp8_idct_dc_add4uv = neon_idct::DcAdd4uv::F;

        c.vp8_v_loop_filter16y = neon::V16::F;
        c.vp8_h_loop_filter16y = neon::H16::F;
        c.vp8_v_loop_filter8uv = neon::V8uv::F;
        c.vp8_h_loop_filter8uv = neon::H8uv::F;

        c.vp8_v_loop_filter16y_inner = neon::V16Inner::F;
        c.vp8_h_loop_filter16y_inner = neon::H16Inner::F;
        c.vp8_v_loop_filter8uv_inner = neon::V8uvInner::F;
        c.vp8_h_loop_filter8uv_inner = neon::H8uvInner::F;

        c.vp8_h_loop_filter16y_mb = h16_mb_neon;
        c.vp8_h_loop_filter8uv_mb = h8uv_mb_neon;
        c.vp8_v_loop_filter16y_mb = v16_mb_neon;
        c.vp8_v_loop_filter8uv_mb = v8uv_mb_neon;

        c.vp8_v_loop_filter_simple = neon::VSimple::F;
        c.vp8_h_loop_filter_simple = neon::HSimple::F;
        c.vp8_h_loop_filter_simple_mb = h_simple_mb_neon;
        c.vp8_v_loop_filter_simple_mb = v_simple_mb_neon;
    }

    #[cfg(wpd_asm_armv6)]
    mod armv6 {
        use super::*;
        use wpd::asm::vp8::{armv6, armv6_idct, armv6_wht_dc};

        h_simple_mb!(h_simple_mb_armv6, armv6::HSimple::F);
        v_simple_mb!(v_simple_mb_armv6, armv6::VSimple::F);

        pub fn init(c: &mut VP8DSPContext) {
            c.vp8_luma_dc_wht = armv6_idct::Wht::F;
            c.vp8_luma_dc_wht_dc = armv6_wht_dc::WhtDc::F;
            c.vp8_idct_add = armv6_idct::Add::F;
            c.vp8_idct_dc_add = armv6_idct::DcAdd::F;
            c.vp8_idct_dc_add4y = armv6_idct::DcAdd4y::F;
            c.vp8_idct_dc_add4uv = armv6_idct::DcAdd4uv::F;

            c.vp8_v_loop_filter16y = armv6::V16::F;
            c.vp8_h_loop_filter16y = armv6::H16::F;
            c.vp8_v_loop_filter8uv = armv6::V8uv::F;
            c.vp8_h_loop_filter8uv = armv6::H8uv::F;

            c.vp8_v_loop_filter16y_inner = armv6::V16Inner::F;
            c.vp8_h_loop_filter16y_inner = armv6::H16Inner::F;
            c.vp8_v_loop_filter8uv_inner = armv6::V8uvInner::F;
            c.vp8_h_loop_filter8uv_inner = armv6::H8uvInner::F;

            c.vp8_v_loop_filter_simple = armv6::VSimple::F;
            c.vp8_h_loop_filter_simple = armv6::HSimple::F;
            c.vp8_h_loop_filter_simple_mb = h_simple_mb_armv6;
            c.vp8_v_loop_filter_simple_mb = v_simple_mb_armv6;
        }
    }

    pub fn init(c: &mut VP8DSPContext) {
        let flags = wpd::cpu::flags();

        #[cfg(wpd_asm_armv6)]
        if flags.contains(CpuFlags::ARMV6) {
            armv6::init(c);
        }
        if flags.contains(CpuFlags::NEON) {
            init_neon(c);
        }
    }
}

/// Fills in `c` with the best implementation the running CPU allows.
///
/// # Safety
///
/// `c` must point to a writable, aligned `VP8DSPContext`.
#[no_mangle]
pub unsafe extern "C" fn ff_vp8dsp_init(c: *mut VP8DSPContext) {
    let mut table = VP8DSPContext {
        vp8_luma_dc_wht: luma_dc_wht_c,
        vp8_luma_dc_wht_dc: luma_dc_wht_dc_c,
        vp8_idct_add: idct_add_c,
        vp8_idct_dc_add: idct_dc_add_c,
        vp8_idct_dc_add4y: idct_dc_add4y_c,
        vp8_idct_dc_add4uv: idct_dc_add4uv_c,

        vp8_v_loop_filter16y: v_loop_filter16y_c,
        vp8_h_loop_filter16y: h_loop_filter16y_c,
        vp8_v_loop_filter8uv: v_loop_filter8uv_c,
        vp8_h_loop_filter8uv: h_loop_filter8uv_c,

        vp8_v_loop_filter16y_inner: v_loop_filter16y_inner_c,
        vp8_h_loop_filter16y_inner: h_loop_filter16y_inner_c,
        vp8_v_loop_filter8uv_inner: v_loop_filter8uv_inner_c,
        vp8_h_loop_filter8uv_inner: h_loop_filter8uv_inner_c,

        vp8_h_loop_filter16y_mb: h_loop_filter16y_mb_c,
        vp8_h_loop_filter8uv_mb: h_loop_filter8uv_mb_c,
        vp8_v_loop_filter16y_mb: v_loop_filter16y_mb_c,
        vp8_v_loop_filter8uv_mb: v_loop_filter8uv_mb_c,

        vp8_v_loop_filter_simple: v_loop_filter_simple_c,
        vp8_h_loop_filter_simple: h_loop_filter_simple_c,

        vp8_h_loop_filter_simple_mb: h_loop_filter_simple_mb_c,
        vp8_v_loop_filter_simple_mb: v_loop_filter_simple_mb_c,
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

    unsafe { c.write(table) }
}
