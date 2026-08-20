use super::clip_uint8;

#[inline(always)]
fn clip_int8(v: i32) -> i32 {
    i32::from(clip_uint8(v + 0x80)) - 0x80
}

type Win = [i32; 8];

#[inline(always)]
fn load8<const VERT: bool>(buf: &[u8], q: usize, s: usize) -> Win {
    if !VERT {
        let r = &buf[q..q + 8];
        [
            r[0].into(),
            r[1].into(),
            r[2].into(),
            r[3].into(),
            r[4].into(),
            r[5].into(),
            r[6].into(),
            r[7].into(),
        ]
    } else {
        [
            buf[q].into(),
            buf[q + s].into(),
            buf[q + 2 * s].into(),
            buf[q + 3 * s].into(),
            buf[q + 4 * s].into(),
            buf[q + 5 * s].into(),
            buf[q + 6 * s].into(),
            buf[q + 7 * s].into(),
        ]
    }
}

#[inline(always)]
fn load4<const VERT: bool>(buf: &[u8], q: usize, s: usize) -> [i32; 4] {
    if !VERT {
        let r = &buf[q..q + 4];
        [r[0].into(), r[1].into(), r[2].into(), r[3].into()]
    } else {
        [
            buf[q].into(),
            buf[q + s].into(),
            buf[q + 2 * s].into(),
            buf[q + 3 * s].into(),
        ]
    }
}

#[inline(always)]
fn filter_common(buf: &mut [u8], w: [i32; 4], p1i: usize, s: usize, is4tap: bool) {
    let mut a = 3 * (w[2] - w[1]);

    if is4tap {
        a += clip_int8(w[0] - w[3]);
    }
    a = clip_int8(a);

    let f1 = (a + 4).min(127) >> 3;
    let f2 = (a + 3).min(127) >> 3;

    buf[p1i + s] = clip_uint8(w[1] + f2);
    buf[p1i + 2 * s] = clip_uint8(w[2] - f1);

    if !is4tap {
        let a = (f1 + 1) >> 1;
        buf[p1i] = clip_uint8(w[0] + a);
        buf[p1i + 3 * s] = clip_uint8(w[3] - a);
    }
}

#[inline(always)]
fn simple_limit(w: [i32; 4], flim: i32) -> bool {
    2 * (w[1] - w[2]).abs() + ((w[0] - w[3]).abs() >> 1) <= flim
}

#[inline(always)]
fn normal_limit(w: &Win, e: i32, i: i32) -> bool {
    let d = |a: usize, b: usize| (w[a] - w[b]).abs() <= i;

    simple_limit([w[2], w[3], w[4], w[5]], e)
        && d(0, 1)
        && d(1, 2)
        && d(2, 3)
        && d(7, 6)
        && d(6, 5)
        && d(5, 4)
}

#[inline(always)]
fn hev(w: &Win, thresh: i32) -> bool {
    (w[2] - w[3]).abs() > thresh || (w[5] - w[4]).abs() > thresh
}

#[inline(always)]
fn filter_mbedge(buf: &mut [u8], w: &Win, p2i: usize, s: usize) {
    let a = clip_int8(w[2] - w[5]);
    let a = clip_int8(a + 3 * (w[4] - w[3]));

    let a0 = (27 * a + 63) >> 7;
    let a1 = (18 * a + 63) >> 7;
    let a2 = (9 * a + 63) >> 7;

    buf[p2i] = clip_uint8(w[1] + a2);
    buf[p2i + s] = clip_uint8(w[2] + a1);
    buf[p2i + 2 * s] = clip_uint8(w[3] + a0);
    buf[p2i + 3 * s] = clip_uint8(w[4] - a0);
    buf[p2i + 4 * s] = clip_uint8(w[5] - a1);
    buf[p2i + 5 * s] = clip_uint8(w[6] - a2);
}

const fn strides<const VERT: bool>(stride: usize) -> (usize, usize) {
    if VERT {
        (1, stride)
    } else {
        (stride, 1)
    }
}

pub fn loop_filter<const SIZE: usize, const VERT: bool, const INNER: bool>(
    buf: &mut [u8],
    stride: usize,
    flim_e: i32,
    flim_i: i32,
    hev_thresh: i32,
) {
    let (sa, sb) = strides::<VERT>(stride);

    for j in 0..SIZE {
        let p3i = j * sa;
        let w = load8::<VERT>(buf, p3i, sb);

        if !normal_limit(&w, flim_e, flim_i) {
            continue;
        }
        let is4tap = hev(&w, hev_thresh);

        if is4tap || INNER {
            let taps = [w[2], w[3], w[4], w[5]];

            filter_common(buf, taps, p3i + 2 * sb, sb, is4tap);
        } else {
            filter_mbedge(buf, &w, p3i + sb, sb);
        }
    }
}

pub fn loop_filter_simple<const VERT: bool>(buf: &mut [u8], stride: usize, flim: i32) {
    let (sa, sb) = strides::<VERT>(stride);

    for j in 0..16 {
        let p1i = j * sa;
        let w = load4::<VERT>(buf, p1i, sb);

        if simple_limit(w, flim) {
            filter_common(buf, w, p1i, sb, true);
        }
    }
}

pub fn luma_dc_wht(block: &mut [[i16; 16]; 16], dc: &mut [i16; 16]) {
    for i in 0..4 {
        let d = |k: usize| i32::from(dc[k * 4 + i]);
        let (t0, t1) = (d(0) + d(3), d(1) + d(2));
        let (t2, t3) = (d(1) - d(2), d(0) - d(3));

        dc[i] = (t0 + t1) as i16;
        dc[4 + i] = (t3 + t2) as i16;
        dc[8 + i] = (t0 - t1) as i16;
        dc[12 + i] = (t3 - t2) as i16;
    }

    for i in 0..4 {
        let d = |k: usize| i32::from(dc[i * 4 + k]);
        let (t0, t1) = (d(0) + d(3) + 3, d(1) + d(2));
        let (t2, t3) = (d(1) - d(2), d(0) - d(3) + 3);

        dc[i * 4..i * 4 + 4].fill(0);

        block[i * 4][0] = ((t0 + t1) >> 3) as i16;
        block[i * 4 + 1][0] = ((t3 + t2) >> 3) as i16;
        block[i * 4 + 2][0] = ((t0 - t1) >> 3) as i16;
        block[i * 4 + 3][0] = ((t3 - t2) >> 3) as i16;
    }
}

pub fn luma_dc_wht_dc(block: &mut [[i16; 16]; 16], dc: &mut [i16; 16]) {
    let val = ((i32::from(dc[0]) + 3) >> 3) as i16;

    dc[0] = 0;
    for b in block.iter_mut() {
        b[0] = val;
    }
}

fn mul_20091(a: i32) -> i32 {
    ((a * 20091) >> 16) + a
}

fn mul_35468(a: i32) -> i32 {
    (a * 35468) >> 16
}

pub fn idct_add(dst: &mut [u8], stride: usize, block: &mut [i16; 16]) {
    let mut tmp = [0i16; 16];

    for i in 0..4 {
        let b = |k: usize| i32::from(block[k * 4 + i]);
        let t0 = b(0) + b(2);
        let t1 = b(0) - b(2);
        let t2 = mul_35468(b(1)) - mul_20091(b(3));
        let t3 = mul_20091(b(1)) + mul_35468(b(3));

        for k in 0..4 {
            block[k * 4 + i] = 0;
        }

        tmp[i * 4] = (t0 + t3) as i16;
        tmp[i * 4 + 1] = (t1 + t2) as i16;
        tmp[i * 4 + 2] = (t1 - t2) as i16;
        tmp[i * 4 + 3] = (t0 - t3) as i16;
    }

    for i in 0..4 {
        let t = |k: usize| i32::from(tmp[k * 4 + i]);
        let t0 = t(0) + t(2);
        let t1 = t(0) - t(2);
        let t2 = mul_35468(t(1)) - mul_20091(t(3));
        let t3 = mul_20091(t(1)) + mul_35468(t(3));

        let row = &mut dst[i * stride..i * stride + 4];
        row[0] = clip_uint8(i32::from(row[0]) + ((t0 + t3 + 4) >> 3));
        row[1] = clip_uint8(i32::from(row[1]) + ((t1 + t2 + 4) >> 3));
        row[2] = clip_uint8(i32::from(row[2]) + ((t1 - t2 + 4) >> 3));
        row[3] = clip_uint8(i32::from(row[3]) + ((t0 - t3 + 4) >> 3));
    }
}

pub fn idct_dc_add(dst: &mut [u8], stride: usize, block: &mut [i16; 16]) {
    let dc = (i32::from(block[0]) + 4) >> 3;

    block[0] = 0;
    for i in 0..4 {
        let row = &mut dst[i * stride..i * stride + 4];
        for p in row.iter_mut() {
            *p = clip_uint8(i32::from(*p) + dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_edge_is_left_alone() {
        let mut buf = [128u8; 8 * 4];
        let want = buf;

        loop_filter::<4, true, false>(&mut buf, 4, 20, 10, 7);
        assert_eq!(buf, want);
    }

    #[test]
    fn a_step_edge_is_smoothed() {
        let mut buf = [0u8; 8];

        buf[4..].fill(4);
        loop_filter::<1, true, false>(&mut buf, 1, 60, 10, 7);
        assert_eq!(buf, [0, 1, 1, 2, 2, 3, 3, 4]);
    }

    #[test]
    fn the_dc_only_transform_shifts_every_sample() {
        let mut block = [0i16; 16];
        let mut dst = [10u8; 16];

        block[0] = 16;
        idct_dc_add(&mut dst, 4, &mut block);
        assert_eq!(dst, [12u8; 16]);
        assert_eq!(block[0], 0);
    }

    #[test]
    fn the_transform_clears_its_coefficients() {
        let mut block = [7i16; 16];
        let mut dst = [128u8; 16];

        idct_add(&mut dst, 4, &mut block);
        assert_eq!(block, [0i16; 16]);
    }
}

pub type WhtFn = fn(&mut [[i16; 16]; 16], &mut [i16; 16]);
pub type IdctFn = fn(&mut [u8], usize, usize, &mut [i16; 16]);
pub type Idct4Fn = fn(&mut [u8], usize, usize, &mut [[i16; 16]; 4]);
pub type LfFn = fn(&mut [u8], usize, usize, i32, i32, i32);
pub type LfUvFn = fn(&mut [u8], usize, &mut [u8], usize, usize, i32, i32, i32);
pub type LfMbFn = fn(&mut [u8], usize, usize, i32, i32, i32, i32);
pub type LfUvMbFn = fn(&mut [u8], usize, &mut [u8], usize, usize, i32, i32, i32, i32);
pub type LfSimpleFn = fn(&mut [u8], usize, usize, i32);
pub type LfSimpleMbFn = fn(&mut [u8], usize, usize, i32, i32);

pub struct Vp8Dsp {
    pub luma_dc_wht: WhtFn,
    pub luma_dc_wht_dc: WhtFn,
    pub idct_add: IdctFn,
    pub idct_dc_add: IdctFn,
    pub idct_dc_add4y: Idct4Fn,
    pub idct_dc_add4uv: Idct4Fn,

    pub v_loop_filter16y: LfFn,
    pub h_loop_filter16y: LfFn,
    pub v_loop_filter8uv: LfUvFn,
    pub h_loop_filter8uv: LfUvFn,

    pub v_loop_filter16y_inner: LfFn,
    pub h_loop_filter16y_inner: LfFn,
    pub v_loop_filter8uv_inner: LfUvFn,
    pub h_loop_filter8uv_inner: LfUvFn,

    pub h_loop_filter16y_mb: LfMbFn,
    pub h_loop_filter8uv_mb: LfUvMbFn,
    pub v_loop_filter16y_mb: LfMbFn,
    pub v_loop_filter8uv_mb: LfUvMbFn,

    pub v_loop_filter_simple: LfSimpleFn,
    pub h_loop_filter_simple: LfSimpleFn,
    pub h_loop_filter_simple_mb: LfSimpleMbFn,
    pub v_loop_filter_simple_mb: LfSimpleMbFn,
}

fn wht_c(block: &mut [[i16; 16]; 16], dc: &mut [i16; 16]) {
    luma_dc_wht(block, dc);
}

fn wht_dc_c(block: &mut [[i16; 16]; 16], dc: &mut [i16; 16]) {
    luma_dc_wht_dc(block, dc);
}

fn idct_add_c(p: &mut [u8], o: usize, s: usize, block: &mut [i16; 16]) {
    idct_add(&mut p[o..], s, block);
}

fn idct_dc_add_c(p: &mut [u8], o: usize, s: usize, block: &mut [i16; 16]) {
    idct_dc_add(&mut p[o..], s, block);
}

fn idct_dc_add4y_c(p: &mut [u8], o: usize, s: usize, block: &mut [[i16; 16]; 4]) {
    for (i, b) in block.iter_mut().enumerate() {
        idct_dc_add(&mut p[o + 4 * i..], s, b);
    }
}

fn idct_dc_add4uv_c(p: &mut [u8], o: usize, s: usize, block: &mut [[i16; 16]; 4]) {
    for (i, b) in block.iter_mut().enumerate() {
        idct_dc_add(&mut p[o + 4 * s * (i / 2) + 4 * (i % 2)..], s, b);
    }
}

macro_rules! lf_c {
    ($name:ident, $size:literal, $vert:literal, $inner:literal, $back:expr) => {
        fn $name(p: &mut [u8], o: usize, s: usize, e: i32, i: i32, hev: i32) {
            let back = $back(s);

            loop_filter::<$size, $vert, $inner>(&mut p[o - back..], s, e, i, hev);
        }
    };
}

macro_rules! lf_uv_c {
    ($name:ident, $single:ident) => {
        #[allow(clippy::too_many_arguments)]
        fn $name(
            u: &mut [u8],
            ou: usize,
            v: &mut [u8],
            ov: usize,
            s: usize,
            e: i32,
            i: i32,
            hev: i32,
        ) {
            $single(u, ou, s, e, i, hev);
            $single(v, ov, s, e, i, hev);
        }
    };
}

lf_c!(v16_c, 16, true, false, |s| 4 * s);
lf_c!(h16_c, 16, false, false, |_| 4);
lf_c!(v16_inner_c, 16, true, true, |s| 4 * s);
lf_c!(h16_inner_c, 16, false, true, |_| 4);
lf_c!(v8_c, 8, true, false, |s| 4 * s);
lf_c!(h8_c, 8, false, false, |_| 4);
lf_c!(v8_inner_c, 8, true, true, |s| 4 * s);
lf_c!(h8_inner_c, 8, false, true, |_| 4);

lf_uv_c!(v8uv_c, v8_c);
lf_uv_c!(h8uv_c, h8_c);
lf_uv_c!(v8uv_inner_c, v8_inner_c);
lf_uv_c!(h8uv_inner_c, h8_inner_c);

fn v_simple_c(p: &mut [u8], o: usize, s: usize, flim: i32) {
    loop_filter_simple::<true>(&mut p[o - 2 * s..], s, flim);
}

fn h_simple_c(p: &mut [u8], o: usize, s: usize, flim: i32) {
    loop_filter_simple::<false>(&mut p[o - 2..], s, flim);
}

macro_rules! mb_c {
    ($name:ident, $edge:ident, $inner:ident, $step:expr) => {
        fn $name(p: &mut [u8], o: usize, s: usize, e: i32, be: i32, i: i32, hev: i32) {
            let step = $step(s);

            $edge(p, o, s, e, i, hev);
            $inner(p, o + step, s, be, i, hev);
            $inner(p, o + 2 * step, s, be, i, hev);
            $inner(p, o + 3 * step, s, be, i, hev);
        }
    };
}

macro_rules! uv_mb_c {
    ($name:ident, $edge:ident, $inner:ident, $step:expr) => {
        #[allow(clippy::too_many_arguments)]
        fn $name(
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
            let step = $step(s);

            $edge(u, ou, v, ov, s, e, i, hev);
            $inner(u, ou + step, v, ov + step, s, be, i, hev);
        }
    };
}

mb_c!(h16_mb_c, h16_c, h16_inner_c, |_| 4);
mb_c!(v16_mb_c, v16_c, v16_inner_c, |s: usize| 4 * s);
uv_mb_c!(h8uv_mb_c, h8uv_c, h8uv_inner_c, |_| 4);
uv_mb_c!(v8uv_mb_c, v8uv_c, v8uv_inner_c, |s: usize| 4 * s);

fn h_simple_mb_c(p: &mut [u8], o: usize, s: usize, e: i32, be: i32) {
    h_simple_c(p, o, s, e);
    h_simple_c(p, o + 4, s, be);
    h_simple_c(p, o + 8, s, be);
    h_simple_c(p, o + 12, s, be);
}

fn v_simple_mb_c(p: &mut [u8], o: usize, s: usize, e: i32, be: i32) {
    v_simple_c(p, o, s, e);
    v_simple_c(p, o + 4 * s, s, be);
    v_simple_c(p, o + 8 * s, s, be);
    v_simple_c(p, o + 12 * s, s, be);
}

impl Vp8Dsp {
    pub const fn scalar() -> Self {
        Self {
            luma_dc_wht: wht_c,
            luma_dc_wht_dc: wht_dc_c,
            idct_add: idct_add_c,
            idct_dc_add: idct_dc_add_c,
            idct_dc_add4y: idct_dc_add4y_c,
            idct_dc_add4uv: idct_dc_add4uv_c,

            v_loop_filter16y: v16_c,
            h_loop_filter16y: h16_c,
            v_loop_filter8uv: v8uv_c,
            h_loop_filter8uv: h8uv_c,

            v_loop_filter16y_inner: v16_inner_c,
            h_loop_filter16y_inner: h16_inner_c,
            v_loop_filter8uv_inner: v8uv_inner_c,
            h_loop_filter8uv_inner: h8uv_inner_c,

            h_loop_filter16y_mb: h16_mb_c,
            h_loop_filter8uv_mb: h8uv_mb_c,
            v_loop_filter16y_mb: v16_mb_c,
            v_loop_filter8uv_mb: v8uv_mb_c,

            v_loop_filter_simple: v_simple_c,
            h_loop_filter_simple: h_simple_c,
            h_loop_filter_simple_mb: h_simple_mb_c,
            v_loop_filter_simple_mb: v_simple_mb_c,
        }
    }

    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = Self::scalar();

        #[cfg(feature = "asm")]
        crate::asm::vp8::init(&mut table, crate::cpu::flags());

        table
    }
}

impl Default for Vp8Dsp {
    fn default() -> Self {
        Self::new()
    }
}
