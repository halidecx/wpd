//! C ABI for the animation compositor, as declared by `src/anim.h`.
//!
//! The geometry — whether a frame stands on its own, and how it divides into
//! blended and copied regions — is [`wpd::anim`]. What is here brings the
//! canvas into the format the next frame will be composited in, disposes what
//! the frame before asked to be disposed, and walks the regions.

use std::ffi::{c_int, c_uint};
use std::{mem, ptr};

use wpd::anim::{regions, Placement, Region};
use wpd::image::Format;

use wpd::blit::{self, Rect};
use wpd::dsp::vp8l::Vp8lDsp;

use crate::convert::{
    convert_to_argb, format_bpp, format_is_packed, premultiply_after_pack,
};
use crate::decoder::{
    rl24, rl32, Subframe, WPDDecoder, ALPHA_COMPRESSION_VP8L, TAG_ALPH, TAG_VP8,
    TAG_VP8L,
};
use crate::image::{image_alloc_argb, image_alloc_yuva, image_free, WebPImage};
use crate::vp8::WPD_ERROR_INVALID_DATA;
use crate::vp8l::{vp8l_decode_frame, VP8L_TARGET_ARGB};
use wpd::dsp::yuv::YuvDsp;
use wpd::rescale::premultiply_argb_row;

const WPD_OK: c_int = 0;
const WPD_ANIM_SUBFRAME: c_int = 1;

/// `Placement` from `src/anim.h`.
#[repr(C)]
pub struct CPlacement {
    pub canvas_width: c_int,
    pub canvas_height: c_int,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub anmf_flags: c_int,
    pub frame_index: c_int,
    pub frame_has_alpha: c_int,
    pub key_frame: c_int,
    pub prev_anmf_flags: c_int,
    pub prev_width: c_int,
    pub prev_height: c_int,
    pub prev_pos_x: c_int,
    pub prev_pos_y: c_int,
    pub prev_key_frame: c_int,
    pub premultiply: c_int,
    pub no_fancy_upsampling: c_int,
    pub clear_argb: [u8; 4],
    pub clear_yuva: [u8; 4],
}

/// `CompositeTargets` from `src/anim.h`.
#[repr(C)]
pub struct CompositeTargets {
    pub ldsp: *const Vp8lDsp,
    pub ydsp: *const YuvDsp,
    pub canvas: *mut WebPImage,
}

const _: () = assert!(mem::size_of::<CPlacement>() == 16 * 4 + 8);
const _: () =
    assert!(mem::size_of::<CompositeTargets>() == 3 * mem::size_of::<*const ()>());

impl CPlacement {
    fn geometry(&self) -> Placement {
        Placement {
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            anmf_flags: self.anmf_flags as u8,
            frame_index: self.frame_index,
            frame_has_alpha: self.frame_has_alpha != 0,
            key_frame: self.key_frame != 0,
            prev_anmf_flags: self.prev_anmf_flags as u8,
            prev_width: self.prev_width,
            prev_height: self.prev_height,
            prev_pos_x: self.prev_pos_x,
            prev_pos_y: self.prev_pos_y,
            prev_key_frame: self.prev_key_frame != 0,
        }
    }
}

/// # Safety
///
/// `pl` must point to a live `Placement`.
#[no_mangle]
pub unsafe extern "C" fn anim_is_key_frame(
    pl: *const CPlacement,
    width: c_int,
    height: c_int,
) -> c_int {
    let pl = unsafe { &*pl };

    c_int::from(pl.geometry().is_key_frame(width, height))
}

/// Paints `region` of `frame` onto the canvas at the frame's position.
///
/// # Safety
///
/// Both images must hold the region at their respective corners.
unsafe fn paint(
    pl: &CPlacement,
    ct: &CompositeTargets,
    frame: *const WebPImage,
    region: Region,
) {
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    let r = Rect {
        x: region.x,
        y: region.y,
        w: region.w,
        h: region.h,
    };
    let argb = unsafe { (*ct.canvas).format } == Format::Argb as c_int;
    let src = unsafe { (*frame).frame() };
    let mut dst = unsafe { (*ct.canvas).frame_mut() };
    let (x, y) = (pl.pos_x, pl.pos_y);

    match (argb, region.blend) {
        (true, true) => blit::blend_argb(
            unsafe { &*ct.ldsp },
            pl.premultiply != 0,
            &mut dst,
            &src,
            r,
            x,
            y,
        ),
        (true, false) => blit::copy_argb(&mut dst, &src, r, x, y),
        (false, true) => blit::blend_yuva(&mut dst, &src, r, x, y),
        (false, false) => blit::copy_yuva(&mut dst, &src, r, x, y),
    }
}

/// Fills a rectangle of the canvas with the background colour.
unsafe fn clear_rect(
    pl: &CPlacement,
    ct: &CompositeTargets,
    pos_x: c_int,
    pos_y: c_int,
    width: c_int,
    height: c_int,
) {
    let argb = unsafe { (*ct.canvas).format } == Format::Argb as c_int;
    let colour = if argb { pl.clear_argb } else { pl.clear_yuva };
    let mut dst = unsafe { (*ct.canvas).frame_mut() };

    blit::clear(
        &mut dst,
        argb,
        colour,
        Rect {
            x: pos_x,
            y: pos_y,
            w: width,
            h: height,
        },
    );
}

/// The canvas holds whichever alpha convention the output format asked for
/// when its pixels were composited, and the caller may change that format
/// between frames. Bring what is already there into the convention the next
/// frame will be blended in, so the two are never mixed.
unsafe fn reconcile_alpha(pl: &CPlacement, ct: &CompositeTargets) {
    let canvas = unsafe { &mut *ct.canvas };

    if !canvas.data[0].is_null()
        && canvas.format == Format::Argb as c_int
        && canvas.premultiplied != pl.premultiply
    {
        let mut view = unsafe { canvas.frame_mut() };

        for y in 0..view.height {
            let row = view.row(0, y);

            if pl.premultiply != 0 {
                unsafe { ((*ct.ydsp).premultiply_row)(row, true) };
            } else {
                premultiply_argb_row(row, true);
            }
        }
    }
    canvas.premultiplied = pl.premultiply;
}

unsafe fn prepare_canvas(
    pl: &CPlacement,
    ct: &CompositeTargets,
    frame: &WebPImage,
    format: c_int,
) -> c_int {
    let covers_canvas = pl.pos_x == 0
        && pl.pos_y == 0
        && frame.width == pl.canvas_width
        && frame.height == pl.canvas_height;
    let (had_canvas, canvas_format) = {
        let canvas = unsafe { &*ct.canvas };

        (!canvas.data[0].is_null(), canvas.format)
    };

    if pl.key_frame != 0 && had_canvas && canvas_format != format {
        unsafe { image_free(ct.canvas) };
    }
    let fresh = unsafe { (*ct.canvas).data[0] }.is_null();

    if fresh {
        let ret = unsafe {
            if format == Format::Argb as c_int {
                image_alloc_argb(ct.canvas, pl.canvas_width, pl.canvas_height)
            } else {
                image_alloc_yuva(ct.canvas, pl.canvas_width, pl.canvas_height)
            }
        };

        if ret < 0 {
            return ret;
        }
        unsafe { (*ct.canvas).premultiplied = pl.premultiply };
    }
    if fresh || pl.key_frame != 0 {
        if !covers_canvas {
            let (w, h) = unsafe { ((*ct.canvas).width, (*ct.canvas).height) };

            unsafe { clear_rect(pl, ct, 0, 0, w, h) };
        }
    } else {
        if format == Format::Argb as c_int
            && unsafe { (*ct.canvas).format } == Format::Yuva420p as c_int
        {
            /* The canvas is its own source here, so it is moved aside whole
            and the converted picture built into the slot it left. */
            let mut yuva: WebPImage = unsafe { *ct.canvas };

            unsafe {
                *ct.canvas = mem::zeroed();
            }
            let ret = unsafe {
                convert_to_argb(
                    &*ct.ydsp,
                    &mut *ct.canvas,
                    &yuva,
                    pl.no_fancy_upsampling != 0,
                )
            };

            unsafe { image_free(&mut yuva) };
            if ret < 0 {
                return ret;
            }
        }
        if pl.prev_anmf_flags & wpd::container::ANMF_FLAG_DISPOSE as c_int != 0 {
            unsafe {
                clear_rect(
                    pl,
                    ct,
                    pl.prev_pos_x,
                    pl.prev_pos_y,
                    pl.prev_width,
                    pl.prev_height,
                )
            };
        }
    }

    unsafe { reconcile_alpha(pl, ct) };
    WPD_OK
}

/// # Safety
///
/// Every pointer must be live, and `sub` must fit the canvas at the
/// placement's position, which the caller checked against the ANMF header.
#[no_mangle]
pub unsafe extern "C" fn anim_composite(
    pl: *const CPlacement,
    ct: *const CompositeTargets,
    sub: *const WebPImage,
    target: c_int,
) -> c_int {
    let (pl, ct) = unsafe { (&*pl, &*ct) };
    let frame = unsafe { &*sub };
    let ret = unsafe { prepare_canvas(pl, ct, frame, target) };

    if ret < 0 {
        return ret;
    }
    /* A frame coded without an alpha plane has nothing to blend with, and a
    planar canvas cannot split the 2x2 chroma block an overlap would land in. */
    let has_alpha_plane = frame.format != Format::Yuv420p as c_int;
    let chroma_aligned = unsafe { (*ct.canvas).format } != Format::Argb as c_int;
    let mut out = [Region {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        blend: false,
    }; 5];
    let n = regions(
        &pl.geometry(),
        frame.width,
        frame.height,
        has_alpha_plane,
        chroma_aligned,
        &mut out,
    );

    for region in &out[..n] {
        unsafe { paint(pl, ct, sub, *region) };
    }
    WPD_OK
}

impl WPDDecoder {
    /// The decoder's answers to what the compositor asks, gathered at the
    /// call. `key_frame` is the one field it does not know yet:
    /// [`anim_is_key_frame`] decides it from the rest.
    fn placement(&self) -> CPlacement {
        CPlacement {
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            anmf_flags: self.anmf_flags,
            frame_index: self.frame_index,
            frame_has_alpha: c_int::from(self.frame_has_alpha),
            key_frame: 0,
            prev_anmf_flags: self.prev_anmf_flags,
            prev_width: self.prev_width,
            prev_height: self.prev_height,
            prev_pos_x: self.prev_pos_x,
            prev_pos_y: self.prev_pos_y,
            prev_key_frame: c_int::from(self.prev_key_frame),
            premultiply: self.premultiply,
            no_fancy_upsampling: self.options.no_fancy_upsampling,
            clear_argb: self.clear_argb,
            clear_yuva: self.clear_yuva,
        }
    }

    /// Where the named sub-frame image lives. The address is taken from the
    /// decoder as a whole rather than through a borrow of one field, so it
    /// stays usable while the others are written.
    pub(crate) fn image_of(&mut self, which: Subframe) -> *mut WebPImage {
        let this: *mut WPDDecoder = self;

        unsafe {
            match which {
                Subframe::Lossy => ptr::addr_of_mut!((*this).subframe),
                Subframe::Argb => ptr::addr_of_mut!((*this).argb),
                Subframe::Converted => ptr::addr_of_mut!((*this).converted),
            }
        }
    }

    /// Decodes one ANMF chunk and composites it onto the canvas.
    ///
    /// # Safety
    ///
    /// `data` must be readable for `size` bytes and sit inside the buffered
    /// window, which is where the alpha offset is measured from.
    pub(crate) unsafe fn decode_anmf(&mut self, data: *const u8, size: usize) -> c_int {
        if size < 16 {
            return WPD_ERROR_INVALID_DATA;
        }
        let end = unsafe { data.add(size) };
        let mut p = data;

        unsafe {
            self.pos_x = rl24(p) as c_int * 2;
            self.pos_y = rl24(p.add(3)) as c_int * 2;
            self.frame_duration = rl24(p.add(12)) as c_int;
            self.anmf_flags = p.add(15).read() as c_int;
        }
        let declared_width = unsafe { rl24(p.add(6)) } as c_int + 1;
        let declared_height = unsafe { rl24(p.add(9)) } as c_int + 1;

        p = unsafe { p.add(16) };

        if self.pos_x + declared_width > self.canvas_width
            || self.pos_y + declared_height > self.canvas_height
        {
            wpd::log::error(&format!(
                "Frame ({declared_width}x{declared_height} at pos {}x{}) does not \
                 fit into canvas ({}x{})",
                self.pos_x, self.pos_y, self.canvas_width, self.canvas_height
            ));
            return WPD_ERROR_INVALID_DATA;
        }

        self.has_alpha = false;
        self.width = 0;
        self.height = 0;

        let mut sub: Option<Subframe> = None;

        while unsafe { end.offset_from(p) } >= 8 {
            let chunk_type = unsafe { rl32(p) };
            let payload_size = unsafe { rl32(p.add(4)) };

            if payload_size == u32::MAX {
                return WPD_ERROR_INVALID_DATA;
            }
            let padded_size = (payload_size + (payload_size & 1)) as usize;

            p = unsafe { p.add(8) };
            if (unsafe { end.offset_from(p) } as usize) < padded_size {
                break;
            }

            match chunk_type {
                TAG_ALPH => {
                    if payload_size == 0 {
                        wpd::log::error("invalid ALPHA chunk size");
                        return WPD_ERROR_INVALID_DATA;
                    }
                    let alpha_header = unsafe { p.read() } as c_int;

                    self.alpha_data_offset = self.stream_offset(unsafe { p.add(1) });
                    self.alpha_data_size = payload_size as c_int - 1;

                    let filter_m = (alpha_header >> 2) & 0x03;
                    let compression = alpha_header & 0x03;

                    if compression > ALPHA_COMPRESSION_VP8L {
                        wpd::log::warning("skipping unsupported ALPHA chunk");
                    } else {
                        self.has_alpha = true;
                        self.alpha_compression = compression;
                        self.alpha_filter = filter_m;
                    }
                }
                TAG_VP8 if sub.is_none() => {
                    let ret = unsafe { self.vp8_lossy_decode_frame(p, payload_size) };

                    if ret < 0 {
                        return ret;
                    }
                    sub = Some(Subframe::Lossy);
                    self.frame_has_alpha = self.has_alpha;
                }
                TAG_VP8L if sub.is_none() => {
                    self.lossless_canvas_in();

                    let ret = unsafe {
                        vp8l_decode_frame(
                            &mut *self.vp8l,
                            VP8L_TARGET_ARGB,
                            &mut self.argb,
                            p,
                            payload_size as c_uint,
                            0,
                        )
                    };

                    self.lossless_canvas_out();
                    if ret < 0 {
                        return ret;
                    }
                    sub = Some(Subframe::Argb);
                    self.frame_has_alpha = self.lossless_has_alpha;
                }
                _ => {}
            }
            p = unsafe { p.add(padded_size) };
        }

        let Some(mut which) = sub else {
            wpd::log::error("image data not found");
            return WPD_ERROR_INVALID_DATA;
        };
        let (sub_width, sub_height, sub_format) = {
            let img = unsafe { &*self.image_of(which) };

            (img.width, img.height, img.format)
        };

        if sub_width != declared_width || sub_height != declared_height {
            wpd::log::warning(&format!(
                "ANMF declares {declared_width}x{declared_height} but the image is \
                 {sub_width}x{sub_height}"
            ));
        }
        if self.pos_x + sub_width > self.canvas_width
            || self.pos_y + sub_height > self.canvas_height
        {
            wpd::log::error(&format!(
                "Frame ({sub_width}x{sub_height} at pos {}x{}) does not fit into \
                 canvas ({}x{})",
                self.pos_x, self.pos_y, self.canvas_width, self.canvas_height
            ));
            return WPD_ERROR_INVALID_DATA;
        }

        let mut pl = self.placement();

        self.key_frame = unsafe { anim_is_key_frame(&pl, sub_width, sub_height) } != 0;
        pl.key_frame = c_int::from(self.key_frame);

        let argb = Format::Argb as c_int;
        let mut target = Format::Yuva420p as c_int;

        if sub_format == argb
            || format_is_packed(self.out_format)
            || (!self.key_frame
                && !self.canvas.data[0].is_null()
                && self.canvas.format == argb)
        {
            target = argb;
        }

        if target == argb && sub_format != argb {
            let this: *mut WPDDecoder = self;
            let src = self.image_of(which);
            let ret = unsafe {
                convert_to_argb(
                    &(*this).ydsp,
                    &mut (*this).converted,
                    &*src,
                    self.options.no_fancy_upsampling != 0,
                )
            };

            if ret < 0 {
                return ret;
            }
            which = Subframe::Converted;
        }

        /* libwebp premultiplies each frame before compositing it, which is not
        the same as premultiplying the finished canvas. Premultiplying only ever
        goes with a packed output format, which forces the ARGB target above, so
        'sub' is four-byte ARGB here whatever the frame coded as. A sub-frame
        feeds no canvas, so a two-byte output premultiplies after the pack
        instead, in the four-bit domain a still uses. */
        if self.premultiply != 0
            && !(premultiply_after_pack(self.animation, self.anim_mode)
                && format_bpp(self.out_format) == 2)
        {
            let this: *mut WPDDecoder = self;
            let img = unsafe { &mut *self.image_of(which) };
            let mut view = unsafe { img.frame_mut() };

            for y in 0..view.height {
                unsafe { ((*this).ydsp.premultiply_row)(view.row(0, y), true) };
            }
        }

        self.subframe_out = Some(which);

        /* Sub-frame mode owns no canvas, so it skips the allocation and the
        blend altogether; the dispose latch below is bookkeeping the canvas never
        fed. Nothing above reads the canvas except the ARGB target rule, which
        wants a canvas to stay compatible with and correctly declines when there
        is none. Switching modes mid-animation is refused for that reason. */
        if self.anim_mode != WPD_ANIM_SUBFRAME {
            let this: *mut WPDDecoder = self;
            let src = self.image_of(which);
            let ct = unsafe {
                CompositeTargets {
                    ldsp: ptr::addr_of!((*this).ldsp),
                    ydsp: ptr::addr_of!((*this).ydsp),
                    canvas: ptr::addr_of_mut!((*this).canvas),
                }
            };
            let ret = unsafe { anim_composite(&pl, &ct, src, target) };

            if ret < 0 {
                return ret;
            }
        }

        self.frame_timestamp += self.frame_duration as i64;
        self.prev_anmf_flags = self.anmf_flags;
        self.prev_width = sub_width;
        self.prev_height = sub_height;
        self.prev_pos_x = self.pos_x;
        self.prev_pos_y = self.pos_y;
        self.prev_key_frame = self.key_frame;
        self.frame_index += 1;

        WPD_OK
    }
}
