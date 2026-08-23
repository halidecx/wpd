use crate::dsp::filters::FilterDsp;
use crate::error::{Error, Result, Status};
use crate::picture::PlaneMut;
use crate::vp8l::{AlphaDst, Target};

use super::convert::scaled_size;
use super::{Decoder, ALPHA_COMPRESSION_NONE, ALPHA_COMPRESSION_VP8L};

const ALPHA_FILTER_NONE: i32 = 0;
const ALPHA_FILTER_HORIZONTAL: i32 = 1;
const ALPHA_FILTER_VERTICAL: i32 = 2;
const ALPHA_FILTER_GRADIENT: i32 = 3;

fn vp8_decoder(vp8: &mut Vec<crate::vp8::Decoder>) -> Result<&mut crate::vp8::Decoder> {
    if vp8.is_empty() {
        vp8.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        vp8.push(crate::vp8::Decoder::new());
    }
    Ok(&mut vp8[0])
}

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

impl<'a> Decoder<'a> {
    fn vp8_size(&self) -> (i32, i32) {
        self.vp8
            .first()
            .map_or((0, 0), |vp8| (vp8.width, vp8.height))
    }

    fn decode_alpha(&mut self) -> Result<()> {
        let (offset, size) = (self.alpha_data_offset, self.alpha_data_size);
        let width = self.width.max(0) as usize;
        let height = self.height.max(0);
        let extent = width * height as usize;

        if self.alpha_compression == ALPHA_COMPRESSION_NONE {
            let Self {
                input, alpha_plane, ..
            } = self;
            let raw = input.chunk(offset, size);

            /* Match libwebp: a short uncompressed alpha plane is invalid. */
            if raw.len() < extent {
                crate::log::error_args(format_args!(
                    "ALPHA chunk carries {} of {extent} bytes",
                    raw.len()
                ));
                return Err(Error::InvalidData);
            }
            alpha_plane[..extent].copy_from_slice(&raw[..extent]);
        } else if self.alpha_compression == ALPHA_COMPRESSION_VP8L {
            self.lossless_canvas_in();

            let ret = {
                let Self {
                    vp8l,
                    input,
                    alpha_plane,
                    ..
                } = self;
                let dst = AlphaDst {
                    data: &mut alpha_plane[..extent],
                    stride: width,
                };

                vp8l.decode_frame(
                    Target::Alpha,
                    input.chunk(offset, size),
                    true,
                    Some(dst),
                )
            };

            ret?;
            if !self.vp8l.alpha_dst_used() {
                let Self {
                    vp8l,
                    alpha_plane,
                    ldsp,
                    ..
                } = self;
                let argb = vp8l.picture(Target::Alpha).frame();

                for y in 0..height {
                    (ldsp.extract_green)(
                        &mut alpha_plane[y as usize * width..][..width],
                        &argb.row(0, y)[..width * 4],
                    );
                }
            }
            self.vp8l.release_alpha_canvas();
        }

        if self.alpha_filter != ALPHA_FILTER_NONE {
            let mode = self.alpha_filter;
            let mut plane = PlaneMut::borrowed(&mut self.alpha_plane[..extent], width);

            alpha_inverse_prediction(&self.fdsp, &mut plane, width, height, mode);
        }
        Ok(())
    }

    fn alpha_plane_decode(&mut self) -> Result<()> {
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

        let ret = self.decode_alpha();

        self.alpha_pending = false;
        ret
    }

    /* Match libwebp's whole-frame threshold for skipping the loop filter. */
    fn filter_bypass(&self) -> bool {
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

    pub(crate) fn vp8_lossy_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
    ) -> Result<bool> {
        if !self.vp8_active {
            let bypass = self.filter_bypass();
            let Self { vp8, input, .. } = self;
            let vp8 = vp8_decoder(vp8)?;

            vp8.bypass_filtering = bypass;

            match vp8.frame_init(input.chunk(offset, avail), avail, size)? {
                Status::NeedMore => return Ok(false),
                Status::Done => {}
            }

            let (w, h) = self.vp8_size();

            self.update_canvas_size(w, h);
            if self.has_alpha {
                self.alpha_plane_decode()?;
            }
            self.still_lossy = !self.animation;
            self.vp8_active = true;
        } else {
            let Self { vp8, input, .. } = self;
            let vp8 = vp8_decoder(vp8)?;

            vp8.extend(input.chunk(offset, avail), avail);
        }

        let ret = {
            let Self { vp8, input, .. } = self;
            let vp8 = vp8_decoder(vp8)?;

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
            let bypass = self.filter_bypass();
            let Self { vp8, input, .. } = self;
            let vp8 = vp8_decoder(vp8)?;

            vp8.bypass_filtering = bypass;
            vp8.decode_frame(input.chunk(offset, size))
        };

        ret?;

        let (w, h) = self.vp8_size();

        self.update_canvas_size(w, h);
        if self.has_alpha {
            self.alpha_plane_decode()?;
        }
        self.still_lossy = !self.animation;
        Ok(())
    }
}
