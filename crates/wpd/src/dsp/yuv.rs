//! Scalar kernels for the YUV DSP.
//!
//! These are the fallbacks the assembly replaces at runtime; the dispatch
//! table that chooses between them lives in `wpd-capi` for now. See `LOG.md`
//! on the two DSP tiers.
//!
//! # Layouts
//!
//! The packed output layout is a const generic rather than a runtime argument,
//! which is what the C gets from expanding its `YUV_TO_OUT` and
//! `UPSAMPLE_PAIRS` macros once per layout. [`bpp`] is 3 for the two layouts
//! that drop alpha.
//!
//! # Shapes
//!
//! [`upsample_pairs`] keeps the C's pair index `x`, reading chroma at `x - 1`
//! and `x`, but takes the first luma sample and output pixel it touches as
//! `pix` instead of having the caller bias the pointers. The C biases them by
//! a pixel for the block entry point, which would mean forming a slice that
//! starts before the row — sound in neither language, and checkasm does hand
//! it the very first byte of a buffer.

pub const LAYOUT_ARGB: usize = 0;
pub const LAYOUT_RGBA: usize = 1;
pub const LAYOUT_BGRA: usize = 2;
pub const LAYOUT_RGB: usize = 3;
pub const LAYOUT_BGR: usize = 4;
pub const LAYOUT_NB: usize = 5;

pub const UPSAMPLE_BLOCK: usize = 32;

/// Bytes per pixel of a packed layout.
pub const fn bpp(layout: usize) -> usize {
    if layout == LAYOUT_RGB || layout == LAYOUT_BGR {
        3
    } else {
        4
    }
}

/// Byte offsets of alpha, red, green and blue within one pixel. The three-byte
/// layouts have no alpha byte; their entry aliases red, whose store comes
/// later and overwrites it, exactly as the C's `YUV_TO_OUT3` leaves it out.
const fn channels(layout: usize) -> [usize; 4] {
    match layout {
        LAYOUT_RGBA => [3, 0, 1, 2],
        LAYOUT_BGRA => [3, 2, 1, 0],
        LAYOUT_RGB => [0, 0, 1, 2],
        LAYOUT_BGR => [0, 2, 1, 0],
        _ => [0, 1, 2, 3],
    }
}

const YUV_FIX2: u32 = 6;
const YUV_MASK2: i32 = (256 << YUV_FIX2) - 1;

#[inline(always)]
const fn yuv_mult_hi(v: i32, coeff: i32) -> i32 {
    (v * coeff) >> 8
}

#[inline(always)]
const fn yuv_clip8(v: i32) -> i32 {
    if v & !YUV_MASK2 == 0 {
        v >> YUV_FIX2
    } else if v < 0 {
        0
    } else {
        255
    }
}

#[inline(always)]
const fn yuv_to_r(y: i32, v: i32) -> i32 {
    yuv_clip8(yuv_mult_hi(y, 19077) + yuv_mult_hi(v, 26149) - 14234)
}

#[inline(always)]
const fn yuv_to_g(y: i32, u: i32, v: i32) -> i32 {
    yuv_clip8(
        yuv_mult_hi(y, 19077) - yuv_mult_hi(u, 6419) - yuv_mult_hi(v, 13320) + 8708,
    )
}

#[inline(always)]
const fn yuv_to_b(y: i32, u: i32) -> i32 {
    yuv_clip8(yuv_mult_hi(y, 19077) + yuv_mult_hi(u, 33050) - 17685)
}

/// Converts one pixel into the first [`bpp`] bytes of `out`.
#[inline(always)]
pub fn yuv_to_out<const L: usize>(y: i32, u: i32, v: i32, out: &mut [u8]) {
    let c = channels(L);
    let out = &mut out[..bpp(L)];

    if bpp(L) == 4 {
        out[c[0]] = 0xff;
    }
    out[c[1]] = yuv_to_r(y, v) as u8;
    out[c[2]] = yuv_to_g(y, u, v) as u8;
    out[c[3]] = yuv_to_b(y, u) as u8;
}

/// Packs a chroma pair into one word so the two planes advance together.
#[inline(always)]
const fn load_uv(u: u8, v: u8) -> u32 {
    u as u32 | ((v as u32) << 16)
}

#[inline(always)]
fn corner<const L: usize>(y: u8, near: u32, far: u32, out: &mut [u8]) {
    let uv = (3 * near + far + 0x0002_0002) >> 2;

    yuv_to_out::<L>(y.into(), (uv & 0xff) as i32, (uv >> 16) as i32, out);
}

/// Interpolates chroma for the pixel pairs `first..=last`, writing output
/// pixels `pix` onwards.
///
/// `top_y` must reach `pix + 2 * (last - first) + 1`, the chroma rows must
/// reach `last`, and `top_dst` must hold [`bpp`] bytes for each pixel written.
#[allow(clippy::too_many_arguments)]
pub fn upsample_pairs<const L: usize>(
    top_y: &[u8],
    bottom_y: Option<&[u8]>,
    top_u: &[u8],
    top_v: &[u8],
    cur_u: &[u8],
    cur_v: &[u8],
    top_dst: &mut [u8],
    mut bottom_dst: Option<&mut [u8]>,
    first: usize,
    last: usize,
    pix: usize,
) {
    let bpp = bpp(L);
    let mut tl_uv = load_uv(top_u[first - 1], top_v[first - 1]);
    let mut l_uv = load_uv(cur_u[first - 1], cur_v[first - 1]);
    let mut p = pix;

    for x in first..=last {
        let t_uv = load_uv(top_u[x], top_v[x]);
        let uv = load_uv(cur_u[x], cur_v[x]);
        let avg = tl_uv + t_uv + l_uv + uv + 0x0008_0008;
        let diag_12 = (avg + 2 * (t_uv + l_uv)) >> 3;
        let diag_03 = (avg + 2 * (tl_uv + uv)) >> 3;
        let uv0 = (diag_12 + tl_uv) >> 1;
        let uv1 = (diag_03 + t_uv) >> 1;

        yuv_to_out::<L>(
            top_y[p].into(),
            (uv0 & 0xff) as i32,
            (uv0 >> 16) as i32,
            &mut top_dst[bpp * p..],
        );
        yuv_to_out::<L>(
            top_y[p + 1].into(),
            (uv1 & 0xff) as i32,
            (uv1 >> 16) as i32,
            &mut top_dst[bpp * (p + 1)..],
        );
        if let (Some(by), Some(bd)) = (bottom_y, bottom_dst.as_deref_mut()) {
            let b0 = (diag_03 + l_uv) >> 1;
            let b1 = (diag_12 + uv) >> 1;

            yuv_to_out::<L>(
                by[p].into(),
                (b0 & 0xff) as i32,
                (b0 >> 16) as i32,
                &mut bd[bpp * p..],
            );
            yuv_to_out::<L>(
                by[p + 1].into(),
                (b1 & 0xff) as i32,
                (b1 >> 16) as i32,
                &mut bd[bpp * (p + 1)..],
            );
        }
        tl_uv = t_uv;
        l_uv = uv;
        p += 2;
    }
}

/// The first and last pixels of a row, whose chroma has no neighbour on one
/// side and is weighted 3:1 against the row across instead.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn upsample_edge<const L: usize>(
    top_y: u8,
    bottom_y: Option<u8>,
    top_u: u8,
    top_v: u8,
    cur_u: u8,
    cur_v: u8,
    top_dst: &mut [u8],
    bottom_dst: Option<&mut [u8]>,
) {
    let t_uv = load_uv(top_u, top_v);
    let c_uv = load_uv(cur_u, cur_v);

    corner::<L>(top_y, t_uv, c_uv, top_dst);
    if let (Some(y), Some(dst)) = (bottom_y, bottom_dst) {
        corner::<L>(y, c_uv, t_uv, dst);
    }
}

pub fn dispatch_alpha_first(dst: &mut [u8], src: &[u8]) {
    for (d, &a) in dst.chunks_exact_mut(4).zip(src) {
        d[0] = a;
    }
}

pub fn dispatch_alpha_last(dst: &mut [u8], src: &[u8]) {
    for (d, &a) in dst.chunks_exact_mut(4).zip(src) {
        d[3] = a;
    }
}

pub fn pack_rgba(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[1];
        d[1] = s[2];
        d[2] = s[3];
        d[3] = s[0];
    }
}

pub fn pack_bgra(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[3];
        d[1] = s[2];
        d[2] = s[1];
        d[3] = s[0];
    }
}

pub fn pack_rgb(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
        d[0] = s[1];
        d[1] = s[2];
        d[2] = s[3];
    }
}

pub fn pack_bgr(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
        d[0] = s[3];
        d[1] = s[2];
        d[2] = s[1];
    }
}

pub fn pack_rgb565(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(2).zip(src.chunks_exact(4)) {
        d[0] = (s[1] & 0xf8) | (s[2] >> 5);
        d[1] = (s[2] << 3 & 0xe0) | (s[3] >> 3);
    }
}

pub fn pack_bgr565(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(2).zip(src.chunks_exact(4)) {
        d[0] = (s[2] << 3 & 0xe0) | (s[3] >> 3);
        d[1] = (s[1] & 0xf8) | (s[2] >> 5);
    }
}

pub fn pack_rgba4444(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(2).zip(src.chunks_exact(4)) {
        d[0] = (s[1] & 0xf0) | (s[2] >> 4);
        d[1] = (s[3] & 0xf0) | (s[0] >> 4);
    }
}

pub fn pack_bgra4444(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(2).zip(src.chunks_exact(4)) {
        d[0] = (s[3] & 0xf0) | (s[0] >> 4);
        d[1] = (s[1] & 0xf0) | (s[2] >> 4);
    }
}

/// `(x * a * 32897) >> 23` is bit-exact with `(int)(x * a / 255.)` for 8-bit
/// `x` and `a`.
#[inline(always)]
const fn premultiply(x: u8, m: u32) -> u8 {
    ((x as u32 * m) >> 23) as u8
}

pub fn premultiply_row(rgba: &mut [u8], alpha_first: bool) {
    let (ai, r) = if alpha_first { (0, 1) } else { (3, 0) };

    for px in rgba.chunks_exact_mut(4) {
        let a = px[ai] as u32;

        if a != 0xff {
            let m = a * 32897;

            px[r] = premultiply(px[r], m);
            px[r + 1] = premultiply(px[r + 1], m);
            px[r + 2] = premultiply(px[r + 2], m);
        }
    }
}

/// The 4-bit layout, multiplied in its own precision rather than in 8-bit and
/// then truncated. `swap` takes the two bytes of each pixel the other way
/// round.
pub fn premultiply_row_4444(rgba4444: &mut [u8], swap: bool) {
    let (i_rg, i_ba) = if swap { (1, 0) } else { (0, 1) };

    for px in rgba4444.chunks_exact_mut(2) {
        let rg = px[i_rg] as u32;
        let ba = px[i_ba] as u32;
        let a = ba & 0x0f;
        let mult = a * 0x1111;
        let r = (((rg & 0xf0) | (rg >> 4)) * mult) >> 16;
        let g = (((rg & 0x0f) | (rg << 4 & 0xf0)) * mult) >> 16;
        let b = (((ba & 0xf0) | (ba >> 4)) * mult) >> 16;

        px[i_rg] = ((r & 0xf0) | (g >> 4 & 0x0f)) as u8;
        px[i_ba] = ((b & 0xf0) | a) as u8;
    }
}

const GAMMA_FIX: u32 = 12;
const GAMMA_TAB_FIX: u32 = 7;
const GAMMA_TAB_SIZE: usize = 1 << (GAMMA_FIX - GAMMA_TAB_FIX);
const GAMMA_TAB_SCALE: i32 = 1 << GAMMA_TAB_FIX;
const GAMMA_TAB_ROUNDER: i32 = GAMMA_TAB_SCALE >> 1;
const ALPHA_FIX: u32 = 19;

/// `pow(v / 255, 0.8) * 4095`, rounded, with one padding entry so a gather may
/// read a whole dword at the last index.
///
/// The assembly gathers from this table directly, so with `asm` on it carries
/// the C name `src/yuvdsp.h` declares.
#[cfg_attr(feature = "asm", allow(unsafe_code))]
#[cfg_attr(feature = "asm", export_name = "wpd_gamma_to_linear_tab")]
pub static GAMMA_TO_LINEAR: [u16; 257] = [
    0, 49, 85, 117, 147, 176, 204, 231, 257, 282, 307, 331, 355, 379, 402, 425, 447,
    469, 491, 513, 534, 556, 577, 598, 618, 639, 659, 679, 699, 719, 739, 759, 778,
    798, 817, 836, 855, 874, 893, 912, 930, 949, 967, 986, 1004, 1022, 1040, 1059,
    1077, 1094, 1112, 1130, 1148, 1165, 1183, 1200, 1218, 1235, 1252, 1270, 1287, 1304,
    1321, 1338, 1355, 1372, 1389, 1406, 1422, 1439, 1456, 1472, 1489, 1505, 1522, 1538,
    1555, 1571, 1587, 1604, 1620, 1636, 1652, 1668, 1684, 1700, 1716, 1732, 1748, 1764,
    1780, 1796, 1812, 1827, 1843, 1859, 1874, 1890, 1905, 1921, 1937, 1952, 1967, 1983,
    1998, 2014, 2029, 2044, 2059, 2075, 2090, 2105, 2120, 2135, 2151, 2166, 2181, 2196,
    2211, 2226, 2241, 2256, 2270, 2285, 2300, 2315, 2330, 2345, 2359, 2374, 2389, 2403,
    2418, 2433, 2447, 2462, 2477, 2491, 2506, 2520, 2535, 2549, 2564, 2578, 2592, 2607,
    2621, 2636, 2650, 2664, 2679, 2693, 2707, 2721, 2736, 2750, 2764, 2778, 2792, 2806,
    2820, 2835, 2849, 2863, 2877, 2891, 2905, 2919, 2933, 2947, 2961, 2975, 2988, 3002,
    3016, 3030, 3044, 3058, 3072, 3085, 3099, 3113, 3127, 3140, 3154, 3168, 3182, 3195,
    3209, 3222, 3236, 3250, 3263, 3277, 3291, 3304, 3318, 3331, 3345, 3358, 3372, 3385,
    3399, 3412, 3426, 3439, 3452, 3466, 3479, 3493, 3506, 3519, 3533, 3546, 3559, 3573,
    3586, 3599, 3612, 3626, 3639, 3652, 3665, 3678, 3692, 3705, 3718, 3731, 3744, 3757,
    3771, 3784, 3797, 3810, 3823, 3836, 3849, 3862, 3875, 3888, 3901, 3914, 3927, 3940,
    3953, 3966, 3979, 3992, 4005, 4018, 4031, 4044, 4056, 4069, 4082, 4095, 0,
];

/// `255 * pow(v * 128 / 4095, 1 / 0.8)`, rounded.
#[cfg_attr(feature = "asm", allow(unsafe_code))]
#[cfg_attr(feature = "asm", export_name = "wpd_linear_to_gamma_tab")]
pub static LINEAR_TO_GAMMA: [u16; GAMMA_TAB_SIZE + 1] = [
    0, 3, 8, 13, 19, 25, 31, 38, 45, 52, 60, 67, 75, 83, 91, 99, 107, 116, 124, 133,
    142, 151, 160, 169, 178, 187, 197, 206, 216, 226, 235, 245, 255,
];

#[inline(always)]
fn gamma_to_linear(v: u8) -> u32 {
    GAMMA_TO_LINEAR[v as usize] as u32
}

#[inline(always)]
fn linear_to_gamma(base_value: u32, shift: u32) -> i32 {
    let v = base_value << shift;
    let pos = (v >> (GAMMA_TAB_FIX + 2)) as usize;
    let x = (v & ((GAMMA_TAB_SCALE << 2) as u32 - 1)) as i32;
    let tab = &LINEAR_TO_GAMMA[pos..pos + 2];
    let y = tab[1] as i32 * x + tab[0] as i32 * ((GAMMA_TAB_SCALE << 2) - x);

    (y + GAMMA_TAB_ROUNDER) >> GAMMA_TAB_FIX
}

const YUV_FIX: u32 = 16;
const YUV_HALF: i32 = 1 << (YUV_FIX - 1);

#[inline(always)]
const fn rgb_to_y(r: i32, g: i32, b: i32) -> i32 {
    (16839 * r + 33059 * g + 6420 * b + YUV_HALF + (16 << YUV_FIX)) >> YUV_FIX
}

#[inline(always)]
const fn clip_uv(uv: i32) -> i32 {
    let uv = (uv + (YUV_HALF << 2) + (128 << (YUV_FIX + 2))) >> (YUV_FIX + 2);

    if uv & !0xff == 0 {
        uv
    } else if uv < 0 {
        0
    } else {
        255
    }
}

#[inline(always)]
const fn rgb_to_u(r: i32, g: i32, b: i32) -> i32 {
    clip_uv(-9719 * r - 19081 * g + 28800 * b)
}

#[inline(always)]
const fn rgb_to_v(r: i32, g: i32, b: i32) -> i32 {
    clip_uv(28800 * r - 24116 * g - 4684 * b)
}

pub fn argb_to_y(y: &mut [u8], argb: &[u8]) {
    for (o, px) in y.iter_mut().zip(argb.chunks_exact(4)) {
        *o = rgb_to_y(px[1].into(), px[2].into(), px[3].into()) as u8;
    }
}

pub fn argb_to_yuv444(y: &mut [u8], u: &mut [u8], v: &mut [u8], argb: &[u8]) {
    let planes = y.iter_mut().zip(u).zip(v);

    for (((oy, ou), ov), px) in planes.zip(argb.chunks_exact(4)) {
        let (r, g, b) = (px[1] as i32, px[2] as i32, px[3] as i32);

        *oy = rgb_to_y(r, g, b) as u8;
        *ou = rgb_to_u(4 * r, 4 * g, 4 * b) as u8;
        *ov = rgb_to_v(4 * r, 4 * g, 4 * b) as u8;
    }
}

#[inline(always)]
fn sum4(argb: &[u8], c: usize, j: usize, stride: usize) -> i32 {
    linear_to_gamma(
        gamma_to_linear(argb[j + c])
            + gamma_to_linear(argb[j + c + 4])
            + gamma_to_linear(argb[j + c + stride])
            + gamma_to_linear(argb[j + c + stride + 4]),
        0,
    )
}

#[inline(always)]
fn sum2(argb: &[u8], c: usize, j: usize, stride: usize) -> i32 {
    linear_to_gamma(
        gamma_to_linear(argb[j + c]) + gamma_to_linear(argb[j + c + stride]),
        1,
    )
}

#[inline(always)]
fn sum_weighted(
    argb: &[u8],
    c: usize,
    j: usize,
    total_a: u32,
    step: usize,
    stride: usize,
) -> i32 {
    let sum = argb[j] as u32 * gamma_to_linear(argb[j + c])
        + argb[j + step] as u32 * gamma_to_linear(argb[j + c + step])
        + argb[j + stride] as u32 * gamma_to_linear(argb[j + c + stride])
        + argb[j + stride + step] as u32 * gamma_to_linear(argb[j + c + stride + step]);
    let inv = (1u32 << ALPHA_FIX) / total_a;

    linear_to_gamma(sum.wrapping_mul(inv) >> (ALPHA_FIX - 2), 0)
}

/// Chroma for one pair of ARGB rows, averaged 2x2 in linear light. A `stride`
/// of 0 repeats the row, which is how the last row of an odd-height image is
/// handled. `argb` must be `4 * num_pixels + stride` long, whether the width
/// is odd or even.
pub fn argb_to_uv(
    u: &mut [u8],
    v: &mut [u8],
    argb: &[u8],
    stride: usize,
    num_pixels: usize,
    weight_alpha: bool,
) {
    let pairs = num_pixels >> 1;

    for i in 0..pairs {
        let j = 8 * i;
        let total_a = argb[j] as u32
            + argb[j + 4] as u32
            + argb[j + stride] as u32
            + argb[j + stride + 4] as u32;
        let (r, g, b) = if !weight_alpha || total_a == 4 * 0xff || total_a == 0 {
            (
                sum4(argb, 1, j, stride),
                sum4(argb, 2, j, stride),
                sum4(argb, 3, j, stride),
            )
        } else {
            (
                sum_weighted(argb, 1, j, total_a, 4, stride),
                sum_weighted(argb, 2, j, total_a, 4, stride),
                sum_weighted(argb, 3, j, total_a, 4, stride),
            )
        };

        u[i] = rgb_to_u(r, g, b) as u8;
        v[i] = rgb_to_v(r, g, b) as u8;
    }
    if num_pixels & 1 != 0 {
        let j = 8 * pairs;
        let total_a = 2 * (argb[j] as u32 + argb[j + stride] as u32);
        let (r, g, b) = if !weight_alpha || total_a == 4 * 0xff || total_a == 0 {
            (
                sum2(argb, 1, j, stride),
                sum2(argb, 2, j, stride),
                sum2(argb, 3, j, stride),
            )
        } else {
            (
                sum_weighted(argb, 1, j, total_a, 0, stride),
                sum_weighted(argb, 2, j, total_a, 0, stride),
                sum_weighted(argb, 3, j, total_a, 0, stride),
            )
        };

        u[pairs] = rgb_to_u(r, g, b) as u8;
        v[pairs] = rgb_to_v(r, g, b) as u8;
    }
}

/// Copies the alpha channel of a packed ARGB row into its own plane.
pub fn extract_alpha(dst: &mut [u8], argb: &[u8]) {
    for (d, px) in dst.iter_mut().zip(argb.chunks_exact(4)) {
        *d = px[0];
    }
}

/// Point conversion of one row from full-resolution planes.
pub fn yuv444_row<const L: usize>(dst: &mut [u8], y: &[u8], u: &[u8], v: &[u8]) {
    let src = y.iter().zip(u).zip(v);

    for (out, ((&yy, &uu), &vv)) in dst.chunks_exact_mut(bpp(L)).zip(src) {
        yuv_to_out::<L>(yy.into(), uu.into(), vv.into(), out);
    }
}

/// Point sampling of one row from 4:2:0 planes, which libwebp uses when fancy
/// upsampling is turned off.
pub fn yuv420_row<const L: usize>(dst: &mut [u8], y: &[u8], u: &[u8], v: &[u8]) {
    for (i, (out, &yy)) in dst.chunks_exact_mut(bpp(L)).zip(y).enumerate() {
        yuv_to_out::<L>(yy.into(), u[i >> 1].into(), v[i >> 1].into(), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grey_pixel_stays_grey_in_every_layout() {
        let mut out = [0u8; 4];

        yuv_to_out::<LAYOUT_ARGB>(128, 128, 128, &mut out);
        assert_eq!(out[0], 0xff);
        assert!(out[1].abs_diff(out[2]) <= 1 && out[2].abs_diff(out[3]) <= 1);

        let argb = out;
        let mut rgba = [0u8; 4];

        yuv_to_out::<LAYOUT_RGBA>(128, 128, 128, &mut rgba);
        assert_eq!(rgba, [argb[1], argb[2], argb[3], 0xff]);

        let mut bgra = [0u8; 4];

        yuv_to_out::<LAYOUT_BGRA>(128, 128, 128, &mut bgra);
        assert_eq!(bgra, [argb[3], argb[2], argb[1], 0xff]);
    }

    #[test]
    fn the_three_byte_layouts_leave_the_fourth_byte_alone() {
        let mut rgb = [0xa5u8; 4];
        let mut bgr = [0xa5u8; 4];

        yuv_to_out::<LAYOUT_RGB>(200, 40, 90, &mut rgb);
        yuv_to_out::<LAYOUT_BGR>(200, 40, 90, &mut bgr);
        assert_eq!(rgb[3], 0xa5);
        assert_eq!(bgr[3], 0xa5);
        assert_eq!([rgb[0], rgb[1], rgb[2]], [bgr[2], bgr[1], bgr[0]]);
    }

    #[test]
    fn a_flat_chroma_plane_upsamples_flat() {
        let y = [90u8; 8];
        let u = [70u8; 4];
        let v = [190u8; 4];
        let mut dst = [0u8; 32];

        upsample_edge::<LAYOUT_ARGB>(
            y[0], None, u[0], v[0], u[0], v[0], &mut dst, None,
        );
        upsample_pairs::<LAYOUT_ARGB>(
            &y, None, &u, &v, &u, &v, &mut dst, None, 1, 3, 1,
        );

        let first = &dst[0..4];

        for px in dst[..28].chunks_exact(4) {
            assert_eq!(px, first);
        }
    }

    #[test]
    fn premultiplying_by_opaque_alpha_changes_nothing() {
        let mut row = [0xff, 10, 20, 30, 0xff, 200, 210, 220];
        let before = row;

        premultiply_row(&mut row, true);
        assert_eq!(row, before);
    }

    #[test]
    fn packing_round_trips_through_rgba() {
        let src = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut rgba = [0u8; 8];
        let mut back = [0u8; 8];

        pack_rgba(&mut rgba, &src);
        pack_bgra(&mut back, &src);
        assert_eq!(rgba, [2, 3, 4, 1, 6, 7, 8, 5]);
        assert_eq!(back, [4, 3, 2, 1, 8, 7, 6, 5]);
    }

    #[test]
    fn grey_survives_the_round_trip_to_chroma() {
        let argb = [0xffu8, 128, 128, 128, 0xff, 128, 128, 128];
        let mut u = [0u8; 1];
        let mut v = [0u8; 1];

        argb_to_uv(&mut u, &mut v, &argb, 0, 2, false);
        assert!(u[0].abs_diff(128) <= 1);
        assert!(v[0].abs_diff(128) <= 1);
    }
}
