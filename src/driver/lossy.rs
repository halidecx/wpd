//! Driving the lossy frame decoder, as `src/lossy.c` did.
//!
//! Three things happen here that the VP8 decoder itself knows nothing about:
//! the alpha plane a WebP file carries beside the luma, the planar view handed
//! out for it, and the in-loop filter a scaled decode drops. The pixels are
//! [`crate::vp8`]; what is here is the WebP container's idea of a lossy frame.
//!
//! The alpha plane is the decoder's own `Vec`, and the chunk it is decoded
//! from is a slice of the input, so every call below destructures the decoder
//! to take both at once. The C reached for each through the decoder pointer
//! and the aliasing rule was the reader's to reconstruct.

use crate::error::{Error, Result, Status};
use crate::picture::PlaneMut;
use crate::vp8l::{AlphaDst, Target};

use super::convert::scaled_size;
use super::{Decoder, ALPHA_COMPRESSION_NONE, ALPHA_COMPRESSION_VP8L};

const ALPHA_FILTER_NONE: i32 = 0;
const ALPHA_FILTER_HORIZONTAL: i32 = 1;
const ALPHA_FILTER_VERTICAL: i32 = 2;
const ALPHA_FILTER_GRADIENT: i32 = 3;

/// Undoes the per-row prediction an ALPH chunk's filter applied.
///
/// The first row and the first column are predicted from their one neighbour
/// whatever the mode is, so they are walked before the mode is looked at.
fn alpha_inverse_prediction(
    plane: &mut PlaneMut<'_>,
    width: usize,
    height: i32,
    mode: i32,
) {
    if width == 0 || height == 0 {
        return;
    }

    let top = plane.row_mut(0, 0, width);

    for x in 1..width {
        top[x] = top[x].wrapping_add(top[x - 1]);
    }
    for y in 1..height {
        let (above, row) = plane.row_pair_mut(y - 1, y, 0, 1);

        row[0] = row[0].wrapping_add(above[0]);
    }

    /* The mode is chosen once, not per pixel: it cannot change inside a frame,
    and a branch in the innermost loop is one the C did not have. */
    match mode {
        ALPHA_FILTER_HORIZONTAL => {
            for y in 1..height {
                let row = plane.row_mut(y, 0, width);

                for x in 1..width {
                    row[x] = row[x].wrapping_add(row[x - 1]);
                }
            }
        }
        ALPHA_FILTER_VERTICAL => {
            for y in 1..height {
                let (above, row) = plane.row_pair_mut(y - 1, y, 0, width);

                for x in 1..width {
                    row[x] = row[x].wrapping_add(above[x]);
                }
            }
        }
        ALPHA_FILTER_GRADIENT => {
            for y in 1..height {
                let (above, row) = plane.row_pair_mut(y - 1, y, 0, width);

                for x in 1..width {
                    let sum = row[x - 1] as i32 + above[x] as i32 - above[x - 1] as i32;

                    row[x] = row[x].wrapping_add(sum.clamp(0, 255) as u8);
                }
            }
        }
        _ => {}
    }
}

impl<'a> Decoder<'a> {
    /// What the last VP8 frame header declared.
    fn vp8_size(&self) -> (i32, i32) {
        self.vp8
            .as_ref()
            .map_or((0, 0), |vp8| (vp8.width, vp8.height))
    }

    /// Fills the alpha plane in from the ALPH chunk the decoder latched.
    fn decode_alpha(&mut self) -> Result<()> {
        let (offset, size) = (self.alpha_data_offset, self.alpha_data_size);
        let width = self.width.max(0) as usize;
        let height = self.height.max(0);
        let extent = width * height as usize;

        if self.alpha_compression == ALPHA_COMPRESSION_NONE {
            let Self {
                input, alpha_plane, ..
            } = self;
            let mut left = input.chunk(offset, size);

            for y in 0..height as usize {
                let n = width.min(left.len());
                let (row, rest) = left.split_at(n);

                alpha_plane[y * width..][..n].copy_from_slice(row);
                left = rest;
            }
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

            alpha_inverse_prediction(&mut plane, width, height, mode);
        }
        Ok(())
    }

    /// Sizes and clears the alpha plane, then fills it in.
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

    /* libwebp drops the in-loop filter once a scaled decode shrinks the frame
    past three quarters in both directions, on the grounds that nothing
    survives the downscale, so a scaled lossy frame only matches it if the
    filter goes too. The threshold is measured against the whole frame, not the
    cropped part. */
    fn update_filter_bypass(&mut self) {
        self.bypass_filtering = self.options.bypass_filtering;
        if self.options.scale.is_none()
            || self.canvas_width == 0
            || self.canvas_height == 0
        {
            return;
        }
        let (.., src_w, src_h) =
            self.options.crop_or(self.canvas_width, self.canvas_height);
        let Ok((width, height)) = scaled_size(&self.options, src_w, src_h) else {
            return;
        };
        if width < self.canvas_width * 3 / 4 && height < self.canvas_height * 3 / 4 {
            self.bypass_filtering = true;
        }
    }

    /// Returns whether the frame is complete; `false` means more of the chunk
    /// is needed.
    pub(crate) fn vp8_lossy_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
    ) -> Result<bool> {
        self.update_filter_bypass();

        if !self.vp8_active {
            let bypass = self.bypass_filtering;
            let Self { vp8, input, .. } = self;
            let vp8 = vp8.get_or_insert_with(Default::default);

            /* Latched with the frame header, as the C's `vp8_decode_frame_init`
            read it out of the codec context: a mid-frame options change does
            not reach the rows already being filtered. */
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
            let vp8 = vp8.get_or_insert_with(Default::default);

            vp8.extend(input.chunk(offset, avail), avail);
        }

        let ret = {
            let Self { vp8, input, .. } = self;
            let vp8 = vp8.get_or_insert_with(Default::default);

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
        self.update_filter_bypass();

        let ret = {
            let bypass = self.bypass_filtering;
            let Self { vp8, input, .. } = self;
            let vp8 = vp8.get_or_insert_with(Default::default);

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
