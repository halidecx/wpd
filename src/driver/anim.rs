//! The animation compositor's plumbing, as `src/anim.c` did it.
//!
//! The geometry — whether a frame stands on its own, and how it divides into
//! blended and copied regions — is [`crate::anim`]. What is here brings the
//! canvas into the format the next frame will be composited in, disposes what
//! the frame before asked to be disposed, and walks the regions.
//!
//! The canvas is the decoder's own buffer and the frame being composited is
//! one of the codecs' pictures, so both arrive as borrows taken from one
//! destructuring of the decoder. That is what says they are not the same
//! memory — except in the one case where they are, which
//! `prepare_canvas` moves aside by name.

use std::mem;

use crate::anim::{regions, Placement, Region};
use crate::error::{Error, Result};
use crate::image::Format;

use crate::blit::{self, Rect};
use crate::dsp::vp8l::Vp8lDsp;

use super::convert::{
    convert_to_argb, format_bpp, format_is_packed, premultiply_after_pack,
};
use super::{Decoder, Source, ANIM_SUBFRAME, TAG_ALPH, TAG_VP8, TAG_VP8L};
use crate::bits::{rl24, rl32};
use crate::dsp::yuv::YuvDsp;
use crate::picture::{Buffer, Frame};
use crate::rescale::premultiply_argb_row;

/// Everything the compositor asks the decoder about a frame's placement,
/// gathered at the call.
pub struct CPlacement {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub pos_x: i32,
    pub pos_y: i32,
    pub anmf_flags: i32,
    pub frame_index: i32,
    pub frame_has_alpha: bool,
    pub key_frame: bool,
    pub prev_anmf_flags: i32,
    pub prev_width: i32,
    pub prev_height: i32,
    pub prev_pos_x: i32,
    pub prev_pos_y: i32,
    pub prev_key_frame: bool,
    pub premultiply: bool,
    pub no_fancy_upsampling: bool,
    pub clear_argb: [u8; 4],
    pub clear_yuva: [u8; 4],
}

/// The tables the compositor dispatches through, and the canvas it paints on.
pub struct CompositeTargets<'a> {
    pub ldsp: &'a Vp8lDsp,
    pub ydsp: &'a YuvDsp,
    pub canvas: &'a mut Buffer,
}

impl CPlacement {
    fn geometry(&self) -> Placement {
        Placement {
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            anmf_flags: self.anmf_flags as u8,
            frame_index: self.frame_index,
            frame_has_alpha: self.frame_has_alpha,
            key_frame: self.key_frame,
            prev_anmf_flags: self.prev_anmf_flags as u8,
            prev_width: self.prev_width,
            prev_height: self.prev_height,
            prev_pos_x: self.prev_pos_x,
            prev_pos_y: self.prev_pos_y,
            prev_key_frame: self.prev_key_frame,
        }
    }
}

/// Paints `region` of `frame` onto the canvas at the frame's position.
fn paint(
    pl: &CPlacement,
    ldsp: &Vp8lDsp,
    canvas: &mut Buffer,
    frame: &Frame<'_>,
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
    let argb = canvas.format == Some(Format::Argb);
    let mut dst = canvas.frame_mut();
    let (x, y) = (pl.pos_x, pl.pos_y);

    match (argb, region.blend) {
        (true, true) => {
            blit::blend_argb(ldsp, pl.premultiply, &mut dst, frame, r, x, y)
        }
        (true, false) => blit::copy_argb(&mut dst, frame, r, x, y),
        (false, true) => blit::blend_yuva(&mut dst, frame, r, x, y),
        (false, false) => blit::copy_yuva(&mut dst, frame, r, x, y),
    }
}

/// Fills a rectangle of the canvas with the background colour.
fn clear_rect(
    pl: &CPlacement,
    canvas: &mut Buffer,
    pos_x: i32,
    pos_y: i32,
    width: i32,
    height: i32,
) {
    let argb = canvas.format == Some(Format::Argb);
    let colour = if argb { pl.clear_argb } else { pl.clear_yuva };
    let mut dst = canvas.frame_mut();

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
fn reconcile_alpha(pl: &CPlacement, ydsp: &YuvDsp, canvas: &mut Buffer) {
    if !canvas.is_empty()
        && canvas.format == Some(Format::Argb)
        && canvas.premultiplied != pl.premultiply
    {
        let mut view = canvas.frame_mut();

        for y in 0..view.height {
            let row = view.row(0, y);

            if pl.premultiply {
                (ydsp.premultiply_row)(row, true);
            } else {
                premultiply_argb_row(row, true);
            }
        }
    }
    canvas.premultiplied = pl.premultiply;
}

fn prepare_canvas(
    pl: &CPlacement,
    ydsp: &YuvDsp,
    canvas: &mut Buffer,
    frame: &Frame<'_>,
    format: Format,
) -> Result<()> {
    let covers_canvas = pl.pos_x == 0
        && pl.pos_y == 0
        && frame.width == pl.canvas_width
        && frame.height == pl.canvas_height;

    if pl.key_frame && !canvas.is_empty() && canvas.format != Some(format) {
        canvas.release();
    }

    let fresh = canvas.is_empty();

    if fresh {
        let alloc = if format == Format::Argb {
            canvas.alloc_argb(pl.canvas_width, pl.canvas_height)
        } else {
            canvas.alloc_planar(pl.canvas_width, pl.canvas_height, true)
        };

        alloc?;
        canvas.premultiplied = pl.premultiply;
    }
    if fresh || pl.key_frame {
        if !covers_canvas {
            let (w, h) = (canvas.width, canvas.height);

            clear_rect(pl, canvas, 0, 0, w, h);
        }
    } else {
        if format == Format::Argb && canvas.format == Some(Format::Yuva420p) {
            /* The canvas is its own source here, so it is moved aside whole
            and the converted picture built into the slot it left. */
            let yuva = mem::take(canvas);

            convert_to_argb(ydsp, canvas, &yuva.frame(), pl.no_fancy_upsampling)?;
        }
        if pl.prev_anmf_flags & crate::container::ANMF_FLAG_DISPOSE as i32 != 0 {
            clear_rect(
                pl,
                canvas,
                pl.prev_pos_x,
                pl.prev_pos_y,
                pl.prev_width,
                pl.prev_height,
            );
        }
    }

    reconcile_alpha(pl, ydsp, canvas);
    Ok(())
}

/// Composites one decoded sub-frame onto the canvas.
pub fn anim_composite(
    pl: &CPlacement,
    ct: CompositeTargets<'_>,
    frame: &Frame<'_>,
    target: Format,
) -> Result<()> {
    let CompositeTargets { ldsp, ydsp, canvas } = ct;

    prepare_canvas(pl, ydsp, canvas, frame, target)?;
    /* A frame coded without an alpha plane has nothing to blend with, and a
    planar canvas cannot split the 2x2 chroma block an overlap would land in. */
    let has_alpha_plane = frame.format != Format::Yuv420p;
    let chroma_aligned = canvas.format != Some(Format::Argb);
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
        paint(pl, ldsp, canvas, frame, *region);
    }
    Ok(())
}

impl<'a> Decoder<'a> {
    /// The decoder's answers to what the compositor asks, gathered at the
    /// call. `key_frame` is the one field it does not know yet:
    /// [`Placement::is_key_frame`] decides it from the rest.
    fn placement(&self) -> CPlacement {
        CPlacement {
            canvas_width: self.canvas_width,
            canvas_height: self.canvas_height,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            anmf_flags: self.anmf_flags,
            frame_index: self.frame_index,
            frame_has_alpha: self.frame_has_alpha,
            key_frame: false,
            prev_anmf_flags: self.prev_anmf_flags,
            prev_width: self.prev_width,
            prev_height: self.prev_height,
            prev_pos_x: self.prev_pos_x,
            prev_pos_y: self.prev_pos_y,
            prev_key_frame: self.prev_key_frame,
            premultiply: self.premultiply != 0,
            no_fancy_upsampling: self.options.no_fancy_upsampling,
            clear_argb: self.clear_argb,
            clear_yuva: self.clear_yuva,
        }
    }

    /// Reads the ANMF header and latches what the frame declares.
    ///
    /// Returns the declared size, or nothing when the chunk is too short to
    /// carry one.
    fn read_anmf_header(&mut self, header: &[u8]) -> Option<(i32, i32)> {
        if header.len() < 16 {
            return None;
        }
        self.pos_x = rl24(header) as i32 * 2;
        self.pos_y = rl24(&header[3..]) as i32 * 2;
        self.frame_duration = rl24(&header[12..]) as i32;
        self.anmf_flags = header[15] as i32;
        Some((rl24(&header[6..]) as i32 + 1, rl24(&header[9..]) as i32 + 1))
    }

    /// Decodes one ANMF chunk and composites it onto the canvas.
    ///
    /// `base` is where the chunk's payload sits in the stream, which is what
    /// the alpha offset is measured from.
    pub(crate) fn decode_anmf(&mut self, base: usize, size: usize) -> Result<()> {
        let mut header = [0; 16];
        let available = self.input.chunk(base, size.min(16));

        header[..available.len()].copy_from_slice(available);
        let Some((declared_width, declared_height)) =
            self.read_anmf_header(&header[..available.len()])
        else {
            return Err(Error::InvalidData);
        };

        if self.pos_x + declared_width > self.canvas_width
            || self.pos_y + declared_height > self.canvas_height
        {
            crate::log::error_args(format_args!(
                "Frame ({declared_width}x{declared_height} at pos {}x{}) does not \
                 fit into canvas ({}x{})",
                self.pos_x, self.pos_y, self.canvas_width, self.canvas_height
            ));
            return Err(Error::InvalidData);
        }

        self.has_alpha = false;
        self.width = 0;
        self.height = 0;

        let mut sub: Option<Source> = None;
        let mut at = base + 16;
        let end = base + size;

        while end - at >= 8 {
            let (chunk_type, payload_size) = {
                let head = self.input.chunk(at, 8);

                if head.len() < 8 {
                    break;
                }
                (rl32(head), rl32(&head[4..]))
            };

            if payload_size == u32::MAX {
                return Err(Error::InvalidData);
            }
            let payload_size = payload_size as usize;
            let padded_size = payload_size + (payload_size & 1);

            at += 8;
            if end - at < padded_size {
                break;
            }

            match chunk_type {
                TAG_ALPH => {
                    if payload_size == 0 {
                        crate::log::error("invalid ALPHA chunk size");
                        return Err(Error::InvalidData);
                    }
                    let header = self.input.chunk(at, 1)[0] as i32;

                    self.set_alpha_chunk(header, at + 1, payload_size - 1);
                }
                TAG_VP8 if sub.is_none() => {
                    self.vp8_lossy_decode_frame(at, payload_size)?;
                    sub = Some(Source::Lossy);
                    self.frame_has_alpha = self.has_alpha;
                }
                TAG_VP8L if sub.is_none() => {
                    self.lossless_decode(at, payload_size)?;
                    sub = Some(Source::Lossless);
                    self.frame_has_alpha = self.lossless_has_alpha;
                }
                _ => {}
            }
            at += padded_size;
        }

        let Some(mut which) = sub else {
            crate::log::error("image data not found");
            return Err(Error::InvalidData);
        };
        let (sub_width, sub_height, sub_format) = {
            let img = self.frame_of(which);

            (img.width, img.height, img.format)
        };

        if sub_width != declared_width || sub_height != declared_height {
            crate::log::warning_args(format_args!(
                "ANMF declares {declared_width}x{declared_height} but the image is \
                 {sub_width}x{sub_height}"
            ));
        }
        if self.pos_x + sub_width > self.canvas_width
            || self.pos_y + sub_height > self.canvas_height
        {
            crate::log::error_args(format_args!(
                "Frame ({sub_width}x{sub_height} at pos {}x{}) does not fit into \
                 canvas ({}x{})",
                self.pos_x, self.pos_y, self.canvas_width, self.canvas_height
            ));
            return Err(Error::InvalidData);
        }

        let mut pl = self.placement();

        self.key_frame = pl.geometry().is_key_frame(sub_width, sub_height);
        pl.key_frame = self.key_frame;

        let argb = Format::Argb;
        let mut target = Format::Yuva420p;

        if sub_format == argb
            || format_is_packed(self.out_format)
            || (!self.key_frame
                && !self.canvas.is_empty()
                && self.canvas.format == Some(argb))
        {
            target = argb;
        }

        if target == argb && sub_format != argb {
            let no_fancy = self.options.no_fancy_upsampling;
            let Self {
                ydsp,
                converted,
                vp8,
                alpha_plane,
                has_alpha,
                width,
                height,
                ..
            } = self;
            let src = super::lossy_view(
                vp8.first(),
                alpha_plane,
                *has_alpha,
                *width,
                *height,
            );
            convert_to_argb(ydsp, converted, &src, no_fancy)?;
            which = Source::Converted;
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
            let Self {
                ydsp,
                converted,
                vp8l,
                lossless_out,
                ..
            } = self;
            /* The ARGB target rule above leaves only these two: a frame that
            was converted, and a lossless one, which is written back into the
            codec's own canvas as the C did through its latched pointer. */
            let view = match which {
                Source::Converted => Some(converted.frame_mut()),
                Source::Lossless => lossless_out.and_then(|which| vp8l.view_mut(which)),
                Source::Lossy | Source::Canvas | Source::None => None,
            };

            if let Some(mut view) = view {
                for y in 0..view.height {
                    (ydsp.premultiply_row)(view.row(0, y), true);
                }
            }
        }

        self.subframe_out = Some(which);

        /* Sub-frame mode owns no canvas, so it skips the allocation and the
        blend altogether; the dispose latch below is bookkeeping the canvas never
        fed. Nothing above reads the canvas except the ARGB target rule, which
        wants a canvas to stay compatible with and correctly declines when there
        is none. Switching modes mid-animation is refused for that reason. */
        if self.anim_mode != ANIM_SUBFRAME {
            self.composite(&pl, which, target)?;
        }

        self.frame_timestamp += self.frame_duration as i64;
        self.prev_anmf_flags = self.anmf_flags;
        self.prev_width = sub_width;
        self.prev_height = sub_height;
        self.prev_pos_x = self.pos_x;
        self.prev_pos_y = self.pos_y;
        self.prev_key_frame = self.key_frame;
        self.frame_index += 1;

        Ok(())
    }

    /// The canvas, the tables and the sub-frame, taken from one destructuring
    /// so that the compositor cannot be handed the canvas as its own source.
    fn composite(
        &mut self,
        pl: &CPlacement,
        which: Source,
        target: Format,
    ) -> Result<()> {
        let Self {
            ldsp,
            ydsp,
            canvas,
            converted,
            vp8,
            vp8l,
            lossless_out,
            alpha_plane,
            has_alpha,
            width,
            height,
            ..
        } = self;
        let src = super::source_view(
            which,
            vp8.first(),
            vp8l,
            *lossless_out,
            alpha_plane,
            *has_alpha,
            *width,
            *height,
            Some(converted),
            None,
        );

        anim_composite(pl, CompositeTargets { ldsp, ydsp, canvas }, &src, target)
    }
}
