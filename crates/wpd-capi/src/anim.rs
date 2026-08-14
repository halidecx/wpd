//! C ABI for the animation compositor, as declared by `src/anim.h`.
//!
//! The geometry — whether a frame stands on its own, and how it divides into
//! blended and copied regions — is [`wpd::anim`]. What is here brings the
//! canvas into the format the next frame will be composited in, disposes what
//! the frame before asked to be disposed, and walks the regions.

use std::ffi::c_int;
use std::mem;

use wpd::anim::{regions, Placement, Region};
use wpd::image::{ceil_rshift, Format};

use crate::convert::{blend_argb_region, convert_to_argb, copy_argb_region, SubRect};
use crate::convert::{blend_yuva_region, copy_yuva_region};
use crate::dsp::vp8l::WPDLosslessDSP;
use crate::dsp::yuv::WPDYUVDSP;
use crate::image::{image_alloc_argb, image_alloc_yuva, image_free, WebPImage};
use crate::rescale::wpd_premultiply_argb_row;

const WPD_OK: c_int = 0;

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
    pub ldsp: *const WPDLosslessDSP,
    pub ydsp: *const WPDYUVDSP,
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
    let r = SubRect {
        x: region.x,
        y: region.y,
        w: region.w,
        h: region.h,
    };
    let argb = unsafe { (*ct.canvas).format } == Format::Argb as c_int;

    unsafe {
        match (argb, region.blend) {
            (true, true) => blend_argb_region(
                ct.ldsp,
                pl.premultiply,
                ct.canvas,
                frame,
                r,
                pl.pos_x,
                pl.pos_y,
            ),
            (true, false) => copy_argb_region(ct.canvas, frame, r, pl.pos_x, pl.pos_y),
            (false, true) => blend_yuva_region(ct.canvas, frame, r, pl.pos_x, pl.pos_y),
            (false, false) => copy_yuva_region(ct.canvas, frame, r, pl.pos_x, pl.pos_y),
        }
    }
}

/// Fills a rectangle of the canvas with the background colour, in whichever of
/// the two canvas formats is in use.
unsafe fn clear_rect(
    pl: &CPlacement,
    ct: &CompositeTargets,
    pos_x: c_int,
    pos_y: c_int,
    width: c_int,
    height: c_int,
) {
    let canvas = unsafe { &*ct.canvas };

    if canvas.format == Format::Argb as c_int {
        for y in 0..height {
            let row =
                unsafe { canvas.row_mut(0, pos_y + y, (pos_x + width) as usize * 4) };

            for px in row[pos_x as usize * 4..].chunks_exact_mut(4) {
                px.copy_from_slice(&pl.clear_argb);
            }
        }
        return;
    }
    for comp in 0..4 {
        let shift = u32::from(comp == 1 || comp == 2);
        let from = (pos_x >> shift) as usize;
        let len = ceil_rshift(width, shift) as usize;

        for y in 0..ceil_rshift(height, shift) {
            let row = unsafe { canvas.row_mut(comp, (pos_y >> shift) + y, from + len) };

            row[from..].fill(pl.clear_yuva[comp]);
        }
    }
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
        for y in 0..canvas.height {
            let row = unsafe {
                canvas.data[0].offset(y as isize * canvas.linesize[0] as isize)
            };

            unsafe {
                if pl.premultiply != 0 {
                    ((*ct.ydsp).premultiply_row)(row, 1, canvas.width);
                } else {
                    wpd_premultiply_argb_row(row, canvas.width, 1);
                }
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
                convert_to_argb(ct.ydsp, ct.canvas, &yuva, pl.no_fancy_upsampling)
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
