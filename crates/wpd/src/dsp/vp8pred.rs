//! Scalar kernels for VP8 intra prediction.
//!
//! # Shapes
//!
//! A predictor writes an `N`x`N` block and reads some of the row above it, the
//! column to its left, and the corner between them. All of that is one
//! contiguous region of the plane, so each kernel takes that region as a
//! single slice with `o`, the index of the block's top-left sample within it.
//! `o` is therefore also the distance back to the first sample the predictor
//! reads, which differs per mode: `stride + 1` when the corner is read,
//! `stride` when only the row above is, `1` when only the column, and `0` for
//! the modes that read nothing at all.

fn clip_uint8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// The sample `x` along the row above the block.
#[inline(always)]
fn top(buf: &[u8], o: usize, stride: usize, x: usize) -> i32 {
    i32::from(buf[o - stride + x])
}

/// The sample `y` down the column left of the block.
#[inline(always)]
fn left(buf: &[u8], o: usize, stride: usize, y: usize) -> i32 {
    i32::from(buf[o + y * stride - 1])
}

/// The corner sample above and left of the block.
#[inline(always)]
fn corner(buf: &[u8], o: usize, stride: usize) -> i32 {
    i32::from(buf[o - stride - 1])
}

#[inline(always)]
fn row<const N: usize>(buf: &mut [u8], o: usize, stride: usize, y: usize) -> &mut [u8] {
    &mut buf[o + y * stride..][..N]
}

fn fill<const N: usize>(buf: &mut [u8], o: usize, stride: usize, value: u8) {
    for y in 0..N {
        row::<N>(buf, o, stride, y).fill(value);
    }
}

fn avg2(a: i32, b: i32) -> u8 {
    ((a + b + 1) >> 1) as u8
}

fn avg3(a: i32, b: i32, c: i32) -> u8 {
    ((a + 2 * b + c + 2) >> 2) as u8
}

/// `VERT_PRED`, which VP8 smooths rather than copying the row above.
pub fn pred4x4_vertical(buf: &mut [u8], o: usize, stride: usize, tr: &[u8; 4]) {
    let lt = corner(buf, o, stride);
    let t: [i32; 4] = core::array::from_fn(|x| top(buf, o, stride, x));
    let t4 = i32::from(tr[0]);
    let p = [
        avg3(lt, t[0], t[1]),
        avg3(t[0], t[1], t[2]),
        avg3(t[1], t[2], t[3]),
        avg3(t[2], t[3], t4),
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).copy_from_slice(&p);
    }
}

/// `HOR_PRED`, likewise smoothed.
pub fn pred4x4_horizontal(buf: &mut [u8], o: usize, stride: usize) {
    let lt = corner(buf, o, stride);
    let l: [i32; 4] = core::array::from_fn(|y| left(buf, o, stride, y));
    let p = [
        avg3(lt, l[0], l[1]),
        avg3(l[0], l[1], l[2]),
        avg3(l[1], l[2], l[3]),
        ((l[2] + 3 * l[3] + 2) >> 2) as u8,
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).fill(p[y]);
    }
}

/// `DC_PRED`.
pub fn pred4x4_dc(buf: &mut [u8], o: usize, stride: usize) {
    let mut dc = 4;

    for i in 0..4 {
        dc += top(buf, o, stride, i) + left(buf, o, stride, i);
    }
    fill::<4>(buf, o, stride, (dc >> 3) as u8);
}

/// `DIAG_DOWN_RIGHT_PRED`.
pub fn pred4x4_down_right(buf: &mut [u8], o: usize, stride: usize) {
    let lt = corner(buf, o, stride);
    let t: [i32; 4] = core::array::from_fn(|x| top(buf, o, stride, x));
    let l: [i32; 4] = core::array::from_fn(|y| left(buf, o, stride, y));
    let p = [
        avg3(l[3], l[2], l[1]),
        avg3(l[2], l[1], l[0]),
        avg3(l[1], l[0], lt),
        avg3(l[0], lt, t[0]),
        avg3(lt, t[0], t[1]),
        avg3(t[0], t[1], t[2]),
        avg3(t[1], t[2], t[3]),
    ];

    for y in 0..4 {
        let r = row::<4>(buf, o, stride, y);
        for x in 0..4 {
            r[x] = p[3 + x - y];
        }
    }
}

/// `DIAG_DOWN_LEFT_PRED`.
pub fn pred4x4_down_left(buf: &mut [u8], o: usize, stride: usize, tr: &[u8; 4]) {
    let t: [i32; 8] = core::array::from_fn(|i| {
        if i < 4 {
            top(buf, o, stride, i)
        } else {
            i32::from(tr[i - 4])
        }
    });
    let mut p = [0u8; 7];

    for i in 0..6 {
        p[i] = avg3(t[i], t[i + 1], t[i + 2]);
    }
    p[6] = ((t[6] + 3 * t[7] + 2) >> 2) as u8;

    for y in 0..4 {
        let r = row::<4>(buf, o, stride, y);
        for x in 0..4 {
            r[x] = p[x + y];
        }
    }
}

/// `VERT_RIGHT_PRED`.
pub fn pred4x4_vertical_right(buf: &mut [u8], o: usize, stride: usize) {
    let lt = corner(buf, o, stride);
    let t: [i32; 4] = core::array::from_fn(|x| top(buf, o, stride, x));
    let l: [i32; 3] = core::array::from_fn(|y| left(buf, o, stride, y));
    let p = [
        [
            avg2(lt, t[0]),
            avg2(t[0], t[1]),
            avg2(t[1], t[2]),
            avg2(t[2], t[3]),
        ],
        [
            avg3(l[0], lt, t[0]),
            avg3(lt, t[0], t[1]),
            avg3(t[0], t[1], t[2]),
            avg3(t[1], t[2], t[3]),
        ],
        [
            avg3(lt, l[0], l[1]),
            avg2(lt, t[0]),
            avg2(t[0], t[1]),
            avg2(t[1], t[2]),
        ],
        [
            avg3(l[0], l[1], l[2]),
            avg3(l[0], lt, t[0]),
            avg3(lt, t[0], t[1]),
            avg3(t[0], t[1], t[2]),
        ],
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).copy_from_slice(&p[y]);
    }
}

/// `VERT_LEFT_PRED`.
pub fn pred4x4_vertical_left(buf: &mut [u8], o: usize, stride: usize, tr: &[u8; 4]) {
    let t: [i32; 4] = core::array::from_fn(|x| top(buf, o, stride, x));
    let t4 = i32::from(tr[0]);
    let t5 = i32::from(tr[1]);
    let t6 = i32::from(tr[2]);
    let t7 = i32::from(tr[3]);
    let p = [
        [
            avg2(t[0], t[1]),
            avg2(t[1], t[2]),
            avg2(t[2], t[3]),
            avg2(t[3], t4),
        ],
        [
            avg3(t[0], t[1], t[2]),
            avg3(t[1], t[2], t[3]),
            avg3(t[2], t[3], t4),
            avg3(t[3], t4, t5),
        ],
        [
            avg2(t[1], t[2]),
            avg2(t[2], t[3]),
            avg2(t[3], t4),
            avg3(t4, t5, t6),
        ],
        [
            avg3(t[1], t[2], t[3]),
            avg3(t[2], t[3], t4),
            avg3(t[3], t4, t5),
            avg3(t5, t6, t7),
        ],
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).copy_from_slice(&p[y]);
    }
}

/// `HOR_UP_PRED`.
pub fn pred4x4_horizontal_up(buf: &mut [u8], o: usize, stride: usize) {
    let l: [i32; 4] = core::array::from_fn(|y| left(buf, o, stride, y));
    let l3 = l[3] as u8;
    let p = [
        [
            avg2(l[0], l[1]),
            avg3(l[0], l[1], l[2]),
            avg2(l[1], l[2]),
            avg3(l[1], l[2], l[3]),
        ],
        [
            avg2(l[1], l[2]),
            avg3(l[1], l[2], l[3]),
            avg2(l[2], l[3]),
            avg3(l[2], l[3], l[3]),
        ],
        [avg2(l[2], l[3]), avg3(l[2], l[3], l[3]), l3, l3],
        [l3, l3, l3, l3],
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).copy_from_slice(&p[y]);
    }
}

/// `HOR_DOWN_PRED`.
pub fn pred4x4_horizontal_down(buf: &mut [u8], o: usize, stride: usize) {
    let lt = corner(buf, o, stride);
    let t: [i32; 3] = core::array::from_fn(|x| top(buf, o, stride, x));
    let l: [i32; 4] = core::array::from_fn(|y| left(buf, o, stride, y));
    let p = [
        [
            avg2(lt, l[0]),
            avg3(l[0], lt, t[0]),
            avg3(lt, t[0], t[1]),
            avg3(t[0], t[1], t[2]),
        ],
        [
            avg2(l[0], l[1]),
            avg3(lt, l[0], l[1]),
            avg2(lt, l[0]),
            avg3(l[0], lt, t[0]),
        ],
        [
            avg2(l[1], l[2]),
            avg3(l[0], l[1], l[2]),
            avg2(l[0], l[1]),
            avg3(lt, l[0], l[1]),
        ],
        [
            avg2(l[2], l[3]),
            avg3(l[1], l[2], l[3]),
            avg2(l[1], l[2]),
            avg3(l[0], l[1], l[2]),
        ],
    ];

    for y in 0..4 {
        row::<4>(buf, o, stride, y).copy_from_slice(&p[y]);
    }
}

/// `TM_VP8_PRED`, the TrueMotion predictor, at any block size.
pub fn pred_tm<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    let lt = corner(buf, o, stride);
    let mut t = [0u8; N];

    t.copy_from_slice(&buf[o - stride..][..N]);
    for y in 0..N {
        let l = left(buf, o, stride, y);
        let r = row::<N>(buf, o, stride, y);

        for x in 0..N {
            r[x] = clip_uint8(l + i32::from(t[x]) - lt);
        }
    }
}

/// `VERT_PRED8x8`, a plain copy of the row above.
pub fn pred_vertical<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    let mut t = [0u8; N];

    t.copy_from_slice(&buf[o - stride..][..N]);
    for y in 0..N {
        row::<N>(buf, o, stride, y).copy_from_slice(&t);
    }
}

/// `HOR_PRED8x8`, each row filled from its own left neighbour.
pub fn pred_horizontal<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    for y in 0..N {
        let v = buf[o + y * stride - 1];

        row::<N>(buf, o, stride, y).fill(v);
    }
}

/// `DC_PRED8x8`, over both edges.
pub fn pred_dc<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    let mut dc = N as i32;

    for i in 0..N {
        dc += top(buf, o, stride, i) + left(buf, o, stride, i);
    }
    let shift = if N == 8 { 4 } else { 5 };

    fill::<N>(buf, o, stride, (dc >> shift) as u8);
}

/// `LEFT_DC_PRED8x8`, for blocks with no row above.
pub fn pred_left_dc<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    let mut dc = N as i32 / 2;

    for i in 0..N {
        dc += left(buf, o, stride, i);
    }
    fill::<N>(buf, o, stride, (dc / N as i32) as u8);
}

/// `TOP_DC_PRED8x8`, for blocks with no column to the left.
pub fn pred_top_dc<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    let mut dc = N as i32 / 2;

    for i in 0..N {
        dc += top(buf, o, stride, i);
    }
    fill::<N>(buf, o, stride, (dc / N as i32) as u8);
}

/// `DC_128_PRED8x8`, for blocks with neither neighbour.
pub fn pred_dc128<const N: usize>(buf: &mut [u8], o: usize, stride: usize) {
    fill::<N>(buf, o, stride, 128);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 block at `(1, 1)` of a 16-wide plane, with the row above and the
    /// column to its left already filled.
    fn plane(top_value: u8, left_value: u8) -> (Vec<u8>, usize, usize) {
        let (stride, o) = (16, 16 + 1);
        let mut buf = vec![0u8; 18 * stride];

        buf[o - stride - 1] = 200;
        for x in 0..8 {
            buf[o - stride + x] = top_value;
        }
        for y in 0..4 {
            buf[o + y * stride - 1] = left_value;
        }
        (buf, o, stride)
    }

    #[test]
    fn a_uniform_neighbourhood_predicts_itself() {
        let (mut buf, o, stride) = plane(90, 90);

        pred4x4_dc(&mut buf, o, stride);
        for y in 0..4 {
            assert_eq!(&buf[o + y * stride..][..4], &[90u8; 4]);
        }
    }

    #[test]
    fn vertical_smooths_across_the_corner() {
        let (mut buf, o, stride) = plane(100, 0);

        pred4x4_vertical(&mut buf, o, stride, &[100; 4]);
        assert_eq!(&buf[o..][..4], &[125, 100, 100, 100]);
    }

    #[test]
    fn truemotion_adds_the_left_and_top_deltas() {
        let (mut buf, o, stride) = plane(100, 150);

        pred_tm::<4>(&mut buf, o, stride);
        for y in 0..4 {
            assert_eq!(&buf[o + y * stride..][..4], &[50u8; 4]);
        }
    }

    #[test]
    fn the_flat_predictor_ignores_its_neighbours() {
        let (mut buf, o, stride) = plane(0, 0);

        pred_dc128::<8>(&mut buf, o, stride);
        for y in 0..8 {
            assert_eq!(&buf[o + y * stride..][..8], &[128u8; 8]);
        }
    }
}
