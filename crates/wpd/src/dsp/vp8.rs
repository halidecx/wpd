//! Scalar kernels for the lossy (VP8) DSP: the inverse transforms and the
//! loop filter.
//!
//! # Shapes
//!
//! Every kernel here takes the exact region it touches as one slice, so the
//! caller does the address arithmetic and the kernel does none of the pointer
//! juggling the C equivalents do. The filters are given the slice starting at
//! their lowest sample rather than at the edge, which turns the C's negative
//! `p[-4 * stride]` indices into ordinary ones.
//!
//! `stridea` steps from one filtered position to the next along the edge;
//! `strideb` steps from one sample to the next across it. A vertical edge
//! filter has `stridea = 1` and `strideb = stride`, and a horizontal one has
//! them the other way round.

#[inline(always)]
fn clip_uint8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[inline(always)]
fn clip_int8(v: i32) -> i32 {
    i32::from(clip_uint8(v + 0x80)) - 0x80
}

/// The eight samples across an edge: `p3 p2 p1 p0 q0 q1 q2 q3`.
///
/// Loading them once and deciding from the copy, rather than re-reading the
/// plane for every predicate, is what keeps the bounds checks off the hot
/// path: what remains is one check per load and one per store, the same
/// accesses the C makes unchecked.
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

/// The 4-tap and 6-tap common filter, over `p1 p0 q0 q1` at `p1i + k * s`.
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

/// The macroblock-edge filter, which widens to `p2 .. q2` at `p2i + k * s`.
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

/// The direction, size and edge kind are const parameters so that the two
/// strides fold to `1` and `stride` and the loop bound is known, which is what
/// the C gets from expanding its `LOOP_FILTER` macro per variant.
const fn strides<const VERT: bool>(stride: usize) -> (usize, usize) {
    if VERT {
        (1, stride)
    } else {
        (stride, 1)
    }
}

/// The normal loop filter over `SIZE` positions along an edge.
///
/// `buf` starts at the fourth sample before the edge — `dst - 4 * strideb` in
/// the C — and must reach the fourth after it at the last position. `INNER`
/// selects the subblock filter, which never widens to the macroblock taps.
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

/// The simple loop filter, which reads and writes only the two samples either
/// side of the edge.
///
/// `buf` starts at `dst - 2 * strideb`.
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

/// Inverts the Walsh-Hadamard transform of the luma DC coefficients,
/// scattering them into the DC slot of each of the sixteen luma blocks.
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

/// [`luma_dc_wht`] where only the DC of the DC block is non-zero.
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

/// Inverts the 4x4 DCT and adds the residual to `dst`, which starts at the
/// block's top-left sample.
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

/// [`idct_add`] where only the DC coefficient is non-zero.
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
