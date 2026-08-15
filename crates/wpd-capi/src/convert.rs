//! C ABI for format policy, cropping, scaling and the region blitters, as
//! declared by `src/convert.h`.
//!
//! The decision-making — which format packs how, what a crop resolves to, what
//! a scale rounds to — is [`wpd::image`], and so are the YUVA blend kernels.
//! What stays here is the plane walking: the caller's images are C structs
//! whose `data[p]` may already have been offset into a crop or flipped to a
//! negative stride, so the rows are rebuilt per iteration rather than borrowed.

use std::ffi::c_int;
use std::mem::MaybeUninit;
use std::ptr;

use wpd::image::{self, ceil_rshift, Crop, Format};

use crate::dsp::vp8l::WPDLosslessDSP;
use wpd::dsp::yuv::{LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_RGB};

use crate::dsp::yuv::{
    wpd_argb_to_yuva, wpd_yuv420_to_packed, wpd_yuv420_to_packed_simple,
    wpd_yuv444_to_packed, PackRowFn, Premultiply4444Fn, WPDYUVDSP,
};
use crate::image::{
    image_alloc_packed, image_alloc_yuv444, image_alloc_yuva, image_drop_plane,
    image_free, image_scratch_grow, RescaleScratch, WebPImage,
};
use crate::rescale::{
    wpd_multiply_row, wpd_premultiply_argb_row, wpd_rescale_plane, wpd_rescaler_export,
    wpd_rescaler_import, wpd_rescaler_init, WPDRescaler,
};

const WPD_OK: c_int = 0;
const WPD_ERR_INVALID_ARG: c_int = -1;
const WPD_ERR_TOO_LARGE: c_int = -7;

/// `WPDDecoderOptions` from `include/wpd.h`.
#[repr(C)]
pub struct WPDDecoderOptions {
    pub struct_size: usize,
    pub bypass_filtering: c_int,
    pub no_fancy_upsampling: c_int,
    pub use_cropping: c_int,
    pub crop_left: c_int,
    pub crop_top: c_int,
    pub crop_width: c_int,
    pub crop_height: c_int,
    pub use_scaling: c_int,
    pub scaled_width: c_int,
    pub scaled_height: c_int,
    pub flip: c_int,
}

impl WPDDecoderOptions {
    /// The oldest revision this build accepts, and equally how much of a
    /// caller's struct it reads.
    pub(crate) fn v1() -> usize {
        std::mem::offset_of!(WPDDecoderOptions, flip) + std::mem::size_of::<c_int>()
    }

    pub(crate) fn new() -> Self {
        WPDDecoderOptions {
            struct_size: std::mem::size_of::<WPDDecoderOptions>(),
            bypass_filtering: 0,
            no_fancy_upsampling: 0,
            use_cropping: 0,
            crop_left: 0,
            crop_top: 0,
            crop_width: 0,
            crop_height: 0,
            use_scaling: 0,
            scaled_width: 0,
            scaled_height: 0,
            flip: 0,
        }
    }

    /// Field by field rather than whole: the caller's struct may be a shorter
    /// revision than this one, and its `struct_size` is not ours to keep.
    pub(crate) fn copy(&self) -> Self {
        WPDDecoderOptions {
            struct_size: std::mem::size_of::<WPDDecoderOptions>(),
            bypass_filtering: self.bypass_filtering,
            no_fancy_upsampling: self.no_fancy_upsampling,
            use_cropping: self.use_cropping,
            crop_left: self.crop_left,
            crop_top: self.crop_top,
            crop_width: self.crop_width,
            crop_height: self.crop_height,
            use_scaling: self.use_scaling,
            scaled_width: self.scaled_width,
            scaled_height: self.scaled_height,
            flip: self.flip,
        }
    }
}

/// `SubRect` from `src/convert.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubRect {
    pub x: c_int,
    pub y: c_int,
    pub w: c_int,
    pub h: c_int,
}

/// The format an image claims, or ARGB when the field holds something no
/// version of the enum defines. Nothing downstream can act on a format it does
/// not know, and the four-byte packed case is the one the decoder produces.
fn format_of(img: &WebPImage) -> Format {
    img.format().unwrap_or(Format::Argb)
}

#[no_mangle]
pub extern "C" fn format_is_packed(format: c_int) -> c_int {
    c_int::from(Format::from_raw(format).is_some_and(Format::is_packed))
}

#[no_mangle]
pub extern "C" fn format_bpp(format: c_int) -> c_int {
    Format::from_raw(format).map_or(4, Format::bpp) as c_int
}

#[no_mangle]
pub extern "C" fn format_is_premultiplied(format: c_int) -> c_int {
    c_int::from(Format::from_raw(format).is_some_and(Format::is_premultiplied))
}

#[no_mangle]
pub extern "C" fn format_valid(format: c_int) -> c_int {
    c_int::from(Format::from_raw(format).is_some())
}

#[no_mangle]
pub extern "C" fn format_layout(format: c_int) -> c_int {
    Format::from_raw(format).map_or(LAYOUT_ARGB, Format::layout) as c_int
}

/// # Safety
///
/// `img` must point to a live `WebPImage`.
#[no_mangle]
pub unsafe extern "C" fn image_nb_components(img: *const WebPImage) -> c_int {
    match unsafe { img.as_ref() } {
        Some(img) => format_of(img).nb_components() as c_int,
        None => 1,
    }
}

/// # Safety
///
/// `dsp` must point to a live `WPDYUVDSP`.
#[no_mangle]
pub unsafe extern "C" fn format_packer(
    dsp: *const WPDYUVDSP,
    format: c_int,
) -> Option<PackRowFn> {
    let dsp = unsafe { dsp.as_ref() }?;

    Some(match Format::from_raw(format)? {
        Format::Rgba | Format::RgbaPre => dsp.pack_rgba,
        Format::Bgra | Format::BgraPre => dsp.pack_bgra,
        Format::Rgb => dsp.pack_rgb,
        Format::Bgr => dsp.pack_bgr,
        Format::Rgb565 => dsp.pack_rgb565,
        Format::Rgba4444 | Format::Rgba4444Pre => dsp.pack_rgba4444,
        Format::Bgr565 => dsp.pack_bgr565,
        Format::Bgra4444 | Format::Bgra4444Pre => dsp.pack_bgra4444,
        _ => return None,
    })
}

/// # Safety
///
/// As [`format_packer`].
#[no_mangle]
pub unsafe extern "C" fn format_premultiplier_4444(
    dsp: *const WPDYUVDSP,
    format: c_int,
) -> Premultiply4444Fn {
    let dsp = unsafe { &*dsp };

    if Format::from_raw(format) == Some(Format::Bgra4444Pre) {
        dsp.premultiply_row_4444_swap
    } else {
        dsp.premultiply_row_4444
    }
}

/// `WPD_ANIM_SUBFRAME` from `include/wpd.h`.
const ANIM_SUBFRAME: c_int = 1;

#[no_mangle]
pub extern "C" fn premultiply_after_pack(animation: c_int, anim_mode: c_int) -> c_int {
    c_int::from(animation == 0 || anim_mode == ANIM_SUBFRAME)
}

/// # Safety
///
/// `options` must point to a live `WPDDecoderOptions`.
#[no_mangle]
pub unsafe extern "C" fn options_transform(options: *const WPDDecoderOptions) -> c_int {
    let options = unsafe { &*options };

    c_int::from(
        options.use_cropping != 0 || options.use_scaling != 0 || options.flip != 0,
    )
}

/// # Safety
///
/// As [`options_transform`], and both outputs must be writable.
#[no_mangle]
pub unsafe extern "C" fn scaled_size(
    options: *const WPDDecoderOptions,
    src_width: c_int,
    src_height: c_int,
    width: *mut c_int,
    height: *mut c_int,
) -> c_int {
    let options = unsafe { &*options };

    match image::scaled_size(
        options.scaled_width,
        options.scaled_height,
        src_width,
        src_height,
    ) {
        Ok((w, h)) => {
            unsafe {
                width.write(w);
                height.write(h);
            }
            WPD_OK
        }
        Err(_) => WPD_ERR_TOO_LARGE,
    }
}

/// Turns the image upside down in place by walking each plane backwards, which
/// is the one thing a `WebPImage` view expresses that an owned buffer cannot.
///
/// # Safety
///
/// `view` must point to a live `WebPImage`.
#[no_mangle]
pub unsafe extern "C" fn flip_image(view: *mut WebPImage) {
    let view = unsafe { &mut *view };

    for p in 0..format_of(view).nb_components() {
        let shift = u32::from(p == 1 || p == 2);
        let h = ceil_rshift(view.height, shift);

        view.data[p] =
            view.data[p].wrapping_offset((h - 1) as isize * view.linesize[p] as isize);
        view.linesize[p] = -view.linesize[p];
    }
}

/// Points `view` at the crop rectangle inside `src`, or leaves it a copy when
/// cropping is off.
fn crop_image(
    options: &WPDDecoderOptions,
    src: &WebPImage,
    view: &mut WebPImage,
) -> Result<(), c_int> {
    let format = format_of(src);
    let packed = format.is_packed();

    *view = *src;
    if options.use_cropping == 0 {
        return Ok(());
    }
    let crop = Crop {
        left: options.crop_left,
        top: options.crop_top,
        width: options.crop_width,
        height: options.crop_height,
    };
    let (left, top) = image::crop_origin(&crop, src.width, src.height, packed)
        .map_err(|_| WPD_ERR_INVALID_ARG)?;

    for p in 0..format.nb_components() {
        let shift = u32::from(p == 1 || p == 2);
        let bpp = if packed { format.bpp() } else { 1 };
        let step = (top >> shift) as isize * src.linesize[p] as isize
            + (left >> shift) as isize * bpp as isize;

        view.data[p] = view.data[p].wrapping_offset(step);
    }
    view.width = crop.width;
    view.height = crop.height;
    Ok(())
}

/// One plane through the area rescaler with alpha weighted in, which is what
/// libwebp carries across a scale so a transparent edge does not bleed.
///
/// Each row is built in scratch rather than weighted in place: the decoded
/// image is blended onto by the next animation frame and a still may be
/// exported more than once, so it has to survive the scale unchanged.
#[allow(clippy::too_many_arguments)]
unsafe fn rescale_plane_weighted(
    scratch: &RescaleScratch,
    dst: *mut u8,
    dst_stride: c_int,
    dst_width: c_int,
    dst_height: c_int,
    src: *const u8,
    src_stride: c_int,
    alpha: *const u8,
    alpha_stride: c_int,
    src_width: c_int,
    src_height: c_int,
    channels: c_int,
) {
    let mut r = MaybeUninit::<WPDRescaler>::uninit();
    let mut y = 0;

    unsafe {
        wpd_rescaler_init(
            r.as_mut_ptr(),
            src_width,
            src_height,
            dst,
            dst_width,
            dst_height,
            dst_stride,
            channels,
            scratch.work,
        );
    }
    let r = r.as_mut_ptr();

    while y < src_height {
        let len = src_width as usize * channels as usize;

        unsafe {
            ptr::copy_nonoverlapping(
                src.offset(y as isize * src_stride as isize),
                scratch.row,
                len,
            );
            if alpha.is_null() {
                wpd_premultiply_argb_row(scratch.row, src_width, 0);
            } else {
                wpd_multiply_row(
                    scratch.row,
                    alpha.offset(y as isize * alpha_stride as isize),
                    src_width,
                    0,
                );
            }
            if wpd_rescaler_import(r, 1, scratch.row, 0) != 0 {
                y += 1;
            }
            wpd_rescaler_export(r);
        }
    }
}

/// Scales the way libwebp does: an area rescaler over each plane, with the
/// colour channels premultiplied across it.
///
/// `chroma_full` brings U and V up to the output size instead of half it,
/// which is what libwebp feeds its point converter when a scaled lossy frame
/// is going to a packed format.
unsafe fn scale_image(
    scratch: *mut RescaleScratch,
    dst: &mut WebPImage,
    src: &WebPImage,
    width: c_int,
    height: c_int,
    chroma_full: bool,
    weight_luma: bool,
) -> c_int {
    let format = format_of(src);
    let packed = format.is_packed();
    let bpp = if packed { format.bpp() } else { 1 } as c_int;
    /* An already premultiplied source resamples correctly on its own: the
    weighted average of alpha-weighted colour is what the rescaler outputs
    directly, so weighting it a second time would skew it. */
    let premult = packed && format == Format::Argb && src.premultiplied == 0;

    let ret = unsafe {
        if packed {
            image_alloc_packed(dst, width, height, bpp, src.format)
        } else if chroma_full {
            image_alloc_yuv444(dst, width, height)
        } else {
            image_alloc_yuva(dst, width, height)
        }
    };

    if ret < 0 {
        return ret;
    }
    dst.format = src.format;

    let ret = unsafe { image_scratch_grow(scratch, width, src.width, bpp) };

    if ret < 0 {
        return ret;
    }
    let scratch = unsafe { &*scratch };

    for p in 0..format.nb_components() {
        let chroma = p == 1 || p == 2;
        let shift = u32::from(chroma && !chroma_full);
        let sw = if packed {
            src.width
        } else {
            ceil_rshift(src.width, u32::from(chroma))
        };
        let sh = if packed {
            src.height
        } else {
            ceil_rshift(src.height, u32::from(chroma))
        };
        let dw = ceil_rshift(width, shift);
        let dh = ceil_rshift(height, shift);

        if premult || (weight_luma && p == 0) {
            unsafe {
                rescale_plane_weighted(
                    scratch,
                    dst.data[p],
                    dst.linesize[p],
                    dw,
                    dh,
                    src.data[p],
                    src.linesize[p],
                    if premult { ptr::null() } else { src.data[3] },
                    if premult { 0 } else { src.linesize[3] },
                    sw,
                    sh,
                    bpp,
                );
            }
        } else {
            unsafe {
                wpd_rescale_plane(
                    dst.data[p],
                    dst.linesize[p],
                    dw,
                    dh,
                    src.data[p],
                    src.linesize[p],
                    sw,
                    sh,
                    bpp,
                    scratch.work,
                );
            }
        }
    }

    if premult {
        for y in 0..height {
            unsafe {
                wpd_premultiply_argb_row(
                    dst.data[0].offset(y as isize * dst.linesize[0] as isize),
                    width,
                    1,
                );
            }
        }
    } else if weight_luma {
        for y in 0..height {
            unsafe {
                wpd_multiply_row(
                    dst.data[0].offset(y as isize * dst.linesize[0] as isize),
                    dst.data[3].offset(y as isize * dst.linesize[3] as isize),
                    width,
                    1,
                );
            }
        }
    }
    if !packed && format.nb_components() < 4 {
        unsafe { image_drop_plane(dst, 3) };
        dst.format = Format::Yuv420p as c_int;
    }
    dst.chroma_full = c_int::from(!packed && chroma_full);
    dst.premultiplied = src.premultiplied;
    WPD_OK
}

/// # Safety
///
/// Every pointer must be live, and `result` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn transform_image(
    options: *const WPDDecoderOptions,
    scratch: *mut RescaleScratch,
    scaled: *mut WebPImage,
    src: *const WebPImage,
    view: *mut WebPImage,
    result: *mut *mut WebPImage,
    format: c_int,
) -> c_int {
    let options = unsafe { &*options };
    let src = unsafe { &*src };

    if let Err(e) = crop_image(options, src, unsafe { &mut *view }) {
        return e;
    }
    unsafe { result.write(view) };
    if options.use_scaling == 0 {
        return WPD_OK;
    }
    let view = unsafe { &mut *view };
    let planar = !format_of(src).is_packed();
    let target_packed = Format::from_raw(format).is_some_and(Format::is_packed);
    /* Going to a packed format, libwebp brings U and V all the way up to the
    output size and point-converts; staying planar, it keeps them half size
    and weights the luma by alpha across the rescaler. */
    let chroma_full = planar && target_packed;
    let weight_luma = planar
        && !target_packed
        && Format::from_raw(format) != Some(Format::Yuv420p)
        && format_of(src).nb_components() == 4;

    let (width, height) = match image::scaled_size(
        options.scaled_width,
        options.scaled_height,
        view.width,
        view.height,
    ) {
        Ok(size) => size,
        Err(_) => return WPD_ERR_TOO_LARGE,
    };
    let ret = unsafe {
        scale_image(
            scratch,
            &mut *scaled,
            view,
            width,
            height,
            chroma_full,
            weight_luma,
        )
    };

    if ret < 0 {
        return ret;
    }
    unsafe { result.write(scaled) };
    WPD_OK
}

/// # Safety
///
/// The images must hold the rectangle at both ends.
#[no_mangle]
pub unsafe extern "C" fn blend_argb_region(
    dsp: *const WPDLosslessDSP,
    premultiply: c_int,
    dst: *mut WebPImage,
    src: *const WebPImage,
    r: SubRect,
    dst_x: c_int,
    dst_y: c_int,
) {
    let dsp = unsafe { &*dsp };
    let (dst, src) = unsafe { (&*dst, &*src) };
    let blend = if premultiply != 0 {
        dsp.blend_row_argb_premult
    } else {
        dsp.blend_row_argb
    };

    for y in 0..r.h {
        let src_row = unsafe {
            src.data[0].offset(
                (r.y + y) as isize * src.linesize[0] as isize + r.x as isize * 4,
            )
        };
        let dst_row = unsafe {
            dst.data[0].offset(
                (dst_y + r.y + y) as isize * dst.linesize[0] as isize
                    + (dst_x + r.x) as isize * 4,
            )
        };

        unsafe { blend(dst_row, src_row, r.w) };
    }
}

/// # Safety
///
/// As [`blend_argb_region`].
#[no_mangle]
pub unsafe extern "C" fn copy_argb_region(
    dst: *mut WebPImage,
    src: *const WebPImage,
    r: SubRect,
    dst_x: c_int,
    dst_y: c_int,
) {
    let (dst, src) = unsafe { (&*dst, &*src) };

    for y in 0..r.h {
        let src_row = unsafe {
            src.data[0].offset(
                (r.y + y) as isize * src.linesize[0] as isize + r.x as isize * 4,
            )
        };
        let dst_row = unsafe {
            dst.data[0].offset(
                (dst_y + r.y + y) as isize * dst.linesize[0] as isize
                    + (dst_x + r.x) as isize * 4,
            )
        };

        unsafe { ptr::copy_nonoverlapping(src_row, dst_row, r.w as usize * 4) };
    }
}

/// One alpha row of `img`, starting at column `from`.
///
/// # Safety
///
/// The plane must hold `from + width` samples at row `y`.
unsafe fn alpha_row(img: &WebPImage, y: c_int, from: c_int, width: usize) -> &[u8] {
    unsafe { &img.row(3, y, from as usize + width)[from as usize..] }
}

/// Alpha-blends a YUVA region, chroma first so the luma pass can overwrite the
/// alpha plane it reads.
///
/// # Safety
///
/// As [`blend_argb_region`].
#[no_mangle]
pub unsafe extern "C" fn blend_yuva_region(
    dst: *mut WebPImage,
    src: *const WebPImage,
    r: SubRect,
    dst_x: c_int,
    dst_y: c_int,
) {
    let (dst, src) = unsafe { (&*dst, &*src) };
    let (base_x, base_y) = (dst_x + r.x, dst_y + r.y);
    let width = r.w as usize;
    let chroma = width.div_ceil(2);

    for y in 0..ceil_rshift(r.h, 1) {
        let tile_h = (r.h - y * 2).min(2) as usize;
        /* A block is one or two rows tall, so the pair lives on the stack and
        only the rows the block actually spans are passed on. */
        let src_alpha = unsafe {
            [
                alpha_row(src, r.y + y * 2, r.x, width),
                alpha_row(src, r.y + y * 2 + tile_h as c_int - 1, r.x, width),
            ]
        };
        let dst_alpha = unsafe {
            [
                alpha_row(dst, base_y + y * 2, base_x, width),
                alpha_row(dst, base_y + y * 2 + tile_h as c_int - 1, base_x, width),
            ]
        };
        let src_u = unsafe { src.row(1, (r.y >> 1) + y, (r.x >> 1) as usize + chroma) };
        let src_v = unsafe { src.row(2, (r.y >> 1) + y, (r.x >> 1) as usize + chroma) };
        let dst_u = unsafe {
            dst.row_mut(1, (base_y >> 1) + y, (base_x >> 1) as usize + chroma)
        };
        let dst_v = unsafe {
            dst.row_mut(2, (base_y >> 1) + y, (base_x >> 1) as usize + chroma)
        };

        let (dst_u, dst_v) = (
            &mut dst_u[(base_x >> 1) as usize..],
            &mut dst_v[(base_x >> 1) as usize..],
        );
        let (src_u, src_v) =
            (&src_u[(r.x >> 1) as usize..], &src_v[(r.x >> 1) as usize..]);

        if tile_h == 2 {
            image::blend_row_uv(
                dst_u, dst_v, src_u, src_v, &src_alpha, &dst_alpha, width,
            );
        } else {
            let src_alpha = [src_alpha[0]];
            let dst_alpha = [dst_alpha[0]];

            image::blend_row_uv(
                dst_u, dst_v, src_u, src_v, &src_alpha, &dst_alpha, width,
            );
        }
    }

    for y in 0..r.h {
        let src_y = unsafe { src.row(0, r.y + y, r.x as usize + width) };
        let src_a = unsafe { src.row(3, r.y + y, r.x as usize + width) };
        let dst_y_row = unsafe { dst.row_mut(0, base_y + y, base_x as usize + width) };
        let dst_a_row = unsafe { dst.row_mut(3, base_y + y, base_x as usize + width) };

        image::blend_row_ya(
            &mut dst_y_row[base_x as usize..],
            &mut dst_a_row[base_x as usize..],
            &src_y[r.x as usize..],
            &src_a[r.x as usize..],
        );
    }
}

/// # Safety
///
/// As [`blend_argb_region`].
#[no_mangle]
pub unsafe extern "C" fn copy_yuva_region(
    dst: *mut WebPImage,
    src: *const WebPImage,
    r: SubRect,
    dst_x: c_int,
    dst_y: c_int,
) {
    let (dst, src) = unsafe { (&*dst, &*src) };
    let nb = format_of(src).nb_components();
    let (base_x, base_y) = (dst_x + r.x, dst_y + r.y);

    for comp in 0..nb {
        let shift = u32::from(comp == 1 || comp == 2);
        let len = ceil_rshift(r.w, shift) as usize;
        let mut src_p = unsafe {
            src.data[comp].offset(
                (r.y >> shift) as isize * src.linesize[comp] as isize
                    + (r.x >> shift) as isize,
            )
        };
        let mut dst_p = unsafe {
            dst.data[comp].offset(
                (base_y >> shift) as isize * dst.linesize[comp] as isize
                    + (base_x >> shift) as isize,
            )
        };

        for _ in 0..ceil_rshift(r.h, shift) {
            unsafe {
                ptr::copy_nonoverlapping(src_p, dst_p, len);
                src_p = src_p.offset(src.linesize[comp] as isize);
                dst_p = dst_p.offset(dst.linesize[comp] as isize);
            }
        }
    }

    if nb < 4 {
        for y in 0..r.h {
            let row =
                unsafe { dst.row_mut(3, base_y + y, base_x as usize + r.w as usize) };

            row[base_x as usize..].fill(255);
        }
    }
}

/// # Safety
///
/// Every pointer must be live, and `dst` must not alias `src`: the source is
/// borrowed across the allocation that fills the destination.
#[no_mangle]
pub unsafe extern "C" fn convert_to_packed(
    dsp: *const WPDYUVDSP,
    dst: *mut WebPImage,
    src: *const WebPImage,
    format: c_int,
    no_fancy_upsampling: c_int,
    premultiply_packed: c_int,
) -> c_int {
    let layout = format_layout(format) as usize;
    let target = Format::from_raw(format).unwrap_or(Format::Argb);

    if target.bpp() == 2 {
        return unsafe {
            convert_to_packed_2byte(
                dsp,
                dst,
                src,
                format,
                no_fancy_upsampling,
                premultiply_packed,
            )
        };
    }

    let src_ref = unsafe { &*src };
    let ret = unsafe {
        image_alloc_packed(
            dst,
            src_ref.width,
            src_ref.height,
            target.bpp() as c_int,
            format,
        )
    };

    if ret < 0 {
        return ret;
    }
    let dst_ref = unsafe { &*dst };
    let src_format = format_of(src_ref);

    if src_ref.chroma_full != 0 {
        unsafe {
            wpd_yuv444_to_packed(
                layout as c_int,
                dst_ref.data[0],
                dst_ref.linesize[0] as isize,
                src_ref.data[0],
                src_ref.linesize[0] as isize,
                src_ref.data[1],
                src_ref.data[2],
                src_ref.linesize[1] as isize,
                src_ref.width,
                src_ref.height,
            );
        }
        if src_format.nb_components() == 4
            && layout != LAYOUT_RGB
            && layout != LAYOUT_BGR
        {
            let dsp = unsafe { &*dsp };
            let dispatch = if layout == LAYOUT_ARGB {
                dsp.dispatch_alpha_first
            } else {
                dsp.dispatch_alpha_last
            };

            for y in 0..src_ref.height {
                unsafe {
                    dispatch(
                        dst_ref.data[0]
                            .offset(y as isize * dst_ref.linesize[0] as isize),
                        src_ref.data[3]
                            .offset(y as isize * src_ref.linesize[3] as isize),
                        src_ref.width,
                    );
                }
            }
        }
        return WPD_OK;
    }
    unsafe {
        if no_fancy_upsampling != 0 {
            wpd_yuv420_to_packed_simple(
                dsp,
                layout as c_int,
                dst_ref.data[0],
                dst_ref.linesize[0] as isize,
                src_ref.data[0],
                src_ref.linesize[0] as isize,
                src_ref.data[1],
                src_ref.data[2],
                src_ref.linesize[1] as isize,
                src_ref.data[3],
                src_ref.linesize[3] as isize,
                src_ref.width,
                0,
                src_ref.height,
            );
        } else {
            wpd_yuv420_to_packed(
                dsp,
                layout as c_int,
                dst_ref.data[0],
                dst_ref.linesize[0] as isize,
                src_ref.data[0],
                src_ref.linesize[0] as isize,
                src_ref.data[1],
                src_ref.data[2],
                src_ref.linesize[1] as isize,
                src_ref.data[3],
                src_ref.linesize[3] as isize,
                src_ref.width,
                src_ref.height,
            );
        }
    }
    WPD_OK
}

/// The two-byte formats are packed from ARGB, so a source that is not already
/// ARGB is converted through a scratch image first.
unsafe fn convert_to_packed_2byte(
    dsp: *const WPDYUVDSP,
    dst: *mut WebPImage,
    src: *const WebPImage,
    format: c_int,
    no_fancy_upsampling: c_int,
    premultiply_packed: c_int,
) -> c_int {
    let mut temp: WebPImage = unsafe { std::mem::zeroed() };
    let src_ref = unsafe { &*src };
    let mut argb = src;

    if format_of(src_ref) != Format::Argb {
        let ret = unsafe {
            convert_to_packed(
                dsp,
                &mut temp,
                src,
                Format::Argb as c_int,
                no_fancy_upsampling,
                premultiply_packed,
            )
        };

        if ret < 0 {
            unsafe { image_free(&mut temp) };
            return ret;
        }
        argb = &temp;
    }
    let argb = unsafe { &*argb };
    let mut ret =
        unsafe { image_alloc_packed(dst, argb.width, argb.height, 2, format) };

    if ret >= 0 {
        let dst_ref = unsafe { &*dst };
        let pack = unsafe { format_packer(dsp, format) };

        if let Some(pack) = pack {
            for y in 0..argb.height {
                unsafe {
                    pack(
                        dst_ref.data[0]
                            .offset(y as isize * dst_ref.linesize[0] as isize),
                        argb.data[0].offset(y as isize * argb.linesize[0] as isize),
                        argb.width,
                    );
                }
            }
        }
        if format_is_premultiplied(format) != 0 && premultiply_packed != 0 {
            let premultiply = unsafe { format_premultiplier_4444(dsp, format) };

            for y in 0..argb.height {
                unsafe {
                    premultiply(
                        dst_ref.data[0]
                            .offset(y as isize * dst_ref.linesize[0] as isize),
                        argb.width,
                    );
                }
            }
        }
        ret = WPD_OK;
    }
    unsafe { image_free(&mut temp) };
    ret
}

/// # Safety
///
/// As [`convert_to_packed`].
#[no_mangle]
pub unsafe extern "C" fn convert_to_argb(
    dsp: *const WPDYUVDSP,
    dst: *mut WebPImage,
    src: *const WebPImage,
    no_fancy_upsampling: c_int,
) -> c_int {
    unsafe {
        convert_to_packed(dsp, dst, src, Format::Argb as c_int, no_fancy_upsampling, 0)
    }
}

/// # Safety
///
/// As [`convert_to_packed`], including that `dst` does not alias `src`.
#[no_mangle]
pub unsafe extern "C" fn ensure_yuva_rows(
    dsp: *const WPDYUVDSP,
    dst: *mut WebPImage,
    src: *const WebPImage,
    want_alpha: c_int,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let src_ref = unsafe { &*src };

    if row_start == 0 {
        let ret = unsafe { image_alloc_yuva(dst, src_ref.width, src_ref.height) };

        if ret < 0 {
            return ret;
        }
    }
    let dst_ref = unsafe { &*dst };

    if format_of(src_ref) == Format::Argb {
        unsafe {
            wpd_argb_to_yuva(
                dsp,
                dst_ref.data[0],
                dst_ref.linesize[0] as isize,
                dst_ref.data[1],
                dst_ref.data[2],
                dst_ref.linesize[1] as isize,
                if want_alpha != 0 {
                    dst_ref.data[3]
                } else {
                    ptr::null_mut()
                },
                dst_ref.linesize[3] as isize,
                src_ref.data[0],
                src_ref.linesize[0] as isize,
                src_ref.width,
                row_start,
                row_end,
            );
        }
        if want_alpha == 0 {
            for y in row_start..row_end {
                let row = unsafe { dst_ref.row_mut(3, y, src_ref.width as usize) };

                row.fill(255);
            }
        }
        return WPD_OK;
    }

    let opaque = format_of(src_ref) == Format::Yuv420p;

    for p in 0..4 {
        let shift = u32::from(p == 1 || p == 2);
        let w = ceil_rshift(src_ref.width, shift) as usize;
        let h = ceil_rshift(row_end, shift);

        for y in (row_start >> shift)..h {
            let out = unsafe { dst_ref.row_mut(p, y, w) };

            if p == 3 && opaque {
                out.fill(255);
            } else {
                out.copy_from_slice(unsafe { src_ref.row(p, y, w) });
            }
        }
    }
    WPD_OK
}

/// # Safety
///
/// As [`convert_to_packed`].
#[no_mangle]
pub unsafe extern "C" fn ensure_yuva(
    dsp: *const WPDYUVDSP,
    dst: *mut WebPImage,
    src: *const WebPImage,
    want_alpha: c_int,
) -> c_int {
    let height = unsafe { (*src).height };

    unsafe { ensure_yuva_rows(dsp, dst, src, want_alpha, 0, height) }
}
