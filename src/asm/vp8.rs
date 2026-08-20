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

pub use super::Raw;

/* The kind picks the signature alias and the argument list that goes with it.
 * An arm no arch happens to use costs nothing, unlike an unused macro. */
macro_rules! raw_vp8 {
    ($m:ident, $i:ident, lf, $sym:literal) => {
        raw!($m, $i, LfRaw, $sym, (*mut u8, isize, c_int, c_int, c_int));
    };
    ($m:ident, $i:ident, lf_uv, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfUvRaw,
            $sym,
            (*mut u8, *mut u8, isize, c_int, c_int, c_int)
        );
    };
    ($m:ident, $i:ident, lf_mb, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfMbRaw,
            $sym,
            (*mut u8, isize, c_int, c_int, c_int, c_int)
        );
    };
    ($m:ident, $i:ident, lf_uv_mb, $sym:literal) => {
        raw!(
            $m,
            $i,
            LfUvMbRaw,
            $sym,
            (*mut u8, *mut u8, isize, c_int, c_int, c_int, c_int)
        );
    };
    ($m:ident, $i:ident, lf_simple, $sym:literal) => {
        raw!($m, $i, LfSimpleRaw, $sym, (*mut u8, isize, c_int));
    };
    ($m:ident, $i:ident, lf_simple_mb, $sym:literal) => {
        raw!($m, $i, LfSimpleMbRaw, $sym, (*mut u8, isize, c_int, c_int));
    };
    ($m:ident, $i:ident, wht, $sym:literal) => {
        raw!($m, $i, WhtRaw, $sym, (*mut [[i16; 16]; 4], *mut i16));
    };
    ($m:ident, $i:ident, idct, $sym:literal) => {
        raw!($m, $i, IdctRaw, $sym, (*mut u8, *mut i16, isize));
    };
    ($m:ident, $i:ident, idct4, $sym:literal) => {
        raw!($m, $i, Idct4Raw, $sym, (*mut u8, *mut [i16; 16], isize));
    };
}

macro_rules! lf_set {
    ($p:ident,
     $v_simple:literal, $h_simple:literal,
     $v16:literal, $h16:literal, $v8uv:literal, $h8uv:literal,
     $v16i:literal, $h16i:literal, $v8uvi:literal, $h8uvi:literal) => {
        pub mod $p {
            use super::*;

            raw_vp8!(VSimple, v_simple, lf_simple, $v_simple);
            raw_vp8!(HSimple, h_simple, lf_simple, $h_simple);
            raw_vp8!(V16, v16, lf, $v16);
            raw_vp8!(H16, h16, lf, $h16);
            raw_vp8!(V8uv, v8uv, lf_uv, $v8uv);
            raw_vp8!(H8uv, h8uv, lf_uv, $h8uv);
            raw_vp8!(V16Inner, v16_inner, lf, $v16i);
            raw_vp8!(H16Inner, h16_inner, lf, $h16i);
            raw_vp8!(V8uvInner, v8uv_inner, lf_uv, $v8uvi);
            raw_vp8!(H8uvInner, h8uv_inner, lf_uv, $h8uvi);
        }
    };
}

#[allow(unused_macros)]
macro_rules! idct_set {
    ($p:ident, $wht:literal, $add:literal, $dc_add:literal,
     $dc_add4y:literal, $dc_add4uv:literal) => {
        pub mod $p {
            use super::*;

            raw_vp8!(Wht, wht, wht, $wht);
            raw_vp8!(Add, add, idct, $add);
            raw_vp8!(DcAdd, dc_add, idct, $dc_add);
            raw_vp8!(DcAdd4y, dc_add4y, idct4, $dc_add4y);
            raw_vp8!(DcAdd4uv, dc_add4uv, idct4, $dc_add4uv);
        }
    };
}

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

macro_rules! composed_mb {
    ($set:ident) => {
        $crate::composed_mb!(simple pub v_simple_mb, vert, <$set::VSimple as Raw>::F);
        $crate::composed_mb!(simple pub h_simple_mb, horiz, <$set::HSimple as Raw>::F);
        $crate::composed_mb!(luma pub v16_mb, vert,
            <$set::V16 as Raw>::F, <$set::V16Inner as Raw>::F);
        $crate::composed_mb!(luma pub h16_mb, horiz,
            <$set::H16 as Raw>::F, <$set::H16Inner as Raw>::F);
        $crate::composed_mb!(chroma pub v8uv_mb, vert,
            <$set::V8uv as Raw>::F, <$set::V8uvInner as Raw>::F);
        $crate::composed_mb!(chroma pub h8uv_mb, horiz,
            <$set::H8uv as Raw>::F, <$set::H8uvInner as Raw>::F);

        marker!(VSimpleMb, LfSimpleMbRaw, v_simple_mb);
        marker!(HSimpleMb, LfSimpleMbRaw, h_simple_mb);
        marker!(V16Mb, LfMbRaw, v16_mb);
        marker!(H16Mb, LfMbRaw, h16_mb);
        marker!(V8uvMb, LfUvMbRaw, v8uv_mb);
        marker!(H8uvMb, LfUvMbRaw, h8uv_mb);
    };
}

/* Names a composed filter so the ladder can install it like a raw symbol. */
macro_rules! marker {
    ($name:ident, $sig:ty, $f:ident) => {
        pub struct $name;

        impl Raw for $name {
            type Sig = $sig;
            const F: $sig = $f;
        }
    };
}

macro_rules! raw_install_lf {
    ($t:expr, $p:ident, $mb:ident) => {
        $t.v_loop_filter_simple = Some($p::VSimple::F);
        $t.h_loop_filter_simple = Some($p::HSimple::F);
        $t.v_loop_filter_simple_mb = Some($mb::VSimpleMb::F);
        $t.h_loop_filter_simple_mb = Some($mb::HSimpleMb::F);

        $t.v_loop_filter16y = Some($p::V16::F);
        $t.h_loop_filter16y = Some($p::H16::F);
        $t.v_loop_filter8uv = Some($p::V8uv::F);
        $t.h_loop_filter8uv = Some($p::H8uv::F);

        $t.v_loop_filter16y_inner = Some($p::V16Inner::F);
        $t.h_loop_filter16y_inner = Some($p::H16Inner::F);
        $t.v_loop_filter8uv_inner = Some($p::V8uvInner::F);
        $t.h_loop_filter8uv_inner = Some($p::H8uvInner::F);

        $t.v_loop_filter16y_mb = Some($mb::V16Mb::F);
        $t.h_loop_filter16y_mb = Some($mb::H16Mb::F);
        $t.v_loop_filter8uv_mb = Some($mb::V8uvMb::F);
        $t.h_loop_filter8uv_mb = Some($mb::H8uvMb::F);
    };
}

macro_rules! raw_install_idct {
    ($t:expr, $p:ident) => {
        $t.luma_dc_wht = Some($p::Wht::F);
        $t.idct_add = Some($p::Add::F);
        $t.idct_dc_add = Some($p::DcAdd::F);
        $t.idct_dc_add4y = Some($p::DcAdd4y::F);
        $t.idct_dc_add4uv = Some($p::DcAdd4uv::F);
    };
}

#[derive(Default)]
pub struct RawTable {
    pub luma_dc_wht: Option<WhtRaw>,
    pub luma_dc_wht_dc: Option<WhtRaw>,
    pub idct_add: Option<IdctRaw>,
    pub idct_dc_add: Option<IdctRaw>,
    pub idct_dc_add4y: Option<Idct4Raw>,
    pub idct_dc_add4uv: Option<Idct4Raw>,
    pub v_loop_filter_simple: Option<LfSimpleRaw>,
    pub h_loop_filter_simple: Option<LfSimpleRaw>,
    pub v_loop_filter_simple_mb: Option<LfSimpleMbRaw>,
    pub h_loop_filter_simple_mb: Option<LfSimpleMbRaw>,
    pub v_loop_filter16y: Option<LfRaw>,
    pub h_loop_filter16y: Option<LfRaw>,
    pub v_loop_filter8uv: Option<LfUvRaw>,
    pub h_loop_filter8uv: Option<LfUvRaw>,
    pub v_loop_filter16y_inner: Option<LfRaw>,
    pub h_loop_filter16y_inner: Option<LfRaw>,
    pub v_loop_filter8uv_inner: Option<LfUvRaw>,
    pub h_loop_filter8uv_inner: Option<LfUvRaw>,
    pub v_loop_filter16y_mb: Option<LfMbRaw>,
    pub h_loop_filter16y_mb: Option<LfMbRaw>,
    pub v_loop_filter8uv_mb: Option<LfUvMbRaw>,
    pub h_loop_filter8uv_mb: Option<LfUvMbRaw>,
}

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

#[allow(unused_macros)]
macro_rules! ladder {
    ($(
        $(#[$attr:meta])*
        $flag:ident {
            $( @lf $lf:ident, $lf_mb:ident; )?
            $( @idct $idct:ident; )?
            $( $field:ident = $wrap:ident::<$marker:path>; )*
        }
    )*) => {
        pub fn init(c: &mut Vp8Dsp, flags: CpuFlags) {
            $(
                $(#[$attr])*
                if flags.contains(CpuFlags::$flag) {
                    $( install_lf!(c, $lf); )?
                    $( install_idct!(c, $idct); )?
                    $( c.$field = $wrap::<$marker>; )*
                }
            )*
        }

        pub fn raw_table(flags: CpuFlags) -> RawTable {
            let mut t = RawTable::default();

            $(
                $(#[$attr])*
                if flags.contains(CpuFlags::$flag) {
                    $( raw_install_lf!(t, $lf, $lf_mb); )?
                    $( raw_install_idct!(t, $idct); )?
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

    pub mod sse2_mb {
        use super::*;

        composed_mb!(sse2);
    }

    pub mod ssse3_mb {
        use super::*;

        composed_mb!(ssse3);
    }

    pub mod sse2_idct {
        use super::*;

        raw_vp8!(Wht, wht, wht, "ff_vp8_luma_dc_wht_sse2");
        raw_vp8!(Add, add, idct, "ff_vp8_idct_add_sse2");
        raw_vp8!(DcAdd, dc_add, idct, "ff_vp8_idct_dc_add_sse2");
        raw_vp8!(DcAdd4y, dc_add4y, idct4, "ff_vp8_idct_dc_add4y_sse2");
        raw_vp8!(DcAdd4uv, dc_add4uv, idct4, "ff_vp8_idct_dc_add4uv_sse2");
    }

    pub mod sse4 {
        use super::*;

        raw_vp8!(Wht, wht, wht, "ff_vp8_luma_dc_wht_sse4");
        raw_vp8!(DcAdd, dc_add, idct, "ff_vp8_idct_dc_add_sse4");
    }

    pub mod avx2 {
        use super::*;

        raw_vp8!(
            VSimpleMb,
            v_simple_mb,
            lf_simple_mb,
            "ff_vp8_v_loop_filter_simple_mb_avx2"
        );
        raw_vp8!(
            HSimpleMb,
            h_simple_mb,
            lf_simple_mb,
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

    #[repr(C, align(32))]
    pub struct Transposed(pub [u8; 16 * 16]);

    #[allow(clippy::missing_safety_doc)]
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

    #[allow(clippy::missing_safety_doc)]
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

    ladder! {
        SSE2 {
            @lf sse2, sse2_mb;
            @idct sse2_idct;
        }
        SSSE3 {
            @lf ssse3, ssse3_mb;
        }
        SSE41 {
            luma_dc_wht = wht::<sse4::Wht>;
            idct_dc_add = idct::<sse4::DcAdd>;
        }
        AVX2 {
            v_loop_filter_simple_mb = lf_v_simple_mb::<avx2::VSimpleMb>;
            h_loop_filter_simple_mb = lf_h_simple_mb::<avx2::HSimpleMb>;
            h_loop_filter16y_mb = lf_h_mb::<H16MbAvx2>;
            h_loop_filter8uv_mb = lf_h_uv_mb::<H8uvMbAvx2>;
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

    pub mod neon_mb {
        use super::*;

        composed_mb!(neon);
    }

    pub mod fused {
        use super::*;

        raw_vp8!(
            HSimpleMb,
            h_simple_mb,
            lf_simple_mb,
            "ff_vp8_h_loop_filter_simple_mb_neon"
        );
        raw_vp8!(H16Mb, h16_mb, lf_mb, "ff_vp8_h_loop_filter16y_mb_neon");
        raw_vp8!(H8uvMb, h8uv_mb, lf_uv_mb, "ff_vp8_h_loop_filter8uv_mb_neon");
    }

    ladder! {
        NEON {
            @lf neon, neon_mb;
            @idct neon_idct;

            h_loop_filter_simple_mb = lf_h_simple_mb::<fused::HSimpleMb>;
            h_loop_filter16y_mb = lf_h_mb::<fused::H16Mb>;
            h_loop_filter8uv_mb = lf_h_uv_mb::<fused::H8uvMb>;
        }
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

        raw_vp8!(WhtDc, wht_dc, wht, "ff_vp8_luma_dc_wht_dc_armv6");
    }

    pub mod neon_mb {
        use super::*;

        composed_mb!(neon);
    }

    #[cfg(wpd_asm_armv6)]
    pub mod armv6_mb {
        use super::*;

        composed_mb!(armv6);
    }

    ladder! {
        #[cfg(wpd_asm_armv6)]
        ARMV6 {
            @lf armv6, armv6_mb;
            @idct armv6_idct;

            luma_dc_wht_dc = wht::<armv6_wht_dc::WhtDc>;
        }
        NEON {
            @lf neon, neon_mb;
            @idct neon_idct;
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

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
