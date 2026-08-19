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

use super::clip_uint8;

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

    for (y, &v) in p.iter().enumerate() {
        row::<4>(buf, o, stride, y).fill(v);
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
        row::<4>(buf, o, stride, y).copy_from_slice(&p[y..y + 4]);
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

    for (y, pred) in p.iter().enumerate() {
        row::<4>(buf, o, stride, y).copy_from_slice(pred);
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

    for (y, pred) in p.iter().enumerate() {
        row::<4>(buf, o, stride, y).copy_from_slice(pred);
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

    for (y, pred) in p.iter().enumerate() {
        row::<4>(buf, o, stride, y).copy_from_slice(pred);
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

    for (y, pred) in p.iter().enumerate() {
        row::<4>(buf, o, stride, y).copy_from_slice(pred);
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

/// The intra prediction table the decoder calls through.
///
/// As with [`crate::dsp::vp8::Vp8Dsp`], an entry takes the whole plane and the
/// offset of the block's top-left sample; every predictor reads the row above
/// and the column to the left of that, which are ordinary indices here.
///
/// The four bytes above and to the right of a block are passed separately
/// because the last macroblock in a row has no such neighbour and the decoder
/// substitutes a replicated sample, exactly as the C does.
pub type Pred4x4Fn = fn(&mut [u8], usize, usize, &[u8; 4]);
pub type PredFn = fn(&mut [u8], usize, usize);

pub const PRED4X4_COUNT: usize = 10;
pub const PRED8X8_COUNT: usize = 7;

/// `VP8Pred4x4Mode`.
pub const VERT_PRED: usize = 0;
pub const HOR_PRED: usize = 1;
pub const DC_PRED: usize = 2;
pub const DIAG_DOWN_LEFT_PRED: usize = 3;
pub const DIAG_DOWN_RIGHT_PRED: usize = 4;
pub const VERT_RIGHT_PRED: usize = 5;
pub const HOR_DOWN_PRED: usize = 6;
pub const VERT_LEFT_PRED: usize = 7;
pub const HOR_UP_PRED: usize = 8;
pub const TM_VP8_PRED: usize = 9;

/// `VP8Pred8x8Mode`.
pub const DC_PRED8X8: usize = 0;
pub const HOR_PRED8X8: usize = 1;
pub const VERT_PRED8X8: usize = 2;
pub const PLANE_PRED8X8: usize = 3;
pub const LEFT_DC_PRED8X8: usize = 4;
pub const TOP_DC_PRED8X8: usize = 5;
pub const DC_128_PRED8X8: usize = 6;

pub struct Vp8Pred {
    pub pred4x4: [Pred4x4Fn; PRED4X4_COUNT],
    pub pred8x8: [PredFn; PRED8X8_COUNT],
    pub pred16x16: [PredFn; PRED8X8_COUNT],
}

/// Gives a predictor that ignores the above-right samples the same shape as
/// one that reads them, so the table has a single entry type.
macro_rules! no_tr {
    ($name:ident, $k:expr) => {
        fn $name(p: &mut [u8], o: usize, s: usize, _tr: &[u8; 4]) {
            $k(p, o, s);
        }
    };
}

no_tr!(horizontal4_c, pred4x4_horizontal);
no_tr!(dc4_c, pred4x4_dc);
no_tr!(down_right4_c, pred4x4_down_right);
no_tr!(vertical_right4_c, pred4x4_vertical_right);
no_tr!(horizontal_down4_c, pred4x4_horizontal_down);
no_tr!(horizontal_up4_c, pred4x4_horizontal_up);
no_tr!(tm4_c, pred_tm::<4>);

impl Vp8Pred {
    /// The scalar table, before any assembly is substituted in.
    pub const fn scalar() -> Self {
        Self {
            pred4x4: [
                pred4x4_vertical,
                horizontal4_c,
                dc4_c,
                pred4x4_down_left,
                down_right4_c,
                vertical_right4_c,
                horizontal_down4_c,
                pred4x4_vertical_left,
                horizontal_up4_c,
                tm4_c,
            ],
            pred8x8: [
                pred_dc::<8>,
                pred_horizontal::<8>,
                pred_vertical::<8>,
                pred_tm::<8>,
                pred_left_dc::<8>,
                pred_top_dc::<8>,
                pred_dc128::<8>,
            ],
            pred16x16: [
                pred_dc::<16>,
                pred_horizontal::<16>,
                pred_vertical::<16>,
                pred_tm::<16>,
                pred_left_dc::<16>,
                pred_top_dc::<16>,
                pred_dc128::<16>,
            ],
        }
    }

    /// The best table the running CPU allows.
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = Self::scalar();

        #[cfg(feature = "asm")]
        crate::asm::vp8pred::init(&mut table, crate::cpu::flags());

        table
    }
}

impl Default for Vp8Pred {
    fn default() -> Self {
        Self::new()
    }
}
