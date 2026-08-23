use crate::dsp::filters::FilterDsp;
use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{Error, Result};
use crate::input::Input;
use crate::picture::{Buffer, Frame};
use crate::vp8l::Output as Lossless;

use super::{empty_view, lossless_view, lossy_view, Source};

/// Everything outside a frame that decoding it reads, and nothing it writes.
/// A slot is handed one of these rather than reaching back into the decoder,
/// which is what lets several slots be filled at once.
#[derive(Clone, Copy)]
pub(crate) struct FrameEnv<'e, 'i> {
    pub(crate) input: &'e Input<'i>,
    pub(crate) ldsp: &'e Vp8lDsp,
    pub(crate) fdsp: &'e FilterDsp,
    pub(crate) bypass_filtering: bool,
    pub(crate) threads: usize,
}

/// Everything one frame's decode produces and nothing that outlives it.
///
/// A still uses a single slot. An animation's frames depend on nothing but
/// their own bytes, so they can be decoded into a slot each, which is what
/// makes them separable; compositing, which depends on every frame before it,
/// still walks them in order.
#[derive(Default)]
pub(crate) struct FrameSlot {
    pub(crate) vp8: Vec<crate::vp8::Decoder>,
    pub(crate) vp8l: crate::vp8l::Decoder,

    pub(crate) has_alpha: bool,
    pub(crate) alpha_compression: i32,
    pub(crate) alpha_filter: i32,
    pub(crate) alpha_data_offset: usize,
    pub(crate) alpha_data_size: usize,
    pub(crate) alpha_plane: Vec<u8>,

    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) lossless_has_alpha: bool,
    pub(crate) lossless_out: Option<Lossless>,

    /// Where a sub-frame was converted to ARGB, when it had to be.
    pub(crate) converted: Buffer,
}

impl FrameSlot {
    pub(crate) fn view(&self, which: Source) -> Frame<'_> {
        match which {
            Source::Lossy => lossy_view(
                self.vp8.first(),
                &self.alpha_plane,
                self.has_alpha,
                self.width,
                self.height,
            ),
            Source::Lossless => lossless_view(&self.vp8l, self.lossless_out),
            Source::Converted => self.converted.frame(),
            Source::Canvas | Source::None => empty_view(),
        }
    }

    /// The view a still is exported from, beside the buffer it is converted
    /// into. They are different fields, which is the only reason both can be
    /// held at once; `which` is never Converted here, since that buffer is the
    /// destination rather than a source.
    pub(crate) fn split_converted(
        &mut self,
        which: Source,
    ) -> (Frame<'_>, &mut Buffer) {
        let Self {
            vp8,
            vp8l,
            alpha_plane,
            has_alpha,
            width,
            height,
            lossless_out,
            converted,
            ..
        } = self;
        let img = match which {
            Source::Lossy => {
                lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height)
            }
            Source::Lossless => lossless_view(vp8l, *lossless_out),
            Source::Converted | Source::Canvas | Source::None => empty_view(),
        };

        (img, converted)
    }

    pub(crate) fn reset(&mut self) {
        self.vp8l.reset();
        self.width = 0;
        self.height = 0;
        self.has_alpha = false;
        self.lossless_has_alpha = false;
        self.lossless_out = None;
    }

    pub(crate) fn release(&mut self) {
        self.vp8l.release();
        self.converted.release();
    }

    /// Records the size the image turned out to be, warning where it is not
    /// the size the frame before it was.
    pub(crate) fn set_size(&mut self, w: i32, h: i32) {
        if self.width != 0 && self.width != w {
            crate::log::warning_args(format_args!(
                "Width mismatch. {} != {w}",
                self.width
            ));
        }
        self.width = w;
        if self.height != 0 && self.height != h {
            crate::log::warning_args(format_args!(
                "Height mismatch. {} != {h}",
                self.height
            ));
        }
        self.height = h;
    }

    pub(crate) fn lossless_decode(
        &mut self,
        env: &FrameEnv<'_, '_>,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        /* The canvas a lossless image is decoded against is whatever this
         * slot has been told to expect, which is nothing for a still and the
         * sub-frame's declared size inside an ANMF. */
        self.vp8l.set_canvas(self.width, self.height);

        let ret = self.vp8l.decode_frame(
            crate::vp8l::Target::Argb,
            env.input.chunk(offset, size),
            false,
            None,
        );

        self.width = self.vp8l.width;
        self.height = self.vp8l.height;
        self.lossless_has_alpha = self.vp8l.has_alpha;
        ret?;
        self.lossless_out = Some(Lossless::Argb);
        Ok(())
    }

    pub(crate) fn set_alpha_chunk(
        &mut self,
        header: i32,
        offset: usize,
        size: usize,
    ) -> Result<()> {
        if header >> 4 & 3 > super::ALPHA_PREPROCESSED_LEVELS || header >> 6 != 0 {
            crate::log::error_args(format_args!(
                "invalid ALPHA chunk header 0x{header:02x}"
            ));
            return Err(Error::InvalidData);
        }
        self.alpha_data_offset = offset;
        self.alpha_data_size = size;

        let compression = header & 3;

        if compression > super::ALPHA_COMPRESSION_VP8L {
            crate::log::warning("skipping unsupported ALPHA chunk");
            return Ok(());
        }
        self.has_alpha = true;
        self.alpha_compression = compression;
        self.alpha_filter = header >> 2 & 3;
        Ok(())
    }

    /// Walks the sub-chunks of one ANMF and decodes the image it carries,
    /// with its alpha channel if it has one. It reads nothing but its own
    /// bytes, which is what lets several frames be decoded at once.
    pub(crate) fn decode_anmf_image(
        &mut self,
        env: &FrameEnv<'_, '_>,
        base: usize,
        size: usize,
    ) -> Result<Source> {
        self.has_alpha = false;
        self.width = 0;
        self.height = 0;

        let mut sub: Option<Source> = None;
        let mut at = base + 16;
        let end = base + size;

        while end - at >= 8 {
            let (chunk_type, payload_size) = {
                let head = env.input.chunk(at, 8);

                if head.len() < 8 {
                    break;
                }
                (crate::bits::rl32(head), crate::bits::rl32(&head[4..]))
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
                crate::container::TAG_ALPH => {
                    if payload_size == 0 {
                        crate::log::error("invalid ALPHA chunk size");
                        return Err(Error::InvalidData);
                    }
                    if sub.is_some() {
                        crate::log::error("ALPHA chunk after the image it belongs to");
                        return Err(Error::InvalidData);
                    }
                    let header = env.input.chunk(at, 1)[0] as i32;

                    self.set_alpha_chunk(header, at + 1, payload_size - 1)?;
                }
                crate::container::TAG_VP8 if sub.is_none() => {
                    self.lossy_decode_frame(env, at, payload_size)?;
                    sub = Some(Source::Lossy);
                }
                crate::container::TAG_VP8L if sub.is_none() => {
                    self.lossless_decode(env, at, payload_size)?;
                    sub = Some(Source::Lossless);
                }
                _ => {}
            }
            at += padded_size;
        }

        sub.ok_or_else(|| {
            crate::log::error("image data not found");
            Error::InvalidData
        })
    }

    /// Whether the image this slot holds carries an alpha channel.
    pub(crate) fn frame_has_alpha(&self, which: Source) -> bool {
        match which {
            Source::Lossless => self.lossless_has_alpha,
            _ => self.has_alpha,
        }
    }

    pub(crate) fn size(&self) -> (i32, i32) {
        self.vp8
            .first()
            .map_or((0, 0), |vp8| (vp8.width, vp8.height))
    }

    pub(crate) fn vp8_decoder(&mut self) -> Result<&mut crate::vp8::Decoder> {
        if self.vp8.is_empty() {
            self.vp8
                .try_reserve_exact(1)
                .map_err(|_| crate::error::Error::NoMemory)?;
            self.vp8.push(crate::vp8::Decoder::new());
        }
        Ok(&mut self.vp8[0])
    }
}
