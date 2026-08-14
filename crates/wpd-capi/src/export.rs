//! C ABI for handing a decoded picture out, as declared by `src/export.h`.
//!
//! This is the last place a picture passes through, and it is all plumbing:
//! which conversion the output format needs, which rows have already been
//! done, and whether the bytes go into the decoder's memory or the caller's.
//! The arithmetic that a caller's buffer sizes drive is [`wpd::image`]; what
//! is here walks rows of a `WebPImage` that may already be a crop or a flip.
//!
//! `ExportSettings` and `ExportTargets` mirror the two structs `src/export.h`
//! declares. The first is scalars, the second pointers, so neither carries
//! interior padding and a field added on one side without the other changes
//! the size — which the assertions below catch at compile time.

use std::ffi::c_int;
use std::{mem, ptr};

use wpd::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};
use wpd::dsp::yuv::LAYOUT_ARGB;
use wpd::image::{external_plane_fits, Format};

use crate::convert::{
    convert_to_packed, ensure_yuva, ensure_yuva_rows, flip_image, format_bpp,
    format_is_packed, format_layout, format_packer, format_premultiplier_4444,
    premultiply_after_pack, transform_image, WPDDecoderOptions,
};
use crate::dsp::yuv::{
    wpd_yuv420_to_packed_rows, wpd_yuv420_to_packed_simple, PackRowFn,
    Premultiply4444Fn, WPDYUVDSP,
};
use crate::image::{image_alloc_argb, image_alloc_packed, RescaleScratch, WebPImage};

const WPD_OK: c_int = 0;
const WPD_ERR_UNSUPPORTED: c_int = -5;
const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;

const WPD_DISPOSE_BACKGROUND: c_int = 1;
const WPD_DISPOSE_NONE: c_int = 0;
const WPD_BLEND_ALPHA: c_int = 0;
const WPD_BLEND_NONE: c_int = 1;

/// `WPDOutputPlane` from `include/wpd.h`.
#[repr(C)]
pub struct WPDOutputPlane {
    pub data: *mut u8,
    pub size: usize,
    pub stride: isize,
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
    pub private_data: *mut core::ffi::c_void,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
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

/// `ExportTargets` from `src/export.h`.
#[repr(C)]
pub struct ExportTargets {
    pub dsp: *const WPDYUVDSP,
    pub options: *const WPDDecoderOptions,
    pub rescale: *mut RescaleScratch,
    pub transformed: *mut WebPImage,
    pub output: *mut WebPImage,
    pub converted: *mut WebPImage,
    pub ext: *const WPDOutputPlane,
    pub converted_rows: *mut c_int,
    pub converted_format: *mut c_int,
}

const _: () = assert!(mem::size_of::<ExportSettings>() == 10 * 4 + 8);
const _: () =
    assert!(mem::size_of::<ExportTargets>() == 9 * mem::size_of::<*const ()>());

extern "C" {
    fn frame_clear(frame: *mut WPDFrame);
    fn frame_extent(frame: *const WPDFrame) -> usize;
}

/// How far into `WPDFrame` the sub-frame placement fields start, which a
/// caller compiled against an older revision has not made room for.
fn has_alpha_extent() -> usize {
    mem::offset_of!(WPDFrame, has_alpha) + mem::size_of::<c_int>()
}

impl ExportTargets {
    fn dsp(&self) -> &WPDYUVDSP {
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

/// # Safety
///
/// `img` must point to a live `WebPImage` and `frame` to a `WPDFrame` of at
/// least its own declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn export_frame(
    set: *const ExportSettings,
    img: *const WebPImage,
    format: c_int,
    frame: *mut WPDFrame,
) {
    let set = unsafe { &*set };
    let img = unsafe { &*img };

    unsafe { frame_clear(frame) };

    let out = unsafe { &mut *frame };

    for p in 0..frame_planes(format) {
        out.data[p] = img.data[p];
        out.stride[p] = img.linesize[p] as isize;
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

/// One row of a packed image, `len` bytes of it.
///
/// # Safety
///
/// The image must hold `len` bytes at row `y`.
unsafe fn packed_row(img: &WebPImage, y: c_int, len: usize) -> &[u8] {
    unsafe { img.row(0, y, len) }
}

/// # Safety
///
/// Every pointer must be live and the caller's planes must be as they were
/// declared.
unsafe fn export_external_rows(
    set: &ExportSettings,
    t: &ExportTargets,
    img: &WebPImage,
    format: c_int,
    frame: *mut WPDFrame,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let row = img.width as usize * format_bpp(format) as usize;
    let ext = t.ext(0);
    let pack = if img.format == format {
        None
    } else {
        unsafe { format_packer(t.dsp, format) }
    };

    if pack.is_none() && format_bpp(img.format) != format_bpp(format) {
        return WPD_ERR_UNSUPPORTED;
    }
    if !external_plane_fits(ext.size, ext.stride, row, img.height) {
        return WPD_ERR_BUFFER_TOO_SMALL;
    }
    let mut dst = unsafe { ext.data.offset(row_start as isize * ext.stride) };

    for y in row_start..row_end {
        match pack {
            Some(pack) => unsafe {
                pack(
                    dst,
                    img.data[0].offset(y as isize * img.linesize[0] as isize),
                    img.width,
                )
            },
            None => unsafe {
                ptr::copy_nonoverlapping(packed_row(img, y, row).as_ptr(), dst, row)
            },
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
#[no_mangle]
pub unsafe extern "C" fn export_external_planar_rows(
    set: *const ExportSettings,
    t: *const ExportTargets,
    img: *const WebPImage,
    format: c_int,
    frame: *mut WPDFrame,
    row_start: c_int,
    row_end: c_int,
) -> c_int {
    let (set, t, img) = unsafe { (&*set, &*t, &*img) };
    let planes = if Format::from_raw(format) == Some(Format::Yuva420p) {
        4
    } else {
        3
    };

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
            unsafe {
                ptr::copy_nonoverlapping(img.row(p, y, w).as_ptr(), dst, w);
                dst = dst.offset(ext.stride);
            }
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
    img: &WebPImage,
    format: c_int,
    frame: *mut WPDFrame,
) -> c_int {
    unsafe { export_external_planar_rows(set, t, img, format, frame, 0, img.height) }
}

/// # Safety
///
/// Every pointer must be live.
#[no_mangle]
pub unsafe extern "C" fn export_packed(
    set: *const ExportSettings,
    t: *const ExportTargets,
    img: *mut WebPImage,
    frame: *mut WPDFrame,
) -> c_int {
    let (set, t) = unsafe { (&*set, &*t) };
    let format = set.out_format;
    let mut view: WebPImage = unsafe { mem::zeroed() };
    /* Each of these is written once and only from one of the others, so no
    assignment can have its own destination as its source. The C reused a
    single view for all three, which meant assigning it from a pointer that was
    sometimes itself: a harmless self-assignment there, and an alias Rust has
    no reason to expect. */
    let mut relabelled: WebPImage;
    let mut flipped: WebPImage;
    let mut processed: *mut WebPImage = ptr::null_mut();

    let ret = unsafe {
        transform_image(
            t.options,
            t.rescale,
            t.transformed,
            img,
            &mut view,
            &mut processed,
            format,
        )
    };

    if ret < 0 {
        return ret;
    }
    let mut img = unsafe { &*processed };
    let target = Format::from_raw(format);

    if matches!(target, Some(Format::Yuv420p) | Some(Format::Yuva420p)) {
        let want_alpha = target == Some(Format::Yuva420p);
        let native = Format::from_raw(img.format);
        let mut planar = if (native == Some(Format::Yuv420p) && !want_alpha)
            || native == Some(Format::Yuva420p)
        {
            img
        } else {
            let ret =
                unsafe { ensure_yuva(t.dsp, t.output, img, c_int::from(want_alpha)) };

            if ret < 0 {
                return ret;
            }
            unsafe { &*t.output }
        };

        if t.options().flip != 0 {
            flipped = *planar;
            unsafe { flip_image(&mut flipped) };
            planar = &flipped;
        }
        if set.ext_active != 0 {
            return unsafe { export_external_planar(set, t, planar, format, frame) };
        }
        unsafe { export_frame(set, planar, format, frame) };
        return WPD_OK;
    }

    if format_is_packed(format) == 0 {
        if t.options().flip != 0 {
            flipped = *img;
            unsafe { flip_image(&mut flipped) };
            img = &flipped;
        }
        if set.ext_active == 0 {
            unsafe { export_frame(set, img, img.format, frame) };
            return WPD_OK;
        }
        if format_is_packed(img.format) == 0 {
            return unsafe { export_external_planar(set, t, img, img.format, frame) };
        }
        return unsafe {
            export_external_rows(set, t, img, img.format, frame, 0, img.height)
        };
    }

    if format_is_packed(img.format) == 0 || format_bpp(format) == 2 {
        let ret = unsafe {
            convert_to_packed(
                t.dsp,
                t.output,
                img,
                format,
                t.options().no_fancy_upsampling,
                premultiply_after_pack(set.animation, set.anim_mode),
            )
        };

        if ret < 0 {
            return ret;
        }
        img = unsafe { &*t.output };
    } else if img.format != format {
        match unsafe { format_packer(t.dsp, format) } {
            None => {
                if target != Some(Format::ArgbPre)
                    || Format::from_raw(img.format) != Some(Format::Argb)
                {
                    return WPD_ERR_UNSUPPORTED;
                }
                /* Premultiplied ARGB over ARGB is a relabelling when the
                colour is already weighted, which is how a composited
                animation canvas arrives. A still has to be copied, because
                the caller may hold the picture past the next decode. */
                if set.animation != 0 {
                    relabelled = *img;
                    relabelled.format = format;
                    img = &relabelled;
                } else {
                    let ret = unsafe {
                        image_alloc_packed(t.output, img.width, img.height, 4, format)
                    };

                    if ret < 0 {
                        return ret;
                    }
                    let out = unsafe { &*t.output };
                    let row = img.width as usize * 4;

                    for y in 0..img.height {
                        unsafe { out.row_mut(0, y, row) }
                            .copy_from_slice(unsafe { packed_row(img, y, row) });
                    }
                    img = out;
                }
            }
            Some(pack) => {
                let ret = unsafe {
                    image_alloc_packed(
                        t.output,
                        img.width,
                        img.height,
                        format_bpp(format),
                        format,
                    )
                };

                if ret < 0 {
                    return ret;
                }
                let out = unsafe { &*t.output };

                for y in 0..img.height {
                    unsafe {
                        pack(
                            out.data[0].offset(y as isize * out.linesize[0] as isize),
                            img.data[0].offset(y as isize * img.linesize[0] as isize),
                            img.width,
                        );
                    }
                }
                img = out;
            }
        }
    }

    if set.premultiply != 0 && set.animation == 0 && format_bpp(format) != 2 {
        let alpha_first =
            c_int::from(format_layout(img.format) == LAYOUT_ARGB as c_int);

        for y in 0..img.height {
            unsafe {
                (t.dsp().premultiply_row)(
                    img.data[0].offset(y as isize * img.linesize[0] as isize),
                    alpha_first,
                    img.width,
                );
            }
        }
    }
    if t.options().flip != 0 {
        flipped = *img;
        unsafe { flip_image(&mut flipped) };
        img = &flipped;
    }
    if set.ext_active != 0 {
        return unsafe {
            export_external_rows(set, t, img, format, frame, 0, img.height)
        };
    }
    unsafe { export_frame(set, img, format, frame) };
    WPD_OK
}

/// Converts and hands out rows `[0, upto)` of the still lossy frame,
/// converting each row exactly once however many times it is asked for.
///
/// # Safety
///
/// Every pointer must be live.
#[no_mangle]
pub unsafe extern "C" fn export_still_packed(
    set: *const ExportSettings,
    t: *const ExportTargets,
    src: *const WebPImage,
    frame: *mut WPDFrame,
    upto: c_int,
) -> c_int {
    let (set, t, src) = unsafe { (&*set, &*t, &*src) };
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
    /* Bound only now: both helpers may have grown the image, and a reference
    taken before that would be to the struct as it was, which is a value the
    compiler is entitled to keep. */
    let dst = unsafe { &*t.converted };

    if set.ext_active != 0 {
        let ret = unsafe {
            export_external_rows(set, t, dst, format, frame, converted_from, upto)
        };

        if ret < 0 {
            return ret;
        }
        t.finish(upto, format);
        return WPD_OK;
    }
    t.finish(upto, format);
    unsafe { export_frame(set, dst, format, frame) };
    WPD_OK
}

/// Upsamples straight into the output format. Returns the first row written,
/// or a negative status.
unsafe fn still_packed_direct(
    set: &ExportSettings,
    t: &ExportTargets,
    src: &WebPImage,
    first: c_int,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let layout = format_layout(format);
    let mut converted_from = first;

    if first == 0 {
        let ret = unsafe {
            image_alloc_packed(
                t.converted,
                src.width,
                src.height,
                format_bpp(format),
                format,
            )
        };

        if ret < 0 {
            return ret;
        }
    }
    let dst = unsafe { &*t.converted };

    if t.options().no_fancy_upsampling != 0 {
        unsafe { upsample_simple(t, dst, src, layout, first, upto) };
    } else if upto > first {
        converted_from = unsafe { upsample_fancy(t, dst, src, layout, first, upto) };
    }
    if set.premultiply != 0 {
        let alpha_first = c_int::from(layout == LAYOUT_ARGB as c_int);

        for y in converted_from..upto {
            unsafe {
                (t.dsp().premultiply_row)(
                    dst.data[0].offset(y as isize * dst.linesize[0] as isize),
                    alpha_first,
                    dst.width,
                );
            }
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
    src: &WebPImage,
    first: c_int,
    upto: c_int,
) -> c_int {
    let format = set.out_format;
    let mut converted_from = first;

    if first == 0 {
        let ret = unsafe { image_alloc_argb(t.output, src.width, src.height) };

        if ret < 0 {
            return ret;
        }
        let ret = unsafe {
            image_alloc_packed(t.converted, src.width, src.height, 2, format)
        };

        if ret < 0 {
            return ret;
        }
    }
    if upto > first {
        let Some(pack) = (unsafe { format_packer(t.dsp, format) }) else {
            return WPD_ERR_UNSUPPORTED;
        };
        let premultiply = unsafe { format_premultiplier_4444(t.dsp, format) };
        let argb = unsafe { &*t.output };

        if t.options().no_fancy_upsampling != 0 {
            unsafe { upsample_simple(t, argb, src, LAYOUT_ARGB as c_int, first, upto) };
        } else {
            converted_from = unsafe {
                upsample_fancy(t, argb, src, LAYOUT_ARGB as c_int, first, upto)
            };
        }
        unsafe {
            pack_2byte_rows(
                set,
                &*t.converted,
                argb,
                src.width,
                pack,
                premultiply,
                converted_from,
                upto,
            )
        };
    }
    converted_from
}

#[allow(clippy::too_many_arguments)]
unsafe fn pack_2byte_rows(
    set: &ExportSettings,
    dst: &WebPImage,
    argb: &WebPImage,
    width: c_int,
    pack: PackRowFn,
    premultiply: Premultiply4444Fn,
    from: c_int,
    upto: c_int,
) {
    for y in from..upto {
        let row = unsafe { dst.data[0].offset(y as isize * dst.linesize[0] as isize) };

        unsafe {
            pack(
                row,
                argb.data[0].offset(y as isize * argb.linesize[0] as isize),
                width,
            );
            if set.premultiply != 0 {
                premultiply(row, width);
            }
        }
    }
}

unsafe fn upsample_simple(
    t: &ExportTargets,
    dst: &WebPImage,
    src: &WebPImage,
    layout: c_int,
    first: c_int,
    upto: c_int,
) {
    unsafe {
        wpd_yuv420_to_packed_simple(
            t.dsp,
            layout,
            dst.data[0],
            dst.linesize[0] as isize,
            src.data[0],
            src.linesize[0] as isize,
            src.data[1],
            src.data[2],
            src.linesize[1] as isize,
            src.data[3],
            src.linesize[3] as isize,
            src.width,
            first,
            upto,
        );
    }
}

/// Returns the first row the fancy upsampler actually wrote, which is one
/// above `first` when it starts on an even row.
unsafe fn upsample_fancy(
    t: &ExportTargets,
    dst: &WebPImage,
    src: &WebPImage,
    layout: c_int,
    first: c_int,
    upto: c_int,
) -> c_int {
    unsafe {
        wpd_yuv420_to_packed_rows(
            t.dsp,
            layout,
            dst.data[0],
            dst.linesize[0] as isize,
            src.data[0],
            src.linesize[0] as isize,
            src.data[1],
            src.data[2],
            src.linesize[1] as isize,
            src.data[3],
            src.linesize[3] as isize,
            src.width,
            src.height,
            first,
            upto,
        )
    }
}

/// Hands out rows `[0, upto)` of the still lossless frame, premultiplying and
/// packing each row exactly once however many times it is asked for.
///
/// # Safety
///
/// Every pointer must be live.
#[no_mangle]
pub unsafe extern "C" fn export_still_lossless(
    set: *const ExportSettings,
    t: *const ExportTargets,
    img: *mut WebPImage,
    frame: *mut WPDFrame,
    upto: c_int,
) -> c_int {
    let (set, t, img) = unsafe { (&*set, &*t, &*img) };
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
        let want_alpha = c_int::from(target == Some(Format::Yuva420p));
        let ret =
            unsafe { ensure_yuva_rows(t.dsp, t.output, img, want_alpha, first, upto) };

        if ret < 0 {
            return ret;
        }
        let out = unsafe { &*t.output };

        if set.ext_active != 0 {
            let ret = unsafe {
                export_external_planar_rows(set, t, out, format, frame, first, upto)
            };

            if ret < 0 {
                return ret;
            }
        } else {
            unsafe { export_frame(set, out, format, frame) };
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    if format_is_packed(format) == 0 {
        if set.ext_active == 0 {
            unsafe { export_frame(set, img, img.format, frame) };
            t.finish(upto, format);
            return WPD_OK;
        }
        let ret = unsafe {
            export_external_rows(set, t, img, img.format, frame, first, upto)
        };

        if ret < 0 {
            return ret;
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    let premultiply = unsafe { format_premultiplier_4444(t.dsp, format) };
    let alpha_first = c_int::from(format_layout(format) == LAYOUT_ARGB as c_int);

    if set.ext_active != 0 {
        let ret =
            unsafe { export_external_rows(set, t, img, format, frame, first, upto) };

        if ret < 0 {
            return ret;
        }
        if set.premultiply != 0 {
            let ext = t.ext(0);

            for y in first..upto {
                let row = unsafe { ext.data.offset(y as isize * ext.stride) };

                unsafe {
                    if format_bpp(format) == 2 {
                        premultiply(row, img.width);
                    } else {
                        (t.dsp().premultiply_row)(row, alpha_first, img.width);
                    }
                }
            }
        }
        t.finish(upto, format);
        return WPD_OK;
    }

    let pack = unsafe { format_packer(t.dsp, format) };

    if set.premultiply == 0 && (pack.is_none() || img.format == format) {
        unsafe { export_frame(set, img, format, frame) };
        t.finish(upto, format);
        return WPD_OK;
    }

    if first == 0 {
        let ret = unsafe {
            image_alloc_packed(
                t.output,
                img.width,
                img.height,
                format_bpp(format),
                format,
            )
        };

        if ret < 0 {
            return ret;
        }
    }
    let out = unsafe { &*t.output };

    for y in first..upto {
        let dst = unsafe { out.data[0].offset(y as isize * out.linesize[0] as isize) };
        let src = unsafe { img.data[0].offset(y as isize * img.linesize[0] as isize) };

        unsafe {
            match pack {
                Some(pack) => pack(dst, src, img.width),
                None => ptr::copy_nonoverlapping(src, dst, img.width as usize * 4),
            }
            if set.premultiply != 0 {
                if format_bpp(format) == 2 {
                    premultiply(dst, img.width);
                } else {
                    (t.dsp().premultiply_row)(dst, alpha_first, img.width);
                }
            }
        }
    }
    unsafe { export_frame(set, out, format, frame) };
    t.finish(upto, format);
    WPD_OK
}
