use std::ffi::c_int;

use super::count;
use std::slice;

use wpd::convert::YuvPlanes;
use wpd::dsp::yuv as k;
use wpd::dsp::yuv::{
    bpp, YuvDsp, LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_NB, LAYOUT_RGB,
    LAYOUT_RGBA, UPSAMPLE_BLOCK,
};
use wpd::picture::{PlaneMut, PlaneRef};

use crate::frame::plane_extent;

pub type UpsampleBlockFn = unsafe extern "C" fn(
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *const u8,
    *mut u8,
    *mut u8,
    c_int,
);
pub type DispatchAlphaFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type PackRowFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type PremultiplyRowFn = unsafe extern "C" fn(*mut u8, c_int, c_int);
pub type Premultiply4444Fn = unsafe extern "C" fn(*mut u8, c_int);
pub type ArgbToYFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type ArgbToYuv444Fn =
    unsafe extern "C" fn(*mut u8, *mut u8, *mut u8, *const u8, c_int);
pub type ArgbToUvFn =
    unsafe extern "C" fn(*mut u8, *mut u8, *const u8, isize, c_int, c_int);
/* Forward direction only; the inverse divides per pixel and stays scalar. */
pub type MultiplyRowFn = unsafe extern "C" fn(*mut u8, *const u8, c_int);
pub type MultiplyArgbFn = unsafe extern "C" fn(*mut u8, c_int);
pub type YuvRowFn =
    unsafe extern "C" fn(*mut u8, *const u8, *const u8, *const u8, c_int);

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
pub struct WPDYUVDSP {
    pub upsample_block: [UpsampleBlockFn; LAYOUT_NB],
    pub dispatch_alpha_first: DispatchAlphaFn,
    pub dispatch_alpha_last: DispatchAlphaFn,
    pub pack_rgba: PackRowFn,
    pub pack_bgra: PackRowFn,
    pub pack_rgb: PackRowFn,
    pub pack_bgr: PackRowFn,
    pub pack_rgb565: PackRowFn,
    pub pack_rgba4444: PackRowFn,
    pub pack_bgr565: PackRowFn,
    pub pack_bgra4444: PackRowFn,
    pub premultiply_row: PremultiplyRowFn,
    pub premultiply_row_4444: Premultiply4444Fn,
    pub premultiply_row_4444_swap: Premultiply4444Fn,
    pub argb_to_y: ArgbToYFn,
    pub argb_to_yuv444: ArgbToYuv444Fn,
    pub argb_to_uv: ArgbToUvFn,
    pub multiply_row: MultiplyRowFn,
    pub premultiply_argb_row: MultiplyArgbFn,
    pub yuv444_row: [YuvRowFn; LAYOUT_NB],
    pub yuv420_row: [YuvRowFn; LAYOUT_NB],
}

macro_rules! upsample_block_tramp {
    ($name:ident, $layout:expr) => {
        unsafe extern "C" fn $name(
            top_y: *const u8,
            bottom_y: *const u8,
            top_u: *const u8,
            top_v: *const u8,
            cur_u: *const u8,
            cur_v: *const u8,
            top_dst: *mut u8,
            bottom_dst: *mut u8,
            num_blocks: c_int,
        ) {
            let Some(blocks) = count(num_blocks) else {
                return;
            };
            let last = blocks * (UPSAMPLE_BLOCK / 2);
            let pixels = 2 * last;
            let bpp = bpp($layout);

            unsafe {
                k::upsample_pairs::<$layout>(
                    slice::from_raw_parts(top_y, pixels),
                    (!bottom_y.is_null())
                        .then(|| slice::from_raw_parts(bottom_y, pixels)),
                    slice::from_raw_parts(top_u, last + 1),
                    slice::from_raw_parts(top_v, last + 1),
                    slice::from_raw_parts(cur_u, last + 1),
                    slice::from_raw_parts(cur_v, last + 1),
                    slice::from_raw_parts_mut(top_dst, bpp * pixels),
                    (!bottom_dst.is_null())
                        .then(|| slice::from_raw_parts_mut(bottom_dst, bpp * pixels)),
                    1,
                    last,
                    0,
                )
            }
        }
    };
}

upsample_block_tramp!(upsample_block_argb_c, LAYOUT_ARGB);
upsample_block_tramp!(upsample_block_rgba_c, LAYOUT_RGBA);
upsample_block_tramp!(upsample_block_bgra_c, LAYOUT_BGRA);
upsample_block_tramp!(upsample_block_rgb_c, LAYOUT_RGB);
upsample_block_tramp!(upsample_block_bgr_c, LAYOUT_BGR);

/* dst holds $d bytes per pixel, src $s, and the kernel takes the two rows. */
macro_rules! row_tramp {
    ($name:ident, $kernel:ident, $d:literal, $s:literal) => {
        unsafe extern "C" fn $name(dst: *mut u8, src: *const u8, n: c_int) {
            let Some(n) = count(n) else {
                return;
            };

            unsafe {
                k::$kernel(
                    slice::from_raw_parts_mut(dst, $d * n),
                    slice::from_raw_parts(src, $s * n),
                )
            }
        }
    };
}

/* One row in place, plus whatever fixed argument selects the variant. */
macro_rules! inplace_tramp {
    ($name:ident, $kernel:ident, $bpp:literal, $extra:expr) => {
        unsafe extern "C" fn $name(row: *mut u8, n: c_int) {
            let Some(n) = count(n) else {
                return;
            };

            k::$kernel(unsafe { slice::from_raw_parts_mut(row, $bpp * n) }, $extra);
        }
    };
}

macro_rules! yuv_row_tramp {
    ($name:ident, $kernel:ident, $layout:expr, $chroma:literal) => {
        unsafe extern "C" fn $name(
            dst: *mut u8,
            y: *const u8,
            u: *const u8,
            v: *const u8,
            n: c_int,
        ) {
            let Some(n) = count(n) else {
                return;
            };
            let samples = n.div_ceil($chroma);

            unsafe {
                k::$kernel::<$layout>(
                    slice::from_raw_parts_mut(dst, bpp($layout) * n),
                    slice::from_raw_parts(y, n),
                    slice::from_raw_parts(u, samples),
                    slice::from_raw_parts(v, samples),
                )
            }
        }
    };
}

yuv_row_tramp!(yuv444_row_argb_c, yuv444_row, LAYOUT_ARGB, 1);
yuv_row_tramp!(yuv444_row_rgba_c, yuv444_row, LAYOUT_RGBA, 1);
yuv_row_tramp!(yuv444_row_bgra_c, yuv444_row, LAYOUT_BGRA, 1);
yuv_row_tramp!(yuv444_row_rgb_c, yuv444_row, LAYOUT_RGB, 1);
yuv_row_tramp!(yuv444_row_bgr_c, yuv444_row, LAYOUT_BGR, 1);
yuv_row_tramp!(yuv420_row_argb_c, yuv420_row, LAYOUT_ARGB, 2);
yuv_row_tramp!(yuv420_row_rgba_c, yuv420_row, LAYOUT_RGBA, 2);
yuv_row_tramp!(yuv420_row_bgra_c, yuv420_row, LAYOUT_BGRA, 2);
yuv_row_tramp!(yuv420_row_rgb_c, yuv420_row, LAYOUT_RGB, 2);
yuv_row_tramp!(yuv420_row_bgr_c, yuv420_row, LAYOUT_BGR, 2);

row_tramp!(dispatch_alpha_first_c, dispatch_alpha_first, 4, 1);
row_tramp!(dispatch_alpha_last_c, dispatch_alpha_last, 4, 1);
row_tramp!(pack_rgba_c, pack_rgba, 4, 4);
row_tramp!(pack_bgra_c, pack_bgra, 4, 4);
row_tramp!(pack_rgb_c, pack_rgb, 3, 4);
row_tramp!(pack_bgr_c, pack_bgr, 3, 4);
row_tramp!(pack_rgb565_c, pack_rgb565, 2, 4);
row_tramp!(pack_bgr565_c, pack_bgr565, 2, 4);
row_tramp!(pack_rgba4444_c, pack_rgba4444, 2, 4);
row_tramp!(pack_bgra4444_c, pack_bgra4444, 2, 4);
row_tramp!(argb_to_y_c, argb_to_y, 1, 4);

inplace_tramp!(premultiply_row_4444_c, premultiply_row_4444, 2, false);
inplace_tramp!(premultiply_row_4444_swap_c, premultiply_row_4444, 2, true);

unsafe extern "C" fn premultiply_row_c(rgba: *mut u8, alpha_first: c_int, n: c_int) {
    let Some(n) = count(n) else {
        return;
    };
    let row = unsafe { slice::from_raw_parts_mut(rgba, 4 * n) };

    k::premultiply_row(row, alpha_first != 0);
}

unsafe extern "C" fn multiply_row_c(plane: *mut u8, alpha: *const u8, n: c_int) {
    let Some(n) = count(n) else {
        return;
    };

    unsafe {
        k::multiply_row(
            slice::from_raw_parts_mut(plane, n),
            slice::from_raw_parts(alpha, n),
            false,
        )
    }
}

unsafe extern "C" fn premultiply_argb_row_c(argb: *mut u8, n: c_int) {
    let Some(n) = count(n) else {
        return;
    };
    let row = unsafe { slice::from_raw_parts_mut(argb, 4 * n) };

    k::premultiply_argb_row(row, false);
}

unsafe extern "C" fn argb_to_yuv444_c(
    y: *mut u8,
    u: *mut u8,
    v: *mut u8,
    argb: *const u8,
    n: c_int,
) {
    let Some(n) = count(n) else {
        return;
    };

    unsafe {
        k::argb_to_yuv444(
            slice::from_raw_parts_mut(y, n),
            slice::from_raw_parts_mut(u, n),
            slice::from_raw_parts_mut(v, n),
            slice::from_raw_parts(argb, 4 * n),
        )
    }
}

unsafe extern "C" fn argb_to_uv_c(
    u: *mut u8,
    v: *mut u8,
    argb: *const u8,
    argb_stride: isize,
    n: c_int,
    weight_alpha: c_int,
) {
    let Some(n) = count(n) else {
        return;
    };
    let Ok(stride) = usize::try_from(argb_stride) else {
        return;
    };
    let chroma = n.div_ceil(2);

    unsafe {
        k::argb_to_uv(
            slice::from_raw_parts_mut(u, chroma),
            slice::from_raw_parts_mut(v, chroma),
            slice::from_raw_parts(argb, 4 * n + stride),
            stride,
            n,
            weight_alpha != 0,
        )
    }
}

#[cfg(all(
    feature = "asm",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
fn init_asm(dsp: &mut WPDYUVDSP) {
    let t = wpd::asm::yuv::raw_table(wpd::cpu::flags());

    if let Some(v) = t.upsample_block {
        dsp.upsample_block = v;
    }
    if let Some(v) = t.upsample_rgb {
        dsp.upsample_block[LAYOUT_RGB] = v;
    }
    if let Some(v) = t.upsample_bgr {
        dsp.upsample_block[LAYOUT_BGR] = v;
    }
    if let Some(v) = t.dispatch_alpha_first {
        dsp.dispatch_alpha_first = v;
    }
    if let Some(v) = t.dispatch_alpha_last {
        dsp.dispatch_alpha_last = v;
    }
    if let Some(v) = t.packers {
        let [rgba, bgra, rgb, bgr, rgb565, rgba4444, bgr565, bgra4444] = v;

        dsp.pack_rgba = rgba;
        dsp.pack_bgra = bgra;
        dsp.pack_rgb = rgb;
        dsp.pack_bgr = bgr;
        dsp.pack_rgb565 = rgb565;
        dsp.pack_rgba4444 = rgba4444;
        dsp.pack_bgr565 = bgr565;
        dsp.pack_bgra4444 = bgra4444;
    }
    if let Some(v) = t.premultiply_row {
        dsp.premultiply_row = v;
    }
    if let Some(v) = t.premultiply_row_4444 {
        dsp.premultiply_row_4444 = v;
    }
    if let Some(v) = t.premultiply_row_4444_swap {
        dsp.premultiply_row_4444_swap = v;
    }
    if let Some(v) = t.argb_to_y {
        dsp.argb_to_y = v;
    }
    if let Some(v) = t.argb_to_yuv444 {
        dsp.argb_to_yuv444 = v;
    }
    if let Some(v) = t.argb_to_uv {
        dsp.argb_to_uv = v;
    }
    if let Some(v) = t.multiply_row {
        dsp.multiply_row = v;
    }
    if let Some(v) = t.premultiply_argb_row {
        dsp.premultiply_argb_row = v;
    }
    if let Some(v) = t.yuv444_row {
        dsp.yuv444_row = v;
    }
    if let Some(v) = t.yuv420_row {
        dsp.yuv420_row = v;
    }
    if let Some([rgb, bgr]) = t.yuv444_row_rgb {
        dsp.yuv444_row[LAYOUT_RGB] = rgb;
        dsp.yuv444_row[LAYOUT_BGR] = bgr;
    }
    if let Some([rgb, bgr]) = t.yuv420_row_rgb {
        dsp.yuv420_row[LAYOUT_RGB] = rgb;
        dsp.yuv420_row[LAYOUT_BGR] = bgr;
    }
}

impl WPDYUVDSP {
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = WPDYUVDSP {
            upsample_block: [
                upsample_block_argb_c,
                upsample_block_rgba_c,
                upsample_block_bgra_c,
                upsample_block_rgb_c,
                upsample_block_bgr_c,
            ],
            dispatch_alpha_first: dispatch_alpha_first_c,
            dispatch_alpha_last: dispatch_alpha_last_c,
            pack_rgba: pack_rgba_c,
            pack_bgra: pack_bgra_c,
            pack_rgb: pack_rgb_c,
            pack_bgr: pack_bgr_c,
            pack_rgb565: pack_rgb565_c,
            pack_rgba4444: pack_rgba4444_c,
            pack_bgr565: pack_bgr565_c,
            pack_bgra4444: pack_bgra4444_c,
            premultiply_row: premultiply_row_c,
            premultiply_row_4444: premultiply_row_4444_c,
            premultiply_row_4444_swap: premultiply_row_4444_swap_c,
            argb_to_y: argb_to_y_c,
            argb_to_yuv444: argb_to_yuv444_c,
            argb_to_uv: argb_to_uv_c,
            multiply_row: multiply_row_c,
            premultiply_argb_row: premultiply_argb_row_c,
            yuv444_row: [
                yuv444_row_argb_c,
                yuv444_row_rgba_c,
                yuv444_row_bgra_c,
                yuv444_row_rgb_c,
                yuv444_row_bgr_c,
            ],
            yuv420_row: [
                yuv420_row_argb_c,
                yuv420_row_rgba_c,
                yuv420_row_bgra_c,
                yuv420_row_rgb_c,
                yuv420_row_bgr_c,
            ],
        };

        #[cfg(all(
            feature = "asm",
            any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
        ))]
        init_asm(&mut table);

        table
    }
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_yuv_dsp_init(dsp: *mut WPDYUVDSP) {
    unsafe { dsp.write(WPDYUVDSP::new()) }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_yuv420_to_packed_rows(
    layout: c_int,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    a: *const u8,
    a_stride: isize,
    width: c_int,
    height: c_int,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    if width <= 0
        || height <= 0
        || row_start < 0
        || row_start >= row_end
        || row_end > height
        || !(0..LAYOUT_NB as c_int).contains(&layout)
    {
        return row_start;
    }
    if dst_stride <= 0 || y_stride <= 0 || uv_stride <= 0 {
        return row_start;
    }
    if !a.is_null() && a_stride <= 0 {
        return row_start;
    }
    if dst.is_null() || y.is_null() || u.is_null() || v.is_null() {
        return row_start;
    }

    let layout = layout as usize;
    let (w, h) = (width as usize, height as usize);
    let Some(dst_row) = bpp(layout).checked_mul(w) else {
        return row_start;
    };
    let Some(dst_len) = plane_extent(dst_stride, h, dst_row) else {
        return row_start;
    };
    let Some(y_len) = plane_extent(y_stride, h, w) else {
        return row_start;
    };
    let chroma_h = h.div_ceil(2);
    let chroma_w = w.div_ceil(2);
    let Some(uv_len) = plane_extent(uv_stride, chroma_h, chroma_w) else {
        return row_start;
    };
    let alpha_len = if a.is_null() {
        None
    } else {
        let Some(len) = plane_extent(a_stride, h, w) else {
            return row_start;
        };
        Some(len)
    };

    unsafe {
        let mut out = PlaneMut::borrowed(
            slice::from_raw_parts_mut(dst, dst_len),
            dst_stride as usize,
        );
        let plane = |p: *const u8, stride: isize, len: usize| {
            PlaneRef::borrowed(slice::from_raw_parts(p, len), stride as usize)
        };
        let src = YuvPlanes {
            y: plane(y, y_stride, y_len),
            u: plane(u, uv_stride, uv_len),
            v: plane(v, uv_stride, uv_len),
            a: alpha_len.map(|len| plane(a, a_stride, len)),
        };

        wpd::convert::yuv420_to_packed_rows(
            &YuvDsp::new(),
            layout,
            &mut out,
            &src,
            w,
            h,
            row_start as usize,
            row_end as usize,
            /* A checkasm entry point measures one kernel, not a decode. */
            1,
        ) as c_int
    }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn wpd_yuv420_to_packed(
    layout: c_int,
    dst: *mut u8,
    dst_stride: isize,
    y: *const u8,
    y_stride: isize,
    u: *const u8,
    v: *const u8,
    uv_stride: isize,
    a: *const u8,
    a_stride: isize,
    width: c_int,
    height: c_int,
) {
    unsafe {
        wpd_yuv420_to_packed_rows(
            layout, dst, dst_stride, y, y_stride, u, v, uv_stride, a, a_stride, width,
            height, 0, height,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_conversion_bounds_return_before_touching_pointers() {
        for (layout, width, height, start, end) in [
            (-1, 1, 1, 0, 1),
            (LAYOUT_NB as c_int, 1, 1, 0, 1),
            (LAYOUT_ARGB as c_int, 0, 1, 0, 1),
            (LAYOUT_ARGB as c_int, 1, 0, 0, 1),
            (LAYOUT_ARGB as c_int, 1, 1, -1, 1),
            (LAYOUT_ARGB as c_int, 1, 1, 1, 1),
            (LAYOUT_ARGB as c_int, 1, 1, 0, 2),
        ] {
            assert_eq!(
                unsafe {
                    wpd_yuv420_to_packed_rows(
                        layout,
                        std::ptr::null_mut(),
                        4,
                        std::ptr::null(),
                        1,
                        std::ptr::null(),
                        std::ptr::null(),
                        1,
                        std::ptr::null(),
                        0,
                        width,
                        height,
                        start,
                        end,
                    )
                },
                start
            );
        }
    }
}
