pub mod anim;
pub mod convert;
pub mod export;
pub mod lossy;
pub mod slot;

use crate::anim::AnimState;
use crate::bits::rl32;
use crate::container::{
    Coding, Info, Raw, Scan, METADATA_NB, TAG_ALPH, TAG_ANMF, TAG_VP8, TAG_VP8L,
};
use crate::dsp::filters::FilterDsp;
use crate::dsp::rescale::RescaleDsp;
use crate::dsp::vp8l::Vp8lDsp;
use crate::dsp::yuv::YuvDsp;
use crate::error::Error;
use crate::handout::{Handout, RowSink};
use crate::image::Format;
use crate::info::{FrameInfo, ImageInfo};
use crate::input::Input;
use crate::options::Options;
use crate::picture::{Buffer, Frame, PlaneRef};
use crate::rescale::Scratches;
use crate::vp8l::Output as Lossless;

use self::slot::FrameSlot;

use self::convert::{
    ensure_yuva_rows, format_is_packed, format_is_premultiplied, format_valid,
};
use self::export::{
    export_external_planar_rows, export_own, export_packed, export_still_lossless,
    export_still_packed, ExportSettings, ExportTargets, RowTargets,
};

pub const FORMAT_NONE: i32 = -1;

pub const ANIM_COMPOSITED: i32 = 0;
pub const ANIM_SUBFRAME: i32 = 1;

pub type Failure = (&'static str, Error);

pub fn described((message, e): Failure) -> String {
    format!("{message} ({})", e.message())
}

pub(crate) struct StillLatch<'a> {
    still_lossless: &'a mut bool,
    converted_rows: &'a mut i32,
}

#[derive(Default)]
pub struct Decoder<'a> {
    /// What the frame being worked on has produced. A still uses this and
    /// nothing else; an animation fills it from the batch decoded ahead.
    pub(crate) frame: slot::FrameSlot,
    pub(crate) ldsp: Vp8lDsp,
    pub(crate) ydsp: YuvDsp,
    pub(crate) rdsp: RescaleDsp,
    pub(crate) fdsp: FilterDsp,
    pub(crate) out_format: OutFormat,
    pub(crate) options: Options,
    /* options.n_threads resolved once, so the decode paths need not ask
     * the machine how many processors it has for every frame. */
    pub(crate) threads: Threads,

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
    pub(crate) converted_format: OutFormat,
    pub(crate) still_lossless: bool,
    pub(crate) canvas_width: i32,
    pub(crate) canvas_height: i32,

    pub(crate) output: Buffer,
    pub(crate) transformed: Buffer,
    pub(crate) rescale: Scratches,

    /// Frames decoded before the walk has reached them. Compositing still
    /// happens in order, on this thread.
    pub(crate) ahead: slot::Ahead,

    pub(crate) canvas: Buffer,
    pub(crate) subframe_out: Option<Source>,
    pub(crate) anim_mode: i32,
    pub(crate) anim: AnimState,
    pub(crate) clear_argb: [u8; 4],
    pub(crate) clear_yuva: ClearYuva,

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
    pub(crate) input_mode: InputMode,

    pub(crate) sink: Option<Box<dyn RowSink>>,

    pub(crate) status: Option<Error>,
    pub(crate) error: Vec<u8>,
}

pub(crate) const ALPHA_COMPRESSION_NONE: i32 = 0;
pub(crate) const ALPHA_COMPRESSION_VP8L: i32 = 1;
const ALPHA_PREPROCESSED_LEVELS: i32 = 1;

const ERROR_MAX: usize = 128;

/* Each carries a default that is not the zero value, so that a derived
 * Default for the decoder is the same decoder new() hands out. */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct OutFormat(pub i32);

impl Default for OutFormat {
    fn default() -> Self {
        Self(FORMAT_NONE)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Threads(pub usize);

impl Default for Threads {
    fn default() -> Self {
        Self(crate::task::resolve(0))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ClearYuva(pub [u8; 4]);

impl Default for ClearYuva {
    fn default() -> Self {
        Self([16, 128, 128, 0])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum InputMode {
    #[default]
    Untouched,
    Append,
    Update,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Source {
    Lossy,
    Lossless,
    Converted,
    Canvas,
    #[default]
    None,
}

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
        None => empty_view(),
    }
}

pub(crate) fn empty_view<'a>() -> Frame<'a> {
    Frame::packed(&[], 0, 0, 0, Format::Argb)
}

/// Everything but the canvas comes out of the frame that produced it; the
/// canvas belongs to the animation rather than to any one frame.
pub(crate) fn source_view<'a>(
    which: Source,
    frame: &'a FrameSlot,
    canvas: Option<&'a Buffer>,
) -> Frame<'a> {
    match which {
        Source::Canvas => canvas.map_or_else(empty_view, Buffer::frame),
        which => frame.view(which),
    }
}

impl<'a> Decoder<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail(&mut self, message: &'static str, e: Error) -> Error {
        self.status = Some(e);
        self.error.clear();
        self.error
            .extend_from_slice(described((message, e)).as_bytes());
        self.error.truncate(ERROR_MAX - 1);
        self.error.push(0);
        e
    }

    pub(crate) fn scanned(&self) -> Info {
        *self.scan.info()
    }

    pub(crate) fn file_at(&self, offset: usize) -> &[u8] {
        self.input.at(offset)
    }

    #[inline]
    pub fn require_open(&self) -> Result<(), Failure> {
        if !self.opened {
            return Err(("no file opened", Error::InvalidArgument));
        }
        Ok(())
    }

    pub(crate) fn frame_of(&self, which: Source) -> Frame<'_> {
        source_view(which, &self.frame, Some(&self.canvas))
    }

    pub(crate) fn lossless_canvas_in(&mut self) {
        self.frame
            .vp8l
            .set_canvas(self.frame.width, self.frame.height);
    }

    pub(crate) fn lossless_canvas_out(&mut self) {
        self.frame.width = self.frame.vp8l.width;
        self.frame.height = self.frame.vp8l.height;
        self.frame.lossless_has_alpha = self.frame.vp8l.has_alpha;
    }

    pub(crate) fn lossless_decode(
        &mut self,
        offset: usize,
        size: usize,
    ) -> Result<(), Error> {
        let (frame, env) = self.frame_parts();

        frame.lossless_decode(&env, offset, size)
    }

    /// Whether the output format on its own decides an animation frame must
    /// be ARGB before it is composited. Where it does not, the canvas does,
    /// and only the walk over the frames knows what the canvas holds.
    pub(crate) fn frame_to_argb(&self) -> bool {
        self.animation && format_is_packed(self.out_format.0)
    }

    /// Whether a frame is premultiplied before it is composited, which is
    /// what libwebp does.
    pub(crate) fn frame_premultiply(&self) -> bool {
        self.animation
            && format_is_premultiplied(self.out_format.0)
            && !(convert::premultiply_after_pack(self.animation, self.anim_mode)
                && convert::format_bpp(self.out_format.0) == 2)
    }

    pub(crate) fn export_settings(&self) -> ExportSettings {
        ExportSettings {
            out_format: self.out_format.0,
            premultiply: format_is_premultiplied(self.out_format.0),
            animation: self.animation,
            anim_mode: self.anim_mode,
            duration: self.frame_duration,
            pos_x: self.anim.pos_x,
            pos_y: self.anim.pos_y,
            anmf_flags: self.anim.anmf_flags,
            has_alpha: if self.animation {
                self.anim.frame_has_alpha
            } else {
                self.frame.has_alpha || self.frame.lossless_has_alpha
            },
            timestamp: self.frame_timestamp - self.frame_duration as i64,
            threads: self.threads.0,
        }
    }

    pub(crate) fn export_parts(
        &mut self,
        which: Source,
    ) -> (ExportTargets<'_>, Frame<'_>) {
        let (targets, img, _) = self.export_parts_latched(which);

        (targets, img)
    }

    fn export_parts_latched(
        &mut self,
        which: Source,
    ) -> (ExportTargets<'_>, Frame<'_>, StillLatch<'_>) {
        let Self {
            still_lossless,
            converted_rows,
            ydsp,
            rdsp,
            options,
            rescale,
            transformed,
            output,
            sink,
            frame,
            canvas,
            ..
        } = self;
        let img = source_view(which, frame, Some(canvas));

        (
            ExportTargets {
                dsp: ydsp,
                rdsp,
                options,
                rescale,
                transformed,
                output,
                ext: sink.as_deref_mut(),
            },
            img,
            StillLatch {
                still_lossless,
                converted_rows,
            },
        )
    }

    #[inline(never)]
    fn export_complete_still_lossless<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<(), Error> {
        let set = self.export_settings();
        let (targets, img, latch) = self.export_parts_latched(Source::Lossless);
        let height = img.height;

        export_packed(&set, targets, img, out)?;

        *latch.still_lossless = true;
        *latch.converted_rows = height;
        Ok(())
    }

    pub(crate) fn row_parts(&mut self, which: Source) -> (RowTargets<'_>, Frame<'_>) {
        let Self {
            ydsp,
            options,
            output,
            sink,
            converted_rows,
            converted_format,
            frame,
            ..
        } = self;
        let (img, converted) = frame.split_converted(which);

        (
            RowTargets {
                dsp: ydsp,
                options,
                output,
                converted,
                ext: sink.as_deref_mut(),
                converted_rows,
                converted_format: &mut converted_format.0,
            },
            img,
        )
    }

    fn anim_state_reset(&mut self) {
        self.frame.reset();
        self.ahead.clear();
        self.canvas.release();
        self.still_done = false;
        self.vp8_active = false;
        self.still_lossy = false;
        self.alpha_pending = false;
        self.converted_rows = 0;
        self.converted_format = OutFormat::default();
        self.still_lossless = false;
        self.subframe_out = None;
        self.anim = AnimState::default();
        self.frame_duration = 0;
        self.frame_timestamp = 0;
    }

    fn reset(&mut self) {
        self.meta = [None, None, None];
        self.anim_state_reset();
        self.frame.release();
        self.ahead.release();
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
        self.input_mode = InputMode::Untouched;
        self.animation = false;
        self.canvas_width = 0;
        self.canvas_height = 0;
        self.anim_loop_count = 0;
        self.anim_frame_count = 0;
        self.anim_background_argb = 0;
        self.clear_argb = [0; 4];
        self.clear_yuva = ClearYuva::default();
        self.info_has_alpha = false;
        self.info_coding = Coding::Unknown;
        self.status = None;
        self.error.clear();
    }

    fn file_compact(&mut self) {
        let mut keep = self.pos;

        if self.alpha_pending && self.frame.alpha_data_offset < keep {
            keep = self.frame.alpha_data_offset;
        }
        self.input.compact(keep);
    }

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

    pub fn open(&mut self, data: &[u8]) -> Result<(), Error> {
        self.reset();
        if let Err(e) = self.input.own(data) {
            return Err(self.fail("cannot buffer input", e));
        }
        self.opened_headers()
    }

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
        if self.input_mode == InputMode::Update {
            return Err(
                self.fail("cannot mix append and update", Error::InvalidArgument)
            );
        }
        self.input_mode = InputMode::Append;

        self.file_compact();
        if let Err(e) = self.input.append(data) {
            return Err(self.fail("cannot buffer input", e));
        }

        match self.rescan_headers() {
            Err(Error::Truncated) | Ok(()) => Ok(()),
            Err(e) => Err(self.fail("cannot read headers", e)),
        }
    }

    pub fn update(&mut self, data: &'a [u8]) -> Result<(), Error> {
        self.update_with(data.len(), |input| input.borrow(data))
    }

    pub fn update_owned(&mut self, data: Vec<u8>) -> Result<(), Error> {
        self.update_with(data.len(), move |input| input.replace_owned(data))
    }

    fn update_with(
        &mut self,
        len: usize,
        install: impl FnOnce(&mut Input<'a>),
    ) -> Result<(), Error> {
        if !self.streaming || self.eos {
            return Err(self.fail("not an open stream", Error::InvalidArgument));
        }
        if self.input_mode == InputMode::Append {
            return Err(
                self.fail("cannot mix append and update", Error::InvalidArgument)
            );
        }
        if len < self.input.size() {
            return Err(self.fail("stream buffer shrank", Error::InvalidArgument));
        }
        self.input_mode = InputMode::Update;
        install(&mut self.input);

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
        if !self.streaming || self.eos || self.input_mode != InputMode::Update {
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
    pub fn set_core_options(&mut self, options: Options) -> Result<(), Error> {
        let bad_crop = options
            .crop
            .is_some_and(|(l, t, w, h)| l < 0 || t < 0 || w <= 0 || h <= 0);
        let bad_scale = options
            .scale
            .is_some_and(|(w, h)| w < 0 || h < 0 || (w == 0 && h == 0));

        if bad_crop || bad_scale || options.n_threads < 0 {
            return Err(self.fail("invalid decoder options", Error::InvalidArgument));
        }
        if self.anim_mode == ANIM_SUBFRAME && options.transforms() {
            return Err(self.fail(
                "cropping, scaling and flipping are defined against the canvas, \
                 which sub-frame mode does not produce",
                Error::InvalidArgument,
            ));
        }
        self.options = options;
        self.threads = Threads(crate::task::resolve(options.n_threads));
        /* A batch decoded ahead baked in the settings that were current when
         * it ran, so it is no longer about the decode being asked for. */
        self.ahead.clear();
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
        if mode != self.anim_mode && self.animation && self.anim.frame_index != 0 {
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
        self.out_format = OutFormat(format);
        self.ahead.clear();
        Ok(())
    }

    fn drop_converted_rows(&mut self) {
        self.converted_rows = 0;
        self.converted_format = OutFormat::default();
    }

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
        if self.input_mode == InputMode::Append {
            return Err(
                self.fail("an appended stream cannot be rewound", Error::Unsupported)
            );
        }
        let raw = self.scanned().raw;

        self.anim_state_reset();
        self.pos = if raw == Raw::No { 12 } else { 0 };
        self.status = None;
        self.error.clear();
        Ok(())
    }

    pub fn frame_entry(&mut self, index: i32) -> Result<FrameInfo, Error> {
        self.headers_ready()?;

        let hs = self.scanned();

        /* Match libwebp: a still is one canvas-sized frame. */
        if !self.animation {
            if index != 0 {
                return Err(self.fail("no such frame", Error::InvalidArgument));
            }
            return Ok(FrameInfo {
                width: self.canvas_width,
                height: self.canvas_height,
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
            no_blend: entry.blend != crate::container::Blend::Alpha,
            has_alpha: entry.has_alpha,
            complete: entry.complete,
        })
    }

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

    fn lossless_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
        complete: bool,
    ) -> Result<bool, Error> {
        self.lossless_canvas_in();

        let Self { frame, input, .. } = self;
        let ret = frame
            .vp8l
            .still_step(input.chunk(offset, avail), size, complete);

        self.lossless_canvas_out();

        let done = ret? == crate::error::Status::Done;

        if self.frame.vp8l.still_active() || done {
            self.still_lossless = true;
            self.frame.lossless_out = Some(Lossless::Still);
        }
        Ok(done)
    }

    fn lossless_peek(&mut self) -> Result<(), Error> {
        self.frame.vp8l.still_peek()?;
        self.frame.lossless_out = Some(Lossless::Still);
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
            !self.options.transforms() && format_is_packed(self.out_format.0);
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
        self.frame.width = 0;
        self.frame.height = 0;

        if hs.raw == Raw::Lossless {
            self.lossless_decode(hs.raw_image_offset, hs.raw_image_size)
                .map_err(|e| ("VP8L decode failed", e))?;
            self.still_done = true;
            self.still_lossless = true;
            self.converted_rows = self.frame_of(Source::Lossless).height;
            self.export_complete_still_lossless(out)
                .map_err(|e| ("cannot output frame", e))?;
            return Ok(true);
        }
        if hs.raw == Raw::AlphaAndLossy {
            if hs.raw_alpha_size == 0 {
                return Err(("invalid ALPHA chunk", Error::InvalidData));
            }
            let header = self.file_at(hs.raw_alpha_offset)[0] as i32;

            if header & 3 > ALPHA_COMPRESSION_VP8L {
                return Err(("unsupported ALPHA compression", Error::Unsupported));
            }
            self.frame
                .set_alpha_chunk(header, hs.raw_alpha_offset + 1, hs.raw_alpha_size - 1)
                .map_err(|e| ("invalid ALPHA chunk", e))?;
        }
        self.vp8_lossy_decode_frame(hs.raw_image_offset, hs.raw_image_size)
            .map_err(|e| ("VP8 decode failed", e))?;
        self.still_done = true;

        let set = self.export_settings();
        let (t, img) = self.export_parts(Source::Lossy);

        export_packed(&set, t, img, out).map_err(|e| ("cannot output frame", e))?;
        Ok(true)
    }
}

impl Decoder<'_> {
    pub fn next_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<bool, Failure> {
        let decoder = self;

        decoder.require_open()?;
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
                    /* Match libwebp: an ALPH after its image is invalid. */
                    if decoder.still_done || decoder.vp8_active {
                        return Err((
                            "ALPHA chunk after the image it belongs to",
                            Error::InvalidData,
                        ));
                    }
                    let header = decoder.file_at(payload_pos)[0] as i32;

                    decoder.alpha_pending = true;
                    decoder
                        .frame
                        .set_alpha_chunk(header, payload_pos + 1, size - 1)
                        .map_err(|e| ("invalid ALPHA chunk", e))?;
                }
                TAG_VP8 => {
                    if decoder.animation || decoder.still_done {
                        continue;
                    }
                    let ret = if decoder.vp8_active {
                        decoder.vp8_lossy_step(payload_pos, size, size).and_then(
                            |done| done.then_some(()).ok_or(Error::InvalidData),
                        )
                    } else {
                        decoder.frame.width = 0;
                        decoder.frame.height = 0;
                        decoder.vp8_lossy_decode_frame(payload_pos, size)
                    };

                    ret.map_err(|e| ("VP8 decode failed", e))?;
                    return decoder.emit_still_lossy(out);
                }
                TAG_VP8L => {
                    if decoder.animation || decoder.still_done {
                        continue;
                    }
                    if decoder.frame.vp8l.still_active() {
                        decoder
                            .lossless_step(payload_pos, size, size, true)
                            .and_then(|done| {
                                done.then_some(()).ok_or(Error::InvalidData)
                            })
                            .map_err(|e| ("VP8L decode failed", e))?;
                        return decoder.emit_still_lossless(out);
                    }
                    decoder.frame.width = 0;
                    decoder.frame.height = 0;
                    decoder
                        .lossless_decode(payload_pos, size)
                        .map_err(|e| ("VP8L decode failed", e))?;
                    decoder.still_done = true;

                    decoder
                        .export_complete_still_lossless(out)
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
    pub fn partial_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
        rows_valid: &mut i32,
    ) -> Result<bool, Failure> {
        let decoder = self;

        decoder.require_open()?;

        let set = decoder.export_settings();

        if decoder.still_lossless && decoder.frame.vp8l.still_active() {
            decoder
                .lossless_peek()
                .map_err(|e| ("VP8L decode failed", e))?;
        }

        fn lossless_rows(d: &Decoder<'_>) -> i32 {
            if d.frame.vp8l.still_active() {
                d.frame.vp8l.still_rows_out()
            } else {
                d.frame_of(Source::Lossless).height
            }
        }
        fn lossy_rows(d: &Decoder<'_>) -> i32 {
            match (d.vp8_active, d.frame.vp8.first()) {
                (true, Some(vp8)) => vp8.rows_finalized(),
                (true, None) => 0,
                (false, _) => d.frame.height,
            }
        }

        if decoder.options.transforms() {
            let source = if decoder.still_lossless {
                if lossless_rows(decoder) < decoder.frame_of(Source::Lossless).height {
                    return Ok(false);
                }
                Source::Lossless
            } else if decoder.still_lossy {
                if lossy_rows(decoder) < decoder.frame.height {
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

        if !format_is_packed(decoder.out_format.0) {
            let have = decoder.frame_of(Source::Lossy).format as i32;
            let format = if decoder.out_format == OutFormat::default() {
                have
            } else {
                decoder.out_format.0
            };
            let first = if decoder.converted_format.0 == format {
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
                    frame,
                    ..
                } = &mut *decoder;
                let src = frame.view(Source::Lossy);

                ensure_yuva_rows(ydsp, output, &src, want_alpha, first, rows)
                    .map_err(|e| ("cannot output frame", e))?;
            }

            decoder.converted_rows = rows;
            decoder.converted_format.0 = format;

            let ret = {
                let Decoder {
                    sink,
                    output,
                    frame,
                    ..
                } = &mut *decoder;
                let plane = if planar {
                    output.frame()
                } else {
                    frame.view(Source::Lossy)
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

        if rows != 0 && rows < decoder.frame.height {
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
    pub fn status(&self) -> Option<Error> {
        self.status
    }

    pub fn error_message(&self) -> &str {
        match self.error.split_last() {
            Some((_nul, text)) => std::str::from_utf8(text).unwrap_or(""),
            None => "unknown decoder error",
        }
    }

    pub fn error_raw(&self) -> &[u8] {
        &self.error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_LOSSLESS: &[u8] = &[
        0x2f, 0x01, 0x40, 0x00, 0x00, 0x88, 0x88, 0x08, 0x00, 0x00, 0x00, 0x00,
    ];

    fn riff_lossless() -> Vec<u8> {
        let mut out = Vec::new();

        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(12 + RAW_LOSSLESS.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WEBPVP8L");
        out.extend_from_slice(&(RAW_LOSSLESS.len() as u32).to_le_bytes());
        out.extend_from_slice(RAW_LOSSLESS);
        out
    }

    struct NeverFits;

    impl RowSink for NeverFits {
        fn fits(&self, _p: usize, _row_len: usize, _rows: i32) -> bool {
            false
        }

        fn row(&mut self, _p: usize, _y: i32, _len: usize) -> &mut [u8] {
            unreachable!("a plane that does not fit is never written")
        }
    }

    fn failed_export(data: &[u8]) -> Decoder<'_> {
        let mut decoder = Decoder::new();

        decoder.set_sink(Some(Box::new(NeverFits)));
        decoder.open(data).unwrap();

        let mut out = Handout::default();

        assert_eq!(
            decoder.next_picture(&mut out).map_err(|(m, _)| m),
            Err("cannot output frame")
        );
        decoder
    }

    #[test]
    fn a_headerless_lossless_still_latches_before_its_export() {
        let decoder = failed_export(RAW_LOSSLESS);

        assert!(decoder.still_lossless);
        assert_eq!(decoder.converted_rows, 2);
    }

    #[test]
    fn a_riff_lossless_still_latches_after_its_export() {
        let data = riff_lossless();
        let decoder = failed_export(&data);

        assert!(!decoder.still_lossless);
        assert_eq!(decoder.converted_rows, 0);
    }

    #[test]
    fn a_decoder_with_no_file_open_is_turned_away() {
        let decoder = Decoder::new();

        assert_eq!(
            decoder.require_open().map_err(|(m, _)| m),
            Err("no file opened")
        );
        assert_eq!(failed_export(RAW_LOSSLESS).require_open(), Ok(()));
    }
}
