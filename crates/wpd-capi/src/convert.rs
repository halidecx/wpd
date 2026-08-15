//! Format policy, cropping, scaling and format conversion.
//!
//! The decision-making — which format packs how, what a crop resolves to, what
//! a scale rounds to — is [`wpd::image`]; the row walking is [`wpd::convert`]
//! and [`wpd::rescale`]. What is here is the picture-level plumbing: every
//! source is a borrowed [`Frame`] and every destination a [`Buffer`] the
//! decoder owns, so a crop is a `window` and a flip a reading order rather
//! than arithmetic on a pointer.

use std::ffi::c_int;

use wpd::convert::YuvPlanes;
use wpd::dsp::yuv::{RowFn, YuvDsp, LAYOUT_ARGB};
use wpd::image::{self, ceil_rshift, Crop, Format};
use wpd::picture::{Buffer, Frame};
use wpd::rescale::{rescale_plane, rescale_plane_weighted, Scratch};

use crate::vp8::{WPD_ENOMEM, WPD_ERROR_TOO_LARGE};

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

/// What the allocators report, as the rest of the decoder's statuses.
pub(crate) fn alloc_status(e: wpd::error::Error) -> c_int {
    match e {
        wpd::error::Error::NoMemory => WPD_ENOMEM,
        _ => WPD_ERROR_TOO_LARGE,
    }
}

/// The crop rectangle inside `src`, or `src` itself when cropping is off.
pub fn crop_image<'a>(
    options: &WPDDecoderOptions,
    src: Frame<'a>,
) -> Result<Frame<'a>, c_int> {
    if options.use_cropping == 0 {
        return Ok(src);
    }
    let crop = Crop {
        left: options.crop_left,
        top: options.crop_top,
        width: options.crop_width,
        height: options.crop_height,
    };
    let packed = src.format.is_packed();
    let (left, top) = image::crop_origin(&crop, src.width, src.height, packed)
        .map_err(|_| WPD_ERR_INVALID_ARG)?;

    Ok(src.window(left, top, crop.width, crop.height))
}

/// Scales the way libwebp does: an area rescaler over each plane, with the
/// colour channels premultiplied across it.
///
/// `chroma_full` brings U and V up to the output size instead of half it,
/// which is what libwebp feeds its point converter when a scaled lossy frame
/// is going to a packed format.
fn scale_image(
    scratch: &mut Scratch,
    dst: &mut Buffer,
    src: &Frame<'_>,
    width: c_int,
    height: c_int,
    chroma_full: bool,
    weight_luma: bool,
) -> c_int {
    let format = src.format;
    let packed = format.is_packed();
    let bpp = if packed { format.bpp() } else { 1 };
    /* An already premultiplied source resamples correctly on its own: the
    weighted average of alpha-weighted colour is what the rescaler outputs
    directly, so weighting it a second time would skew it. */
    let premult = packed && format == Format::Argb && !src.premultiplied;
    let alloc = if packed {
        dst.alloc_packed(width, height, bpp, format)
    } else {
        dst.alloc_planar(width, height, !chroma_full)
    };

    if let Err(e) = alloc {
        return alloc_status(e);
    }
    dst.format = Some(format);
    dst.chroma_full = !packed && chroma_full;
    if scratch.grow(width, src.width, bpp).is_err() {
        return WPD_ERR_TOO_LARGE;
    }

    let mut out = dst.frame_mut();

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
                &src.plane[p],
                (!premult).then_some(&src.plane[3]),
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
                &src.plane[p],
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

    if !packed && format.nb_components() < 4 {
        dst.drop_plane(3);
        dst.format = Some(Format::Yuv420p);
    }
    dst.premultiplied = src.premultiplied;
    WPD_OK
}

/// Resolves the crop and the scale, returning the picture the output should be
/// read from — a window on `src`, or the whole of `scaled`.
pub fn transform_image<'a>(
    options: &WPDDecoderOptions,
    scratch: &mut Scratch,
    scaled: &'a mut Buffer,
    src: Frame<'a>,
    format: c_int,
) -> Result<Frame<'a>, c_int> {
    let view = crop_image(options, src)?;

    if options.use_scaling == 0 {
        return Ok(view);
    }

    let planar = !src.format.is_packed();
    let target_packed = format_is_packed(format);
    /* Going to a packed format, libwebp brings U and V all the way up to the
    output size and point-converts; staying planar, it keeps them half size
    and weights the luma by alpha across the rescaler. */
    let chroma_full = planar && target_packed;
    let weight_luma = planar
        && !target_packed
        && Format::from_raw(format) != Some(Format::Yuv420p)
        && src.format.nb_components() == 4;
    let (width, height) = scaled_size(options, view.width, view.height)?;
    let ret = scale_image(
        scratch,
        scaled,
        &view,
        width,
        height,
        chroma_full,
        weight_luma,
    );

    if ret < 0 {
        return Err(ret);
    }
    Ok(scaled.frame())
}

/// The planar source of a conversion, as the row drivers take it.
pub fn yuv_planes<'a>(src: &Frame<'a>) -> YuvPlanes<'a> {
    YuvPlanes {
        y: src.plane[0],
        u: src.plane[1],
        v: src.plane[2],
        a: (src.format.nb_components() == 4).then_some(src.plane[3]),
    }
}

pub fn convert_to_packed(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    format: c_int,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> c_int {
    let layout = format_layout(format);
    let target = Format::from_raw(format).unwrap_or(Format::Argb);

    if target.bpp() == 2 {
        return convert_to_packed_2byte(
            dsp,
            dst,
            src,
            format,
            no_fancy_upsampling,
            premultiply_packed,
        );
    }

    let (width, height) = (src.width, src.height);

    if let Err(e) = dst.alloc_packed(width, height, target.bpp(), target) {
        return alloc_status(e);
    }

    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();
    let plane = &mut out.planes_mut()[0];
    let (w, h) = (width as usize, height as usize);

    if src.chroma_full {
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
fn convert_to_packed_2byte(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    format: c_int,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> c_int {
    let mut temp = Buffer::default();

    if src.format != Format::Argb {
        let ret = convert_to_packed(
            dsp,
            &mut temp,
            src,
            Format::Argb as c_int,
            no_fancy_upsampling,
            premultiply_packed,
        );

        if ret < 0 {
            return ret;
        }
    }

    let argb = if temp.is_empty() { *src } else { temp.frame() };
    let target = Format::from_raw(format).unwrap_or(Format::Argb);

    if let Err(e) = dst.alloc_packed(argb.width, argb.height, 2, target) {
        return alloc_status(e);
    }

    let mut out = dst.frame_mut();

    if let Some(pack) = format_packer(dsp, format) {
        for y in 0..argb.height {
            pack(out.row(0, y), argb.row(0, y));
        }
    }
    if format_is_premultiplied(format) && premultiply_packed {
        let premultiply = format_premultiplier_4444(dsp, format);

        for y in 0..argb.height {
            premultiply(out.row(0, y));
        }
    }
    WPD_OK
}

pub fn convert_to_argb(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    no_fancy_upsampling: bool,
) -> c_int {
    convert_to_packed(
        dsp,
        dst,
        src,
        Format::Argb as c_int,
        no_fancy_upsampling,
        false,
    )
}

pub fn ensure_yuva_rows(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    want_alpha: bool,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let (width, height) = (src.width, src.height);

    if row_start == 0 {
        if let Err(e) = dst.alloc_planar(width, height, true) {
            return alloc_status(e);
        }
    }

    let mut out = dst.frame_mut();
    let w = width as usize;

    if src.format == Format::Argb {
        wpd::convert::argb_to_yuva(
            dsp,
            out.planes_mut(),
            &src.plane[0],
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

    let opaque = src.format == Format::Yuv420p;

    for p in 0..4 {
        let shift = u32::from(p == 1 || p == 2);
        let w = ceil_rshift(width, shift) as usize;

        for y in (row_start >> shift)..ceil_rshift(row_end, shift) {
            if p == 3 && opaque {
                out.row(3, y).fill(255);
            } else {
                out.row(p, y).copy_from_slice(src.plane[p].row(y, 0, w));
            }
        }
    }
    WPD_OK
}

pub fn ensure_yuva(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    want_alpha: bool,
) -> c_int {
    let height = src.height;

    ensure_yuva_rows(dsp, dst, src, want_alpha, 0, height)
}
