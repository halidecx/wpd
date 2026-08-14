//! C ABI for the rescaler, as declared by `src/rescaler.h`.
//!
//! The struct keeps its C layout because `src/convert.c` still drives it
//! directly and reads its fields through the inline helpers in that header.

use std::ffi::c_int;
use std::slice;

use wpd::rescale as k;

#[repr(C)]
pub struct WPDRescaler {
    pub x_expand: c_int,
    pub y_expand: c_int,
    pub num_channels: c_int,
    pub fx_scale: u32,
    pub fy_scale: u32,
    pub fxy_scale: u32,
    pub y_accum: c_int,
    pub y_add: c_int,
    pub y_sub: c_int,
    pub x_add: c_int,
    pub x_sub: c_int,
    pub src_width: c_int,
    pub src_height: c_int,
    pub dst_width: c_int,
    pub dst_height: c_int,
    pub src_y: c_int,
    pub dst_y: c_int,
    pub dst: *mut u8,
    pub dst_stride: c_int,
    pub irow: *mut u32,
    pub frow: *mut u32,
}

impl WPDRescaler {
    fn width(&self) -> usize {
        self.num_channels as usize * self.dst_width as usize
    }

    fn import(&self) -> k::Import {
        k::Import {
            num_channels: self.num_channels as usize,
            src_width: self.src_width as usize,
            dst_width: self.dst_width as usize,
            x_add: self.x_add as u32,
            x_sub: self.x_sub as u32,
            fx_scale: self.fx_scale,
        }
    }

    fn export(&self) -> k::Export {
        k::Export {
            y_accum: self.y_accum,
            y_sub: self.y_sub as u32,
            fy_scale: self.fy_scale,
            fxy_scale: self.fxy_scale,
        }
    }

    fn has_pending_output(&self) -> bool {
        self.dst_y < self.dst_height && self.y_accum <= 0
    }
}

/// # Safety
///
/// `dst` must have `dst_height` rows of `dst_stride`, and `work` room for
/// `2 * num_channels * dst_width` words.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_rescaler_init(
    r: *mut WPDRescaler,
    src_width: c_int,
    src_height: c_int,
    dst: *mut u8,
    dst_width: c_int,
    dst_height: c_int,
    dst_stride: c_int,
    num_channels: c_int,
    work: *mut u32,
) {
    let x_expand = src_width < dst_width;
    let y_expand = src_height < dst_height;
    let width = num_channels as usize * dst_width as usize;

    unsafe { slice::from_raw_parts_mut(work, 2 * width).fill(0) };

    let x_add = if x_expand { dst_width - 1 } else { src_width };
    let x_sub = if x_expand { src_width - 1 } else { dst_width };
    let y_add = if y_expand { src_height - 1 } else { src_height };
    let y_sub = if y_expand { dst_height - 1 } else { dst_height };

    let fx_scale = if x_expand {
        0
    } else {
        k::frac(1, x_sub as u32)
    };
    let fy_scale = if y_expand {
        k::frac(1, x_add as u32)
    } else {
        k::frac(1, y_sub as u32)
    };
    let fxy_scale = if y_expand {
        0
    } else {
        let den = u64::from(x_add as u32) * u64::from(y_add as u32);
        let ratio = (u64::from(dst_height as u32) << 32)
            .checked_div(den)
            .unwrap_or(0);

        if ratio > u64::from(u32::MAX) {
            0
        } else {
            ratio as u32
        }
    };

    unsafe {
        r.write(WPDRescaler {
            x_expand: x_expand.into(),
            y_expand: y_expand.into(),
            num_channels,
            fx_scale,
            fy_scale,
            fxy_scale,
            y_accum: if y_expand { y_sub } else { y_add },
            y_add,
            y_sub,
            x_add,
            x_sub,
            src_width,
            src_height,
            dst_width,
            dst_height,
            src_y: 0,
            dst_y: 0,
            dst,
            dst_stride,
            irow: work,
            frow: work.add(width),
        })
    }
}

/// # Safety
///
/// `r` must point to an initialised rescaler.
#[no_mangle]
pub unsafe extern "C" fn wpd_rescaler_needed_lines(
    r: *const WPDRescaler,
    max_num_lines: c_int,
) -> c_int {
    let r = unsafe { &*r };
    let num_lines = (r.y_accum + r.y_sub - 1) / r.y_sub;

    num_lines.min(max_num_lines)
}

/// # Safety
///
/// `src` must have `num_lines` rows of `src_stride`, each at least
/// `src_width * num_channels` long.
#[no_mangle]
pub unsafe extern "C" fn wpd_rescaler_import(
    r: *mut WPDRescaler,
    num_lines: c_int,
    src: *const u8,
    src_stride: c_int,
) -> c_int {
    let r = unsafe { &mut *r };
    let width = r.width();
    let row_len = r.src_width as usize * r.num_channels as usize;
    let mut src = src;
    let mut imported = 0;

    while imported < num_lines && !r.has_pending_output() {
        if r.y_expand != 0 {
            std::mem::swap(&mut r.irow, &mut r.frow);
        }

        let p = r.import();

        unsafe {
            let row = slice::from_raw_parts(src, row_len);
            let frow = slice::from_raw_parts_mut(r.frow, width);

            if r.x_expand != 0 {
                k::import_row_expand(frow, row, p);
            } else {
                k::import_row_shrink(frow, row, p);
            }
            if r.y_expand == 0 {
                let irow = slice::from_raw_parts_mut(r.irow, width);

                k::accumulate(irow, slice::from_raw_parts(r.frow, width));
            }
            src = src.offset(src_stride as isize);
        }

        r.src_y += 1;
        imported += 1;
        r.y_accum -= r.y_sub;
    }
    imported
}

/// # Safety
///
/// `r` must point to an initialised rescaler with room for another output row.
#[no_mangle]
pub unsafe extern "C" fn wpd_rescaler_export_row(r: *mut WPDRescaler) {
    let r = unsafe { &mut *r };

    if r.y_accum > 0 {
        return;
    }

    let width = r.width();
    let p = r.export();

    unsafe {
        let dst = slice::from_raw_parts_mut(r.dst, width);
        let irow = slice::from_raw_parts_mut(r.irow, width);
        let frow = slice::from_raw_parts(r.frow, width);

        if r.y_expand != 0 {
            k::export_row_expand(dst, irow, frow, p);
        } else if r.fxy_scale != 0 {
            k::export_row_shrink(dst, irow, frow, p);
        } else {
            k::export_row_direct(dst, irow);
        }
        r.dst = r.dst.offset(r.dst_stride as isize);
    }

    r.y_accum += r.y_add;
    r.dst_y += 1;
}

/// # Safety
///
/// As [`wpd_rescaler_export_row`].
#[no_mangle]
pub unsafe extern "C" fn wpd_rescaler_export(r: *mut WPDRescaler) -> c_int {
    let mut exported = 0;

    while unsafe { (*r).has_pending_output() } {
        unsafe { wpd_rescaler_export_row(r) };
        exported += 1;
    }
    exported
}

/// # Safety
///
/// `src` and `dst` must have their stated dimensions, and `work` room for
/// `2 * num_channels * dst_width` words.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wpd_rescale_plane(
    dst: *mut u8,
    dst_stride: c_int,
    dst_width: c_int,
    dst_height: c_int,
    src: *const u8,
    src_stride: c_int,
    src_width: c_int,
    src_height: c_int,
    num_channels: c_int,
    work: *mut u32,
) {
    let mut r = std::mem::MaybeUninit::<WPDRescaler>::uninit();

    unsafe {
        wpd_rescaler_init(
            r.as_mut_ptr(),
            src_width,
            src_height,
            dst,
            dst_width,
            dst_height,
            dst_stride,
            num_channels,
            work,
        );

        let r = r.as_mut_ptr();
        let mut row = 0;

        while row < src_height {
            row += wpd_rescaler_import(
                r,
                src_height - row,
                src.offset(row as isize * src_stride as isize),
                src_stride,
            );
            wpd_rescaler_export(r);
        }
    }
}

/// # Safety
///
/// `argb` must have `num_pixels` four-byte pixels.
#[no_mangle]
pub unsafe extern "C" fn wpd_premultiply_argb_row(
    argb: *mut u8,
    num_pixels: c_int,
    inverse: c_int,
) {
    let row = unsafe { slice::from_raw_parts_mut(argb, 4 * num_pixels as usize) };

    k::premultiply_argb_row(row, inverse != 0);
}

/// # Safety
///
/// `plane` and `alpha` must both have `num_pixels` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_multiply_row(
    plane: *mut u8,
    alpha: *const u8,
    num_pixels: c_int,
    inverse: c_int,
) {
    let n = num_pixels as usize;

    unsafe {
        k::multiply_row(
            slice::from_raw_parts_mut(plane, n),
            slice::from_raw_parts(alpha, n),
            inverse != 0,
        )
    }
}
