//! Handing a decoded picture out.
//!
//! This is the last place a picture passes through, and it is all plumbing:
//! which conversion the output format needs, which rows have already been
//! done, and whether the bytes go into the decoder's memory or the caller's.
//! The arithmetic that a caller's buffer sizes drive is [`crate::image`]; what
//! is here walks rows of a [`Frame`] that may already be a crop or a flip.
//!
//! The scratch an export writes through arrives as borrows of the decoder's
//! own fields, which is why there are two sets of them. A whole-frame export
//! may be handed the conversion buffer as its *source* — that is how sub-frame
//! mode returns a converted animation frame — while the resumable row exports
//! write into it. The C passed one struct of pointers to both and the rule
//! that they are never the same buffer at the same time was written nowhere.
//!
//! Nothing here knows where a caller's own memory is. A destination that is
//! not the decoder's own arrives as a [`RowSink`], which hands back one row at
//! a time; what those rows are made of belongs to whoever supplied them.

use crate::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};
use crate::dsp::yuv::LAYOUT_ARGB;
use crate::image::Format;

use super::convert::{
    convert_to_packed, ensure_yuva, ensure_yuva_rows, format_bpp, format_is_packed,
    format_layout, format_packer, format_premultiplier_4444, premultiply_after_pack,
    transform_image, yuv_planes,
};
use crate::dsp::yuv::{RowFn, YuvDsp};
use crate::error::{Error, Result};
use crate::handout::{Handout, Pixels, RowSink};
use crate::options::Options;
use crate::picture::{Buffer, Frame};
use crate::rescale::Scratch;

/// What the decoder was asked for, gathered at the call rather than reached
/// for, so nothing here can read a field that has moved on since.
pub struct ExportSettings {
    pub out_format: i32,
    pub premultiply: bool,
    pub animation: bool,
    pub anim_mode: i32,
    pub duration: i32,
    pub pos_x: i32,
    pub pos_y: i32,
    pub anmf_flags: i32,
    pub has_alpha: bool,
    pub timestamp: i64,
}

/// The scratch a whole-frame export writes through.
///
/// The conversion buffer is not here: a whole-frame export may be handed it as
/// its source.
pub struct ExportTargets<'a> {
    pub dsp: &'a YuvDsp,
    pub options: &'a Options,
    pub rescale: &'a mut Scratch,
    pub transformed: &'a mut Buffer,
    pub output: &'a mut Buffer,
    /// Where the rows go when the caller supplied its own memory. `None` is
    /// what says the decoder hands out its own instead, which is why there is
    /// no separate flag: the destination and the choice are one value.
    pub ext: Option<&'a mut (dyn RowSink + 'static)>,
}

/// The scratch a resumable row export writes through, plus how far it has got.
///
/// These paths read the codec's own picture and convert into the decoder's
/// buffers, so the conversion buffer is theirs to write.
pub struct RowTargets<'a> {
    pub dsp: &'a YuvDsp,
    pub options: &'a Options,
    pub output: &'a mut Buffer,
    pub converted: &'a mut Buffer,
    pub ext: Option<&'a mut (dyn RowSink + 'static)>,
    pub converted_rows: &'a mut i32,
    pub converted_format: &'a mut i32,
}

/// The planes a format hands out: three or four for planar, one for packed.
fn frame_planes(format: i32) -> usize {
    match Format::from_raw(format) {
        Some(Format::Yuva420p) => 4,
        Some(Format::Yuv420p) => 3,
        _ => 1,
    }
}

/// Describes `img` as the picture a decode hands back.
pub(crate) fn export_frame(
    set: &ExportSettings,
    img: &Frame<'_>,
    format: i32,
    out: &mut Handout<'_>,
) {
    let flags = set.anmf_flags as u8;

    out.pixels = Pixels::None;
    out.format = Format::from_raw(format).unwrap_or(Format::Argb);
    out.width = img.width;
    out.height = img.height;
    out.duration = set.duration;
    out.timestamp = set.timestamp;
    out.pos_x = set.pos_x;
    out.pos_y = set.pos_y;
    out.dispose_to_background = flags & ANMF_FLAG_DISPOSE != 0;
    out.blend = flags & ANMF_FLAG_NO_BLEND == 0;
    out.has_alpha = set.has_alpha;
}

/// As [`export_frame`], keeping the picture itself, which is what a caller
/// that supplied no buffer of its own reads.
pub(crate) fn export_own<'a>(
    set: &ExportSettings,
    img: Frame<'a>,
    format: i32,
    out: &mut Handout<'a>,
) {
    export_frame(set, &img, format, out);
    out.pixels = Pixels::Own(img);
}

/// Packs rows `[row_start, row_end)` of `img` into the caller's own plane.
#[allow(clippy::too_many_arguments)]
fn export_external_rows(
    set: &ExportSettings,
    dsp: &YuvDsp,
    ext: &mut dyn RowSink,
    img: &Frame<'_>,
    format: i32,
    out: &mut Handout<'_>,
    row_start: i32,
    row_end: i32,
) -> Result<()> {
    let row = img.width as usize * format_bpp(format);
    let pack = if img.format as i32 == format {
        None
    } else {
        format_packer(dsp, format)
    };

    if pack.is_none() && img.format.bpp() != format_bpp(format) {
        return Err(Error::Unsupported);
    }
    if !ext.fits(0, row, img.height) {
        return Err(Error::BufferTooSmall);
    }

    for y in row_start..row_end {
        /* The caller's plane may have a negative stride, so it is asked for a
        row at a time rather than borrowed whole. */
        let dst = ext.row(0, y, row);

        match pack {
            Some(pack) => pack(dst, img.row(0, y)),
            None => dst.copy_from_slice(img.row(0, y)),
        }
    }

    export_frame(set, img, format, out);
    out.pixels = Pixels::Sink;
    Ok(())
}

/// As [`export_external_rows`], for a planar format's three or four planes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn export_external_planar_rows(
    set: &ExportSettings,
    ext: &mut dyn RowSink,
    img: &Frame<'_>,
    format: i32,
    out: &mut Handout<'_>,
    row_start: i32,
    row_end: i32,
) -> Result<()> {
    let planes = frame_planes(format);

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = crate::image::ceil_rshift(img.width, shift) as usize;
        let h = crate::image::ceil_rshift(img.height, shift);

        if !ext.fits(p, w, h) {
            return Err(Error::BufferTooSmall);
        }
    }

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = crate::image::ceil_rshift(img.width, shift) as usize;
        let y0 = row_start >> shift;
        let h = crate::image::ceil_rshift(row_end, shift);

        for y in y0..h {
            ext.row(p, y, w).copy_from_slice(&img.row(p, y)[..w]);
        }
    }

    export_frame(set, img, format, out);
    out.pixels = Pixels::Sink;
    Ok(())
}

fn export_external_planar(
    set: &ExportSettings,
    ext: &mut dyn RowSink,
    img: &Frame<'_>,
    format: i32,
    out: &mut Handout<'_>,
) -> Result<()> {
    let height = img.height;

    export_external_planar_rows(set, ext, img, format, out, 0, height)
}

/// Which conversion the output format needs, decided before anything is
/// written so that the buffer it writes into is borrowed exactly once.
enum Route {
    /// The picture is already in the output format.
    AsIs,
    /// Upsample, or go through ARGB for a two-byte format.
    Upsample,
    /// Repack a four-byte format into another.
    Pack(RowFn),
    /// Premultiplied ARGB over an animation canvas whose colour is already
    /// weighted, which is a relabelling.
    Relabel,
    /// The same, for a still, which the caller may hold past the next decode.
    Copy,
}

/// Hands out a whole frame: crop, scale, convert, premultiply, flip, and then
/// either the decoder's own memory or the caller's.
///
/// # Safety
///
/// `frame` must be writable, and the caller's planes as they were declared.
pub fn export_packed<'a>(
    set: &ExportSettings,
    t: ExportTargets<'a>,
    img: Frame<'a>,
    out: &mut Handout<'a>,
) -> Result<()> {
    let ExportTargets {
        dsp,
        options,
        rescale,
        transformed,
        output,
        ext,
    } = t;
    let format = set.out_format;
    let img = transform_image(options, rescale, transformed, img, format)?;
    let target = Format::from_raw(format);

    if matches!(target, Some(Format::Yuv420p) | Some(Format::Yuva420p)) {
        let want_alpha = target == Some(Format::Yuva420p);
        let native = img.format;
        let mut planar = if (native == Format::Yuv420p && !want_alpha)
            || native == Format::Yuva420p
        {
            img
        } else {
            ensure_yuva(dsp, output, &img, want_alpha)?;
            output.frame()
        };

        if options.flip {
            planar = planar.flipped();
        }
        if let Some(ext) = ext {
            return export_external_planar(set, ext, &planar, format, out);
        }
        export_own(set, planar, format, out);
        return Ok(());
    }

    if !format_is_packed(format) {
        let img = if options.flip { img.flipped() } else { img };
        let native = img.format as i32;
        let Some(ext) = ext else {
            export_own(set, img, native, out);
            return Ok(());
        };

        if !format_is_packed(native) {
            return export_external_planar(set, ext, &img, native, out);
        }
        return export_external_rows(set, dsp, ext, &img, native, out, 0, img.height);
    }

    let route = if !format_is_packed(img.format as i32) || format_bpp(format) == 2 {
        Route::Upsample
    } else if img.format as i32 != format {
        match format_packer(dsp, format) {
            Some(pack) => Route::Pack(pack),
            None => {
                if target != Some(Format::ArgbPre) || img.format != Format::Argb {
                    return Err(Error::Unsupported);
                }
                if set.animation {
                    Route::Relabel
                } else {
                    Route::Copy
                }
            }
        }
    } else {
        Route::AsIs
    };
    /* Premultiplying only ever goes with a route through `output`: no picture
    the decoder holds is premultiplied, so a format that is cannot be `AsIs`,
    and the relabelling is only reached for an animation, which premultiplies
    each frame before compositing it instead. */
    let premultiply = set.premultiply && !set.animation && format_bpp(format) != 2;
    let mut img = match route {
        Route::AsIs => img,
        Route::Relabel => {
            let mut img = img;

            img.format = Format::ArgbPre;
            img
        }
        route => {
            let packed = Format::from_raw(format).unwrap_or(Format::Argb);

            match route {
                Route::Upsample => {
                    convert_to_packed(
                        dsp,
                        output,
                        &img,
                        format,
                        options.no_fancy_upsampling,
                        premultiply_after_pack(set.animation, set.anim_mode),
                    )?;
                }
                Route::Pack(pack) => {
                    output.alloc_packed(img.width, img.height, packed.bpp(), packed)?;

                    let mut view = output.frame_mut();

                    for y in 0..img.height {
                        pack(view.row(0, y), img.row(0, y));
                    }
                }
                _ => {
                    output.alloc_packed(img.width, img.height, 4, Format::ArgbPre)?;

                    let mut view = output.frame_mut();

                    for y in 0..img.height {
                        view.row(0, y).copy_from_slice(img.row(0, y));
                    }
                }
            }
            if premultiply {
                let alpha_first = format_layout(format) == LAYOUT_ARGB;
                let mut view = output.frame_mut();

                for y in 0..view.height {
                    (dsp.premultiply_row)(view.row(0, y), alpha_first);
                }
            }
            output.frame()
        }
    };

    if options.flip {
        img = img.flipped();
    }
    if let Some(ext) = ext {
        return export_external_rows(set, dsp, ext, &img, format, out, 0, img.height);
    }
    export_own(set, img, format, out);
    Ok(())
}

/// Converts and hands out rows `[0, upto)` of the still lossy frame,
/// converting each row exactly once however many times it is asked for.
///
/// # Safety
///
/// `frame` must be writable, and the caller's planes as they were declared.
pub fn export_still_packed<'a>(
    set: &ExportSettings,
    t: RowTargets<'a>,
    src: &Frame<'_>,
    out: &mut Handout<'a>,
    upto: i32,
) -> Result<()> {
    let RowTargets {
        dsp,
        options,
        output,
        converted,
        ext,
        converted_rows,
        converted_format,
    } = t;
    let format = set.out_format;
    let done = *converted_rows;
    let first = if *converted_format == format { done } else { 0 };
    let upto = upto.max(done);
    let converted_from = if format_bpp(format) == 2 {
        still_packed_2byte(set, dsp, options, output, converted, src, first, upto)?
    } else {
        still_packed_direct(set, dsp, options, converted, src, first, upto)?
    };

    /* Bound only now: both helpers may have grown the image, and a view taken
    before that would be of the memory as it was. */
    let dst = converted.frame();

    if let Some(ext) = ext {
        export_external_rows(set, dsp, ext, &dst, format, out, converted_from, upto)?;
        *converted_rows = upto;
        *converted_format = format;
        return Ok(());
    }
    *converted_rows = upto;
    *converted_format = format;
    export_own(set, dst, format, out);
    Ok(())
}

/// Upsamples straight into the output format. Returns the first row written.
#[allow(clippy::too_many_arguments)]
fn still_packed_direct(
    set: &ExportSettings,
    dsp: &YuvDsp,
    options: &Options,
    dst: &mut Buffer,
    src: &Frame<'_>,
    first: i32,
    upto: i32,
) -> Result<i32> {
    let format = set.out_format;
    let layout = format_layout(format);
    let target = Format::from_raw(format).unwrap_or(Format::Argb);
    let mut converted_from = first;

    if first == 0 {
        dst.alloc_packed(src.width, src.height, target.bpp(), target)?;
    }
    if options.no_fancy_upsampling {
        upsample_simple(dsp, dst, src, layout, first, upto);
    } else if upto > first {
        converted_from = upsample_fancy(dsp, dst, src, layout, first, upto);
    }
    if set.premultiply {
        let alpha_first = layout == LAYOUT_ARGB;
        let mut view = dst.frame_mut();

        for y in converted_from..upto {
            (dsp.premultiply_row)(view.row(0, y), alpha_first);
        }
    }
    Ok(converted_from)
}

/// The two-byte formats are packed from ARGB, so the intermediate has to be
/// carried between calls too, rather than rebuilt for the whole frame.
///
/// Returns the first row it wrote.
#[allow(clippy::too_many_arguments)]
fn still_packed_2byte(
    set: &ExportSettings,
    dsp: &YuvDsp,
    options: &Options,
    argb: &mut Buffer,
    dst: &mut Buffer,
    src: &Frame<'_>,
    first: i32,
    upto: i32,
) -> Result<i32> {
    let format = set.out_format;
    let target = Format::from_raw(format).unwrap_or(Format::Argb);
    let mut converted_from = first;

    if first == 0 {
        argb.alloc_argb(src.width, src.height)?;
        dst.alloc_packed(src.width, src.height, 2, target)?;
    }
    if upto > first {
        let Some(pack) = format_packer(dsp, format) else {
            return Err(Error::Unsupported);
        };
        let premultiply = format_premultiplier_4444(dsp, format);

        if options.no_fancy_upsampling {
            upsample_simple(dsp, argb, src, LAYOUT_ARGB, first, upto);
        } else {
            converted_from = upsample_fancy(dsp, argb, src, LAYOUT_ARGB, first, upto);
        }
        pack_2byte_rows(
            set,
            dst,
            &argb.frame(),
            pack,
            premultiply,
            converted_from,
            upto,
        );
    }
    Ok(converted_from)
}

fn pack_2byte_rows(
    set: &ExportSettings,
    dst: &mut Buffer,
    argb: &Frame<'_>,
    pack: RowFn,
    premultiply: fn(&mut [u8]),
    from: i32,
    upto: i32,
) {
    let mut out = dst.frame_mut();

    for y in from..upto {
        let row = out.row(0, y);

        pack(row, argb.row(0, y));
        if set.premultiply {
            premultiply(row);
        }
    }
}

fn upsample_simple(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    layout: usize,
    first: i32,
    upto: i32,
) {
    let width = src.width as usize;
    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();

    crate::convert::yuv420_to_packed_simple(
        dsp,
        layout,
        &mut out.planes_mut()[0],
        &planes,
        width,
        first as usize,
        upto as usize,
    );
}

/// Returns the first row the fancy upsampler actually wrote, which is one
/// above `first` when it starts on an even row.
fn upsample_fancy(
    dsp: &YuvDsp,
    dst: &mut Buffer,
    src: &Frame<'_>,
    layout: usize,
    first: i32,
    upto: i32,
) -> i32 {
    let (width, height) = (src.width as usize, src.height as usize);
    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();

    crate::convert::yuv420_to_packed_rows(
        dsp,
        layout,
        &mut out.planes_mut()[0],
        &planes,
        width,
        height,
        first as usize,
        upto as usize,
    ) as i32
}

/// Hands out rows `[0, upto)` of the still lossless frame, premultiplying and
/// packing each row exactly once however many times it is asked for.
///
/// # Safety
///
/// `frame` must be writable, and the caller's planes as they were declared.
pub fn export_still_lossless<'a>(
    set: &ExportSettings,
    t: RowTargets<'a>,
    img: &Frame<'a>,
    out: &mut Handout<'a>,
    upto: i32,
) -> Result<()> {
    let RowTargets {
        dsp,
        output,
        ext,
        converted_rows,
        converted_format,
        ..
    } = t;
    let format = set.out_format;
    let done = *converted_rows;
    let first = if *converted_format == format { done } else { 0 };
    let upto = upto.max(done);
    let target = Format::from_raw(format);
    let mut finish = || {
        *converted_rows = upto;
        *converted_format = format;
    };

    if matches!(target, Some(Format::Yuv420p) | Some(Format::Yuva420p)) {
        let want_alpha = target == Some(Format::Yuva420p);

        ensure_yuva_rows(dsp, output, img, want_alpha, first, upto)?;

        let planar = output.frame();

        match ext {
            Some(ext) => export_external_planar_rows(
                set, ext, &planar, format, out, first, upto,
            )?,
            None => export_own(set, planar, format, out),
        }
        finish();
        return Ok(());
    }

    if !format_is_packed(format) {
        let native = img.format as i32;
        let Some(ext) = ext else {
            export_own(set, *img, native, out);
            finish();
            return Ok(());
        };

        export_external_rows(set, dsp, ext, img, native, out, first, upto)?;
        finish();
        return Ok(());
    }

    let premultiply = format_premultiplier_4444(dsp, format);
    let alpha_first = format_layout(format) == LAYOUT_ARGB;
    let out_len = img.width as usize * format_bpp(format);

    if let Some(ext) = ext {
        export_external_rows(set, dsp, ext, img, format, out, first, upto)?;
        if set.premultiply {
            for y in first..upto {
                let row = ext.row(0, y, out_len);

                if format_bpp(format) == 2 {
                    premultiply(row);
                } else {
                    (dsp.premultiply_row)(row, alpha_first);
                }
            }
        }
        finish();
        return Ok(());
    }

    let pack = format_packer(dsp, format);

    if !set.premultiply && (pack.is_none() || img.format as i32 == format) {
        export_own(set, *img, format, out);
        finish();
        return Ok(());
    }

    if first == 0 {
        let target = Format::from_raw(format).unwrap_or(Format::Argb);

        output.alloc_packed(img.width, img.height, target.bpp(), target)?;
    }

    {
        let mut view = output.frame_mut();

        for y in first..upto {
            let dst = view.row(0, y);

            match pack {
                Some(pack) => pack(dst, img.row(0, y)),
                None => dst.copy_from_slice(img.row(0, y)),
            }
            if set.premultiply {
                if format_bpp(format) == 2 {
                    premultiply(dst);
                } else {
                    (dsp.premultiply_row)(dst, alpha_first);
                }
            }
        }
    }

    export_own(set, output.frame(), format, out);
    finish();
    Ok(())
}
