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
    ($name:ident, $single:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        ) {
            unsafe {
                $single(dst, stride, mbedge_lim);
                $single(dst.add(4), stride, bedge_lim);
                $single(dst.add(8), stride, bedge_lim);
                $single(dst.add(12), stride, bedge_lim);
            }
        }
    };
}

macro_rules! v_simple_mb {
    ($name:ident, $single:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        ) {
            unsafe {
                $single(dst, stride, mbedge_lim);
                $single(dst.offset(4 * stride), stride, bedge_lim);
                $single(dst.offset(8 * stride), stride, bedge_lim);
                $single(dst.offset(12 * stride), stride, bedge_lim);
            }
        }
    };
}

macro_rules! h_mb {
    ($name:ident, $mbedge:path, $inner:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe {
                $mbedge(dst, stride, mbedge_e, flim_i, hev);
                $inner(dst.add(4), stride, bedge_e, flim_i, hev);
                $inner(dst.add(8), stride, bedge_e, flim_i, hev);
                $inner(dst.add(12), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! v_mb {
    ($name:ident, $mbedge:path, $inner:path) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe {
                $mbedge(dst, stride, mbedge_e, flim_i, hev);
                $inner(dst.offset(4 * stride), stride, bedge_e, flim_i, hev);
                $inner(dst.offset(8 * stride), stride, bedge_e, flim_i, hev);
                $inner(dst.offset(12 * stride), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! h_uv_mb {
    ($name:ident, $mbedge:path, $inner:path) => {
        unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe {
                $mbedge(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
                $inner(dst_u.add(4), dst_v.add(4), stride, bedge_e, flim_i, hev);
            }
        }
    };
}

macro_rules! v_uv_mb {
    ($name:ident, $mbedge:path, $inner:path) => {
        unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        ) {
            unsafe {
                $mbedge(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
                $inner(
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

/// Declares one instruction set's loop filter symbols under fixed names.
macro_rules! lf_symbols {
    ($m:ident,
     $v_simple:literal, $h_simple:literal,
     $v16:literal, $h16:literal, $v8uv:literal, $h8uv:literal,
     $v16_inner:literal, $h16_inner:literal,
     $v8uv_inner:literal, $h8uv_inner:literal) => {
        mod $m {
            use std::ffi::c_int;

            extern "C" {
                #[link_name = $v_simple]
                pub fn v_simple(dst: *mut u8, stride: isize, flim: c_int);
                #[link_name = $h_simple]
                pub fn h_simple(dst: *mut u8, stride: isize, flim: c_int);
                #[link_name = $v16]
                pub fn v16(dst: *mut u8, stride: isize, e: c_int, i: c_int, hev: c_int);
                #[link_name = $h16]
                pub fn h16(dst: *mut u8, stride: isize, e: c_int, i: c_int, hev: c_int);
                #[link_name = $v8uv]
                pub fn v8uv(
                    dst_u: *mut u8,
                    dst_v: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
                #[link_name = $h8uv]
                pub fn h8uv(
                    dst_u: *mut u8,
                    dst_v: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
                #[link_name = $v16_inner]
                pub fn v16_inner(
                    dst: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
                #[link_name = $h16_inner]
                pub fn h16_inner(
                    dst: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
                #[link_name = $v8uv_inner]
                pub fn v8uv_inner(
                    dst_u: *mut u8,
                    dst_v: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
                #[link_name = $h8uv_inner]
                pub fn h8uv_inner(
                    dst_u: *mut u8,
                    dst_v: *mut u8,
                    stride: isize,
                    e: c_int,
                    i: c_int,
                    hev: c_int,
                );
            }
        }
    };
}

/// Declares one instruction set's transform symbols under fixed names.
#[allow(unused_macros)]
macro_rules! idct_symbols {
    ($m:ident, $wht:literal, $add:literal, $dc_add:literal,
     $dc_add4y:literal, $dc_add4uv:literal) => {
        mod $m {
            extern "C" {
                #[link_name = $wht]
                pub fn wht(block: *mut [[i16; 16]; 4], dc: *mut i16);
                #[link_name = $add]
                pub fn add(dst: *mut u8, block: *mut i16, stride: isize);
                #[link_name = $dc_add]
                pub fn dc_add(dst: *mut u8, block: *mut i16, stride: isize);
                #[link_name = $dc_add4y]
                pub fn dc_add4y(dst: *mut u8, block: *mut [i16; 16], stride: isize);
                #[link_name = $dc_add4uv]
                pub fn dc_add4uv(dst: *mut u8, block: *mut [i16; 16], stride: isize);
            }
        }
    };
}

#[cfg(all(feature = "asm", any(target_arch = "x86", target_arch = "x86_64")))]
mod asm {
    use super::*;
    use wpd::cpu::CpuFlags;

    lf_symbols!(
        sse2,
        "ff_vp8_v_loop_filter_simple_sse2",
        "ff_vp8_h_loop_filter_simple_sse2",
        "ff_vp8_v_loop_filter16y_mbedge_sse2",
        "ff_vp8_h_loop_filter16y_mbedge_sse2",
        "ff_vp8_v_loop_filter8uv_mbedge_sse2",
        "ff_vp8_h_loop_filter8uv_mbedge_sse2",
        "ff_vp8_v_loop_filter16y_inner_sse2",
        "ff_vp8_h_loop_filter16y_inner_sse2",
        "ff_vp8_v_loop_filter8uv_inner_sse2",
        "ff_vp8_h_loop_filter8uv_inner_sse2"
    );
    lf_symbols!(
        ssse3,
        "ff_vp8_v_loop_filter_simple_ssse3",
        "ff_vp8_h_loop_filter_simple_ssse3",
        "ff_vp8_v_loop_filter16y_mbedge_ssse3",
        "ff_vp8_h_loop_filter16y_mbedge_ssse3",
        "ff_vp8_v_loop_filter8uv_mbedge_ssse3",
        "ff_vp8_h_loop_filter8uv_mbedge_ssse3",
        "ff_vp8_v_loop_filter16y_inner_ssse3",
        "ff_vp8_h_loop_filter16y_inner_ssse3",
        "ff_vp8_v_loop_filter8uv_inner_ssse3",
        "ff_vp8_h_loop_filter8uv_inner_ssse3"
    );

    h_simple_mb!(h_simple_mb_sse2, sse2::h_simple);
    v_simple_mb!(v_simple_mb_sse2, sse2::v_simple);
    h_mb!(h16_mb_sse2, sse2::h16, sse2::h16_inner);
    v_mb!(v16_mb_sse2, sse2::v16, sse2::v16_inner);
    h_uv_mb!(h8uv_mb_sse2, sse2::h8uv, sse2::h8uv_inner);
    v_uv_mb!(v8uv_mb_sse2, sse2::v8uv, sse2::v8uv_inner);

    h_simple_mb!(h_simple_mb_ssse3, ssse3::h_simple);
    v_simple_mb!(v_simple_mb_ssse3, ssse3::v_simple);
    h_mb!(h16_mb_ssse3, ssse3::h16, ssse3::h16_inner);
    v_mb!(v16_mb_ssse3, ssse3::v16, ssse3::v16_inner);
    h_uv_mb!(h8uv_mb_ssse3, ssse3::h8uv, ssse3::h8uv_inner);
    v_uv_mb!(v8uv_mb_ssse3, ssse3::v8uv, ssse3::v8uv_inner);

    extern "C" {
        fn ff_vp8_idct_dc_add_sse2(dst: *mut u8, block: *mut i16, stride: isize);
        fn ff_vp8_idct_dc_add_sse4(dst: *mut u8, block: *mut i16, stride: isize);
        fn ff_vp8_idct_add_sse2(dst: *mut u8, block: *mut i16, stride: isize);
        fn ff_vp8_idct_dc_add4y_sse2(
            dst: *mut u8,
            block: *mut [i16; 16],
            stride: isize,
        );
        fn ff_vp8_idct_dc_add4uv_sse2(
            dst: *mut u8,
            block: *mut [i16; 16],
            stride: isize,
        );
        fn ff_vp8_luma_dc_wht_sse2(block: *mut [[i16; 16]; 4], dc: *mut i16);
        fn ff_vp8_luma_dc_wht_sse4(block: *mut [[i16; 16]; 4], dc: *mut i16);

        fn ff_vp8_v_loop_filter8uv_inner_avx2(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            e: c_int,
            i: c_int,
            hev: c_int,
        );
        fn ff_vp8_v_loop_filter_simple_mb_avx2(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        );
        fn ff_vp8_h_loop_filter_simple_mb_avx2(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        );
        fn ff_vp8_h_loop_filter16y_mb_transpose_avx2(
            dst: *mut u8,
            stride: isize,
            tmp: *mut u8,
        );
        fn ff_vp8_h_loop_filter16y_mb_itranspose_avx2(
            dst: *mut u8,
            stride: isize,
            tmp: *const u8,
        );
        fn ff_vp8_h_loop_filter8uv_mb_transpose_avx2(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            tmp: *mut u8,
        );
        fn ff_vp8_h_loop_filter8uv_mb_itranspose_avx2(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            tmp: *const u8,
        );
    }

    /// The AVX2 horizontal macroblock filters transpose into this, run the
    /// vertical SSSE3 kernels over it, and transpose back.
    #[repr(C, align(32))]
    struct Transposed([u8; 16 * 16]);

    unsafe extern "C" fn h16_mb_avx2(
        dst: *mut u8,
        stride: isize,
        mbedge_e: c_int,
        bedge_e: c_int,
        flim_i: c_int,
        hev: c_int,
    ) {
        let mut tmp = Transposed([0; 16 * 16]);
        let t = tmp.0.as_mut_ptr();

        unsafe {
            ff_vp8_h_loop_filter16y_mb_transpose_avx2(dst, stride, t);
            ssse3::v16(t.add(4 * 16), 16, mbedge_e, flim_i, hev);
            ssse3::v16_inner(t.add(8 * 16), 16, bedge_e, flim_i, hev);
            ssse3::v16_inner(t.add(12 * 16), 16, bedge_e, flim_i, hev);
            ff_vp8_h_loop_filter16y_mb_itranspose_avx2(dst, stride, t);
            ssse3::h16_inner(dst.add(12), stride, bedge_e, flim_i, hev);
        }
    }

    unsafe extern "C" fn h8uv_mb_avx2(
        dst_u: *mut u8,
        dst_v: *mut u8,
        stride: isize,
        mbedge_e: c_int,
        bedge_e: c_int,
        flim_i: c_int,
        hev: c_int,
    ) {
        let mut tmp = Transposed([0; 16 * 16]);
        let t = tmp.0.as_mut_ptr();

        unsafe {
            ff_vp8_h_loop_filter8uv_mb_transpose_avx2(dst_u, dst_v, stride, t);
            ssse3::v16(t.add(4 * 16), 16, mbedge_e, flim_i, hev);
            ssse3::v16_inner(t.add(8 * 16), 16, bedge_e, flim_i, hev);
            ff_vp8_h_loop_filter8uv_mb_itranspose_avx2(dst_u, dst_v, stride, t);
        }
    }

    pub fn init(c: &mut VP8DSPContext) {
        let flags = wpd::cpu::flags();

        if flags.contains(CpuFlags::SSE2) {
            c.vp8_idct_add = ff_vp8_idct_add_sse2;
            c.vp8_luma_dc_wht = ff_vp8_luma_dc_wht_sse2;
            c.vp8_idct_dc_add = ff_vp8_idct_dc_add_sse2;
            c.vp8_idct_dc_add4y = ff_vp8_idct_dc_add4y_sse2;
            c.vp8_idct_dc_add4uv = ff_vp8_idct_dc_add4uv_sse2;

            c.vp8_v_loop_filter_simple = sse2::v_simple;
            c.vp8_h_loop_filter_simple = sse2::h_simple;
            c.vp8_v_loop_filter_simple_mb = v_simple_mb_sse2;
            c.vp8_h_loop_filter_simple_mb = h_simple_mb_sse2;

            c.vp8_v_loop_filter16y_inner = sse2::v16_inner;
            c.vp8_h_loop_filter16y_inner = sse2::h16_inner;
            c.vp8_v_loop_filter8uv_inner = sse2::v8uv_inner;
            c.vp8_h_loop_filter8uv_inner = sse2::h8uv_inner;

            c.vp8_v_loop_filter16y = sse2::v16;
            c.vp8_h_loop_filter16y = sse2::h16;
            c.vp8_v_loop_filter8uv = sse2::v8uv;
            c.vp8_h_loop_filter8uv = sse2::h8uv;

            c.vp8_h_loop_filter16y_mb = h16_mb_sse2;
            c.vp8_h_loop_filter8uv_mb = h8uv_mb_sse2;
            c.vp8_v_loop_filter16y_mb = v16_mb_sse2;
            c.vp8_v_loop_filter8uv_mb = v8uv_mb_sse2;
        }

        if flags.contains(CpuFlags::SSSE3) {
            c.vp8_v_loop_filter_simple = ssse3::v_simple;
            c.vp8_h_loop_filter_simple = ssse3::h_simple;
            c.vp8_v_loop_filter_simple_mb = v_simple_mb_ssse3;
            c.vp8_h_loop_filter_simple_mb = h_simple_mb_ssse3;

            c.vp8_v_loop_filter16y_inner = ssse3::v16_inner;
            c.vp8_h_loop_filter16y_inner = ssse3::h16_inner;
            c.vp8_v_loop_filter8uv_inner = ssse3::v8uv_inner;
            c.vp8_h_loop_filter8uv_inner = ssse3::h8uv_inner;

            c.vp8_v_loop_filter16y = ssse3::v16;
            c.vp8_h_loop_filter16y = ssse3::h16;
            c.vp8_v_loop_filter8uv = ssse3::v8uv;
            c.vp8_h_loop_filter8uv = ssse3::h8uv;

            c.vp8_h_loop_filter16y_mb = h16_mb_ssse3;
            c.vp8_h_loop_filter8uv_mb = h8uv_mb_ssse3;
            c.vp8_v_loop_filter16y_mb = v16_mb_ssse3;
            c.vp8_v_loop_filter8uv_mb = v8uv_mb_ssse3;
        }

        if flags.contains(CpuFlags::SSE41) {
            c.vp8_idct_dc_add = ff_vp8_idct_dc_add_sse4;
            c.vp8_luma_dc_wht = ff_vp8_luma_dc_wht_sse4;
        }

        if flags.contains(CpuFlags::AVX2) {
            c.vp8_v_loop_filter8uv_inner = ff_vp8_v_loop_filter8uv_inner_avx2;
            c.vp8_v_loop_filter_simple_mb = ff_vp8_v_loop_filter_simple_mb_avx2;
            c.vp8_h_loop_filter_simple_mb = ff_vp8_h_loop_filter_simple_mb_avx2;
            c.vp8_h_loop_filter16y_mb = h16_mb_avx2;
            c.vp8_h_loop_filter8uv_mb = h8uv_mb_avx2;
        }
    }
}

#[cfg(all(feature = "asm", target_arch = "aarch64"))]
mod asm {
    use super::*;
    use wpd::cpu::CpuFlags;

    lf_symbols!(
        neon,
        "ff_vp8_v_loop_filter16_simple_neon",
        "ff_vp8_h_loop_filter16_simple_neon",
        "ff_vp8_v_loop_filter16_neon",
        "ff_vp8_h_loop_filter16_neon",
        "ff_vp8_v_loop_filter8uv_neon",
        "ff_vp8_h_loop_filter8uv_neon",
        "ff_vp8_v_loop_filter16_inner_neon",
        "ff_vp8_h_loop_filter16_inner_neon",
        "ff_vp8_v_loop_filter8uv_inner_neon",
        "ff_vp8_h_loop_filter8uv_inner_neon"
    );
    idct_symbols!(
        idct,
        "ff_vp8_luma_dc_wht_neon",
        "ff_vp8_idct_add_neon",
        "ff_vp8_idct_dc_add_neon",
        "ff_vp8_idct_dc_add4y_neon",
        "ff_vp8_idct_dc_add4uv_neon"
    );

    extern "C" {
        fn ff_vp8_h_loop_filter_simple_mb_neon(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: c_int,
            bedge_lim: c_int,
        );
        fn ff_vp8_h_loop_filter16y_mb_neon(
            dst: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        );
        fn ff_vp8_h_loop_filter8uv_mb_neon(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: c_int,
            bedge_e: c_int,
            flim_i: c_int,
            hev: c_int,
        );
    }

    v_simple_mb!(v_simple_mb_neon, neon::v_simple);
    v_mb!(v16_mb_neon, neon::v16, neon::v16_inner);
    v_uv_mb!(v8uv_mb_neon, neon::v8uv, neon::v8uv_inner);

    pub fn init(c: &mut VP8DSPContext) {
        if !wpd::cpu::flags().contains(CpuFlags::NEON) {
            return;
        }

        c.vp8_luma_dc_wht = idct::wht;
        c.vp8_idct_add = idct::add;
        c.vp8_idct_dc_add = idct::dc_add;
        c.vp8_idct_dc_add4y = idct::dc_add4y;
        c.vp8_idct_dc_add4uv = idct::dc_add4uv;

        c.vp8_v_loop_filter16y = neon::v16;
        c.vp8_h_loop_filter16y = neon::h16;
        c.vp8_v_loop_filter8uv = neon::v8uv;
        c.vp8_h_loop_filter8uv = neon::h8uv;

        c.vp8_v_loop_filter16y_inner = neon::v16_inner;
        c.vp8_h_loop_filter16y_inner = neon::h16_inner;
        c.vp8_v_loop_filter8uv_inner = neon::v8uv_inner;
        c.vp8_h_loop_filter8uv_inner = neon::h8uv_inner;

        c.vp8_h_loop_filter16y_mb = ff_vp8_h_loop_filter16y_mb_neon;
        c.vp8_h_loop_filter8uv_mb = ff_vp8_h_loop_filter8uv_mb_neon;
        c.vp8_v_loop_filter16y_mb = v16_mb_neon;
        c.vp8_v_loop_filter8uv_mb = v8uv_mb_neon;

        c.vp8_v_loop_filter_simple = neon::v_simple;
        c.vp8_h_loop_filter_simple = neon::h_simple;
        c.vp8_h_loop_filter_simple_mb = ff_vp8_h_loop_filter_simple_mb_neon;
        c.vp8_v_loop_filter_simple_mb = v_simple_mb_neon;
    }
}

#[cfg(all(feature = "asm", target_arch = "arm"))]
mod asm {
    use super::*;
    use wpd::cpu::CpuFlags;

    lf_symbols!(
        neon,
        "ff_vp8_v_loop_filter16_simple_neon",
        "ff_vp8_h_loop_filter16_simple_neon",
        "ff_vp8_v_loop_filter16_neon",
        "ff_vp8_h_loop_filter16_neon",
        "ff_vp8_v_loop_filter8uv_neon",
        "ff_vp8_h_loop_filter8uv_neon",
        "ff_vp8_v_loop_filter16_inner_neon",
        "ff_vp8_h_loop_filter16_inner_neon",
        "ff_vp8_v_loop_filter8uv_inner_neon",
        "ff_vp8_h_loop_filter8uv_inner_neon"
    );
    idct_symbols!(
        idct_neon,
        "ff_vp8_luma_dc_wht_neon",
        "ff_vp8_idct_add_neon",
        "ff_vp8_idct_dc_add_neon",
        "ff_vp8_idct_dc_add4y_neon",
        "ff_vp8_idct_dc_add4uv_neon"
    );

    h_simple_mb!(h_simple_mb_neon, neon::h_simple);
    v_simple_mb!(v_simple_mb_neon, neon::v_simple);
    h_mb!(h16_mb_neon, neon::h16, neon::h16_inner);
    v_mb!(v16_mb_neon, neon::v16, neon::v16_inner);
    h_uv_mb!(h8uv_mb_neon, neon::h8uv, neon::h8uv_inner);
    v_uv_mb!(v8uv_mb_neon, neon::v8uv, neon::v8uv_inner);

    fn init_neon(c: &mut VP8DSPContext) {
        c.vp8_luma_dc_wht = idct_neon::wht;
        c.vp8_idct_add = idct_neon::add;
        c.vp8_idct_dc_add = idct_neon::dc_add;
        c.vp8_idct_dc_add4y = idct_neon::dc_add4y;
        c.vp8_idct_dc_add4uv = idct_neon::dc_add4uv;

        c.vp8_v_loop_filter16y = neon::v16;
        c.vp8_h_loop_filter16y = neon::h16;
        c.vp8_v_loop_filter8uv = neon::v8uv;
        c.vp8_h_loop_filter8uv = neon::h8uv;

        c.vp8_v_loop_filter16y_inner = neon::v16_inner;
        c.vp8_h_loop_filter16y_inner = neon::h16_inner;
        c.vp8_v_loop_filter8uv_inner = neon::v8uv_inner;
        c.vp8_h_loop_filter8uv_inner = neon::h8uv_inner;

        c.vp8_h_loop_filter16y_mb = h16_mb_neon;
        c.vp8_h_loop_filter8uv_mb = h8uv_mb_neon;
        c.vp8_v_loop_filter16y_mb = v16_mb_neon;
        c.vp8_v_loop_filter8uv_mb = v8uv_mb_neon;

        c.vp8_v_loop_filter_simple = neon::v_simple;
        c.vp8_h_loop_filter_simple = neon::h_simple;
        c.vp8_h_loop_filter_simple_mb = h_simple_mb_neon;
        c.vp8_v_loop_filter_simple_mb = v_simple_mb_neon;
    }

    #[cfg(wpd_asm_armv6)]
    mod armv6 {
        use super::*;

        lf_symbols!(
            lf,
            "ff_vp8_v_loop_filter16_simple_armv6",
            "ff_vp8_h_loop_filter16_simple_armv6",
            "ff_vp8_v_loop_filter16_armv6",
            "ff_vp8_h_loop_filter16_armv6",
            "ff_vp8_v_loop_filter8uv_armv6",
            "ff_vp8_h_loop_filter8uv_armv6",
            "ff_vp8_v_loop_filter16_inner_armv6",
            "ff_vp8_h_loop_filter16_inner_armv6",
            "ff_vp8_v_loop_filter8uv_inner_armv6",
            "ff_vp8_h_loop_filter8uv_inner_armv6"
        );
        idct_symbols!(
            idct,
            "ff_vp8_luma_dc_wht_armv6",
            "ff_vp8_idct_add_armv6",
            "ff_vp8_idct_dc_add_armv6",
            "ff_vp8_idct_dc_add4y_armv6",
            "ff_vp8_idct_dc_add4uv_armv6"
        );

        extern "C" {
            fn ff_vp8_luma_dc_wht_dc_armv6(block: *mut [[i16; 16]; 4], dc: *mut i16);
        }

        h_simple_mb!(h_simple_mb_armv6, lf::h_simple);
        v_simple_mb!(v_simple_mb_armv6, lf::v_simple);

        pub fn init(c: &mut VP8DSPContext) {
            c.vp8_luma_dc_wht = idct::wht;
            c.vp8_luma_dc_wht_dc = ff_vp8_luma_dc_wht_dc_armv6;
            c.vp8_idct_add = idct::add;
            c.vp8_idct_dc_add = idct::dc_add;
            c.vp8_idct_dc_add4y = idct::dc_add4y;
            c.vp8_idct_dc_add4uv = idct::dc_add4uv;

            c.vp8_v_loop_filter16y = lf::v16;
            c.vp8_h_loop_filter16y = lf::h16;
            c.vp8_v_loop_filter8uv = lf::v8uv;
            c.vp8_h_loop_filter8uv = lf::h8uv;

            c.vp8_v_loop_filter16y_inner = lf::v16_inner;
            c.vp8_h_loop_filter16y_inner = lf::h16_inner;
            c.vp8_v_loop_filter8uv_inner = lf::v8uv_inner;
            c.vp8_h_loop_filter8uv_inner = lf::h8uv_inner;

            c.vp8_v_loop_filter_simple = lf::v_simple;
            c.vp8_h_loop_filter_simple = lf::h_simple;
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
