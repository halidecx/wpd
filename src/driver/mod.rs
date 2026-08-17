//! The decoder, and the walk over a file that drives it.
//!
//! This is the assembly: the container scanner, the two frame decoders, the
//! compositor and the export are pieces, and what is here decides which of
//! them a chunk belongs to and hands each the part of the decoder it needs.
//!
//! Nothing here is the C ABI. A caller's memory arrives as a
//! [`crate::handout::RowSink`] and a finished picture leaves as a
//! [`Handout`]; the versioned structs `include/wpd.h` declares, and the
//! pointers in them, belong to whoever crosses that boundary.

pub mod anim;
pub mod convert;
pub mod export;
pub mod lossy;

use crate::container::{Coding, Info, Raw, Scan, METADATA_NB};
use crate::dsp::vp8l::Vp8lDsp;
use crate::dsp::yuv::YuvDsp;
use crate::error::Error;
use crate::handout::{Handout, RowSink};
use crate::image::Format;
use crate::info::{FrameInfo, ImageInfo};
use crate::input::Input;
use crate::options::Options;
use crate::picture::{Buffer, Frame, PlaneRef};
use crate::rescale::Scratch;
use crate::vp8l::Output as Lossless;

use self::convert::{
    ensure_yuva_rows, format_is_packed, format_is_premultiplied, format_valid,
};
use self::export::{
    export_external_planar_rows, export_own, export_packed, export_still_lossless,
    export_still_packed, ExportSettings, ExportTargets, RowTargets,
};

/// What `out_format` holds when no output format has been asked for, which is
/// `WPD_PIX_FMT_NONE` where the C ABI names it.
pub const FORMAT_NONE: i32 = -1;

/// The two ways an animation is handed out, as `WPD_ANIM_*` numbers them: the
/// composited canvas, or each sub-frame on its own at its own position.
pub const ANIM_COMPOSITED: i32 = 0;
pub const ANIM_SUBFRAME: i32 = 1;

/// A failure, and what was being done when it happened.
///
/// A picture borrows the decoder that produced it, so a decode that fails
/// part way cannot record its own message: the decoder is still lent out
/// until the caller drops what it asked for. The message travels back with
/// the failure instead, and whoever receives it calls [`Decoder::fail`].
pub type Failure = (&'static str, Error);

/// How a failure reads once it is written down: what was being done, and what
/// went wrong with it.
pub fn described((message, e): Failure) -> String {
    format!("{message} ({})", e.message())
}

/// The decoder, with the lifetime of the file it was pointed at.
///
/// Everything a decode builds up is owned here, and released by [`Drop`]
/// rather than by a sequence a new field can be left out of. Two habits from
/// the C are kept deliberately. Positions in the stream are offsets, never
/// pointers, because appending to a stream may move or drop the bytes under
/// them. And nothing reaches into the decoder from the modules it drives:
/// what the export and the compositor need is gathered into a struct at the
/// call, so neither can read a field that has moved on since.
pub struct Decoder<'a> {
    /// Built on the first lossy frame, as the C's `vp8_decode_init` was: a
    /// file with no VP8 chunk in it never pays for the lossy decoder.
    pub(crate) vp8: Vec<crate::vp8::Decoder>,
    pub(crate) bypass_filtering: bool,
    pub(crate) ldsp: Vp8lDsp,
    pub(crate) ydsp: YuvDsp,
    pub(crate) out_format: i32,
    pub(crate) premultiply: i32,
    pub(crate) options: Options,

    pub(crate) input: Input<'a>,
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) scan: Scan,
    pub(crate) animation: bool,
    pub(crate) still_done: bool,
    pub(crate) vp8_active: bool,
    pub(crate) still_lossy: bool,
    pub(crate) alpha_pending: bool,
    pub(crate) converted_rows: i32,
    pub(crate) converted_format: i32,
    pub(crate) still_lossless: bool,
    pub(crate) frame_index: i32,
    pub(crate) canvas_width: i32,
    pub(crate) canvas_height: i32,

    pub(crate) has_alpha: bool,
    pub(crate) alpha_compression: i32,
    pub(crate) alpha_filter: i32,
    /// An offset, not a pointer: appending to a stream can move the bytes.
    pub(crate) alpha_data_offset: usize,
    pub(crate) alpha_data_size: usize,
    pub(crate) alpha_plane: Vec<u8>,

    pub(crate) vp8l: crate::vp8l::Decoder,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) lossless_has_alpha: bool,

    /// Which of the lossless decoder's pictures the last decode filled in, so
    /// that the borrow handed out for it is named rather than latched as a
    /// pointer the way the C's `WebPImage` was.
    pub(crate) lossless_out: Option<Lossless>,

    /// Plane memory the decoder owns, released when it drops.
    pub(crate) converted: Buffer,
    pub(crate) output: Buffer,
    pub(crate) transformed: Buffer,
    pub(crate) rescale: Scratch,

    pub(crate) canvas: Buffer,
    pub(crate) subframe_out: Option<Source>,
    pub(crate) anim_mode: i32,
    pub(crate) anmf_flags: i32,
    pub(crate) pos_x: i32,
    pub(crate) pos_y: i32,
    pub(crate) frame_has_alpha: bool,
    pub(crate) key_frame: bool,
    pub(crate) prev_anmf_flags: i32,
    pub(crate) prev_width: i32,
    pub(crate) prev_height: i32,
    pub(crate) prev_pos_x: i32,
    pub(crate) prev_pos_y: i32,
    pub(crate) prev_key_frame: bool,
    pub(crate) clear_argb: [u8; 4],
    pub(crate) clear_yuva: [u8; 4],

    pub(crate) anim_loop_count: i32,
    pub(crate) anim_frame_count: i32,
    pub(crate) anim_background_argb: u32,
    pub(crate) frame_duration: i32,
    pub(crate) frame_timestamp: i64,

    pub(crate) info_has_alpha: bool,
    pub(crate) info_coding: Coding,

    pub(crate) meta: [Option<Vec<u8>>; METADATA_NB],

    pub(crate) opened: bool,
    pub(crate) streaming: bool,
    pub(crate) eos: bool,
    pub(crate) headers_valid: bool,
    pub(crate) truncated: bool,
    pub(crate) input_mode: u8,

    /// Where a decode's rows go when the caller supplied its own memory.
    ///
    /// The decoder knows only that it is somewhere it can ask for a row of;
    /// what the memory actually is belongs to whoever handed it over, which
    /// for the C ABI is a pointer, a length and a stride that may run
    /// backwards.
    pub(crate) sink: Option<Box<dyn RowSink>>,

    pub(crate) status: Option<Error>,
    /// A fixed buffer rather than a `String`: `wpd_decoder_error` hands out a
    /// pointer into it, and the C's was good for the decoder's whole life.
    pub(crate) error: [u8; ERROR_MAX],
}

pub(crate) const ALPHA_COMPRESSION_NONE: i32 = 0;
pub(crate) const ALPHA_COMPRESSION_VP8L: i32 = 1;

/// A four-character chunk tag as it sits in the file, which is what `MKTAG`
/// built.
const fn mktag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    a as u32 | (b as u32) << 8 | (c as u32) << 16 | (d as u32) << 24
}

pub(crate) const TAG_ALPH: u32 = mktag(b'A', b'L', b'P', b'H');
pub(crate) const TAG_VP8: u32 = mktag(b'V', b'P', b'8', b' ');
pub(crate) const TAG_VP8L: u32 = mktag(b'V', b'P', b'8', b'L');
const TAG_ANMF: u32 = mktag(b'A', b'N', b'M', b'F');

/// As long a failure message as the C's fixed buffer held.
const ERROR_MAX: usize = 128;

/// Black in the CCIR range the YUVA canvas is cleared to, which is what
/// `RGB_TO_Y_CCIR(0, 0, 0)` and its two companions came to.
const CLEAR_YUVA_BLACK: [u8; 4] = [16, 128, 128, 0];

/// Which picture an export reads.
///
/// Naming it rather than passing a borrow is what lets the decoder hand out
/// the source and the scratch from one destructuring; naming it rather than
/// latching a pointer is what stops a sub-frame decode handing out a view of
/// memory a later frame has moved on from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The VP8 decoder's planes, with the alpha plane beside them.
    Lossy,
    /// Whichever lossless picture [`Decoder::lossless_out`] names.
    Lossless,
    /// The decoder's own conversion buffer, which an animation sub-frame is
    /// brought to ARGB in.
    Converted,
    /// The composited animation canvas.
    Canvas,
    /// Nothing, which is what a sub-frame decode that produced no image has.
    None,
}

/// The VP8 decoder's picture, with the alpha plane the container keeps beside
/// it, as a borrowed view.
pub(crate) fn lossy_view<'a>(
    vp8: Option<&'a crate::vp8::Decoder>,
    alpha: &'a [u8],
    has_alpha: bool,
    width: i32,
    height: i32,
) -> Frame<'a> {
    let mut plane = [PlaneRef::borrowed(&[], 0); 4];

    if let Some(vp8) = vp8 {
        for (p, out) in plane.iter_mut().enumerate().take(3) {
            let g = vp8.picture.planes[p];
            let data = vp8.picture.plane(p);

            *out = PlaneRef::borrowed(&data[g.origin.min(data.len())..], g.stride);
        }
    }

    let format = if has_alpha {
        plane[3] = PlaneRef::borrowed(alpha, width.max(0) as usize);
        Format::Yuva420p
    } else {
        Format::Yuv420p
    };

    Frame::borrowed(plane, width, height, format)
}

pub(crate) fn lossless_view(
    vp8l: &crate::vp8l::Decoder,
    which: Option<Lossless>,
) -> Frame<'_> {
    match which.and_then(|which| vp8l.view(which)) {
        Some(frame) => frame,
        None => Frame::packed(&[], 0, 0, 0, Format::Argb),
    }
}

pub(crate) fn rl24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

pub(crate) fn rl32(b: &[u8]) -> u32 {
    rl24(b) | (b[3] as u32) << 24
}

impl Default for Decoder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Decoder<'a> {
    pub fn new() -> Self {
        Decoder {
            vp8: Vec::new(),
            bypass_filtering: false,
            ldsp: Vp8lDsp::new(),
            ydsp: YuvDsp::new(),
            out_format: FORMAT_NONE,
            premultiply: 0,
            options: Options::default(),

            input: Input::new(),
            pos: 0,
            end: 0,
            scan: Scan::new(),
            animation: false,
            still_done: false,
            vp8_active: false,
            still_lossy: false,
            alpha_pending: false,
            converted_rows: 0,
            converted_format: FORMAT_NONE,
            still_lossless: false,
            frame_index: 0,
            canvas_width: 0,
            canvas_height: 0,

            has_alpha: false,
            alpha_compression: ALPHA_COMPRESSION_NONE,
            alpha_filter: 0,
            alpha_data_offset: 0,
            alpha_data_size: 0,
            alpha_plane: Vec::new(),

            vp8l: crate::vp8l::Decoder::new(),
            width: 0,
            height: 0,
            lossless_has_alpha: false,

            lossless_out: None,
            converted: Buffer::default(),
            output: Buffer::default(),
            transformed: Buffer::default(),
            rescale: Scratch::default(),

            canvas: Buffer::default(),
            subframe_out: None,
            anim_mode: ANIM_COMPOSITED,
            anmf_flags: 0,
            pos_x: 0,
            pos_y: 0,
            frame_has_alpha: false,
            key_frame: false,
            prev_anmf_flags: 0,
            prev_width: 0,
            prev_height: 0,
            prev_pos_x: 0,
            prev_pos_y: 0,
            prev_key_frame: false,
            clear_argb: [0; 4],
            clear_yuva: CLEAR_YUVA_BLACK,

            anim_loop_count: 0,
            anim_frame_count: 0,
            anim_background_argb: 0,
            frame_duration: 0,
            frame_timestamp: 0,

            info_has_alpha: false,
            info_coding: Coding::Unknown,

            meta: [None, None, None],

            opened: false,
            streaming: false,
            eos: false,
            headers_valid: false,
            truncated: false,
            input_mode: 0,

            sink: None,

            status: None,
            error: [0; ERROR_MAX],
        }
    }

    /// Records a failure and hands it straight back, so that noting it and
    /// returning it are one expression.
    pub fn fail(&mut self, message: &'static str, e: Error) -> Error {
        self.status = Some(e);

        let text = described((message, e));
        let bytes = text.as_bytes();
        let len = bytes.len().min(ERROR_MAX - 1);

        self.error = [0; ERROR_MAX];
        self.error[..len].copy_from_slice(&bytes[..len]);
        e
    }

    /// What the last scan found. Read back rather than copied out, so nothing
    /// can be looking at an older answer than the scanner has.
    pub(crate) fn scanned(&self) -> Info {
        *self.scan.info()
    }

    /// The stream from `offset` on, which is empty when those bytes have been
    /// dropped or have not arrived.
    pub(crate) fn file_at(&self, offset: usize) -> &[u8] {
        self.input.at(offset)
    }

    /// The named picture, which is what an export reads.
    ///
    /// The C latched a `WebPImage` of pointers into a codec's memory at the
    /// end of every decode; these are all fields of the same struct, so a
    /// borrow says the same thing and cannot go stale.
    pub(crate) fn frame_of(&self, which: Source) -> Frame<'_> {
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
            Source::Canvas => self.canvas.frame(),
            Source::None => Frame::packed(&[], 0, 0, 0, Format::Argb),
        }
    }

    pub(crate) fn update_canvas_size(&mut self, w: i32, h: i32) {
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

    /* The canvas is negotiated between the container and the lossless module:
    the container knows what the file declared, the module knows what the frame
    header said, and either may be the first to learn it. */
    pub(crate) fn lossless_canvas_in(&mut self) {
        self.vp8l.set_canvas(self.width, self.height);
    }

    pub(crate) fn lossless_canvas_out(&mut self) {
        self.width = self.vp8l.width;
        self.height = self.vp8l.height;
        self.lossless_has_alpha = self.vp8l.has_alpha;
    }

    /// Decodes a whole VP8L chunk at `offset` into the decoder's ARGB canvas.
    pub(crate) fn lossless_decode(
        &mut self,
        offset: usize,
        size: usize,
    ) -> Result<(), Error> {
        self.lossless_canvas_in();

        let Self { vp8l, input, .. } = self;
        let ret = vp8l.decode_frame(
            crate::vp8l::Target::Argb,
            input.chunk(offset, size),
            false,
            None,
        );

        self.lossless_canvas_out();
        ret?;
        self.lossless_out = Some(Lossless::Argb);
        Ok(())
    }

    /* The decoder's answers to the questions the export asks, gathered at the
    call rather than reached for: the export owns no decoder state, so nothing
    it sees can drift from what the frame was decoded as. */
    pub(crate) fn export_settings(&self) -> ExportSettings {
        ExportSettings {
            out_format: self.out_format,
            premultiply: self.premultiply != 0,
            animation: self.animation,
            anim_mode: self.anim_mode,
            duration: self.frame_duration,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            anmf_flags: self.anmf_flags,
            /* An animation latches each sub-frame's alpha as it decodes it; a
            still has only the one image, whose two decoders report it
            separately. */
            has_alpha: if self.animation {
                self.frame_has_alpha
            } else {
                self.has_alpha || self.lossless_has_alpha
            },
            timestamp: self.frame_timestamp - self.frame_duration as i64,
        }
    }

    /// The scratch a whole-frame export writes through, and the picture it
    /// reads.
    ///
    /// The two come out of one destructuring because they are borrows of the
    /// same decoder, and that is what proves the source is not one of the
    /// buffers being written into — which is the whole of what the C's
    /// pointers were asserting silently.
    pub(crate) fn export_parts(
        &mut self,
        which: Source,
    ) -> (ExportTargets<'_>, Frame<'_>) {
        let Self {
            ydsp,
            options,
            rescale,
            transformed,
            output,
            converted,
            sink,
            vp8,
            vp8l,
            lossless_out,
            alpha_plane,
            canvas,
            has_alpha,
            width,
            height,
            ..
        } = self;
        let img = match which {
            Source::Lossy => {
                lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height)
            }
            Source::Lossless => lossless_view(vp8l, *lossless_out),
            Source::Converted => converted.frame(),
            Source::Canvas => canvas.frame(),
            Source::None => Frame::packed(&[], 0, 0, 0, Format::Argb),
        };

        (
            ExportTargets {
                dsp: ydsp,
                options,
                rescale,
                transformed,
                output,
                ext: sink.as_deref_mut(),
            },
            img,
        )
    }

    #[inline(never)]
    fn export_complete_still_lossless<'o>(
        &'o mut self,
        set: &ExportSettings,
        out: &mut Handout<'o>,
        height: i32,
    ) -> Result<(), Error> {
        let Self {
            ydsp,
            options,
            rescale,
            transformed,
            output,
            sink,
            vp8l,
            lossless_out,
            still_lossless,
            converted_rows,
            ..
        } = self;
        let img = lossless_view(vp8l, *lossless_out);
        let targets = ExportTargets {
            dsp: ydsp,
            options,
            rescale,
            transformed,
            output,
            ext: sink.as_deref_mut(),
        };

        export_packed(set, targets, img, out)?;
        *still_lossless = true;
        *converted_rows = height;
        Ok(())
    }

    /// As [`Self::export_parts`], for the resumable row exports, which convert
    /// a codec's own picture into the decoder's buffers rather than reading
    /// one of them.
    pub(crate) fn row_parts(&mut self, which: Source) -> (RowTargets<'_>, Frame<'_>) {
        let Self {
            ydsp,
            options,
            output,
            converted,
            sink,
            converted_rows,
            converted_format,
            vp8,
            vp8l,
            lossless_out,
            alpha_plane,
            has_alpha,
            width,
            height,
            ..
        } = self;
        let img = match which {
            Source::Lossy => {
                lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height)
            }
            Source::Lossless => lossless_view(vp8l, *lossless_out),
            _ => Frame::packed(&[], 0, 0, 0, Format::Argb),
        };

        (
            RowTargets {
                dsp: ydsp,
                options,
                output,
                converted,
                ext: sink.as_deref_mut(),
                converted_rows,
                converted_format,
            },
            img,
        )
    }

    /// Everything a decode builds up as it walks the frames, which both
    /// opening a new file and rewinding the current one have to put back. The
    /// buffers the frames are decoded into are kept: they are sized on use and
    /// reused.
    fn anim_state_reset(&mut self) {
        self.vp8l.reset();
        self.canvas.release();
        self.still_done = false;
        self.vp8_active = false;
        self.still_lossy = false;
        self.alpha_pending = false;
        self.converted_rows = 0;
        self.converted_format = FORMAT_NONE;
        self.still_lossless = false;
        self.lossless_out = None;
        self.subframe_out = None;
        self.frame_index = 0;
        self.width = 0;
        self.height = 0;
        self.has_alpha = false;
        self.lossless_has_alpha = false;
        self.frame_has_alpha = false;
        self.key_frame = false;
        self.prev_key_frame = false;
        self.anmf_flags = 0;
        self.prev_anmf_flags = 0;
        self.prev_width = 0;
        self.prev_height = 0;
        self.prev_pos_x = 0;
        self.prev_pos_y = 0;
        self.pos_x = 0;
        self.pos_y = 0;
        self.frame_duration = 0;
        self.frame_timestamp = 0;
    }

    /// Clears everything derived from a file but keeps the input allocation,
    /// which a stream grows across many calls.
    fn reset(&mut self) {
        self.meta = [None, None, None];
        self.anim_state_reset();
        self.vp8l.release();
        self.converted.release();
        self.output.release();
        self.transformed.release();
        self.input.reset();
        self.scan.reset();
        self.pos = 0;
        self.end = 0;
        self.opened = false;
        self.streaming = false;
        self.eos = false;
        self.headers_valid = false;
        self.truncated = false;
        self.input_mode = 0;
        self.animation = false;
        self.canvas_width = 0;
        self.canvas_height = 0;
        self.anim_loop_count = 0;
        self.anim_frame_count = 0;
        self.anim_background_argb = 0;
        self.clear_argb = [0; 4];
        self.clear_yuva = CLEAR_YUVA_BLACK;
        self.info_has_alpha = false;
        self.info_coding = Coding::Unknown;
        self.status = None;
        self.error = [0; ERROR_MAX];
    }

    /// Drops input the decoder can no longer look at. The chunk at `pos` is
    /// kept whole: a VP8 chunk decoded row by row keeps range coders pointing
    /// into it until the frame is done, and those are rebased on the next step.
    fn file_compact(&mut self) {
        let mut keep = self.pos;

        if self.alpha_pending && self.alpha_data_offset < keep {
            keep = self.alpha_data_offset;
        }
        self.input.compact(keep);
    }

    /// Takes a copy of each metadata chunk the scanner has reached, since the
    /// buffer it sits in is dropped as the stream moves past it.
    fn capture_metadata(&mut self) -> Result<(), Error> {
        let hs = self.scanned();
        let (discarded, size) = (self.input.discarded(), self.input.size());

        for i in 0..METADATA_NB {
            let offset = hs.meta_offset[i];
            let bytes = hs.meta_size[i] as usize;

            if offset == 0 || self.meta[i].is_some() {
                continue;
            }
            if offset < discarded || offset > size || bytes > size - offset {
                continue;
            }
            let mut copy = Vec::new();

            if copy.try_reserve_exact(bytes).is_err() {
                return Err(Error::NoMemory);
            }
            copy.extend_from_slice(&self.file_at(offset)[..bytes]);
            self.meta[i] = Some(copy);
        }
        Ok(())
    }

    fn rescan_headers(&mut self) -> Result<(), Error> {
        let base = self.input.discarded();
        let walked = self
            .scan
            .headers(self.input.bytes(), base, self.streaming, true);
        /* Read back whatever the walk reached, error or not: a stream whose
        headers are merely incomplete keeps decoding from what has arrived. */
        let meta = self.capture_metadata();

        walked?;
        meta?;

        let hs = self.scanned();

        self.end = hs.end;
        self.canvas_width = hs.width;
        self.canvas_height = hs.height;
        self.animation = hs.animation;
        self.anim_frame_count = hs.frame_count;
        self.anim_loop_count = hs.loop_count;
        self.anim_background_argb = hs.background_argb;
        self.info_has_alpha = hs.has_alpha;
        self.info_coding = hs.coding;
        self.truncated = hs.truncated;
        if !self.headers_valid {
            self.pos = if hs.raw == Raw::No { 12 } else { 0 };
            self.headers_valid = true;
        }
        Ok(())
    }

    /// No more input is coming, so a chunk list that stops short of what it
    /// promised, or that never carried an image, cannot be completed.
    fn check_final_headers(&mut self, message: &'static str) -> Result<(), Error> {
        let hs = self.scanned();

        if hs.truncated {
            return Err(self.fail(message, Error::Truncated));
        }
        if hs.images == 0 && hs.frame_count == 0 {
            return Err(self.fail("no image data found", Error::InvalidData));
        }
        Ok(())
    }

    /// What both opens do once the bytes are in: read the headers, and undo
    /// the open if they are not a whole file's worth.
    fn opened_headers(&mut self) -> Result<(), Error> {
        let read = match self.rescan_headers() {
            Err(e) => Err(self.fail("cannot read headers", e)),
            Ok(()) => self.check_final_headers("file ends inside a chunk"),
        };

        if let Err(e) = read {
            self.input.reset();
            self.headers_valid = false;
            return Err(e);
        }
        self.opened = true;
        self.eos = true;
        Ok(())
    }

    /// Opens a file the decoder takes a copy of.
    pub fn open(&mut self, data: &[u8]) -> Result<(), Error> {
        self.reset();
        if let Err(e) = self.input.own(data) {
            return Err(self.fail("cannot buffer input", e));
        }
        self.opened_headers()
    }

    /// Opens a file the decoder reads in place, for as long as `'a` lasts.
    pub fn open_borrowed(&mut self, data: &'a [u8]) -> Result<(), Error> {
        self.reset();
        self.input.borrow(data);
        self.opened_headers()
    }

    pub fn open_stream(&mut self) -> Result<(), Error> {
        self.reset();
        self.opened = true;
        self.streaming = true;
        Ok(())
    }

    pub fn append(&mut self, data: &[u8]) -> Result<(), Error> {
        if !self.streaming || self.eos {
            return Err(self.fail("not an open stream", Error::InvalidArgument));
        }
        if data.is_empty() {
            return Ok(());
        }
        if self.input_mode == 2 {
            return Err(
                self.fail("cannot mix append and update", Error::InvalidArgument)
            );
        }
        self.input_mode = 1;

        self.file_compact();
        if let Err(e) = self.input.append(data) {
            return Err(self.fail("cannot buffer input", e));
        }

        /* Headers that are merely incomplete are the normal state of a stream. */
        match self.rescan_headers() {
            Err(Error::Truncated) | Ok(()) => Ok(()),
            Err(e) => Err(self.fail("cannot read headers", e)),
        }
    }

    /// Replaces the stream with a longer prefix of the same file, which the
    /// decoder reads in place.
    pub fn update(&mut self, data: &'a [u8]) -> Result<(), Error> {
        if !self.streaming || self.eos {
            return Err(self.fail("not an open stream", Error::InvalidArgument));
        }
        if self.input_mode == 1 {
            return Err(
                self.fail("cannot mix append and update", Error::InvalidArgument)
            );
        }
        if data.len() < self.input.size() {
            return Err(self.fail("stream buffer shrank", Error::InvalidArgument));
        }
        self.input_mode = 2;
        self.input.borrow(data);

        match self.rescan_headers() {
            Err(Error::Truncated) | Ok(()) => Ok(()),
            Err(e) => {
                self.input.reset();
                self.headers_valid = false;
                Err(self.fail("cannot read headers", e))
            }
        }
    }

    pub fn update_owned(&mut self, data: Vec<u8>) -> Result<(), Error> {
        if !self.streaming || self.eos {
            return Err(self.fail("not an open stream", Error::InvalidArgument));
        }
        if self.input_mode == 1 {
            return Err(
                self.fail("cannot mix append and update", Error::InvalidArgument)
            );
        }
        if data.len() < self.input.size() {
            return Err(self.fail("stream buffer shrank", Error::InvalidArgument));
        }
        self.input_mode = 2;
        self.input.replace_owned(data);

        match self.rescan_headers() {
            Err(Error::Truncated) | Ok(()) => Ok(()),
            Err(e) => {
                self.input.reset();
                self.headers_valid = false;
                Err(self.fail("cannot read headers", e))
            }
        }
    }

    pub fn take_update_buffer(&mut self) -> Result<Vec<u8>, Error> {
        if !self.streaming || self.eos || self.input_mode != 2 {
            return Err(self.fail("not an updated stream", Error::InvalidArgument));
        }
        Ok(self.input.take_owned())
    }

    pub fn end_of_stream(&mut self) -> Result<(), Error> {
        if !self.streaming {
            return Err(self.fail("not an open stream", Error::InvalidArgument));
        }
        self.eos = true;
        if let Err(e) = self.rescan_headers() {
            return Err(self.fail("cannot read headers", e));
        }
        self.check_final_headers("stream ended early")
    }
}

impl Decoder<'_> {
    /// A crop that names no pixels and a scale that names no size are the two
    /// things the type cannot rule out on its own.
    pub fn set_core_options(&mut self, options: Options) -> Result<(), Error> {
        let bad_crop = options
            .crop
            .is_some_and(|(l, t, w, h)| l < 0 || t < 0 || w <= 0 || h <= 0);
        let bad_scale = options
            .scale
            .is_some_and(|(w, h)| w < 0 || h < 0 || (w == 0 && h == 0));

        if bad_crop || bad_scale {
            return Err(self.fail("invalid decoder options", Error::InvalidArgument));
        }
        if self.anim_mode == ANIM_SUBFRAME && options.transforms() {
            return Err(self.fail(
                "cropping, scaling and flipping are defined against the canvas, \
                 which sub-frame mode does not produce",
                Error::InvalidArgument,
            ));
        }
        self.bypass_filtering = options.bypass_filtering;
        self.options = options;
        Ok(())
    }

    pub fn set_animation_mode(&mut self, mode: i32) -> Result<(), Error> {
        if mode != ANIM_COMPOSITED && mode != ANIM_SUBFRAME {
            return Err(self.fail("invalid animation mode", Error::InvalidArgument));
        }
        if mode == ANIM_SUBFRAME && self.options.transforms() {
            return Err(self.fail(
                "sub-frame mode cannot be combined with cropping, scaling or flipping",
                Error::InvalidArgument,
            ));
        }
        /* Sub-frame mode never builds the canvas the composited one carries
        from frame to frame, so the two cannot be swapped part-way through an
        animation. wpd_decoder_rewind() clears the frame index and reopens the
        choice. */
        if mode != self.anim_mode && self.animation && self.frame_index != 0 {
            return Err(self.fail(
                "the animation mode cannot change mid-animation",
                Error::InvalidArgument,
            ));
        }
        self.anim_mode = mode;
        Ok(())
    }

    pub fn set_output_format(&mut self, format: i32) -> Result<(), Error> {
        if format != FORMAT_NONE && !format_valid(format) {
            return Err(self.fail("invalid output format", Error::InvalidArgument));
        }
        self.out_format = format;
        self.premultiply = i32::from(format_is_premultiplied(format));
        Ok(())
    }

    /// Rows already handed out live in whichever buffer was current at the
    /// time, so a new destination has to be filled from the top again.
    fn drop_converted_rows(&mut self) {
        self.converted_rows = 0;
        self.converted_format = FORMAT_NONE;
    }

    /// Points the decode at somewhere to put its rows, or back at the
    /// decoder's own memory.
    ///
    /// Rows already handed out live in whichever destination was current at
    /// the time, so a new one has to be filled from the top again. Naming the
    /// same destination twice is the caller's business, not the decoder's:
    /// this cannot tell one sink from another, so it takes every call as a
    /// change and whoever can compare them does not call it.
    /// Whether a destination has been named.
    pub fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    pub fn set_sink(&mut self, sink: Option<Box<dyn RowSink>>) {
        if self.sink.is_some() || sink.is_some() {
            self.drop_converted_rows();
        }
        self.sink = sink;
    }
}

impl Decoder<'_> {
    /// The two checks every question about the file's shape shares.
    pub fn headers_ready(&mut self) -> Result<(), Error> {
        if !self.opened {
            return Err(self.fail("invalid decoder state", Error::InvalidArgument));
        }
        if !self.headers_valid {
            return Err(self.fail("headers incomplete", Error::Truncated));
        }
        Ok(())
    }

    pub fn image_info(&mut self) -> Result<ImageInfo, Error> {
        self.headers_ready()?;
        Ok(ImageInfo {
            width: self.canvas_width,
            height: self.canvas_height,
            has_alpha: self.info_has_alpha,
            is_animation: self.animation,
            frame_count: self.anim_frame_count,
            loop_count: self.anim_loop_count,
            background_argb: self.anim_background_argb,
            coding: self.info_coding,
            metadata: self.scanned().metadata,
        })
    }

    pub fn rewind(&mut self) -> Result<(), Error> {
        if !self.opened || !self.headers_valid {
            return Err(self.fail("invalid decoder state", Error::InvalidArgument));
        }
        /* wpd_decoder_append() is free to drop bytes the decoder has moved
        past, so the head of the file may simply no longer be there. */
        if self.input_mode == 1 {
            return Err(
                self.fail("an appended stream cannot be rewound", Error::Unsupported)
            );
        }
        let raw = self.scanned().raw;

        self.anim_state_reset();
        self.pos = if raw == Raw::No { 12 } else { 0 };
        self.status = None;
        self.error = [0; ERROR_MAX];
        Ok(())
    }

    pub fn frame_entry(&mut self, index: i32) -> Result<FrameInfo, Error> {
        self.headers_ready()?;

        let hs = self.scanned();

        /* A still image is one frame covering the whole canvas, which is what
        libwebp's demuxer reports for it too. */
        if !self.animation {
            if index != 0 {
                return Err(self.fail("no such frame", Error::InvalidArgument));
            }
            return Ok(FrameInfo {
                width: self.canvas_width,
                height: self.canvas_height,
                /* The image's own alpha, not the VP8X declaration the image
                info reports, so that this agrees with the frame decoding
                produces. */
                has_alpha: hs.image_has_alpha,
                complete: if hs.raw == Raw::No {
                    hs.images != 0
                } else {
                    self.eos
                },
                ..FrameInfo::default()
            });
        }

        let Ok(index) = usize::try_from(index) else {
            return Err(self.fail("no such frame", Error::InvalidArgument));
        };
        let Some(entry) = self.scan.frame(index).copied() else {
            return Err(self.fail("no such frame", Error::InvalidArgument));
        };

        Ok(FrameInfo {
            pos_x: entry.pos_x,
            pos_y: entry.pos_y,
            width: entry.width,
            height: entry.height,
            duration: entry.duration,
            dispose_to_background: entry.dispose
                == crate::container::Dispose::Background,
            blend: entry.blend == crate::container::Blend::Alpha,
            has_alpha: entry.has_alpha,
            complete: entry.complete,
        })
    }

    /// The named metadata chunk. `Ok(None)` means the file carries none.
    pub fn metadata(&mut self, which: i32) -> Result<Option<&[u8]>, Error> {
        if !self.opened {
            return Err(self.fail("invalid decoder state", Error::InvalidArgument));
        }
        if which <= 0 || which & (which - 1) != 0 || which >> METADATA_NB != 0 {
            return Err(self.fail("invalid metadata type", Error::InvalidArgument));
        }
        Ok(self.meta[which.trailing_zeros() as usize].as_deref())
    }
}

impl<'a> Decoder<'a> {
    fn still_lossy_pending(&self, chunk_type: u32) -> bool {
        chunk_type == TAG_VP8 && !self.animation && !self.still_done
    }

    fn still_lossless_pending(&self, chunk_type: u32) -> bool {
        chunk_type == TAG_VP8L && !self.animation && !self.still_done
    }

    /// The resumable lossless path, plus the copy the container keeps of what
    /// it left behind: which picture is being filled in.
    ///
    /// Returns whether the image is complete; `false` means more of the chunk
    /// is needed.
    fn lossless_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
        complete: bool,
    ) -> Result<bool, Error> {
        self.lossless_canvas_in();

        let Self { vp8l, input, .. } = self;
        let ret = vp8l.still_step(input.chunk(offset, avail), size, complete);

        self.lossless_canvas_out();

        let done = ret? == crate::error::Status::Done;

        if self.vp8l.still_active() || done {
            self.still_lossless = true;
            self.lossless_out = Some(Lossless::Still);
        }
        Ok(done)
    }

    fn lossless_peek(&mut self) -> Result<(), Error> {
        self.vp8l.still_peek()?;
        self.lossless_out = Some(Lossless::Still);
        Ok(())
    }

    fn emit_still_lossless<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<bool, Failure> {
        self.still_done = true;

        let set = self.export_settings();
        let ret = if self.options.transforms() {
            let (t, img) = self.export_parts(Source::Lossless);

            export_packed(&set, t, img, out)
        } else {
            let (t, img) = self.row_parts(Source::Lossless);
            let height = img.height;

            export_still_lossless(&set, t, &img, out, height)
        };

        ret.map_err(|e| ("cannot output frame", e))?;
        Ok(true)
    }

    fn emit_still_lossy<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<bool, Failure> {
        self.still_done = true;

        let packed_only =
            !self.options.transforms() && format_is_packed(self.out_format);
        let set = self.export_settings();
        let ret = if packed_only {
            let (t, img) = self.row_parts(Source::Lossy);
            let height = img.height;

            export_still_packed(&set, t, &img, out, height)
        } else {
            let (t, img) = self.export_parts(Source::Lossy);

            export_packed(&set, t, img, out)
        };

        ret.map_err(|e| ("cannot output frame", e))?;
        Ok(true)
    }

    /// A file with no RIFF wrapper: one image chunk, and for the lossy shape
    /// possibly an ALPH chunk ahead of it.
    fn decode_raw<'o>(&'o mut self, out: &mut Handout<'o>) -> Result<bool, Failure> {
        let hs = self.scanned();

        if !self.eos {
            return Ok(false);
        }
        if hs.truncated {
            return Err(("raw image is truncated", Error::Truncated));
        }
        if hs.raw_image_size > i32::MAX as usize {
            return Err(("raw image is too large", Error::TooLarge));
        }
        self.width = 0;
        self.height = 0;

        let source = if hs.raw == Raw::Lossless {
            self.lossless_decode(hs.raw_image_offset, hs.raw_image_size)
                .map_err(|e| ("VP8L decode failed", e))?;
            self.still_done = true;
            self.still_lossless = true;
            self.converted_rows = self.frame_of(Source::Lossless).height;
            Source::Lossless
        } else {
            if hs.raw == Raw::AlphaAndLossy {
                if hs.raw_alpha_size == 0 {
                    return Err(("invalid ALPHA chunk", Error::InvalidData));
                }
                let header = self.file_at(hs.raw_alpha_offset)[0] as i32;

                if header & 3 > ALPHA_COMPRESSION_VP8L {
                    return Err(("unsupported ALPHA compression", Error::Unsupported));
                }
                self.has_alpha = true;
                self.alpha_compression = header & 3;
                self.alpha_filter = header >> 2 & 3;
                self.alpha_data_offset = hs.raw_alpha_offset + 1;
                self.alpha_data_size = hs.raw_alpha_size - 1;
            }
            self.vp8_lossy_decode_frame(hs.raw_image_offset, hs.raw_image_size)
                .map_err(|e| ("VP8 decode failed", e))?;
            self.still_done = true;
            Source::Lossy
        };
        let set = self.export_settings();
        let (t, img) = self.export_parts(source);

        export_packed(&set, t, img, out).map_err(|e| ("cannot output frame", e))?;
        Ok(true)
    }
}

impl Decoder<'_> {
    /// Decodes the next frame into `out`. Returns whether a picture came out;
    /// `false` means the file is finished or the stream has not caught up.
    pub fn next_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<bool, Failure> {
        let decoder = self;

        if !decoder.opened {
            return Err(("no file opened", Error::InvalidArgument));
        }
        if !decoder.headers_valid {
            if !decoder.eos {
                return Ok(false); /* the headers have not arrived yet */
            }
            return Err(("no image data found", Error::Truncated));
        }
        if decoder.scanned().raw != Raw::No {
            return if decoder.still_done {
                Ok(false)
            } else {
                decoder.decode_raw(out)
            };
        }

        while decoder.pos + 8 <= decoder.end {
            let chunk_pos = decoder.pos;
            let (chunk_type, size) = {
                let chunk = decoder.file_at(chunk_pos);

                (rl32(chunk), rl32(&chunk[4..]))
            };
            let payload_pos = chunk_pos + 8;

            if size == u32::MAX {
                return Err(("invalid chunk size", Error::InvalidData));
            }
            let size = size as usize;
            let padded_size = size + (size & 1);

            if decoder.end - payload_pos < padded_size {
                if !decoder.eos {
                    let avail = decoder.end - payload_pos;

                    if decoder.still_lossy_pending(chunk_type) {
                        let done = decoder
                            .vp8_lossy_step(payload_pos, avail, size)
                            .map_err(|e| ("VP8 decode failed", e))?;

                        if done {
                            return decoder.emit_still_lossy(out);
                        }
                    } else if decoder.still_lossless_pending(chunk_type) {
                        let done = decoder
                            .lossless_step(payload_pos, avail, size, false)
                            .map_err(|e| ("VP8L decode failed", e))?;

                        if done {
                            return decoder.emit_still_lossless(out);
                        }
                    }
                    return Ok(false); /* the rest of this chunk has not arrived yet */
                }
                return Err(("chunk runs past the end of the file", Error::Truncated));
            }
            decoder.pos += 8 + padded_size;

            match chunk_type {
                TAG_ALPH => {
                    if size == 0 {
                        return Err(("invalid ALPHA chunk size", Error::InvalidData));
                    }
                    let alpha_header = decoder.file_at(payload_pos)[0] as i32;

                    decoder.alpha_data_offset = payload_pos + 1;
                    decoder.alpha_pending = true;
                    decoder.alpha_data_size = size - 1;

                    let filter_m = (alpha_header >> 2) & 0x03;
                    let compression = alpha_header & 0x03;

                    if compression > ALPHA_COMPRESSION_VP8L {
                        crate::log::warning("skipping unsupported ALPHA chunk");
                    } else {
                        decoder.has_alpha = true;
                        decoder.alpha_compression = compression;
                        decoder.alpha_filter = filter_m;
                    }
                }
                TAG_VP8 => {
                    if decoder.animation || decoder.still_done {
                        continue;
                    }
                    let ret = if decoder.vp8_active {
                        decoder
                            .vp8_lossy_step(payload_pos, size, size)
                            /* A whole chunk that leaves the frame unfinished is
                            a frame that ends inside the data it declared. */
                            .and_then(|done| {
                                done.then_some(()).ok_or(Error::InvalidData)
                            })
                    } else {
                        decoder.width = 0;
                        decoder.height = 0;
                        decoder.vp8_lossy_decode_frame(payload_pos, size)
                    };

                    ret.map_err(|e| ("VP8 decode failed", e))?;
                    return decoder.emit_still_lossy(out);
                }
                TAG_VP8L => {
                    if decoder.animation || decoder.still_done {
                        continue;
                    }
                    if decoder.vp8l.still_active() {
                        decoder
                            .lossless_step(payload_pos, size, size, true)
                            .and_then(|done| {
                                done.then_some(()).ok_or(Error::InvalidData)
                            })
                            .map_err(|e| ("VP8L decode failed", e))?;
                        return decoder.emit_still_lossless(out);
                    }
                    decoder.width = 0;
                    decoder.height = 0;
                    decoder
                        .lossless_decode(payload_pos, size)
                        .map_err(|e| ("VP8L decode failed", e))?;
                    decoder.still_done = true;

                    let set = decoder.export_settings();
                    let height = decoder.frame_of(Source::Lossless).height;

                    decoder
                        .export_complete_still_lossless(&set, out, height)
                        .map_err(|e| ("cannot output frame", e))?;
                    return Ok(true);
                }
                TAG_ANMF => {
                    if !decoder.animation
                        || decoder.canvas_width == 0
                        || decoder.canvas_height == 0
                    {
                        return Err((
                            "ANMF chunk without animation header",
                            Error::InvalidData,
                        ));
                    }
                    decoder
                        .decode_anmf(payload_pos, size)
                        .map_err(|e| ("animation frame decode failed", e))?;

                    let set = decoder.export_settings();
                    let source = match (decoder.anim_mode, decoder.subframe_out) {
                        (ANIM_SUBFRAME, Some(which)) => which,
                        (ANIM_SUBFRAME, None) => Source::None,
                        _ => Source::Canvas,
                    };
                    let (t, img) = decoder.export_parts(source);

                    export_packed(&set, t, img, out)
                        .map_err(|e| ("cannot output frame", e))?;
                    return Ok(true);
                }
                _ => {}
            }
        }

        Ok(false)
    }
}

impl Decoder<'_> {
    /// As much of the frame in progress as is finished. Returns whether a
    /// picture was produced, and fills `rows` in with how many of its rows
    /// are valid.
    pub fn partial_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
        rows_valid: &mut i32,
    ) -> Result<bool, Failure> {
        let decoder = self;

        if !decoder.opened {
            return Err(("no file opened", Error::InvalidArgument));
        }
        let set = decoder.export_settings();

        if decoder.still_lossless && decoder.vp8l.still_active() {
            decoder
                .lossless_peek()
                .map_err(|e| ("VP8L decode failed", e))?;
        }

        fn lossless_rows(d: &Decoder<'_>) -> i32 {
            if d.vp8l.still_active() {
                d.vp8l.still_rows_out()
            } else {
                d.frame_of(Source::Lossless).height
            }
        }
        fn lossy_rows(d: &Decoder<'_>) -> i32 {
            match (d.vp8_active, d.vp8.first()) {
                (true, Some(vp8)) => vp8.rows_finalized(),
                (true, None) => 0,
                (false, _) => d.height,
            }
        }

        if decoder.options.transforms() {
            let source = if decoder.still_lossless {
                if lossless_rows(decoder) < decoder.frame_of(Source::Lossless).height {
                    return Ok(false);
                }
                Source::Lossless
            } else if decoder.still_lossy {
                if lossy_rows(decoder) < decoder.height {
                    return Ok(false);
                }
                Source::Lossy
            } else {
                return Ok(false);
            };
            let (t, img) = decoder.export_parts(source);

            export_packed(&set, t, img, out).map_err(|e| ("cannot output frame", e))?;
            *rows_valid = out.height;
            return Ok(true);
        }

        if decoder.still_lossless {
            let upto = lossless_rows(decoder);
            let done = decoder.converted_rows;
            let (t, img) = decoder.row_parts(Source::Lossless);

            export_still_lossless(&set, t, &img, out, upto)
                .map_err(|e| ("cannot output frame", e))?;
            *rows_valid = upto.max(done);
            return Ok(true);
        }
        if !decoder.still_lossy {
            return Ok(false);
        }

        let mut rows = lossy_rows(decoder);

        if !format_is_packed(decoder.out_format) {
            let have = decoder.frame_of(Source::Lossy).format as i32;
            let format = if decoder.out_format == FORMAT_NONE {
                have
            } else {
                decoder.out_format
            };
            let first = if decoder.converted_format == format {
                decoder.converted_rows
            } else {
                0
            };

            if rows < first {
                rows = first;
            }

            let planar = have != Format::Yuva420p as i32 && format != have;

            if planar {
                let want_alpha = format == Format::Yuva420p as i32;
                let Decoder {
                    ydsp,
                    output,
                    vp8,
                    alpha_plane,
                    has_alpha,
                    width,
                    height,
                    ..
                } = &mut *decoder;
                let src =
                    lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height);
                ensure_yuva_rows(ydsp, output, &src, want_alpha, first, rows)
                    .map_err(|e| ("cannot output frame", e))?;
            }

            decoder.converted_rows = rows;
            decoder.converted_format = format;

            let ret = {
                let Decoder {
                    sink,
                    output,
                    vp8,
                    alpha_plane,
                    has_alpha,
                    width,
                    height,
                    ..
                } = &mut *decoder;
                let plane = if planar {
                    output.frame()
                } else {
                    lossy_view(vp8.first(), alpha_plane, *has_alpha, *width, *height)
                };

                match sink.as_deref_mut() {
                    Some(ext) => export_external_planar_rows(
                        &set, ext, &plane, format, out, first, rows,
                    ),
                    None => {
                        export_own(&set, plane, format, out);
                        Ok(())
                    }
                }
            };

            ret.map_err(|e| ("cannot output frame", e))?;
            *rows_valid = rows;
            return Ok(true);
        }

        /* The fancy upsampler pairs a row with the one below it, so the last
        finished row cannot be converted until the row after it exists. */
        if rows != 0 && rows < decoder.height {
            rows -= 1;
        }

        let done = decoder.converted_rows;
        let (t, img) = decoder.row_parts(Source::Lossy);

        export_still_packed(&set, t, &img, out, rows)
            .map_err(|e| ("cannot output frame", e))?;
        *rows_valid = rows.max(done);
        Ok(true)
    }
}

impl Decoder<'_> {
    /// What the last decode stopped on, or nothing if it did not stop.
    pub fn status(&self) -> Option<Error> {
        self.status
    }

    /// The last failure's message, which says more than the status does.
    pub fn error_message(&self) -> &str {
        if self.error[0] == 0 {
            return "unknown decoder error";
        }
        let end = self.error.iter().position(|&b| b == 0).unwrap_or(0);

        std::str::from_utf8(&self.error[..end]).unwrap_or("")
    }

    /// The message as it sits in memory, NUL-terminated, which is what
    /// `wpd_decoder_error` hands a pointer to.
    pub fn error_raw(&self) -> &[u8] {
        &self.error
    }
}
