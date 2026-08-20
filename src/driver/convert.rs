use super::ANIM_SUBFRAME;
use crate::convert::YuvPlanes;
use crate::dsp::yuv::{RowFn, YuvDsp, LAYOUT_ARGB};
use crate::error::{Error, Result};
use crate::image::{self, ceil_rshift, Crop, Format};
use crate::options::Options;
use crate::picture::{Buffer, Frame};
use crate::rescale::{rescale_plane, rescale_plane_weighted, Scratch};

pub fn format_is_packed(format: i32) -> bool {
    Format::from_raw(format).is_some_and(Format::is_packed)
}

pub fn format_bpp(format: i32) -> usize {
    Format::from_raw(format).map_or(4, Format::bpp)
}

pub fn format_is_premultiplied(format: i32) -> bool {
    Format::from_raw(format).is_some_and(Format::is_premultiplied)
}

pub fn format_valid(format: i32) -> bool {
    Format::from_raw(format).is_some()
}

pub fn format_planes(format: i32) -> usize {
    Format::from_raw(format).map_or(1, Format::nb_components)
}

pub fn format_plane_dims(format: i32, p: usize, w: i32, h: i32) -> (usize, usize) {
    if format_planes(format) == 1 {
        return (w.max(0) as usize * format_bpp(format), h.max(0) as usize);
    }
    let shift = image::plane_shift(p);

    (
        ceil_rshift(w, shift).max(0) as usize,
        ceil_rshift(h, shift).max(0) as usize,
    )
}

pub fn format_layout(format: i32) -> usize {
    Format::from_raw(format).map_or(LAYOUT_ARGB, Format::layout)
}

pub fn format_packer(dsp: &YuvDsp, format: i32) -> Option<RowFn> {
    dsp.packer(Format::from_raw(format)?)
}

pub fn format_premultiplier_4444(dsp: &YuvDsp, format: i32) -> fn(&mut [u8]) {
    dsp.premultiplier_4444(Format::from_raw(format).unwrap_or(Format::Argb))
}

pub fn premultiply_after_pack(animation: bool, anim_mode: i32) -> bool {
    !animation || anim_mode == ANIM_SUBFRAME
}

pub fn scaled_size(
    options: &Options,
    src_width: i32,
    src_height: i32,
) -> Result<(i32, i32)> {
    let (w, h) = options.scale.unwrap_or((0, 0));

    image::scaled_size(w, h, src_width, src_height).map_err(|_| Error::TooLarge)
}

pub fn crop_image<'a>(options: &Options, src: Frame<'a>) -> Result<Frame<'a>> {
    let Some((left, top, width, height)) = options.crop else {
        return Ok(src);
    };
    let crop = Crop {
        left,
        top,
        width,
        height,
    };
    let packed = src.format.is_packed();
    let (left, top) = image::crop_origin(&crop, src.width, src.height, packed)
        .map_err(|_| Error::InvalidArgument)?;

    Ok(src.window(left, top, crop.width, crop.height))
}

fn scale_image(
    dsp: &YuvDsp,
    scratch: &mut Scratch,
    dst: &mut Buffer,
    src: &Frame<'_>,
    width: i32,
    height: i32,
    chroma_full: bool,
    weight_luma: bool,
) -> Result<()> {
    let format = src.format;
    let packed = format.is_packed();
    let bpp = if packed { format.bpp() } else { 1 };
    let premult = packed && format == Format::Argb && !src.premultiplied;
    let alloc = if packed {
        dst.alloc_packed(width, height, bpp, format)
    } else {
        dst.alloc_planar(width, height, !chroma_full)
    };

    alloc?;
    dst.format = Some(format);
    dst.chroma_full = !packed && chroma_full;
    scratch
        .grow(width, src.width, bpp)
        .map_err(|_| Error::TooLarge)?;

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
                dsp,
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
            (dsp.premultiply_argb_row)(out.row(0, y), true);
        }
    } else if weight_luma {
        for y in 0..height {
            let (luma, alpha) = out.row_pair(0, 3, y);

            (dsp.multiply_row)(luma, alpha, true);
        }
    }

    if !packed && format.nb_components() < 4 {
        dst.drop_plane(3);
        dst.format = Some(Format::Yuv420p);
    }
    dst.premultiplied = src.premultiplied;
    Ok(())
}

pub fn transform_image<'a>(
    dsp: &YuvDsp,
    options: &Options,
    scratch: &mut Scratch,
    scaled: &'a mut Buffer,
    src: Frame<'a>,
    format: i32,
) -> Result<Frame<'a>> {
    let view = crop_image(options, src)?;

    if options.scale.is_none() {
        return Ok(view);
    }

    let planar = !src.format.is_packed();
    let target_packed = format_is_packed(format);
    /* libwebp point-converts full-resolution chroma for packed output. */
    let chroma_full = planar && target_packed;
    let weight_luma = planar
        && !target_packed
        && Format::from_raw(format) != Some(Format::Yuv420p)
        && src.format.nb_components() == 4;
    let (width, height) = scaled_size(options, view.width, view.height)?;

    scale_image(
        dsp,
        scratch,
        scaled,
        &view,
        width,
        height,
        chroma_full,
        weight_luma,
    )?;
    Ok(scaled.frame())
}

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
    format: i32,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> Result<()> {
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

    dst.alloc_packed(width, height, target.bpp(), target)?;

    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();
    let plane = &mut out.planes_mut()[0];
    let (w, h) = (width as usize, height as usize);

    if src.chroma_full {
        crate::convert::yuv444_to_packed(layout, plane, &planes, w, h);
        if let (Some(a), Some(dispatch)) = (&planes.a, dsp.alpha_dispatcher(layout)) {
            for y in 0..height {
                dispatch(plane.row_mut(y, 0, 4 * w), a.row(y, 0, w));
            }
        }
        return Ok(());
    }
    if no_fancy_upsampling {
        crate::convert::yuv420_to_packed_simple(dsp, layout, plane, &planes, w, 0, h);
    } else {
        crate::convert::yuv420_to_packed_rows(dsp, layout, plane, &planes, w, h, 0, h);
    }
    Ok(())
}

fn convert_to_packed_2byte(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    format: i32,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
) -> Result<()> {
    let mut temp = Buffer::default();

    if src.format != Format::Argb {
        convert_to_packed(
            dsp,
            &mut temp,
            src,
            Format::Argb as i32,
            no_fancy_upsampling,
            premultiply_packed,
        )?;
    }

    let argb = if temp.is_empty() { *src } else { temp.frame() };
    let target = Format::from_raw(format).unwrap_or(Format::Argb);

    dst.alloc_packed(argb.width, argb.height, 2, target)?;

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
    Ok(())
}

pub fn convert_to_argb(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    no_fancy_upsampling: bool,
) -> Result<()> {
    convert_to_packed(
        dsp,
        dst,
        src,
        Format::Argb as i32,
        no_fancy_upsampling,
        false,
    )
}

pub fn ensure_yuva_rows(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    want_alpha: bool,
    row_start: i32,
    row_end: i32,
) -> Result<()> {
    let (width, height) = (src.width, src.height);

    if row_start == 0 {
        dst.alloc_planar(width, height, true)?;
    }

    let mut out = dst.frame_mut();
    let w = width as usize;

    if src.format == Format::Argb {
        crate::convert::argb_to_yuva(
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
        return Ok(());
    }

    let opaque = src.format == Format::Yuv420p;

    for p in 0..4 {
        let shift = image::plane_shift(p);
        let w = ceil_rshift(width, shift) as usize;

        for y in (row_start >> shift)..ceil_rshift(row_end, shift) {
            if p == 3 && opaque {
                out.row(3, y).fill(255);
            } else {
                out.row(p, y).copy_from_slice(src.plane[p].row(y, 0, w));
            }
        }
    }
    Ok(())
}

pub fn ensure_yuva(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    want_alpha: bool,
) -> Result<()> {
    let height = src.height;

    ensure_yuva_rows(dsp, dst, src, want_alpha, 0, height)
}
