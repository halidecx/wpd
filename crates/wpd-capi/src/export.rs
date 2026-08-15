//! Handing a decoded picture out.
//!
//! This is the last place a picture passes through, and it is all plumbing:
//! which conversion the output format needs, which rows have already been
//! done, and whether the bytes go into the decoder's memory or the caller's.
//! The arithmetic that a caller's buffer sizes drive is [`wpd::image`]; what
//! is here walks rows of a [`Frame`] that may already be a crop or a flip.
//!
//! The scratch an export writes through arrives as borrows of the decoder's
//! own fields, which is why there are two sets of them. A whole-frame export
//! may be handed the conversion buffer as its *source* — that is how sub-frame
//! mode returns a converted animation frame — while the resumable row exports
//! write into it. The C passed one struct of pointers to both and the rule
//! that they are never the same buffer at the same time was written nowhere.
//!
//! The only place the C ABI's shape reappears is [`handout`], which turns a
//! flipped view back into the negative stride the header promises, and the
//! caller's own output planes, which are the one thing here that is neither
//! the decoder's memory nor checked by the compiler.

use std::ffi::{c_int, c_void};
use std::{mem, ptr, slice};

use wpd::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};
use wpd::dsp::yuv::LAYOUT_ARGB;
use wpd::image::{external_plane_fits, Format};

use crate::convert::{
    convert_to_packed, ensure_yuva, ensure_yuva_rows, format_bpp, format_is_packed,
    format_layout, format_packer, format_premultiplier_4444, premultiply_after_pack,
    transform_image, yuv_planes,
};
use wpd::dsp::yuv::{RowFn, YuvDsp};
use wpd::error::{Error, Result};
use wpd::handout::{Handout, Pixels, RowSink};
use wpd::options::Options;
use wpd::picture::{Buffer, Frame};
use wpd::rescale::Scratch;

const WPD_DISPOSE_BACKGROUND: c_int = 1;
const WPD_DISPOSE_NONE: c_int = 0;
const WPD_BLEND_ALPHA: c_int = 0;
const WPD_BLEND_NONE: c_int = 1;

/// `WPDOutputPlane` from `include/wpd.h`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WPDOutputPlane {
    pub data: *mut u8,
    pub size: usize,
    pub stride: isize,
}

impl WPDOutputPlane {
    pub(crate) fn empty() -> Self {
        WPDOutputPlane {
            data: ptr::null_mut(),
            size: 0,
            stride: 0,
        }
    }
}

/// The caller's own output planes, which the decoder writes into when
/// `wpd_decoder_set_output_buffer` has named them.
///
/// This is the one destination in the decoder that is neither its own memory
/// nor checked by the compiler: a plane is a pointer, a byte count and a
/// stride that may run backwards. [`wpd::image::external_plane_fits`] is asked
/// about the geometry before a single row is written, and nothing else here
/// takes the caller's word for anything.
pub struct External(pub [WPDOutputPlane; 4]);

impl RowSink for External {
    fn fits(&self, p: usize, row_len: usize, rows: i32) -> bool {
        let plane = &self.0[p];

        !plane.data.is_null()
            && plane.stride != 0
            && external_plane_fits(plane.size, plane.stride, row_len, rows)
    }

    fn row(&mut self, p: usize, y: i32, len: usize) -> &mut [u8] {
        let plane = &self.0[p];

        /* Sound because `fits` has agreed that the plane holds this row: the
        stride may be negative, so the offset is signed and the pointer walks
        backwards from the plane's first byte exactly as the C's did. */
        unsafe {
            slice::from_raw_parts_mut(plane.data.offset(y as isize * plane.stride), len)
        }
    }
}

/// `WPDFrame` from `include/wpd.h`.
#[repr(C)]
pub struct WPDFrame {
    pub struct_size: usize,
    pub data: [*const u8; 4],
    pub stride: [isize; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: c_int,
    pub duration: c_int,
    pub timestamp: i64,
    pub private_data: *mut c_void,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
}

impl WPDFrame {
    /// A zeroed frame of this build's revision, which is what the header's
    /// `WPD_FRAME_INIT` produces.
    pub(crate) fn zeroed() -> Self {
        WPDFrame {
            struct_size: mem::size_of::<WPDFrame>(),
            data: [ptr::null(); 4],
            stride: [0; 4],
            width: 0,
            height: 0,
            format: 0,
            duration: 0,
            timestamp: 0,
            private_data: ptr::null_mut(),
            pos_x: 0,
            pos_y: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
        }
    }
}

/// What the decoder was asked for, gathered at the call rather than reached
/// for, so nothing here can read a field that has moved on since.
pub struct ExportSettings {
    pub out_format: c_int,
    pub premultiply: bool,
    pub animation: bool,
    pub anim_mode: c_int,
    pub duration: c_int,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub anmf_flags: c_int,
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
    pub converted_rows: &'a mut c_int,
    pub converted_format: &'a mut c_int,
}

/// How far into `WPDFrame` the sub-frame placement fields start, which a
/// caller compiled against an older revision has not made room for.
fn has_alpha_extent() -> usize {
    mem::offset_of!(WPDFrame, has_alpha) + mem::size_of::<c_int>()
}

/// The oldest revision of `WPDFrame` this build accepts.
pub(crate) fn private_data_extent() -> usize {
    mem::offset_of!(WPDFrame, private_data) + mem::size_of::<*mut c_void>()
}

/// # Safety
///
/// `frame`, when not null, must point to a `WPDFrame` of at least its own
/// declared `struct_size` bytes.
pub(crate) unsafe fn frame_valid(frame: *const WPDFrame) -> bool {
    unsafe { frame.as_ref() }.is_some_and(|f| f.struct_size >= private_data_extent())
}

/// How much of the caller's frame this build may touch: the newest revision of
/// the struct it declares room for in full, capped at the newest this build
/// knows about. A size landing between two revisions rounds down to the older
/// one rather than writing part of a field pair the caller may not have.
///
/// # Safety
///
/// As [`frame_valid`], and the frame must not be null.
pub(crate) unsafe fn frame_extent(frame: *const WPDFrame) -> usize {
    if unsafe { (*frame).struct_size } >= has_alpha_extent() {
        has_alpha_extent()
    } else {
        private_data_extent()
    }
}

/// Zeroes everything past `struct_size`, which is the caller's and survives.
///
/// # Safety
///
/// As [`frame_extent`], and the frame must be writable.
pub(crate) unsafe fn frame_clear(frame: *mut WPDFrame) {
    let head = mem::size_of::<usize>();
    let extent = unsafe { frame_extent(frame) };

    unsafe { ptr::write_bytes(frame.cast::<u8>().add(head), 0, extent - head) };
}

/// The planes a format hands out: three or four for planar, one for packed.
fn frame_planes(format: c_int) -> usize {
    match Format::from_raw(format) {
        Some(Format::Yuva420p) => 4,
        Some(Format::Yuv420p) => 3,
        _ => 1,
    }
}

/// Writes a finished handout into the caller's `WPDFrame`.
///
/// This is the only place the C ABI's shape is built, and the only place a
/// flip becomes the negative stride `include/wpd.h` promises: everywhere
/// inside the decoder a flip is a reading order.
///
/// # Safety
///
/// `frame` must point to a `WPDFrame` of at least its own declared
/// `struct_size` bytes.
pub(crate) unsafe fn write_frame(
    handout: &Handout<'_>,
    ext: &[WPDOutputPlane; 4],
    frame: *mut WPDFrame,
) {
    unsafe { frame_clear(frame) };

    let out = unsafe { &mut *frame };
    let planes = handout.planes();

    match &handout.pixels {
        Pixels::Own(img) => {
            for p in 0..planes {
                let (data, stride) = handout_plane(img, p);

                out.data[p] = data;
                out.stride[p] = stride;
            }
        }
        Pixels::Sink => {
            for (p, plane) in ext.iter().enumerate() {
                out.data[p] = if p < planes { plane.data } else { ptr::null() };
                out.stride[p] = if p < planes { plane.stride } else { 0 };
            }
        }
        Pixels::None => {}
    }
    out.width = handout.width;
    out.height = handout.height;
    out.format = handout.format as c_int;
    out.duration = handout.duration;
    out.timestamp = handout.timestamp;
    if unsafe { frame_extent(frame) } < has_alpha_extent() {
        return;
    }
    let out = unsafe { &mut *frame };

    out.pos_x = handout.pos_x;
    out.pos_y = handout.pos_y;
    out.dispose = if handout.dispose_to_background {
        WPD_DISPOSE_BACKGROUND
    } else {
        WPD_DISPOSE_NONE
    };
    out.blend = if handout.blend {
        WPD_BLEND_ALPHA
    } else {
        WPD_BLEND_NONE
    };
    out.has_alpha = c_int::from(handout.has_alpha);
}

/// The `(pointer, stride)` pair the C ABI hands plane `p` out as.
///
/// A flip is a reading order everywhere inside the decoder; here it becomes
/// the negative stride `include/wpd.h` promises, pointing at what is now the
/// first row.
fn handout_plane(img: &Frame<'_>, p: usize) -> (*const u8, isize) {
    if img.plane[p].is_empty() {
        return (ptr::null(), 0);
    }
    let stride = img.plane[p].stride() as isize;

    (
        img.row(p, 0).as_ptr(),
        if img.flip { -stride } else { stride },
    )
}

/// Describes `img` as the picture a decode hands back.
pub(crate) fn export_frame(
    set: &ExportSettings,
    img: &Frame<'_>,
    format: c_int,
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
    format: c_int,
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
    format: c_int,
    out: &mut Handout<'_>,
    row_start: c_int,
    row_end: c_int,
) -> Result<()> {
    let row = img.width as usize * format_bpp(format);
    let pack = if img.format as c_int == format {
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
    format: c_int,
    out: &mut Handout<'_>,
    row_start: c_int,
    row_end: c_int,
) -> Result<()> {
    let planes = frame_planes(format);

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = wpd::image::ceil_rshift(img.width, shift) as usize;
        let h = wpd::image::ceil_rshift(img.height, shift);

        if !ext.fits(p, w, h) {
            return Err(Error::BufferTooSmall);
        }
    }

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = wpd::image::ceil_rshift(img.width, shift) as usize;
        let y0 = row_start >> shift;
        let h = wpd::image::ceil_rshift(row_end, shift);

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
    format: c_int,
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
        let native = img.format as c_int;
        let Some(ext) = ext else {
            export_own(set, img, native, out);
            return Ok(());
        };

        if !format_is_packed(native) {
            return export_external_planar(set, ext, &img, native, out);
        }
        return export_external_rows(set, dsp, ext, &img, native, out, 0, img.height);
    }

    let route = if !format_is_packed(img.format as c_int) || format_bpp(format) == 2 {
        Route::Upsample
    } else if img.format as c_int != format {
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
    upto: c_int,
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
    first: c_int,
    upto: c_int,
) -> Result<c_int> {
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
    first: c_int,
    upto: c_int,
) -> Result<c_int> {
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
    from: c_int,
    upto: c_int,
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
    first: c_int,
    upto: c_int,
) {
    let width = src.width as usize;
    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();

    wpd::convert::yuv420_to_packed_simple(
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
    first: c_int,
    upto: c_int,
) -> c_int {
    let (width, height) = (src.width as usize, src.height as usize);
    let planes = yuv_planes(src);
    let mut out = dst.frame_mut();

    wpd::convert::yuv420_to_packed_rows(
        dsp,
        layout,
        &mut out.planes_mut()[0],
        &planes,
        width,
        height,
        first as usize,
        upto as usize,
    ) as c_int
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
    upto: c_int,
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
        let native = img.format as c_int;
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

    if !set.premultiply && (pack.is_none() || img.format as c_int == format) {
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
