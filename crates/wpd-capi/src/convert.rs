//! Format policy, cropping, scaling and format conversion.
//!
//! The decision-making — which format packs how, what a crop resolves to, what
//! a scale rounds to — is [`wpd::image`]; the row walking is [`wpd::convert`]
//! and [`wpd::rescale`]. What is left here is the picture-level plumbing, and
//! it is still written against `WebPImage`, because a crop is an offset into
//! `data[p]` and a flip a negative `linesize[p]` and neither survives contact
//! with an owning type. Each entry point bridges into a `Frame` or `FrameMut`
//! for the row work.

use std::ffi::c_int;

use wpd::convert::YuvPlanes;
use wpd::dsp::yuv::{RowFn, YuvDsp, LAYOUT_ARGB};
use wpd::image::{self, ceil_rshift, Crop, Format};
use wpd::rescale::{rescale_plane, rescale_plane_weighted, Scratch};

use crate::image::{
    image_alloc_packed, image_alloc_yuv444, image_alloc_yuva, image_drop_plane,
    image_free, WebPImage,
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

/// The format an image claims, or ARGB when the field holds something no
/// version of the enum defines. Nothing downstream can act on a format it does
/// not know, and the four-byte packed case is the one the decoder produces.
fn format_of(img: &WebPImage) -> Format {
    img.format().unwrap_or(Format::Argb)
}

pub fn format_is_packed(format: c_int) -> bool {
    Format::from_raw(format).is_some_and(Format::is_packed)
}

pub fn format_bpp(format: c_int) -> usize {
    Format::from_raw(format).map_or(4, Format::bpp)
}

pub fn format_is_premultiplied(format: c_int) -> bool {
    Format::from_raw(format).is_some_and(Format::is_premultiplied)
}

pub fn format_valid(format: c_int) -> bool {
    Format::from_raw(format).is_some()
}

pub fn format_layout(format: c_int) -> usize {
    Format::from_raw(format).map_or(LAYOUT_ARGB, Format::layout)
}

pub fn format_packer(dsp: &YuvDsp, format: c_int) -> Option<RowFn> {
    dsp.packer(Format::from_raw(format)?)
}

pub fn format_premultiplier_4444(dsp: &YuvDsp, format: c_int) -> fn(&mut [u8]) {
    dsp.premultiplier_4444(Format::from_raw(format).unwrap_or(Format::Argb))
}

/// `WPD_ANIM_SUBFRAME` from `include/wpd.h`.
const ANIM_SUBFRAME: c_int = 1;

pub fn premultiply_after_pack(animation: bool, anim_mode: c_int) -> bool {
    !animation || anim_mode == ANIM_SUBFRAME
}

pub fn options_transform(options: &WPDDecoderOptions) -> bool {
    options.use_cropping != 0 || options.use_scaling != 0 || options.flip != 0
}

pub fn scaled_size(
    options: &WPDDecoderOptions,
    src_width: c_int,
    src_height: c_int,
) -> Result<(c_int, c_int), c_int> {
    image::scaled_size(
        options.scaled_width,
        options.scaled_height,
        src_width,
        src_height,
    )
    .map_err(|_| WPD_ERR_TOO_LARGE)
}

/// Turns the image upside down in place by walking each plane backwards, which
/// is the one thing a `WebPImage` view expresses that an owned buffer cannot.
pub fn flip_image(view: &mut WebPImage) {
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

/// Scales the way libwebp does: an area rescaler over each plane, with the
/// colour channels premultiplied across it.
///
/// `chroma_full` brings U and V up to the output size instead of half it,
/// which is what libwebp feeds its point converter when a scaled lossy frame
/// is going to a packed format.
///
/// # Safety
///
/// `dst` must not alias `src`: the source is borrowed across the allocation
/// that fills the destination.
unsafe fn scale_image(
    scratch: &mut Scratch,
    dst: &mut WebPImage,
    src: &WebPImage,
    width: c_int,
    height: c_int,
    chroma_full: bool,
    weight_luma: bool,
) -> c_int {
    let format = format_of(src);
    let packed = format.is_packed();
    let bpp = if packed { format.bpp() } else { 1 };
    /* An already premultiplied source resamples correctly on its own: the
    weighted average of alpha-weighted colour is what the rescaler outputs
    directly, so weighting it a second time would skew it. */
    let premult = packed && format == Format::Argb && src.premultiplied == 0;

    let ret = unsafe {
        if packed {
            image_alloc_packed(dst, width, height, bpp as c_int, src.format)
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
    dst.chroma_full = c_int::from(!packed && chroma_full);
    if scratch.grow(width, src.width, bpp).is_err() {
        return WPD_ERR_TOO_LARGE;
    }

    {
        let inp = unsafe { src.frame() };
        let mut out = unsafe { dst.frame_mut() };

        for p in 0..format.nb_components() {
            let chroma = p == 1 || p == 2;
            let shift = u32::from(chroma && !chroma_full);
            let (sw, sh) = if packed {
                (src.width, src.height)
            } else {
                (
                    ceil_rshift(src.width, u32::from(chroma)),
                    ceil_rshift(src.height, u32::from(chroma)),
                )
            };
            let dw = ceil_rshift(width, shift);
            let dh = ceil_rshift(height, shift);
            let plane = &mut out.planes_mut()[p];

            if premult || (weight_luma && p == 0) {
                rescale_plane_weighted(
                    scratch,
                    plane,
                    dw,
                    dh,
                    &inp.plane[p],
                    (!premult).then_some(&inp.plane[3]),
                    sw,
                    sh,
                    bpp,
                );
            } else {
                rescale_plane(
                    scratch.work_mut(),
                    plane,
                    dw,
                    dh,
                    &inp.plane[p],
                    sw,
                    sh,
                    bpp,
                );
            }
        }

        if premult {
            for y in 0..height {
                wpd::rescale::premultiply_argb_row(out.row(0, y), true);
            }
        } else if weight_luma {
            for y in 0..height {
                let (luma, alpha) = out.row_pair(0, 3, y);

                wpd::rescale::multiply_row(luma, alpha, true);
            }
        }
    }

    if !packed && format.nb_components() < 4 {
        unsafe { image_drop_plane(dst, 3) };
        dst.format = Format::Yuv420p as c_int;
    }
    dst.premultiplied = src.premultiplied;
    WPD_OK
}

/// Resolves the crop and the scale, leaving `result` pointing at whichever of
/// `view` and `scaled` the output should be read from.
///
/// # Safety
///
/// `scaled` must not alias `src`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn transform_image<'a>(
    options: &WPDDecoderOptions,
    scratch: &mut Scratch,
    scaled: &'a mut WebPImage,
    src: &WebPImage,
    view: &'a mut WebPImage,
    format: c_int,
) -> Result<&'a WebPImage, c_int> {
    crop_image(options, src, view)?;
    if options.use_scaling == 0 {
        return Ok(view);
    }

    let planar = !format_of(src).is_packed();
    let target_packed = format_is_packed(format);
    /* Going to a packed format, libwebp brings U and V all the way up to the
    output size and point-converts; staying planar, it keeps them half size
    and weights the luma by alpha across the rescaler. */
    let chroma_full = planar && target_packed;
    let weight_luma = planar
        && !target_packed
        && Format::from_raw(format) != Some(Format::Yuv420p)
        && format_of(src).nb_components() == 4;
    let (width, height) = scaled_size(options, view.width, view.height)?;
    let ret = unsafe {
        scale_image(
            scratch,
            scaled,
            view,
            width,
            height,
            chroma_full,
            weight_luma,
        )
    };

    if ret < 0 {
        return Err(ret);
    }
    Ok(scaled)
}

/// The planar source of a conversion, as the row drivers take it.
///
/// # Safety
///
/// `img` must be a live planar `WebPImage` read the way its geometry says.
unsafe fn yuv_planes(img: &WebPImage, alpha: bool) -> YuvPlanes<'_> {
    let f = unsafe { img.frame() };

    YuvPlanes {
        y: f.plane[0],
        u: f.plane[1],
        v: f.plane[2],
        a: alpha.then_some(f.plane[3]),
    }
}

/// # Safety
///
/// `dst` must not alias `src`: the source is borrowed across the allocation
/// that fills the destination.
pub unsafe fn convert_to_packed(
    dsp: &YuvDsp,
    dst: &mut WebPImage,
    src: &WebPImage,
    format: c_int,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> c_int {
    let layout = format_layout(format);
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

    let (width, height) = (src.width, src.height);
    let ret = unsafe {
        image_alloc_packed(dst, width, height, target.bpp() as c_int, format)
    };

    if ret < 0 {
        return ret;
    }

    let alpha = format_of(src).nb_components() == 4;
    let planes = unsafe { yuv_planes(src, alpha) };
    let mut out = unsafe { dst.frame_mut() };
    let plane = &mut out.planes_mut()[0];
    let (w, h) = (width as usize, height as usize);

    if src.chroma_full != 0 {
        wpd::convert::yuv444_to_packed(layout, plane, &planes, w, h);
        if let (Some(a), Some(dispatch)) = (&planes.a, dsp.alpha_dispatcher(layout)) {
            for y in 0..height {
                dispatch(plane.row_mut(y, 0, 4 * w), a.row(y, 0, w));
            }
        }
        return WPD_OK;
    }
    if no_fancy_upsampling {
        wpd::convert::yuv420_to_packed_simple(dsp, layout, plane, &planes, w, 0, h);
    } else {
        wpd::convert::yuv420_to_packed_rows(dsp, layout, plane, &planes, w, h, 0, h);
    }
    WPD_OK
}

/// The two-byte formats are packed from ARGB, so a source that is not already
/// ARGB is converted through a scratch image first.
///
/// # Safety
///
/// As [`convert_to_packed`].
unsafe fn convert_to_packed_2byte(
    dsp: &YuvDsp,
    dst: &mut WebPImage,
    src: &WebPImage,
    format: c_int,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> c_int {
    let mut temp = WebPImage::empty();
    let mut ret = WPD_OK;

    if format_of(src) != Format::Argb {
        ret = unsafe {
            convert_to_packed(
                dsp,
                &mut temp,
                src,
                Format::Argb as c_int,
                no_fancy_upsampling,
                premultiply_packed,
            )
        };
    }
    if ret >= 0 {
        let argb = if temp.data[0].is_null() { src } else { &temp };

        ret = unsafe { image_alloc_packed(dst, argb.width, argb.height, 2, format) };
        if ret >= 0 {
            let inp = unsafe { argb.frame() };
            let mut out = unsafe { dst.frame_mut() };

            if let Some(pack) = format_packer(dsp, format) {
                for y in 0..argb.height {
                    pack(out.row(0, y), inp.row(0, y));
                }
            }
            if format_is_premultiplied(format) && premultiply_packed {
                let premultiply = format_premultiplier_4444(dsp, format);

                for y in 0..argb.height {
                    premultiply(out.row(0, y));
                }
            }
            ret = WPD_OK;
        }
    }
    unsafe { image_free(&mut temp) };
    ret
}

/// # Safety
///
/// As [`convert_to_packed`].
pub unsafe fn convert_to_argb(
    dsp: &YuvDsp,
    dst: &mut WebPImage,
    src: &WebPImage,
    no_fancy_upsampling: bool,
) -> c_int {
    unsafe {
        convert_to_packed(
            dsp,
            dst,
            src,
            Format::Argb as c_int,
            no_fancy_upsampling,
            false,
        )
    }
}

/// # Safety
///
/// As [`convert_to_packed`].
pub unsafe fn ensure_yuva_rows(
    dsp: &YuvDsp,
    dst: &mut WebPImage,
    src: &WebPImage,
    want_alpha: bool,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let (width, height) = (src.width, src.height);

    if row_start == 0 {
        let ret = unsafe { image_alloc_yuva(dst, width, height) };

        if ret < 0 {
            return ret;
        }
    }

    let inp = unsafe { src.frame() };
    let mut out = unsafe { dst.frame_mut() };
    let w = width as usize;

    if format_of(src) == Format::Argb {
        wpd::convert::argb_to_yuva(
            dsp,
            out.planes_mut(),
            &inp.plane[0],
            want_alpha,
            w,
            row_start,
            row_end,
        );
        if !want_alpha {
            for y in row_start..row_end {
                out.row(3, y).fill(255);
            }
        }
        return WPD_OK;
    }

    let opaque = format_of(src) == Format::Yuv420p;

    for p in 0..4 {
        let shift = u32::from(p == 1 || p == 2);
        let w = ceil_rshift(width, shift) as usize;

        for y in (row_start >> shift)..ceil_rshift(row_end, shift) {
            if p == 3 && opaque {
                out.row(3, y).fill(255);
            } else {
                out.row(p, y).copy_from_slice(inp.plane[p].row(y, 0, w));
            }
        }
    }
    WPD_OK
}

/// # Safety
///
/// As [`convert_to_packed`].
pub unsafe fn ensure_yuva(
    dsp: &YuvDsp,
    dst: &mut WebPImage,
    src: &WebPImage,
    want_alpha: bool,
) -> c_int {
    let height = src.height;

    unsafe { ensure_yuva_rows(dsp, dst, src, want_alpha, 0, height) }
}
