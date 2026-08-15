//! Driving the lossy frame decoder, as `src/lossy.c` did.
//!
//! Three things happen here that the VP8 decoder itself knows nothing about:
//! the alpha plane a WebP file carries beside the luma, the planar view handed
//! out for it, and the in-loop filter a scaled decode drops. The pixels are
//! [`wpd::vp8`]; what is here is the WebP container's idea of a lossy frame.

use std::ffi::{c_int, c_uint};
use std::{ptr, slice};

use wpd::image::Format;

use crate::convert::scaled_size;
use crate::decoder::{
    WPDDecoder, ALPHA_COMPRESSION_NONE, ALPHA_COMPRESSION_VP8L, WPD_OK,
};
use crate::image::WebPImage;
use crate::vp8::{
    vp8_decode_extend, vp8_decode_frame, vp8_decode_frame_init, vp8_decode_init,
    vp8_decode_rows, WpdFrame, WpdPacket, WPD_ENOMEM,
};
use crate::vp8l::{
    vp8l_alpha_dst_used, vp8l_decode_frame, vp8l_set_alpha_dst, VP8L_TARGET_ALPHA,
};

const ALPHA_FILTER_NONE: c_int = 0;
const ALPHA_FILTER_HORIZONTAL: c_int = 1;
const ALPHA_FILTER_VERTICAL: c_int = 2;
const ALPHA_FILTER_GRADIENT: c_int = 3;

/// Undoes the per-row prediction an ALPH chunk's filter applied.
///
/// The first row and the first column are predicted from their one neighbour
/// whatever the mode is, so they are walked before the mode is looked at.
///
/// # Safety
///
/// Plane three must hold the image's geometry.
unsafe fn alpha_inverse_prediction(frame: &WebPImage, mode: c_int) {
    let ls = frame.linesize[3] as isize;
    let base = frame.data[3];

    unsafe {
        let mut dec = base.add(1);

        for _ in 1..frame.width {
            *dec = (*dec).wrapping_add(*dec.sub(1));
            dec = dec.add(1);
        }

        let mut dec = base.offset(ls);

        for _ in 1..frame.height {
            *dec = (*dec).wrapping_add(*dec.offset(-ls));
            dec = dec.offset(ls);
        }

        /* The mode is chosen once, not per pixel: it cannot change inside a
        frame, and a branch in the innermost loop is one the C did not have. */
        match mode {
            ALPHA_FILTER_HORIZONTAL => rows(frame, ls, |dec| *dec.sub(1)),
            ALPHA_FILTER_VERTICAL => rows(frame, ls, |dec| *dec.offset(-ls)),
            ALPHA_FILTER_GRADIENT => rows(frame, ls, |dec| {
                let sum = *dec.sub(1) as i32 + *dec.offset(-ls) as i32
                    - *dec.offset(-ls - 1) as i32;

                sum.clamp(0, 255) as u8
            }),
            _ => {}
        }
    }
}

/// Adds each pixel's prediction to it, over every row but the first and every
/// column but the first, which the caller has already undone.
///
/// # Safety
///
/// As [`alpha_inverse_prediction`].
unsafe fn rows(frame: &WebPImage, ls: isize, predict: impl Fn(*const u8) -> u8) {
    for y in 1..frame.height {
        let mut dec = unsafe { frame.data[3].offset(y as isize * ls + 1) };

        for _ in 1..frame.width {
            unsafe {
                *dec = (*dec).wrapping_add(predict(dec));
                dec = dec.add(1);
            }
        }
    }
}

impl WPDDecoder {
    /// # Safety
    ///
    /// `data_start` must be readable for `data_size` bytes, and `out` must
    /// carry an alpha plane of the frame's geometry.
    unsafe fn decode_alpha(
        &mut self,
        out: &WebPImage,
        data_start: *const u8,
        data_size: c_uint,
    ) -> c_int {
        if self.alpha_compression == ALPHA_COMPRESSION_NONE {
            let mut src = data_start;
            let mut left = data_size as usize;

            for y in 0..self.height {
                let n = (self.width as usize).min(left);

                unsafe {
                    ptr::copy_nonoverlapping(
                        src,
                        out.data[3].offset(y as isize * out.linesize[3] as isize),
                        n,
                    );
                    src = src.add(n);
                }
                left -= n;
            }
        } else if self.alpha_compression == ALPHA_COMPRESSION_VP8L {
            self.lossless_canvas_in();

            let (ret, direct) = unsafe {
                vp8l_set_alpha_dst(&mut *self.vp8l, out.data[3], out.linesize[3]);

                let ret = vp8l_decode_frame(
                    &mut *self.vp8l,
                    VP8L_TARGET_ALPHA,
                    &mut self.alpha_argb,
                    data_start,
                    data_size,
                    1,
                );
                let direct = vp8l_alpha_dst_used(&*self.vp8l);

                vp8l_set_alpha_dst(&mut *self.vp8l, ptr::null_mut(), 0);
                (ret, direct)
            };

            if ret < 0 {
                return ret;
            }
            if direct == 0 {
                let argb = unsafe { self.alpha_argb.frame() };
                let width = self.width as usize;

                for y in 0..self.height {
                    let src = argb.plane[0].row(y, 0, width * 4);
                    let dst = unsafe {
                        slice::from_raw_parts_mut(
                            out.data[3].offset(y as isize * out.linesize[3] as isize),
                            width,
                        )
                    };

                    (self.ldsp.extract_green)(dst, src);
                }
            }
        }

        if self.alpha_filter != ALPHA_FILTER_NONE {
            unsafe { alpha_inverse_prediction(out, self.alpha_filter) };
        }
        WPD_OK
    }

    /// A view of the three planes the VP8 decoder produced, plus the alpha
    /// plane this module keeps beside them.
    fn export_planes(&mut self, decoded: &WpdFrame) {
        let mut out = WebPImage::empty();

        out.width = self.width;
        out.height = self.height;
        out.format = Format::Yuv420p as c_int;
        for plane in 0..3 {
            out.data[plane] = decoded.data[plane];
            out.linesize[plane] = decoded.linesize[plane];
        }
        if self.has_alpha {
            out.data[3] = self.alpha_plane.as_mut_ptr();
            out.linesize[3] = self.width;
            out.format = Format::Yuva420p as c_int;
        }
        self.subframe = out;
    }

    /// # Safety
    ///
    /// The alpha chunk the decoder latched must still be buffered.
    unsafe fn alpha_plane_decode(&mut self) -> c_int {
        let Some(alpha_size) = (self.width as usize).checked_mul(self.height as usize)
        else {
            return WPD_ENOMEM;
        };

        if self.alpha_plane.len() < alpha_size {
            if self
                .alpha_plane
                .try_reserve(alpha_size - self.alpha_plane.len())
                .is_err()
            {
                return WPD_ENOMEM;
            }
            self.alpha_plane.resize(alpha_size, 0);
        }
        self.alpha_plane[..alpha_size].fill(0);

        let mut out = self.subframe;

        out.data[3] = self.alpha_plane.as_mut_ptr();
        out.linesize[3] = self.width;
        out.format = Format::Yuva420p as c_int;
        self.subframe = out;

        let data = self.file_at(self.alpha_data_offset);
        let ret =
            unsafe { self.decode_alpha(&out, data, self.alpha_data_size as c_uint) };

        self.alpha_pending = false;
        ret
    }

    fn lossy_init(&mut self) -> c_int {
        if self.vp8_initialized {
            return WPD_OK;
        }
        let ret = unsafe { vp8_decode_init(&mut self.codec) };

        if ret < 0 {
            return ret;
        }
        self.vp8_initialized = true;
        WPD_OK
    }

    /* libwebp drops the in-loop filter once a scaled decode shrinks the frame
    past three quarters in both directions, on the grounds that nothing
    survives the downscale, so a scaled lossy frame only matches it if the
    filter goes too. The threshold is measured against the whole frame, not the
    cropped part. */
    fn update_filter_bypass(&mut self) {
        self.codec.bypass_filtering = self.options.bypass_filtering;
        if self.options.use_scaling == 0
            || self.canvas_width == 0
            || self.canvas_height == 0
        {
            return;
        }
        let (src_w, src_h) = if self.options.use_cropping != 0 {
            (self.options.crop_width, self.options.crop_height)
        } else {
            (self.canvas_width, self.canvas_height)
        };
        let Ok((width, height)) = scaled_size(&self.options, src_w, src_h) else {
            return;
        };
        if width < self.canvas_width * 3 / 4 && height < self.canvas_height * 3 / 4 {
            self.codec.bypass_filtering = 1;
        }
    }

    /// Returns 1 when the frame is complete, 0 when more of the chunk is
    /// needed, or a negative status.
    ///
    /// # Safety
    ///
    /// `data_start` must be readable for `avail` bytes and stay valid until
    /// the next call that replaces it.
    pub(crate) unsafe fn vp8_lossy_step(
        &mut self,
        data_start: *const u8,
        avail: c_uint,
        data_size: c_uint,
    ) -> c_int {
        let ret = self.lossy_init();

        if ret < 0 {
            return ret;
        }
        self.update_filter_bypass();

        if !self.vp8_active {
            let ret = unsafe {
                vp8_decode_frame_init(
                    &mut self.codec,
                    data_start,
                    avail as c_int,
                    data_size as c_int,
                )
            };

            if ret < 0 {
                return ret;
            }
            if ret != 0 {
                return 0;
            }
            self.update_canvas_size(self.codec.width, self.codec.height);

            let mut current = WpdFrame::empty();

            unsafe { crate::vp8::vp8_current_frame(&self.codec, &mut current) };
            self.export_planes(&current);
            if self.has_alpha {
                let ret = unsafe { self.alpha_plane_decode() };

                if ret < 0 {
                    return ret;
                }
            }
            self.still_lossy = !self.animation;
            self.vp8_active = true;
        } else {
            unsafe { vp8_decode_extend(&mut self.codec, data_start, avail as c_int) };
        }

        let mut decoded = WpdFrame::empty();
        let ret = unsafe { vp8_decode_rows(&mut self.codec, &mut decoded) };

        if ret < 0 {
            return ret;
        }
        self.export_planes(&decoded);
        if ret != 0 {
            return 0;
        }
        self.vp8_active = false;
        1
    }

    /// # Safety
    ///
    /// `data_start` must be readable for `data_size` bytes.
    pub(crate) unsafe fn vp8_lossy_decode_frame(
        &mut self,
        data_start: *const u8,
        data_size: c_uint,
    ) -> c_int {
        let ret = self.lossy_init();

        if ret < 0 {
            return ret;
        }
        self.update_filter_bypass();

        let mut packet = WpdPacket {
            data: data_start,
            size: data_size as c_int,
        };
        let mut decoded = WpdFrame::empty();
        let ret =
            unsafe { vp8_decode_frame(&mut self.codec, &mut decoded, &mut packet) };

        if ret < 0 {
            return ret;
        }
        self.update_canvas_size(self.codec.width, self.codec.height);
        self.export_planes(&decoded);
        if self.has_alpha {
            let ret = unsafe { self.alpha_plane_decode() };

            if ret < 0 {
                return ret;
            }
        }
        self.still_lossy = !self.animation;
        WPD_OK
    }
}
