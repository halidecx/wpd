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

wpd::composed_mb!(luma h_loop_filter16y_mb_c, horiz,
    h_loop_filter16y_c, h_loop_filter16y_inner_c);
wpd::composed_mb!(luma v_loop_filter16y_mb_c, vert,
    v_loop_filter16y_c, v_loop_filter16y_inner_c);
wpd::composed_mb!(chroma h_loop_filter8uv_mb_c, horiz,
    h_loop_filter8uv_c, h_loop_filter8uv_inner_c);
wpd::composed_mb!(chroma v_loop_filter8uv_mb_c, vert,
    v_loop_filter8uv_c, v_loop_filter8uv_inner_c);
wpd::composed_mb!(simple h_loop_filter_simple_mb_c, horiz, h_loop_filter_simple_c);
wpd::composed_mb!(simple v_loop_filter_simple_mb_c, vert, v_loop_filter_simple_c);

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

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
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
