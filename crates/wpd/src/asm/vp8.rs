//! The lossy DSP assembly.
//!
//! Every symbol the hand-written assembly exports is declared here once. The
//! raw declaration is public, because `wpd-capi` builds its C ABI table out of
//! the same items and has to put the bare symbol in the table so that
//! `checkasm --bench` times the assembly and not a wrapper.
//!
//! What the core crate sees instead is [`init`], which fills a [`Vp8Dsp`] with
//! safe functions. Each one checks that the plane really does extend as far as
//! the kernel reads before handing over a pointer, so the only thing outside
//! this module that has to be true is what the type system already says.
//!
//! # Regions
//!
//! `o` is the offset of the position a filter acts on and `s` the stride, both
//! in bytes. A filter that works across a horizontal edge reads `up` rows above
//! `o` and writes as far as `down` rows below it, over `n` columns; one working
//! across a vertical edge reads `left` bytes before `o` and writes as far as
//! `right - 1` bytes after it, in each of `n` rows. The macroblock variants
//! fold in the three subblock edges inside the macroblock, which is why their
//! `down` and `right` reach a whole macroblock further than the plain ones.

use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::vp8::Vp8Dsp;

pub type LfRaw = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int);
pub type LfUvRaw = unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int);
pub type LfMbRaw = unsafe extern "C" fn(*mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfUvMbRaw =
    unsafe extern "C" fn(*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int);
pub type LfSimpleRaw = unsafe extern "C" fn(*mut u8, isize, c_int);
pub type LfSimpleMbRaw = unsafe extern "C" fn(*mut u8, isize, c_int, c_int);
pub type WhtRaw = unsafe extern "C" fn(*mut [[i16; 16]; 4], *mut i16);
pub type IdctRaw = unsafe extern "C" fn(*mut u8, *mut i16, isize);
pub type Idct4Raw = unsafe extern "C" fn(*mut u8, *mut [i16; 16], isize);

#[inline(always)]
fn check_v(p: &[u8], o: usize, s: usize, up: usize, down: usize, n: usize) {
    assert!(
        o >= up * s && p.len() >= o + down * s + n,
        "plane too small"
    );
}

#[inline(always)]
fn check_h(p: &[u8], o: usize, s: usize, left: usize, right: usize, n: usize) {
    assert!(
        o >= left && p.len() >= o + (n - 1) * s + right,
        "plane too small"
    );
}

/// One assembly entry point, named so it can be used as a type parameter.
///
/// A table field is a plain `fn` pointer with nowhere to keep the symbol, so
/// the symbol travels as an associated constant and each wrapper is
/// monomorphised into its own function item.
pub trait Raw {
    type Sig: Copy;
    const F: Self::Sig;
}

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

macro_rules! raw_lf {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, LfRaw, $sym, (*mut u8, isize, c_int, c_int, c_int));
    };
}

macro_rules! raw_lf_uv {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfUvRaw,
            $sym,
            (*mut u8, *mut u8, isize, c_int, c_int, c_int)
        );
    };
}

#[allow(unused_macros)]
macro_rules! raw_lf_mb {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfMbRaw,
            $sym,
            (*mut u8, isize, c_int, c_int, c_int, c_int)
        );
    };
}

#[allow(unused_macros)]
macro_rules! raw_lf_uv_mb {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfUvMbRaw,
            $sym,
            (*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int)
        );
    };
}

macro_rules! raw_lf_simple {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, LfSimpleRaw, $sym, (*mut u8, isize, c_int));
    };
}

macro_rules! raw_lf_simple_mb {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, LfSimpleMbRaw, $sym, (*mut u8, isize, c_int, c_int));
    };
}

macro_rules! raw_wht {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, WhtRaw, $sym, (*mut [[i16; 16]; 4], *mut i16));
    };
}

macro_rules! raw_idct {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, IdctRaw, $sym, (*mut u8, *mut i16, isize));
    };
}

macro_rules! raw_idct4 {
    ($m:ident, $i:ident, $sym:literal) => {
        raw!($m, $i, Idct4Raw, $sym, (*mut u8, *mut [i16; 16], isize));
    };
}

/// Declares the ten loop filter symbols one instruction set provides, under
/// marker names prefixed with `$p`.
macro_rules! lf_set {
    ($p:ident,
     $v_simple:literal, $h_simple:literal,
     $v16:literal, $h16:literal, $v8uv:literal, $h8uv:literal,
     $v16i:literal, $h16i:literal, $v8uvi:literal, $h8uvi:literal) => {
        pub mod $p {
            use super::*;

            raw_lf_simple!(VSimple, v_simple, $v_simple);
            raw_lf_simple!(HSimple, h_simple, $h_simple);
            raw_lf!(V16, v16, $v16);
            raw_lf!(H16, h16, $h16);
            raw_lf_uv!(V8uv, v8uv, $v8uv);
            raw_lf_uv!(H8uv, h8uv, $h8uv);
            raw_lf!(V16Inner, v16_inner, $v16i);
            raw_lf!(H16Inner, h16_inner, $h16i);
            raw_lf_uv!(V8uvInner, v8uv_inner, $v8uvi);
            raw_lf_uv!(H8uvInner, h8uv_inner, $h8uvi);
        }
    };
}

/// Declares the five transform symbols one instruction set provides.
#[allow(unused_macros)]
macro_rules! idct_set {
    ($p:ident, $wht:literal, $add:literal, $dc_add:literal,
     $dc_add4y:literal, $dc_add4uv:literal) => {
        pub mod $p {
            use super::*;

            raw_wht!(Wht, wht, $wht);
            raw_idct!(Add, add, $add);
            raw_idct!(DcAdd, dc_add, $dc_add);
            raw_idct4!(DcAdd4y, dc_add4y, $dc_add4y);
            raw_idct4!(DcAdd4uv, dc_add4uv, $dc_add4uv);
        }
    };
}

// The safe wrappers. Each is generic over the symbol it calls, so one body
// serves every instruction set.

fn lf_v<T: Raw<Sig = LfRaw>, const N: usize>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    i: i32,
    hev: i32,
) {
    check_v(p, o, s, 4, 3, N);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, i, hev) }
}

fn lf_h<T: Raw<Sig = LfRaw>, const N: usize>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    i: i32,
    hev: i32,
) {
    check_h(p, o, s, 4, 4, N);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, i, hev) }
}

#[allow(clippy::too_many_arguments)]
fn lf_v_uv<T: Raw<Sig = LfUvRaw>>(
    u: &mut [u8],
    ou: usize,
    v: &mut [u8],
    ov: usize,
    s: usize,
    e: i32,
    i: i32,
    hev: i32,
) {
    check_v(u, ou, s, 4, 3, 8);
    check_v(v, ov, s, 4, 3, 8);
    unsafe {
        (T::F)(
            u.as_mut_ptr().add(ou),
            v.as_mut_ptr().add(ov),
            s as isize,
            e,
            i,
            hev,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn lf_h_uv<T: Raw<Sig = LfUvRaw>>(
    u: &mut [u8],
    ou: usize,
    v: &mut [u8],
    ov: usize,
    s: usize,
    e: i32,
    i: i32,
    hev: i32,
) {
    check_h(u, ou, s, 4, 4, 8);
    check_h(v, ov, s, 4, 4, 8);
    unsafe {
        (T::F)(
            u.as_mut_ptr().add(ou),
            v.as_mut_ptr().add(ov),
            s as isize,
            e,
            i,
            hev,
        )
    }
}

#[allow(dead_code)]
fn lf_v_mb<T: Raw<Sig = LfMbRaw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) {
    check_v(p, o, s, 4, 15, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, be, i, hev) }
}

fn lf_h_mb<T: Raw<Sig = LfMbRaw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) {
    check_h(p, o, s, 4, 16, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, be, i, hev) }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn lf_v_uv_mb<T: Raw<Sig = LfUvMbRaw>>(
    u: &mut [u8],
    ou: usize,
    v: &mut [u8],
    ov: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) {
    check_v(u, ou, s, 4, 7, 8);
    check_v(v, ov, s, 4, 7, 8);
    unsafe {
        (T::F)(
            u.as_mut_ptr().add(ou),
            v.as_mut_ptr().add(ov),
            s as isize,
            e,
            be,
            i,
            hev,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn lf_h_uv_mb<T: Raw<Sig = LfUvMbRaw>>(
    u: &mut [u8],
    ou: usize,
    v: &mut [u8],
    ov: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) {
    check_h(u, ou, s, 4, 8, 8);
    check_h(v, ov, s, 4, 8, 8);
    unsafe {
        (T::F)(
            u.as_mut_ptr().add(ou),
            v.as_mut_ptr().add(ov),
            s as isize,
            e,
            be,
            i,
            hev,
        )
    }
}

fn lf_v_simple<T: Raw<Sig = LfSimpleRaw>>(p: &mut [u8], o: usize, s: usize, e: i32) {
    check_v(p, o, s, 2, 1, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e) }
}

fn lf_h_simple<T: Raw<Sig = LfSimpleRaw>>(p: &mut [u8], o: usize, s: usize, e: i32) {
    check_h(p, o, s, 2, 2, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e) }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn lf_v_simple_mb<T: Raw<Sig = LfSimpleMbRaw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
) {
    check_v(p, o, s, 2, 13, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, be) }
}

fn lf_h_simple_mb<T: Raw<Sig = LfSimpleMbRaw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
) {
    check_h(p, o, s, 2, 14, 16);
    unsafe { (T::F)(p.as_mut_ptr().add(o), s as isize, e, be) }
}

/// Composes a macroblock edge filter out of an edge kernel and an inner one,
/// for the instruction sets that do not provide a fused entry point. The C
/// spelled this out per set with its `VP8_*_LOOP_FILTER*_MB` macros.
fn mb_from<E, I, const VERT: bool>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) where
    E: Raw<Sig = LfRaw>,
    I: Raw<Sig = LfRaw>,
{
    let step = if VERT { 4 * s } else { 4 };

    if VERT {
        check_v(p, o, s, 4, 15, 16);
    } else {
        check_h(p, o, s, 4, 16, 16);
    }
    unsafe {
        let d = p.as_mut_ptr().add(o);

        (E::F)(d, s as isize, e, i, hev);
        (I::F)(d.add(step), s as isize, be, i, hev);
        (I::F)(d.add(2 * step), s as isize, be, i, hev);
        (I::F)(d.add(3 * step), s as isize, be, i, hev);
    }
}

#[allow(clippy::too_many_arguments)]
fn uv_mb_from<E, I, const VERT: bool>(
    u: &mut [u8],
    ou: usize,
    v: &mut [u8],
    ov: usize,
    s: usize,
    e: i32,
    be: i32,
    i: i32,
    hev: i32,
) where
    E: Raw<Sig = LfUvRaw>,
    I: Raw<Sig = LfUvRaw>,
{
    let step = if VERT { 4 * s } else { 4 };

    if VERT {
        check_v(u, ou, s, 4, 7, 8);
        check_v(v, ov, s, 4, 7, 8);
    } else {
        check_h(u, ou, s, 4, 8, 8);
        check_h(v, ov, s, 4, 8, 8);
    }
    unsafe {
        let (du, dv) = (u.as_mut_ptr().add(ou), v.as_mut_ptr().add(ov));

        (E::F)(du, dv, s as isize, e, i, hev);
        (I::F)(du.add(step), dv.add(step), s as isize, be, i, hev);
    }
}

fn simple_mb_from<T: Raw<Sig = LfSimpleRaw>, const VERT: bool>(
    p: &mut [u8],
    o: usize,
    s: usize,
    e: i32,
    be: i32,
) {
    let step = if VERT { 4 * s } else { 4 };

    if VERT {
        check_v(p, o, s, 2, 13, 16);
    } else {
        check_h(p, o, s, 2, 14, 16);
    }
    unsafe {
        let d = p.as_mut_ptr().add(o);

        (T::F)(d, s as isize, e);
        (T::F)(d.add(step), s as isize, be);
        (T::F)(d.add(2 * step), s as isize, be);
        (T::F)(d.add(3 * step), s as isize, be);
    }
}

fn wht<T: Raw<Sig = WhtRaw>>(block: &mut [[i16; 16]; 16], dc: &mut [i16; 16]) {
    unsafe { (T::F)(block.as_mut_ptr().cast(), dc.as_mut_ptr()) }
}

fn idct<T: Raw<Sig = IdctRaw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    block: &mut [i16; 16],
) {
    assert!(p.len() >= o + 3 * s + 4, "plane too small");
    unsafe { (T::F)(p.as_mut_ptr().add(o), block.as_mut_ptr(), s as isize) }
}

fn idct4y<T: Raw<Sig = Idct4Raw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    block: &mut [[i16; 16]; 4],
) {
    assert!(p.len() >= o + 3 * s + 16, "plane too small");
    unsafe { (T::F)(p.as_mut_ptr().add(o), block.as_mut_ptr(), s as isize) }
}

fn idct4uv<T: Raw<Sig = Idct4Raw>>(
    p: &mut [u8],
    o: usize,
    s: usize,
    block: &mut [[i16; 16]; 4],
) {
    assert!(p.len() >= o + 7 * s + 8, "plane too small");
    unsafe { (T::F)(p.as_mut_ptr().add(o), block.as_mut_ptr(), s as isize) }
}

/// Installs everything one instruction set's ten loop filters cover, composing
/// the four macroblock entries from the edge and inner kernels.
macro_rules! install_lf {
    ($c:expr, $p:ident) => {
        $c.v_loop_filter_simple = lf_v_simple::<$p::VSimple>;
        $c.h_loop_filter_simple = lf_h_simple::<$p::HSimple>;
        $c.v_loop_filter_simple_mb = simple_mb_from::<$p::VSimple, true>;
        $c.h_loop_filter_simple_mb = simple_mb_from::<$p::HSimple, false>;

        $c.v_loop_filter16y = lf_v::<$p::V16, 16>;
        $c.h_loop_filter16y = lf_h::<$p::H16, 16>;
        $c.v_loop_filter8uv = lf_v_uv::<$p::V8uv>;
        $c.h_loop_filter8uv = lf_h_uv::<$p::H8uv>;

        $c.v_loop_filter16y_inner = lf_v::<$p::V16Inner, 16>;
        $c.h_loop_filter16y_inner = lf_h::<$p::H16Inner, 16>;
        $c.v_loop_filter8uv_inner = lf_v_uv::<$p::V8uvInner>;
        $c.h_loop_filter8uv_inner = lf_h_uv::<$p::H8uvInner>;

        $c.v_loop_filter16y_mb = mb_from::<$p::V16, $p::V16Inner, true>;
        $c.h_loop_filter16y_mb = mb_from::<$p::H16, $p::H16Inner, false>;
        $c.v_loop_filter8uv_mb = uv_mb_from::<$p::V8uv, $p::V8uvInner, true>;
        $c.h_loop_filter8uv_mb = uv_mb_from::<$p::H8uv, $p::H8uvInner, false>;
    };
}

macro_rules! install_idct {
    ($c:expr, $p:ident) => {
        $c.luma_dc_wht = wht::<$p::Wht>;
        $c.idct_add = idct::<$p::Add>;
        $c.idct_dc_add = idct::<$p::DcAdd>;
        $c.idct_dc_add4y = idct4y::<$p::DcAdd4y>;
        $c.idct_dc_add4uv = idct4uv::<$p::DcAdd4uv>;
    };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod arch {
    use super::*;

    lf_set!(
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
    lf_set!(
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

    pub mod sse2_idct {
        use super::*;

        raw_wht!(Wht, wht, "ff_vp8_luma_dc_wht_sse2");
        raw_idct!(Add, add, "ff_vp8_idct_add_sse2");
        raw_idct!(DcAdd, dc_add, "ff_vp8_idct_dc_add_sse2");
        raw_idct4!(DcAdd4y, dc_add4y, "ff_vp8_idct_dc_add4y_sse2");
        raw_idct4!(DcAdd4uv, dc_add4uv, "ff_vp8_idct_dc_add4uv_sse2");
    }

    pub mod sse4 {
        use super::*;

        raw_wht!(Wht, wht, "ff_vp8_luma_dc_wht_sse4");
        raw_idct!(DcAdd, dc_add, "ff_vp8_idct_dc_add_sse4");
    }

    pub mod avx2 {
        use super::*;

        raw_lf_uv!(V8uvInner, v8uv_inner, "ff_vp8_v_loop_filter8uv_inner_avx2");
        raw_lf_simple_mb!(
            VSimpleMb,
            v_simple_mb,
            "ff_vp8_v_loop_filter_simple_mb_avx2"
        );
        raw_lf_simple_mb!(
            HSimpleMb,
            h_simple_mb,
            "ff_vp8_h_loop_filter_simple_mb_avx2"
        );

        extern "C" {
            #[link_name = "ff_vp8_h_loop_filter16y_mb_transpose_avx2"]
            pub fn h16_transpose(dst: *mut u8, stride: isize, tmp: *mut u8);
            #[link_name = "ff_vp8_h_loop_filter16y_mb_itranspose_avx2"]
            pub fn h16_itranspose(dst: *mut u8, stride: isize, tmp: *const u8);
            #[link_name = "ff_vp8_h_loop_filter8uv_mb_transpose_avx2"]
            pub fn h8uv_transpose(
                dst_u: *mut u8,
                dst_v: *mut u8,
                stride: isize,
                tmp: *mut u8,
            );
            #[link_name = "ff_vp8_h_loop_filter8uv_mb_itranspose_avx2"]
            pub fn h8uv_itranspose(
                dst_u: *mut u8,
                dst_v: *mut u8,
                stride: isize,
                tmp: *const u8,
            );
        }
    }

    /// The AVX2 horizontal macroblock filters transpose into this, run the
    /// vertical SSSE3 kernels over it, and transpose back.
    #[repr(C, align(32))]
    pub struct Transposed(pub [u8; 16 * 16]);

    /// # Safety
    ///
    /// As the C prototype: `dst` is the edge of a plane with four columns
    /// before it and sixteen from it, in each of sixteen rows.
    pub unsafe extern "C" fn h16_mb_avx2(
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
            avx2::h16_transpose(dst, stride, t);
            (ssse3::V16::F)(t.add(4 * 16), 16, mbedge_e, flim_i, hev);
            (ssse3::V16Inner::F)(t.add(8 * 16), 16, bedge_e, flim_i, hev);
            (ssse3::V16Inner::F)(t.add(12 * 16), 16, bedge_e, flim_i, hev);
            avx2::h16_itranspose(dst, stride, t);
            (ssse3::H16Inner::F)(dst.add(12), stride, bedge_e, flim_i, hev);
        }
    }

    /// # Safety
    ///
    /// As [`h16_mb_avx2`], for the two chroma planes.
    pub unsafe extern "C" fn h8uv_mb_avx2(
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
            avx2::h8uv_transpose(dst_u, dst_v, stride, t);
            (ssse3::V16::F)(t.add(4 * 16), 16, mbedge_e, flim_i, hev);
            (ssse3::V16Inner::F)(t.add(8 * 16), 16, bedge_e, flim_i, hev);
            avx2::h8uv_itranspose(dst_u, dst_v, stride, t);
        }
    }

    pub struct H16MbAvx2;

    impl Raw for H16MbAvx2 {
        type Sig = LfMbRaw;
        const F: LfMbRaw = h16_mb_avx2;
    }

    pub struct H8uvMbAvx2;

    impl Raw for H8uvMbAvx2 {
        type Sig = LfUvMbRaw;
        const F: LfUvMbRaw = h8uv_mb_avx2;
    }

    pub fn init(c: &mut Vp8Dsp, flags: CpuFlags) {
        if flags.contains(CpuFlags::SSE2) {
            install_lf!(c, sse2);
            install_idct!(c, sse2_idct);
        }
        if flags.contains(CpuFlags::SSSE3) {
            install_lf!(c, ssse3);
        }
        if flags.contains(CpuFlags::SSE41) {
            c.luma_dc_wht = wht::<sse4::Wht>;
            c.idct_dc_add = idct::<sse4::DcAdd>;
        }
        if flags.contains(CpuFlags::AVX2) {
            c.v_loop_filter8uv_inner = lf_v_uv::<avx2::V8uvInner>;
            c.v_loop_filter_simple_mb = lf_v_simple_mb::<avx2::VSimpleMb>;
            c.h_loop_filter_simple_mb = lf_h_simple_mb::<avx2::HSimpleMb>;
            c.h_loop_filter16y_mb = lf_h_mb::<H16MbAvx2>;
            c.h_loop_filter8uv_mb = lf_h_uv_mb::<H8uvMbAvx2>;
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;

    lf_set!(
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
    idct_set!(
        neon_idct,
        "ff_vp8_luma_dc_wht_neon",
        "ff_vp8_idct_add_neon",
        "ff_vp8_idct_dc_add_neon",
        "ff_vp8_idct_dc_add4y_neon",
        "ff_vp8_idct_dc_add4uv_neon"
    );

    pub mod fused {
        use super::*;

        raw_lf_simple_mb!(
            HSimpleMb,
            h_simple_mb,
            "ff_vp8_h_loop_filter_simple_mb_neon"
        );
        raw_lf_mb!(H16Mb, h16_mb, "ff_vp8_h_loop_filter16y_mb_neon");
        raw_lf_uv_mb!(H8uvMb, h8uv_mb, "ff_vp8_h_loop_filter8uv_mb_neon");
    }

    pub fn init(c: &mut Vp8Dsp, flags: CpuFlags) {
        if !flags.contains(CpuFlags::NEON) {
            return;
        }
        install_lf!(c, neon);
        install_idct!(c, neon_idct);

        c.h_loop_filter_simple_mb = lf_h_simple_mb::<fused::HSimpleMb>;
        c.h_loop_filter16y_mb = lf_h_mb::<fused::H16Mb>;
        c.h_loop_filter8uv_mb = lf_h_uv_mb::<fused::H8uvMb>;
    }
}

#[cfg(target_arch = "arm")]
mod arch {
    use super::*;

    lf_set!(
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
    idct_set!(
        neon_idct,
        "ff_vp8_luma_dc_wht_neon",
        "ff_vp8_idct_add_neon",
        "ff_vp8_idct_dc_add_neon",
        "ff_vp8_idct_dc_add4y_neon",
        "ff_vp8_idct_dc_add4uv_neon"
    );

    #[cfg(wpd_asm_armv6)]
    lf_set!(
        armv6,
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
    #[cfg(wpd_asm_armv6)]
    idct_set!(
        armv6_idct,
        "ff_vp8_luma_dc_wht_armv6",
        "ff_vp8_idct_add_armv6",
        "ff_vp8_idct_dc_add_armv6",
        "ff_vp8_idct_dc_add4y_armv6",
        "ff_vp8_idct_dc_add4uv_armv6"
    );

    #[cfg(wpd_asm_armv6)]
    pub mod armv6_wht_dc {
        use super::*;

        raw_wht!(WhtDc, wht_dc, "ff_vp8_luma_dc_wht_dc_armv6");
    }

    pub fn init(c: &mut Vp8Dsp, flags: CpuFlags) {
        #[cfg(wpd_asm_armv6)]
        if flags.contains(CpuFlags::ARMV6) {
            install_lf!(c, armv6);
            install_idct!(c, armv6_idct);
            c.luma_dc_wht_dc = wht::<armv6_wht_dc::WhtDc>;
        }
        if flags.contains(CpuFlags::NEON) {
            install_lf!(c, neon);
            install_idct!(c, neon_idct);
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

    pub fn init(_c: &mut Vp8Dsp, _flags: CpuFlags) {}
}

pub use arch::*;
