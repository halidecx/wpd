use super::ANIM_SUBFRAME;
use crate::convert::{Sampling, YuvPlanes};
use crate::dsp::rescale::RescaleDsp;
use crate::dsp::yuv::{RowFn, YuvDsp, LAYOUT_ARGB};
use crate::error::{Error, Result};
use crate::image::{self, ceil_rshift, Crop, Format};
use crate::options::Options;
use crate::picture::{Buffer, Frame, PlaneMut, PlaneRef};
use crate::rescale::{rescale_plane, rescale_plane_weighted, Scratch, Scratches};

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

    let (w, h) =
        image::scaled_size(w, h, src_width, src_height).map_err(|_| Error::TooLarge)?;

    if !options.fits(w, h) {
        return Err(Error::TooLarge);
    }
    Ok((w, h))
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

    src.window(left, top, crop.width, crop.height)
}

/// One plane's share of a rescale. The rescaler accumulates down the rows and
/// along x, so neither axis can be cut without reproducing that state; planes
/// carry nothing between them, so they are the axis that is free.
struct PlaneScale<'p, 'f> {
    dst: &'p mut PlaneMut<'f>,
    scratch: &'p mut Scratch,
    src: PlaneRef<'f>,
    alpha: Option<PlaneRef<'f>>,
    dst_size: (i32, i32),
    src_size: (i32, i32),
    weighted: bool,
    bpp: usize,
}

fn scale_plane(dsp: &YuvDsp, rdsp: &RescaleDsp, p: &mut PlaneScale<'_, '_>) {
    let (dw, dh) = p.dst_size;
    let (sw, sh) = p.src_size;

    if p.weighted {
        rescale_plane_weighted(
            dsp,
            rdsp,
            p.scratch,
            p.dst,
            dw,
            dh,
            &p.src,
            p.alpha.as_ref(),
            sw,
            sh,
            p.bpp,
        );
    } else {
        rescale_plane(
            rdsp,
            p.scratch.work_mut(),
            p.dst,
            dw,
            dh,
            &p.src,
            sw,
            sh,
            p.bpp,
        );
    }
}

/// The least rescaling worth putting on a thread of its own, in source pixels.
/// The rescaler walks every source row at roughly 0.2ns a pixel, so below this
/// the spawns cost more than the planes they take away: a 35x67 frame scaled
/// down measures 1.69x faster for not splitting, and 480x310 upwards is
/// unchanged. Counting the source rather than the target is what keeps a large
/// image taken down to a thumbnail on threads of its own.
const MIN_RESCALE_PIXELS: usize = 96 * 1024;

#[allow(clippy::too_many_arguments)]
fn scale_image(
    dsp: &YuvDsp,
    rdsp: &RescaleDsp,
    scratch: &mut Scratches,
    dst: &mut Buffer,
    src: &Frame<'_>,
    width: i32,
    height: i32,
    chroma_full: bool,
    weight_luma: bool,
    threads: usize,
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

    let nb = format.nb_components();
    let mut out = dst.frame_mut();
    let mut work = Vec::with_capacity(nb);

    for (p, (plane, one)) in out.planes_mut()[..nb]
        .iter_mut()
        .zip(scratch[..nb].iter_mut())
        .enumerate()
    {
        let chroma = p == 1 || p == 2;
        let shift = u32::from(chroma && !chroma_full);
        let (sw, sh) = if packed {
            (src.width, src.height)
        } else {
            (
                ceil_rshift(src.width, u32::from(chroma && !src.chroma_full)),
                ceil_rshift(src.height, u32::from(chroma && !src.chroma_full)),
            )
        };
        let weighted = premult || (weight_luma && p == 0);
        let dst_size = (ceil_rshift(width, shift), ceil_rshift(height, shift));

        /* Each plane accumulates over its own width, so a subsampled one asks
         * for half of what the luma does rather than being grown to match it.
         */
        one.grow(dst_size.0, sw, bpp).map_err(|_| Error::TooLarge)?;

        work.push(PlaneScale {
            dst: plane,
            scratch: one,
            src: src.plane[p],
            alpha: (!premult).then_some(src.plane[3]),
            dst_size,
            src_size: (sw, sh),
            weighted,
            bpp,
        });
    }

    /* A packed image is one plane, so it is done here rather than paying for
     * a scope it cannot fill, and a small one is scaled here whatever its
     * plane count. */
    let threads = crate::task::pieces(
        (src.width as usize).saturating_mul(src.height as usize),
        MIN_RESCALE_PIXELS,
        threads,
    );

    crate::task::for_each(threads, &mut work, |p| scale_plane(dsp, rdsp, p));

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

#[allow(clippy::too_many_arguments)]
pub fn transform_image<'a>(
    dsp: &YuvDsp,
    rdsp: &RescaleDsp,
    options: &Options,
    scratch: &mut Scratches,
    scaled: &'a mut Buffer,
    src: Frame<'a>,
    format: i32,
    threads: usize,
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
        rdsp,
        scratch,
        scaled,
        &view,
        width,
        height,
        chroma_full,
        weight_luma,
        threads,
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

#[allow(clippy::too_many_arguments)]
pub fn convert_to_packed(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    format: i32,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
    threads: usize,
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
            threads,
        );
    }

    let (width, height) = (src.width, src.height);

    dst.alloc_packed(width, height, target.bpp(), target)?;

    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();
    let plane = &mut out.planes_mut()[0];
    let (w, h) = (width as usize, height as usize);

    if src.chroma_full {
        crate::convert::yuv444_to_packed(dsp, layout, plane, &planes, w, 0, h, threads);
        return Ok(());
    }
    if no_fancy_upsampling {
        crate::convert::yuv420_to_packed_simple(
            dsp, layout, plane, &planes, w, 0, h, threads,
        );
    } else {
        crate::convert::yuv420_to_packed_rows(
            dsp, layout, plane, &planes, w, h, 0, h, threads,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_to_packed_2byte(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    format: i32,
    no_fancy_upsampling: bool,
    premultiply_packed: bool,
    threads: usize,
) -> Result<()> {
    let target = Format::from_raw(format).unwrap_or(Format::Argb);
    let Some(pack) = format_packer(dsp, format) else {
        return Err(Error::Unsupported);
    };
    let premultiply = (format_is_premultiplied(format) && premultiply_packed)
        .then(|| format_premultiplier_4444(dsp, format));

    dst.alloc_packed(src.width, src.height, 2, target)?;

    let mut out = dst.frame_mut();

    if src.format == Format::Argb {
        for y in 0..src.height {
            let row = out.row(0, y);

            pack(row, src.row(0, y));
            if let Some(premultiply) = premultiply {
                premultiply(row);
            }
        }
        return Ok(());
    }

    let sampling = if src.chroma_full {
        Sampling::Full
    } else if no_fancy_upsampling {
        Sampling::Simple
    } else {
        Sampling::Fancy
    };
    let planes = yuv_planes(src);

    crate::convert::yuv_to_packed_2byte(
        dsp,
        &mut out.planes_mut()[0],
        &planes,
        src.width as usize,
        src.height as usize,
        0,
        src.height as usize,
        sampling,
        pack,
        premultiply,
        threads,
    );
    Ok(())
}

pub fn convert_to_argb(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    no_fancy_upsampling: bool,
    threads: usize,
) -> Result<()> {
    convert_to_packed(
        dsp,
        dst,
        src,
        Format::Argb as i32,
        no_fancy_upsampling,
        false,
        threads,
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
        for y in (row_start >> shift)..ceil_rshift(row_end, shift) {
            if p == 3 && opaque {
                out.row(3, y).fill(255);
            } else if shift != 0 && src.chroma_full {
                let top = src.row(p, 2 * y);
                let bottom = src.row(p, (2 * y + 1).min(height - 1));
                for (x, dst) in out.row(p, y).iter_mut().enumerate() {
                    let left = 2 * x;
                    let right = (left + 1).min(width as usize - 1);
                    *dst = ((u16::from(top[left])
                        + u16::from(top[right])
                        + u16::from(bottom[left])
                        + u16::from(bottom[right])
                        + 2)
                        / 4) as u8;
                }
            } else {
                out.row(p, y).copy_from_slice(src.row(p, y));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_chroma_is_preserved_by_scaling_and_averaged_for_planar_output() {
        let dsp = YuvDsp::default();
        let mut src = Buffer::default();
        let mut scaled = Buffer::default();
        let mut planar = Buffer::default();
        let mut scratch = Scratches::default();

        src.alloc_planar(3, 3, false).unwrap();
        for p in 0..4 {
            for y in 0..3 {
                src.frame_mut().row(p, y).copy_from_slice(&[
                    (10 * y) as u8,
                    (10 * y + 2) as u8,
                    (10 * y + 4) as u8,
                ]);
            }
        }
        scale_image(
            &dsp,
            &RescaleDsp::default(),
            &mut scratch,
            &mut scaled,
            &src.frame(),
            3,
            3,
            true,
            false,
            1,
        )
        .unwrap();
        for p in 0..4 {
            for y in 0..3 {
                assert_eq!(scaled.frame().row(p, y), src.frame().row(p, y));
            }
        }
        ensure_yuva(&dsp, &mut planar, &src.frame(), true).unwrap();
        for p in [1, 2] {
            assert_eq!(planar.frame().row(p, 0), &[6, 9]);
            assert_eq!(planar.frame().row(p, 1), &[21, 24]);
        }
    }
}
