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

/// Overlays whatever [`wpd::asm::vp8::raw_table`] selected for the running
/// CPU. The symbols, the macroblock-edge compositions and the instruction-set
/// ladder all live in the core, so this table and the decoder's cannot pick
/// different kernels. The scalar entries above stay here, since they compose
/// this table's own fallbacks.
#[cfg(all(
    feature = "asm",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm"
    )
))]
fn init_asm(c: &mut VP8DSPContext) {
    let t = wpd::asm::vp8::raw_table(wpd::cpu::flags());

    macro_rules! take {
        ($($field:ident => $slot:ident,)*) => {
            $(if let Some(f) = t.$field {
                c.$slot = f;
            })*
        };
    }

    take! {
        luma_dc_wht => vp8_luma_dc_wht,
        luma_dc_wht_dc => vp8_luma_dc_wht_dc,
        idct_add => vp8_idct_add,
        idct_dc_add => vp8_idct_dc_add,
        idct_dc_add4y => vp8_idct_dc_add4y,
        idct_dc_add4uv => vp8_idct_dc_add4uv,
        v_loop_filter16y => vp8_v_loop_filter16y,
        h_loop_filter16y => vp8_h_loop_filter16y,
        v_loop_filter8uv => vp8_v_loop_filter8uv,
        h_loop_filter8uv => vp8_h_loop_filter8uv,
        v_loop_filter16y_inner => vp8_v_loop_filter16y_inner,
        h_loop_filter16y_inner => vp8_h_loop_filter16y_inner,
        v_loop_filter8uv_inner => vp8_v_loop_filter8uv_inner,
        h_loop_filter8uv_inner => vp8_h_loop_filter8uv_inner,
        v_loop_filter16y_mb => vp8_v_loop_filter16y_mb,
        h_loop_filter16y_mb => vp8_h_loop_filter16y_mb,
        v_loop_filter8uv_mb => vp8_v_loop_filter8uv_mb,
        h_loop_filter8uv_mb => vp8_h_loop_filter8uv_mb,
        v_loop_filter_simple => vp8_v_loop_filter_simple,
        h_loop_filter_simple => vp8_h_loop_filter_simple,
        v_loop_filter_simple_mb => vp8_v_loop_filter_simple_mb,
        h_loop_filter_simple_mb => vp8_h_loop_filter_simple_mb,
    }
}

/// Fills in `c` with the best implementation the running CPU allows.
///
/// # Safety
///
/// `c` must point to a writable, aligned `VP8DSPContext`.
#[no_mangle]
pub unsafe extern "C" fn ff_vp8dsp_init(c: *mut VP8DSPContext) {
    #[allow(unused_mut)]
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
    init_asm(&mut table);

    unsafe { c.write(table) }
}
