use std::ffi::c_int;
use std::slice;

use wpd::dsp::vp8 as k;

pub type WhtFn = unsafe extern "C" fn(*mut [[i16; 16]; 16], *mut i16);
pub type IdctFn = unsafe extern "C" fn(*mut u8, *mut i16, isize);
pub type Idct4Fn = unsafe extern "C" fn(*mut u8, *mut [i16; 16], isize);
pub type LfFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int);
pub type LfUvFn = unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int);
pub type LfMbFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfUvMbFn =
    unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfSimpleFn = unsafe extern "C" fn(*mut u8, isize, c_int);
pub type LfSimpleMbFn = unsafe extern "C" fn(*mut u8, isize, c_int, c_int);
pub type LfAllFn =
    unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int, c_int, c_int);
pub type LfUvAllFn =
    unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int, c_int);

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

    pub vp8_loop_filter16y: LfAllFn,
    pub vp8_loop_filter8uv: LfUvAllFn,
}

unsafe fn lf_v<const SIZE: usize, const INNER: bool>(
    dst: *mut u8,
    stride: isize,
    flim_e: c_int,
    flim_i: c_int,
    hev: c_int,
) {
    let s = stride as usize;
    // SAFETY: callers pass `dst` with a positive `stride` inside one writable
    // buffer covering the edge's footprint, 4 rows above and 3 below:
    // `[dst - 4 * stride, dst + 3 * stride + SIZE)`.
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
    // SAFETY: callers pass `dst` with a positive `stride` inside one writable
    // buffer covering the edge's footprint, 4 columns either side over `SIZE`
    // rows: `[dst - 4, dst + (SIZE - 1) * stride + 4)`.
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
            // SAFETY: the caller (checkasm) passes `dst` and `stride` covering
            // the 16-sample edge's footprint that `$driver` requires.
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
            // SAFETY: the caller (checkasm) passes `dst_u` and `dst_v`, sharing
            // `stride`, each covering the 8-sample edge's footprint that
            // `$driver` requires; the two calls never hold slices at once.
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
    // SAFETY: the caller (checkasm) passes `dst` with a positive `stride` inside
    // one writable buffer covering 2 rows above and 1 below the 16-wide edge:
    // `[dst - 2 * stride, dst + stride + 16)`.
    let buf = unsafe { slice::from_raw_parts_mut(dst.offset(-2 * stride), 3 * s + 16) };

    k::loop_filter_simple::<true>(buf, s, flim);
}

unsafe extern "C" fn h_loop_filter_simple_c(dst: *mut u8, stride: isize, flim: c_int) {
    let s = stride as usize;
    // SAFETY: the caller (checkasm) passes `dst` with a positive `stride` inside
    // one writable buffer covering 2 columns either side over 16 rows:
    // `[dst - 2, dst + 15 * stride + 2)`.
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

/* The spec's order: the left edge, the inner columns, the top edge, the inner
 * rows. */
unsafe extern "C" fn loop_filter16y_c(
    dst: *mut u8,
    stride: isize,
    mbedge_e: c_int,
    bedge_e: c_int,
    flim_i: c_int,
    hev: c_int,
    edges: c_int,
) {
    // SAFETY: the caller (checkasm) passes `dst` at a 16x16 macroblock with 4
    // writable rows above and 4 columns left of it at `stride`, which covers
    // every edge filtered here and so each callee's footprint.
    unsafe {
        if edges & 1 != 0 {
            h_loop_filter16y_c(dst, stride, mbedge_e, flim_i, hev);
        }
        for k in [4, 8, 12] {
            h_loop_filter16y_inner_c(dst.add(k), stride, bedge_e, flim_i, hev);
        }
        if edges & 2 != 0 {
            v_loop_filter16y_c(dst, stride, mbedge_e, flim_i, hev);
        }
        for k in [4, 8, 12] {
            let row = dst.offset(k * stride);

            v_loop_filter16y_inner_c(row, stride, bedge_e, flim_i, hev);
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn loop_filter8uv_c(
    dst_u: *mut u8,
    dst_v: *mut u8,
    stride: isize,
    mbedge_e: c_int,
    bedge_e: c_int,
    flim_i: c_int,
    hev: c_int,
    edges: c_int,
) {
    // SAFETY: the caller (checkasm) passes `dst_u` and `dst_v` each at an 8x8
    // block with 4 writable rows above and 4 columns left of it at `stride`,
    // which covers every edge filtered here and so each callee's footprint.
    unsafe {
        let (u4, v4) = (dst_u.add(4), dst_v.add(4));

        if edges & 1 != 0 {
            h_loop_filter8uv_c(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
        }
        h_loop_filter8uv_inner_c(u4, v4, stride, bedge_e, flim_i, hev);
        if edges & 2 != 0 {
            v_loop_filter8uv_c(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
        }

        let (u4, v4) = (dst_u.offset(4 * stride), dst_v.offset(4 * stride));

        v_loop_filter8uv_inner_c(u4, v4, stride, bedge_e, flim_i, hev);
    }
}

unsafe extern "C" fn luma_dc_wht_c(block: *mut [[i16; 16]; 16], dc: *mut i16) {
    // SAFETY: the caller (checkasm) passes `block` valid for 16 blocks of 16
    // coefficients and `dc` for 16 more, aligned, writable and disjoint.
    unsafe { k::luma_dc_wht(&mut *block, &mut *dc.cast::<[i16; 16]>()) }
}

unsafe extern "C" fn luma_dc_wht_dc_c(block: *mut [[i16; 16]; 16], dc: *mut i16) {
    // SAFETY: the caller (checkasm) passes `block` valid for 16 blocks of 16
    // coefficients and `dc` for 16 more, aligned, writable and disjoint.
    unsafe { k::luma_dc_wht_dc(&mut *block, &mut *dc.cast::<[i16; 16]>()) }
}

unsafe extern "C" fn idct_add_c(dst: *mut u8, block: *mut i16, stride: isize) {
    let s = stride as usize;

    // SAFETY: the caller (checkasm) passes `dst` valid for 4 writable rows of
    // 4 bytes at a positive `stride`, and `block` for 16 aligned, writable
    // coefficients outside them.
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

    // SAFETY: the caller passes `dst` valid for 4 writable rows of 4 bytes at a
    // positive `stride`, and `block` for 16 aligned, writable coefficients
    // outside them.
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
        // SAFETY: the caller (checkasm) passes `dst` valid for 4 rows of 16
        // bytes at `stride` and `block` for 4 coefficient blocks, so subblock
        // `i` meets `idct_dc_add_c`'s contract.
        unsafe { idct_dc_add_c(dst.add(4 * i), block.add(i).cast(), stride) }
    }
}

unsafe extern "C" fn idct_dc_add4uv_c(
    dst: *mut u8,
    block: *mut [i16; 16],
    stride: isize,
) {
    for i in 0..4 {
        // SAFETY: the caller (checkasm) passes `dst` valid for 8 rows of 8
        // bytes at `stride` and `block` for 4 coefficient blocks, so the 2x2
        // subblock `i` meets `idct_dc_add_c`'s contract.
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
        loop_filter16y => vp8_loop_filter16y,
        loop_filter8uv => vp8_loop_filter8uv,
    }
}

/// Fills `*c` with the VP8 transform and loop filter kernels for the running
/// CPU.
///
/// # Safety
///
/// `c` must be non-null, aligned and valid for a write of one
/// `VP8DSPContext`. Its previous contents are overwritten without being read.
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

        vp8_loop_filter16y: loop_filter16y_c,
        vp8_loop_filter8uv: loop_filter8uv_c,
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

    // SAFETY: `c` is valid for a write of one `VP8DSPContext` per this
    // function's `# Safety` contract.
    unsafe { c.write(table) }
}
