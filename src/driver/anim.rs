use std::mem;

use crate::anim::{regions, AnimState, Placement, Region};
use crate::bits::{rl24, rl32};
use crate::blit::{self, Rect};
use crate::container::TAG_ANMF;
use crate::dsp::vp8l::Vp8lDsp;
use crate::dsp::yuv::YuvDsp;
use crate::error::{Error, Result};
use crate::image::Format;
use crate::picture::{Buffer, Frame};

use super::convert::{convert_to_argb, format_is_packed, format_is_premultiplied};
use super::slot::{AheadEntry, FrameEnv, FrameSlot};
use super::{Decoder, InputMode, Source, ANIM_SUBFRAME};

pub struct CPlacement {
    pub geom: Placement,
    pub premultiply: bool,
    pub no_fancy_upsampling: bool,
    pub clear_argb: [u8; 4],
    pub clear_yuva: [u8; 4],
    pub threads: usize,
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
                (ydsp.premultiply_argb_row)(row, true);
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

            convert_to_argb(
                ydsp,
                canvas,
                &yuva.frame(),
                pl.no_fancy_upsampling,
                pl.threads,
            )?;
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
            premultiply: format_is_premultiplied(self.out_format.0),
            no_fancy_upsampling: self.options.no_fancy_upsampling,
            clear_argb: self.clear_argb,
            clear_yuva: self.clear_yuva.0,
            threads: self.threads.0,
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

    /// How many frames may be decoded ahead of the one being composited.
    ///
    /// Bounded by threads, by a ceiling of eight, and by memory: every slot
    /// holds a decoded frame, so the count comes down as the canvas grows. A
    /// canvas too small to earn a thread, a streamed animation, and an
    /// animation whose input may still be replaced under it all get one.
    fn ahead_count(&self) -> usize {
        const MAX_SLOTS: usize = 8;
        const MIN_CANVAS_PIXELS: i64 = 96 * 96;
        /* Y, U, V, alpha and the ARGB a sub-frame may be converted into. */
        const BYTES_PER_PIXEL: i64 = 6;
        const BUDGET: i64 = 96 << 20;

        if self.threads.0 < 2 || self.streaming || !self.eos {
            return 1;
        }
        if self.input_mode != InputMode::Untouched {
            return 1;
        }

        let pixels = i64::from(self.canvas_width) * i64::from(self.canvas_height);

        if pixels < MIN_CANVAS_PIXELS {
            return 1;
        }

        let by_memory = BUDGET / (pixels * BYTES_PER_PIXEL).max(1);

        (by_memory.max(1) as usize)
            .min(self.threads.0)
            .min(MAX_SLOTS)
    }

    /// The ANMF payloads from `first` onwards, at most `want` of them. The
    /// walk that calls this has already stepped past `first`, so self.pos is
    /// where the frame after it begins.
    fn anmf_lookahead(&self, first: (usize, usize), want: usize) -> Vec<AheadEntry> {
        let mut found = Vec::with_capacity(want);
        let mut at = self.pos;

        found.push(AheadEntry {
            base: first.0,
            size: first.1,
            out: Err(Error::InvalidData),
        });

        while found.len() < want && at + 8 <= self.end {
            let (chunk_type, size) = {
                let head = self.file_at(at);

                (rl32(head), rl32(&head[4..]))
            };

            /* Anything that is not another frame ends the run: the metadata
             * that follows the last one is not worth walking past. */
            if chunk_type != TAG_ANMF || size == u32::MAX {
                break;
            }

            let size = size as usize;
            let padded = size + (size & 1);

            if self.end - (at + 8) < padded {
                break;
            }
            found.push(AheadEntry {
                base: at + 8,
                size,
                out: Err(Error::InvalidData),
            });
            at += 8 + padded;
        }
        found
    }

    /// Decodes the next run of frames into a slot each. Their images depend on
    /// nothing but their own bytes, so they are independent; everything that
    /// depends on the frames before it stays in decode_anmf().
    fn fill_ahead(&mut self, base: usize, size: usize) {
        let want = self.ahead_count();

        if want < 2 {
            return;
        }

        let entries = self.anmf_lookahead((base, size), want);

        if entries.len() < 2 {
            return;
        }

        if self.ahead.slots.len() < entries.len() {
            let more = entries.len() - self.ahead.slots.len();

            if self.ahead.slots.try_reserve(more).is_err() {
                return;
            }
            self.ahead
                .slots
                .resize_with(entries.len(), FrameSlot::default);
        }
        self.ahead.entries = entries;
        self.ahead.pos = 0;

        let bypass_filtering = self.filter_bypass();
        let to_argb = self.frame_to_argb();
        let premultiply = self.frame_premultiply();
        let no_fancy_upsampling = self.options.no_fancy_upsampling;
        let threads = self.threads.0;
        let Self {
            ahead,
            input,
            ldsp,
            fdsp,
            ydsp,
            ..
        } = self;
        /* Each slot is already on a thread of its own, so a frame does not
         * also split its alpha or its conversion off onto another one. */
        let env = FrameEnv {
            input,
            ldsp,
            fdsp,
            ydsp,
            bypass_filtering,
            no_fancy_upsampling,
            to_argb,
            premultiply,
            threads: 1,
        };
        let mut jobs: Vec<(&mut FrameSlot, &mut AheadEntry)> = ahead
            .slots
            .iter_mut()
            .zip(ahead.entries.iter_mut())
            .collect();

        crate::task::for_each(threads, &mut jobs, |(slot, entry)| {
            entry.out = slot.decode_anmf_image(&env, entry.base, entry.size);
        });
    }

    /// Takes the frame decoded ahead for the chunk at `base`, if there is one.
    /// The slot it was decoded into is swapped in whole, which recycles the
    /// buffers the outgoing frame was using.
    fn take_ahead(&mut self, base: usize) -> Option<Result<Source>> {
        let i = self.ahead.pos;
        let entry = *self.ahead.entries.get(i)?;

        if entry.base != base {
            /* The walk did not arrive where the batch expected, so the batch
             * is about something else; drop it and decode here. */
            self.ahead.clear();
            return None;
        }
        self.ahead.pos += 1;
        std::mem::swap(&mut self.frame, &mut self.ahead.slots[i]);
        Some(entry.out)
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

        if self.ahead.spent() {
            self.fill_ahead(base, size);
        }

        let sub = match self.take_ahead(base) {
            Some(out) => out,
            None => {
                let (frame, env) = self.frame_parts();

                frame.decode_anmf_image(&env, base, size)
            }
        };

        self.alpha_pending = false;

        let mut which = sub?;

        self.anim.frame_has_alpha = self.frame.frame_has_alpha(which);
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
            || format_is_packed(self.out_format.0)
            || (!self.anim.key_frame
                && !self.canvas.is_empty()
                && self.canvas.format == Some(argb))
        {
            target = argb;
        }

        which = {
            let (frame, env) = self.frame_parts();

            frame.prepare(&env, which, target == argb)?
        };

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
            frame,
            ..
        } = self;
        let src = super::source_view(which, frame, None);

        anim_composite(pl, CompositeTargets { ldsp, ydsp, canvas }, &src, target)
    }
}
