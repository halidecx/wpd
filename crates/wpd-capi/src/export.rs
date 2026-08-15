//! C ABI for handing a decoded picture out, as declared by `src/export.h`.
//!
//! This is the last place a picture passes through, and it is all plumbing:
//! which conversion the output format needs, which rows have already been
//! done, and whether the bytes go into the decoder's memory or the caller's.
//! The arithmetic that a caller's buffer sizes drive is [`wpd::image`]; what
//! is here walks rows of a `WebPImage` that may already be a crop or a flip.
//!
//! Every source here is a borrowed [`Frame`] and every destination one of the
//! decoder's [`Buffer`]s. The only place the C ABI's shape reappears is
//! [`handout`], which turns a flipped view back into the negative stride the
//! header promises.

use std::ffi::{c_int, c_void};
use std::{mem, ptr, slice};

use wpd::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};
use wpd::dsp::yuv::LAYOUT_ARGB;
use wpd::image::{external_plane_fits, Format};

use crate::convert::{alloc_status, yuv_planes};
use crate::convert::{
    convert_to_packed, ensure_yuva, ensure_yuva_rows, format_bpp, format_is_packed,
    format_layout, format_packer, format_premultiplier_4444, premultiply_after_pack,
    transform_image, WPDDecoderOptions,
};
use wpd::dsp::yuv::{RowFn, YuvDsp};
use wpd::picture::{Buffer, Frame};
use wpd::rescale::Scratch;

const WPD_OK: c_int = 0;
const WPD_ERR_UNSUPPORTED: c_int = -5;
const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;

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

/// `ExportSettings` from `src/export.h`.
#[repr(C)]
pub struct ExportSettings {
    pub out_format: c_int,
    pub premultiply: c_int,
    pub animation: c_int,
    pub anim_mode: c_int,
    pub ext_active: c_int,
    pub duration: c_int,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub anmf_flags: c_int,
    pub has_alpha: c_int,
    pub timestamp: i64,
}

/// The scratch and the tables an export writes through, gathered at the call
/// so that nothing here reads a decoder field that has moved on since.
pub struct ExportTargets {
    pub dsp: *const YuvDsp,
    pub options: *const WPDDecoderOptions,
    pub rescale: *mut Scratch,
    pub transformed: *mut Buffer,
    pub output: *mut Buffer,
    pub converted: *mut Buffer,
    pub ext: *const WPDOutputPlane,
    pub converted_rows: *mut c_int,
    pub converted_format: *mut c_int,
}

const _: () = assert!(mem::size_of::<ExportSettings>() == 10 * 4 + 8);

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

impl ExportTargets {
    fn dsp(&self) -> &YuvDsp {
        unsafe { &*self.dsp }
    }

    fn options(&self) -> &WPDDecoderOptions {
        unsafe { &*self.options }
    }

    fn ext(&self, p: usize) -> &WPDOutputPlane {
        unsafe { &*self.ext.add(p) }
    }

    fn finish(&self, upto: c_int, format: c_int) {
        unsafe {
            self.converted_rows.write(upto);
            self.converted_format.write(format);
        }
    }
}

/// The planes a format hands out: three or four for planar, one for packed.
fn frame_planes(format: c_int) -> usize {
    match Format::from_raw(format) {
        Some(Format::Yuva420p) => 4,
        Some(Format::Yuv420p) => 3,
        _ => 1,
    }
}

/// The `(pointer, stride)` pair the C ABI hands plane `p` out as.
///
/// A flip is a reading order everywhere inside the decoder; here it becomes
/// the negative stride `include/wpd.h` promises, pointing at what is now the
/// first row.
fn handout(img: &Frame<'_>, p: usize) -> (*const u8, isize) {
    if img.plane[p].is_empty() {
        return (ptr::null(), 0);
    }
    let stride = img.plane[p].stride() as isize;

    (
        img.row(p, 0).as_ptr(),
        if img.flip { -stride } else { stride },
    )
}

/// # Safety
///
/// `frame` must point to a `WPDFrame` of at least its own declared
/// `struct_size` bytes.
pub(crate) unsafe fn export_frame(
    set: &ExportSettings,
    img: &Frame<'_>,
    format: c_int,
    frame: *mut WPDFrame,
) {
    unsafe { frame_clear(frame) };

    let out = unsafe { &mut *frame };

    for p in 0..frame_planes(format) {
        let (data, stride) = handout(img, p);

        out.data[p] = data;
        out.stride[p] = stride;
    }
    out.width = img.width;
    out.height = img.height;
    out.format = format;
    out.duration = set.duration;
    out.timestamp = set.timestamp;
    if unsafe { frame_extent(frame) } < has_alpha_extent() {
        return;
    }
    let out = unsafe { &mut *frame };
    let flags = set.anmf_flags as u8;

    out.pos_x = set.pos_x;
    out.pos_y = set.pos_y;
    out.dispose = if flags & ANMF_FLAG_DISPOSE != 0 {
        WPD_DISPOSE_BACKGROUND
    } else {
        WPD_DISPOSE_NONE
    };
    out.blend = if flags & ANMF_FLAG_NO_BLEND != 0 {
        WPD_BLEND_NONE
    } else {
        WPD_BLEND_ALPHA
    };
    out.has_alpha = set.has_alpha;
}

/// # Safety
///
/// The caller's planes must be as they were declared.
unsafe fn export_external_rows(
    set: &ExportSettings,
    t: &ExportTargets,
    img: &Frame<'_>,
    format: c_int,
    frame: *mut WPDFrame,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let row = img.width as usize * format_bpp(format);
    let ext = t.ext(0);
    let pack = if img.format as c_int == format {
        None
    } else {
        format_packer(t.dsp(), format)
    };

    if pack.is_none() && img.format.bpp() != format_bpp(format) {
        return WPD_ERR_UNSUPPORTED;
    }
    if !external_plane_fits(ext.size, ext.stride, row, img.height) {
        return WPD_ERR_BUFFER_TOO_SMALL;
    }

    let mut dst = unsafe { ext.data.offset(row_start as isize * ext.stride) };

    for y in row_start..row_end {
        /* The caller's plane may have a negative stride, so it is walked a row
        at a time rather than borrowed whole. */
        let out = unsafe { slice::from_raw_parts_mut(dst, row) };

        match pack {
            Some(pack) => pack(out, img.row(0, y)),
            None => out.copy_from_slice(img.row(0, y)),
        }
        dst = unsafe { dst.offset(ext.stride) };
    }

    unsafe { export_frame(set, img, format, frame) };

    let out = unsafe { &mut *frame };

    for p in 1..4 {
        out.data[p] = ptr::null();
        out.stride[p] = 0;
    }
    out.data[0] = ext.data;
    out.stride[0] = ext.stride;
    WPD_OK
}

/// # Safety
///
/// As [`export_external_rows`].
pub(crate) unsafe fn export_external_planar_rows(
    set: &ExportSettings,
    t: &ExportTargets,
    img: &Frame<'_>,
    format: c_int,
    frame: *mut WPDFrame,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let planes = frame_planes(format);

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = wpd::image::ceil_rshift(img.width, shift) as usize;
        let h = wpd::image::ceil_rshift(img.height, shift);
        let ext = t.ext(p);

        if ext.data.is_null()
            || ext.stride == 0
            || !external_plane_fits(ext.size, ext.stride, w, h)
        {
            return WPD_ERR_BUFFER_TOO_SMALL;
        }
    }

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = wpd::image::ceil_rshift(img.width, shift) as usize;
        let y0 = row_start >> shift;
        let h = wpd::image::ceil_rshift(row_end, shift);
        let ext = t.ext(p);
        let mut dst = unsafe { ext.data.offset(y0 as isize * ext.stride) };

        for y in y0..h {
            unsafe { slice::from_raw_parts_mut(dst, w) }
                .copy_from_slice(&img.row(p, y)[..w]);
            dst = unsafe { dst.offset(ext.stride) };
        }
    }

    unsafe { export_frame(set, img, format, frame) };

    let out = unsafe { &mut *frame };

    for p in 0..4 {
        out.data[p] = if p < planes {
            t.ext(p).data
        } else {
            ptr::null()
        };
        out.stride[p] = if p < planes { t.ext(p).stride } else { 0 };
    }
    WPD_OK
}

/// # Safety
///
/// As [`export_external_rows`].
unsafe fn export_external_planar(
    set: &ExportSettings,
    t: &ExportTargets,
    img: &Frame<'_>,
    format: c_int,
    frame: *mut WPDFrame,
) -> c_int {
    let height = img.height;

    unsafe { export_external_planar_rows(set, t, img, format, frame, 0, height) }
}

/// Hands out a whole frame: crop, scale, convert, premultiply, flip, and then
/// either the decoder's own memory or the caller's.
///
/// # Safety
///
/// The targets must be live and `frame` writable.
pub unsafe fn export_packed(
    set: &ExportSettings,
    t: &ExportTargets,
    img: Frame<'_>,
    frame: *mut WPDFrame,
) -> c_int {
    let format = set.out_format;
    let processed = transform_image(
        t.options(),
        unsafe { &mut *t.rescale },
        unsafe { &mut *t.transformed },
        img,
        format,
    );
    let mut img = match processed {
        Ok(img) => img,
        Err(e) => return e,
    };
    let target = Format::from_raw(format);

    if matches!(target, Some(Format::Yuv420p) | Some(Format::Yuva420p)) {
        let want_alpha = target == Some(Format::Yuva420p);
        let native = img.format;
        let mut planar = if (native == Format::Yuv420p && !want_alpha)
            || native == Format::Yuva420p
        {
            img
        } else {
            let ret = ensure_yuva(t.dsp(), unsafe { &mut *t.output }, &img, want_alpha);

            if ret < 0 {
                return ret;
            }
            unsafe { (*t.output).frame() }
        };

        if t.options().flip != 0 {
            planar = planar.flipped();
        }
        if set.ext_active != 0 {
            return unsafe { export_external_planar(set, t, &planar, format, frame) };
        }
        unsafe { export_frame(set, &planar, format, frame) };
        return WPD_OK;
    }

    if !format_is_packed(format) {
        if t.options().flip != 0 {
            img = img.flipped();
        }

        let native = img.format as c_int;

        if set.ext_active == 0 {
            unsafe { export_frame(set, &img, native, frame) };
            return WPD_OK;
        }
        if !format_is_packed(native) {
            return unsafe { export_external_planar(set, t, &img, native, frame) };
        }
        return unsafe {
            export_external_rows(set, t, &img, native, frame, 0, img.height)
        };
    }

    if !format_is_packed(img.format as c_int) || format_bpp(format) == 2 {
        let ret = convert_to_packed(
            t.dsp(),
            unsafe { &mut *t.output },
            &img,
            format,
            t.options().no_fancy_upsampling != 0,
            premultiply_after_pack(set.animation != 0, set.anim_mode),
        );

        if ret < 0 {
            return ret;
        }
        img = unsafe { (*t.output).frame() };
    } else if img.format as c_int != format {
        match format_packer(t.dsp(), format) {
            None => {
                if target != Some(Format::ArgbPre) || img.format != Format::Argb {
                    return WPD_ERR_UNSUPPORTED;
                }
                /* Premultiplied ARGB over ARGB is a relabelling when the
                colour is already weighted, which is how a composited
                animation canvas arrives. A still has to be copied, because
                the caller may hold the picture past the next decode. */
                if set.animation != 0 {
                    img.format = Format::ArgbPre;
                } else {
                    let out = unsafe { &mut *t.output };

                    if let Err(e) =
                        out.alloc_packed(img.width, img.height, 4, Format::ArgbPre)
                    {
                        return alloc_status(e);
                    }

                    let mut view = out.frame_mut();

                    for y in 0..img.height {
                        view.row(0, y).copy_from_slice(img.row(0, y));
                    }
                    img = unsafe { (*t.output).frame() };
                }
            }
            Some(pack) => {
                let out = unsafe { &mut *t.output };
                let target = target.unwrap_or(Format::Argb);

                if let Err(e) =
                    out.alloc_packed(img.width, img.height, target.bpp(), target)
                {
                    return alloc_status(e);
                }

                let mut view = out.frame_mut();

                for y in 0..img.height {
                    pack(view.row(0, y), img.row(0, y));
                }
                img = unsafe { (*t.output).frame() };
            }
        }
    }

    if set.premultiply != 0 && set.animation == 0 && format_bpp(format) != 2 {
        /* Everything that reaches here has been written into `output` on the
        way: the output format is premultiplied, which no picture the decoder
        holds ever is, so a planar source went through convert_to_packed and an
        ARGB one through the packer or through the copy the relabelling path
        makes for a still. */
        let alpha_first = format_layout(img.format as c_int) == LAYOUT_ARGB;
        let mut view = unsafe { (*t.output).frame_mut() };

        for y in 0..view.height {
            (t.dsp().premultiply_row)(view.row(0, y), alpha_first);
        }
        img = unsafe { (*t.output).frame() };
    }
    if t.options().flip != 0 {
        img = img.flipped();
    }
    if set.ext_active != 0 {
        return unsafe {
            export_external_rows(set, t, &img, format, frame, 0, img.height)
        };
    }
    unsafe { export_frame(set, &img, format, frame) };
    WPD_OK
}

/// Converts and hands out rows `[0, upto)` of the still lossy frame,
/// converting each row exactly once however many times it is asked for.
///
/// # Safety
///
/// The targets must be live and `frame` writable.
pub unsafe fn export_still_packed(
    set: &ExportSettings,
    t: &ExportTargets,
    src: &Frame<'_>,
    frame: *mut WPDFrame,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let done = unsafe { t.converted_rows.read() };
    let first = if unsafe { t.converted_format.read() } == format {
        done
    } else {
        0
    };
    let upto = upto.max(done);
    let converted_from = if format_bpp(format) == 2 {
        unsafe { still_packed_2byte(set, t, src, first, upto) }
    } else {
        unsafe { still_packed_direct(set, t, src, first, upto) }
    };

    if converted_from < 0 {
        return converted_from;
    }
    /* Bound only now: both helpers may have grown the image, and a view taken
    before that would be of the memory as it was. */
    let dst = unsafe { (*t.converted).frame() };

    if set.ext_active != 0 {
        let ret = unsafe {
            export_external_rows(set, t, &dst, format, frame, converted_from, upto)
        };

        if ret < 0 {
            return ret;
        }
        t.finish(upto, format);
        return WPD_OK;
    }
    t.finish(upto, format);
    unsafe { export_frame(set, &dst, format, frame) };
    WPD_OK
}

/// Upsamples straight into the output format. Returns the first row written,
/// or a negative status.
unsafe fn still_packed_direct(
    set: &ExportSettings,
    t: &ExportTargets,
    src: &Frame<'_>,
    first: c_int,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let layout = format_layout(format);
    let target = Format::from_raw(format).unwrap_or(Format::Argb);
    let mut converted_from = first;

    if first == 0 {
        let dst = unsafe { &mut *t.converted };

        if let Err(e) = dst.alloc_packed(src.width, src.height, target.bpp(), target) {
            return alloc_status(e);
        }
    }

    let dst = unsafe { &mut *t.converted };

    if t.options().no_fancy_upsampling != 0 {
        upsample_simple(t, dst, src, layout, first, upto);
    } else if upto > first {
        converted_from = upsample_fancy(t, dst, src, layout, first, upto);
    }
    if set.premultiply != 0 {
        let alpha_first = layout == LAYOUT_ARGB;
        let mut view = dst.frame_mut();

        for y in converted_from..upto {
            (t.dsp().premultiply_row)(view.row(0, y), alpha_first);
        }
    }
    converted_from
}

/// The two-byte formats are packed from ARGB, so the intermediate has to be
/// carried between calls too, rather than rebuilt for the whole frame.
///
/// Returns the first row it wrote, or a negative status.
unsafe fn still_packed_2byte(
    set: &ExportSettings,
    t: &ExportTargets,
    src: &Frame<'_>,
    first: c_int,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let target = Format::from_raw(format).unwrap_or(Format::Argb);
    let mut converted_from = first;

    if first == 0 {
        if let Err(e) = unsafe { (*t.output).alloc_argb(src.width, src.height) } {
            return alloc_status(e);
        }
        if let Err(e) =
            unsafe { (*t.converted).alloc_packed(src.width, src.height, 2, target) }
        {
            return alloc_status(e);
        }
    }
    if upto > first {
        let Some(pack) = format_packer(t.dsp(), format) else {
            return WPD_ERR_UNSUPPORTED;
        };
        let premultiply = format_premultiplier_4444(t.dsp(), format);
        let argb = unsafe { &mut *t.output };

        if t.options().no_fancy_upsampling != 0 {
            upsample_simple(t, argb, src, LAYOUT_ARGB, first, upto);
        } else {
            converted_from = upsample_fancy(t, argb, src, LAYOUT_ARGB, first, upto);
        }

        let argb = unsafe { (*t.output).frame() };
        let dst = unsafe { &mut *t.converted };

        pack_2byte_rows(set, dst, &argb, pack, premultiply, converted_from, upto);
    }
    converted_from
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
        if set.premultiply != 0 {
            premultiply(row);
        }
    }
}

fn upsample_simple(
    t: &ExportTargets,
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
        t.dsp(),
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
    t: &ExportTargets,
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
        t.dsp(),
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
/// The targets must be live and `frame` writable.
pub unsafe fn export_still_lossless(
    set: &ExportSettings,
    t: &ExportTargets,
    img: &Frame<'_>,
    frame: *mut WPDFrame,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let done = unsafe { t.converted_rows.read() };
    let first = if unsafe { t.converted_format.read() } == format {
        done
    } else {
        0
    };
    let upto = upto.max(done);
    let target = Format::from_raw(format);

    if matches!(target, Some(Format::Yuv420p) | Some(Format::Yuva420p)) {
        let want_alpha = target == Some(Format::Yuva420p);
        let ret = ensure_yuva_rows(
            t.dsp(),
            unsafe { &mut *t.output },
            img,
            want_alpha,
            first,
            upto,
        );

        if ret < 0 {
            return ret;
        }

        let out = unsafe { (*t.output).frame() };

        if set.ext_active != 0 {
            let ret = unsafe {
                export_external_planar_rows(set, t, &out, format, frame, first, upto)
            };

            if ret < 0 {
                return ret;
            }
        } else {
            unsafe { export_frame(set, &out, format, frame) };
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    if !format_is_packed(format) {
        let native = img.format as c_int;

        if set.ext_active == 0 {
            unsafe { export_frame(set, img, native, frame) };
            t.finish(upto, format);
            return WPD_OK;
        }
        let ret =
            unsafe { export_external_rows(set, t, img, native, frame, first, upto) };

        if ret < 0 {
            return ret;
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    let premultiply = format_premultiplier_4444(t.dsp(), format);
    let alpha_first = format_layout(format) == LAYOUT_ARGB;
    let out_len = img.width as usize * format_bpp(format);

    if set.ext_active != 0 {
        let ret =
            unsafe { export_external_rows(set, t, img, format, frame, first, upto) };

        if ret < 0 {
            return ret;
        }
        if set.premultiply != 0 {
            let ext = t.ext(0);

            for y in first..upto {
                let row = unsafe {
                    slice::from_raw_parts_mut(
                        ext.data.offset(y as isize * ext.stride),
                        out_len,
                    )
                };

                if format_bpp(format) == 2 {
                    premultiply(row);
                } else {
                    (t.dsp().premultiply_row)(row, alpha_first);
                }
            }
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    let pack = format_packer(t.dsp(), format);

    if set.premultiply == 0 && (pack.is_none() || img.format as c_int == format) {
        unsafe { export_frame(set, img, format, frame) };
        t.finish(upto, format);
        return WPD_OK;
    }

    if first == 0 {
        let target = Format::from_raw(format).unwrap_or(Format::Argb);
        let out = unsafe { &mut *t.output };

        if let Err(e) = out.alloc_packed(img.width, img.height, target.bpp(), target) {
            return alloc_status(e);
        }
    }

    {
        let out = unsafe { &mut *t.output };
        let mut view = out.frame_mut();

        for y in first..upto {
            let dst = view.row(0, y);

            match pack {
                Some(pack) => pack(dst, img.row(0, y)),
                None => dst.copy_from_slice(img.row(0, y)),
            }
            if set.premultiply != 0 {
                if format_bpp(format) == 2 {
                    premultiply(dst);
                } else {
                    (t.dsp().premultiply_row)(dst, alpha_first);
                }
            }
        }
    }

    let out = unsafe { (*t.output).frame() };

    unsafe { export_frame(set, &out, format, frame) };
    t.finish(upto, format);
    WPD_OK
}
