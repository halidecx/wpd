use crate::dsp::filters::FilterDsp;
use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{Error, Result, Status};
use crate::input::Input;
use crate::picture::PlaneMut;
use crate::vp8l::{AlphaDst, Target};

use super::convert::scaled_size;
use super::slot::{FrameEnv, FrameSlot};
use super::{Decoder, ALPHA_COMPRESSION_NONE, ALPHA_COMPRESSION_VP8L};

/// Below this a frame's alpha plane is decoded here rather than beside the
/// colour planes. Measured: a 160x120 frame with a nearly uniform alpha loses
/// 11us, one whole spawn, because its alpha is done before the thread that
/// took it exists; a 200x200 one breaks even, and from 300x300 up it wins.
const ALPHA_THREAD_PIXELS: usize = 256 * 256;

const ALPHA_FILTER_NONE: i32 = 0;
const ALPHA_FILTER_HORIZONTAL: i32 = 1;
const ALPHA_FILTER_VERTICAL: i32 = 2;
const ALPHA_FILTER_GRADIENT: i32 = 3;

fn alpha_inverse_prediction(
    dsp: &FilterDsp,
    plane: &mut PlaneMut<'_>,
    width: usize,
    height: i32,
    mode: i32,
) {
    if width == 0 || height == 0 {
        return;
    }

    let unfilter = match mode {
        ALPHA_FILTER_HORIZONTAL => dsp.horizontal_unfilter,
        ALPHA_FILTER_VERTICAL => dsp.vertical_unfilter,
        ALPHA_FILTER_GRADIENT => dsp.gradient_unfilter,
        _ => return,
    };

    (dsp.horizontal_unfilter)(None, plane.row_mut(0, 0, width));
    for y in 1..height {
        let (above, row) = plane.row_pair_mut(y - 1, y, 0, width);

        unfilter(Some(above), row);
    }
}

/// Everything decoding a frame's alpha channel reaches. It is a lossless image
/// of its own and shares nothing with the colour planes, which is what lets the
/// two be borrowed apart and run beside each other.
struct Alpha<'p, 'i> {
    vp8l: &'p mut crate::vp8l::Decoder,
    ldsp: &'p Vp8lDsp,
    fdsp: &'p FilterDsp,
    plane: &'p mut [u8],
    input: &'p Input<'i>,
    offset: usize,
    size: usize,
    width: usize,
    height: i32,
    compression: i32,
    filter: i32,
}

fn decode_alpha(a: Alpha<'_, '_>) -> Result<()> {
    let Alpha {
        vp8l,
        ldsp,
        fdsp,
        plane,
        input,
        offset,
        size,
        width,
        height,
        compression,
        filter,
    } = a;
    let extent = width * height.max(0) as usize;

    if compression == ALPHA_COMPRESSION_NONE {
        let raw = input.chunk(offset, size);

        /* Match libwebp: a short uncompressed alpha plane is invalid. */
        if raw.len() < extent {
            crate::log::error_args(format_args!(
                "ALPHA chunk carries {} of {extent} bytes",
                raw.len()
            ));
            return Err(Error::InvalidData);
        }
        plane[..extent].copy_from_slice(&raw[..extent]);
    } else if compression == ALPHA_COMPRESSION_VP8L {
        vp8l.set_canvas(width as i32, height);

        let dst = AlphaDst {
            data: &mut plane[..extent],
            stride: width,
        };

        vp8l.decode_frame(Target::Alpha, input.chunk(offset, size), true, Some(dst))?;

        if !vp8l.alpha_dst_used() {
            let argb = vp8l.picture(Target::Alpha).frame();

            for y in 0..height {
                (ldsp.extract_green)(
                    &mut plane[y as usize * width..][..width],
                    &argb.row(0, y)[..width * 4],
                );
            }
        }
        vp8l.release_alpha_canvas();
    }

    if filter != ALPHA_FILTER_NONE {
        let mut view = PlaneMut::borrowed(&mut plane[..extent], width);

        alpha_inverse_prediction(fdsp, &mut view, width, height, filter);
    }
    Ok(())
}

impl FrameSlot {
    /// Sizes and clears the plane the frame's alpha will be decoded into.
    /// Split out from the decode so a caller can hand the two to different
    /// threads.
    fn alpha_plane_reserve(&mut self) -> Result<()> {
        let Some(alpha_size) =
            (self.width.max(0) as usize).checked_mul(self.height.max(0) as usize)
        else {
            return Err(Error::NoMemory);
        };

        if self.alpha_plane.len() < alpha_size {
            if self
                .alpha_plane
                .try_reserve(alpha_size - self.alpha_plane.len())
                .is_err()
            {
                return Err(Error::NoMemory);
            }
            self.alpha_plane.resize(alpha_size, 0);
        }
        self.alpha_plane[..alpha_size].fill(0);
        Ok(())
    }

    fn alpha_work<'p, 'i>(&'p mut self, env: &FrameEnv<'p, 'i>) -> Alpha<'p, 'i> {
        Alpha {
            vp8l: &mut self.vp8l,
            ldsp: env.ldsp,
            fdsp: env.fdsp,
            plane: &mut self.alpha_plane,
            input: env.input,
            offset: self.alpha_data_offset,
            size: self.alpha_data_size,
            width: self.width.max(0) as usize,
            height: self.height.max(0),
            compression: self.alpha_compression,
            filter: self.alpha_filter,
        }
    }

    pub(crate) fn alpha_plane_decode(&mut self, env: &FrameEnv<'_, '_>) -> Result<()> {
        self.alpha_plane_reserve()?;
        decode_alpha(self.alpha_work(env))
    }

    /// Decodes a whole lossy frame: the header, then the colour planes with
    /// the alpha channel beside them.
    pub(crate) fn lossy_decode_frame(
        &mut self,
        env: &FrameEnv<'_, '_>,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        {
            let bypass = env.bypass_filtering;
            let chunk = env.input.chunk(offset, size);
            let vp8 = self.vp8_decoder()?;

            vp8.bypass_filtering = bypass;
            if vp8.frame_init(chunk, size, size)? == Status::NeedMore {
                return Err(Error::InvalidData);
            }
        }

        let (w, h) = self.size();

        self.set_size(w, h);

        if !self.has_alpha {
            let chunk = env.input.chunk(offset, size);

            return self.vp8_decoder()?.decode_rows_whole(chunk);
        }

        self.alpha_plane_reserve()?;

        /* A plane this small is done before the row loop has got going, so
         * the handoff would cost more than it saves, and an animation frame
         * gets its parallelism from being one frame of several. */
        let big_enough = (w as usize) * (h as usize) >= ALPHA_THREAD_PIXELS;
        let threads = if big_enough && !env.animation {
            env.threads
        } else {
            1
        };
        let chunk = env.input.chunk(offset, size);
        let Self {
            vp8l,
            alpha_plane,
            alpha_data_offset,
            alpha_data_size,
            alpha_compression,
            alpha_filter,
            width,
            height,
            vp8,
            ..
        } = self;
        let alpha = Alpha {
            vp8l,
            ldsp: env.ldsp,
            fdsp: env.fdsp,
            plane: alpha_plane,
            input: env.input,
            offset: *alpha_data_offset,
            size: *alpha_data_size,
            width: (*width).max(0) as usize,
            height: (*height).max(0),
            compression: *alpha_compression,
            filter: *alpha_filter,
        };
        let Some(vp8) = vp8.first_mut() else {
            return Err(Error::InvalidData);
        };
        let (alpha_ret, rows_ret) = crate::task::join(
            threads,
            || decode_alpha(alpha),
            || vp8.decode_rows_whole(chunk),
        );

        /* The colour planes still decide the frame, as they did when alpha
         * came after them and never ran if they had failed. */
        rows_ret?;
        alpha_ret
    }
}

impl<'a> Decoder<'a> {
    /* Match libwebp's whole-frame threshold for skipping the loop filter. */
    pub(crate) fn filter_bypass(&self) -> bool {
        if self.options.bypass_filtering {
            return true;
        }
        if self.options.scale.is_none()
            || self.canvas_width == 0
            || self.canvas_height == 0
        {
            return false;
        }
        let (.., src_w, src_h) =
            self.options.crop_or(self.canvas_width, self.canvas_height);
        let Ok((width, height)) = scaled_size(&self.options, src_w, src_h) else {
            return false;
        };

        width < self.canvas_width * 3 / 4 && height < self.canvas_height * 3 / 4
    }

    /// The slot a frame decodes into beside everything that decode reads.
    /// They come apart because nothing in the environment lives on the slot.
    pub(crate) fn frame_parts(&mut self) -> (&mut FrameSlot, FrameEnv<'_, 'a>) {
        let bypass_filtering = self.filter_bypass();
        let to_argb = self.frame_to_argb();
        let premultiply = self.frame_premultiply();
        let no_fancy_upsampling = self.options.no_fancy_upsampling;
        let animation = self.animation;
        let Self {
            frame,
            input,
            ldsp,
            fdsp,
            ydsp,
            threads,
            ..
        } = self;

        (
            frame,
            FrameEnv {
                input,
                ldsp,
                fdsp,
                ydsp,
                bypass_filtering,
                no_fancy_upsampling,
                to_argb,
                premultiply,
                animation,
                threads: threads.0,
            },
        )
    }

    pub(crate) fn vp8_lossy_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
    ) -> Result<bool> {
        if !self.vp8_active {
            let bypass = self.filter_bypass();
            let Self { frame, input, .. } = self;
            let vp8 = frame.vp8_decoder()?;

            vp8.bypass_filtering = bypass;

            match vp8.frame_init(input.chunk(offset, avail), avail, size)? {
                Status::NeedMore => return Ok(false),
                Status::Done => {}
            }

            let (w, h) = self.frame.size();

            self.update_canvas_size(w, h);
            if self.frame.has_alpha {
                /* A resumable decode keeps its alpha here: it is decoded once
                 * at frame start, with no row loop yet to run beside it. */
                let (frame, env) = self.frame_parts();

                frame.alpha_plane_decode(&env)?;
                self.alpha_pending = false;
            }
            self.still_lossy = !self.animation;
            self.vp8_active = true;
        } else {
            let Self { frame, input, .. } = self;
            let vp8 = frame.vp8_decoder()?;

            vp8.extend(input.chunk(offset, avail), avail);
        }

        let ret = {
            let Self { frame, input, .. } = self;
            let vp8 = frame.vp8_decoder()?;

            vp8.decode_rows(input.chunk(offset, avail))
        };

        match ret? {
            Status::NeedMore => Ok(false),
            Status::Done => {
                self.vp8_active = false;
                Ok(true)
            }
        }
    }

    pub(crate) fn vp8_lossy_decode_frame(
        &mut self,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        let ret = {
            let (frame, env) = self.frame_parts();

            frame.lossy_decode_frame(&env, offset, size)
        };
        let (w, h) = self.frame.size();

        self.update_canvas_size(w, h);
        self.alpha_pending = false;
        ret?;
        self.still_lossy = !self.animation;
        Ok(())
    }
}
