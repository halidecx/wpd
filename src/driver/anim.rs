use std::mem;

use crate::anim::{regions, AnimState, Placement, Region};
use crate::error::{Error, Result};
use crate::image::Format;

use crate::blit::{self, Rect};
use crate::dsp::vp8l::Vp8lDsp;

use super::convert::{
    convert_to_argb, format_bpp, format_is_packed, format_is_premultiplied,
    premultiply_after_pack,
};
use super::{Decoder, Source, ANIM_SUBFRAME};
use crate::bits::{rl24, rl32};
use crate::container::{TAG_ALPH, TAG_VP8, TAG_VP8L};
use crate::dsp::yuv::YuvDsp;
use crate::picture::{Buffer, Frame};
use crate::rescale::premultiply_argb_row;

pub struct CPlacement {
    pub geom: Placement,
    pub premultiply: bool,
    pub no_fancy_upsampling: bool,
    pub clear_argb: [u8; 4],
    pub clear_yuva: [u8; 4],
}

pub struct CompositeTargets<'a> {
    pub ldsp: &'a Vp8lDsp,
    pub ydsp: &'a YuvDsp,
    pub canvas: &'a mut Buffer,
}

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
    let (x, y) = (pl.geom.frame.pos_x, pl.geom.frame.pos_y);

    match (argb, region.blend) {
        (true, true) => {
            blit::blend_argb(ldsp, pl.premultiply, &mut dst, frame, r, x, y)
        }
        (true, false) => blit::copy_argb(&mut dst, frame, r, x, y),
        (false, true) => blit::blend_yuva(&mut dst, frame, r, x, y),
        (false, false) => blit::copy_yuva(&mut dst, frame, r, x, y),
    }
}

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
    let covers_canvas = pl.geom.frame.pos_x == 0
        && pl.geom.frame.pos_y == 0
        && frame.width == pl.geom.canvas_width
        && frame.height == pl.geom.canvas_height;

    if pl.geom.frame.key_frame && !canvas.is_empty() && canvas.format != Some(format) {
        canvas.release();
    }

    let fresh = canvas.is_empty();

    if fresh {
        let alloc = if format == Format::Argb {
            canvas.alloc_argb(pl.geom.canvas_width, pl.geom.canvas_height)
        } else {
            canvas.alloc_planar(pl.geom.canvas_width, pl.geom.canvas_height, true)
        };

        alloc?;
        canvas.premultiplied = pl.premultiply;
    }
    if fresh || pl.geom.frame.key_frame {
        if !covers_canvas {
            let (w, h) = (canvas.width, canvas.height);

            clear_rect(pl, canvas, 0, 0, w, h);
        }
    } else {
        if format == Format::Argb && canvas.format == Some(Format::Yuva420p) {
            let yuva = mem::take(canvas);

            convert_to_argb(ydsp, canvas, &yuva.frame(), pl.no_fancy_upsampling)?;
        }
        if pl.geom.frame.prev_anmf_flags & crate::container::ANMF_FLAG_DISPOSE != 0 {
            clear_rect(
                pl,
                canvas,
                pl.geom.frame.prev_pos_x,
                pl.geom.frame.prev_pos_y,
                pl.geom.frame.prev_width,
                pl.geom.frame.prev_height,
            );
        }
    }

    reconcile_alpha(pl, ydsp, canvas);
    Ok(())
}

pub fn anim_composite(
    pl: &CPlacement,
    ct: CompositeTargets<'_>,
    frame: &Frame<'_>,
    target: Format,
) -> Result<()> {
    let CompositeTargets { ldsp, ydsp, canvas } = ct;

    prepare_canvas(pl, ydsp, canvas, frame, target)?;
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
        &pl.geom,
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
    fn placement(&self) -> CPlacement {
        CPlacement {
            geom: Placement {
                canvas_width: self.canvas_width,
                canvas_height: self.canvas_height,
                frame: AnimState {
                    key_frame: false,
                    ..self.anim
                },
            },
            premultiply: format_is_premultiplied(self.out_format),
            no_fancy_upsampling: self.options.no_fancy_upsampling,
            clear_argb: self.clear_argb,
            clear_yuva: self.clear_yuva,
        }
    }

    fn read_anmf_header(&mut self, header: &[u8]) -> Option<(i32, i32)> {
        if header.len() < 16 {
            return None;
        }
        self.anim.pos_x = rl24(header) as i32 * 2;
        self.anim.pos_y = rl24(&header[3..]) as i32 * 2;
        self.frame_duration = rl24(&header[12..]) as i32;
        self.anim.anmf_flags = header[15];
        Some((rl24(&header[6..]) as i32 + 1, rl24(&header[9..]) as i32 + 1))
    }

    pub(crate) fn decode_anmf(&mut self, base: usize, size: usize) -> Result<()> {
        let mut header = [0; 16];
        let available = self.input.chunk(base, size.min(16));

        header[..available.len()].copy_from_slice(available);
        let Some((declared_width, declared_height)) =
            self.read_anmf_header(&header[..available.len()])
        else {
            return Err(Error::InvalidData);
        };

        if self.anim.pos_x + declared_width > self.canvas_width
            || self.anim.pos_y + declared_height > self.canvas_height
        {
            crate::log::error_args(format_args!(
                "Frame ({declared_width}x{declared_height} at pos {}x{}) does not \
                 fit into canvas ({}x{})",
                self.anim.pos_x, self.anim.pos_y, self.canvas_width, self.canvas_height
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
                    if sub.is_some() {
                        crate::log::error("ALPHA chunk after the image it belongs to");
                        return Err(Error::InvalidData);
                    }
                    let header = self.input.chunk(at, 1)[0] as i32;

                    self.set_alpha_chunk(header, at + 1, payload_size - 1)?;
                }
                TAG_VP8 if sub.is_none() => {
                    self.vp8_lossy_decode_frame(at, payload_size)?;
                    sub = Some(Source::Lossy);
                    self.anim.frame_has_alpha = self.has_alpha;
                }
                TAG_VP8L if sub.is_none() => {
                    self.lossless_decode(at, payload_size)?;
                    sub = Some(Source::Lossless);
                    self.anim.frame_has_alpha = self.lossless_has_alpha;
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
        if self.anim.pos_x + sub_width > self.canvas_width
            || self.anim.pos_y + sub_height > self.canvas_height
        {
            crate::log::error_args(format_args!(
                "Frame ({sub_width}x{sub_height} at pos {}x{}) does not fit into \
                 canvas ({}x{})",
                self.anim.pos_x, self.anim.pos_y, self.canvas_width, self.canvas_height
            ));
            return Err(Error::InvalidData);
        }

        let mut pl = self.placement();

        self.anim.key_frame = pl.geom.is_key_frame(sub_width, sub_height);
        pl.geom.frame.key_frame = self.anim.key_frame;

        let argb = Format::Argb;
        let mut target = Format::Yuva420p;

        if sub_format == argb
            || format_is_packed(self.out_format)
            || (!self.anim.key_frame
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

        /* libwebp premultiplies frames before compositing, so this uses ARGB. */
        if format_is_premultiplied(self.out_format)
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

        /* Sub-frame mode has no canvas to allocate or blend. */
        if self.anim_mode != ANIM_SUBFRAME {
            self.composite(&pl, which, target)?;
        }

        self.frame_timestamp += self.frame_duration as i64;
        self.anim.prev_anmf_flags = self.anim.anmf_flags;
        self.anim.prev_width = sub_width;
        self.anim.prev_height = sub_height;
        self.anim.prev_pos_x = self.anim.pos_x;
        self.anim.prev_pos_y = self.anim.pos_y;
        self.anim.prev_key_frame = self.anim.key_frame;
        self.anim.frame_index += 1;

        Ok(())
    }

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
