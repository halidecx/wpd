//! The public decoder, as declared by `include/wpd.h`.
//!
//! Everything else in this crate is a piece the decode is assembled from; this
//! is the assembly. `WPDDecoder` is opaque to the caller, so unlike the
//! structs the header declares it is a plain Rust type: the scanner, the two
//! frame decoders and the input buffer are owned values, and releasing them is
//! [`Drop`] rather than a sequence in `wpd_decoder_free` that a new field can
//! be left out of.
//!
//! Two habits from the C are kept deliberately. Positions in the stream are
//! offsets, never pointers, because appending to a stream may move or drop the
//! bytes under them. And nothing reaches into the decoder from the modules it
//! drives: what the export and the compositor need is gathered into a struct
//! at the call, so neither can read a field that has moved on since.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::{mem, ptr, slice};

use wpd::container::{Coding, Info, Raw, Scan, METADATA_NB};
use wpd::image::Format;

use crate::container::{info_clear, WPDImageInfo};
use crate::convert::{
    ensure_yuva_rows, format_bpp, format_is_packed, format_is_premultiplied,
    format_valid, WPDDecoderOptions,
};
use crate::export::{
    export_external_planar_rows, export_own, export_packed, export_still_lossless,
    export_still_packed, frame_clear, frame_valid, write_frame, ExportSettings,
    ExportTargets, External, RowTargets, WPDFrame, WPDOutputPlane,
};
use wpd::dsp::vp8l::Vp8lDsp;
use wpd::dsp::yuv::YuvDsp;
use wpd::handout::Handout;
use wpd::input::Input;
use wpd::options::Options;
use wpd::picture::{Buffer, Frame, PlaneRef};
use wpd::rescale::Scratch;
use wpd::vp8l::Output as Lossless;

pub const WPD_OK: c_int = 0;
pub const WPD_ERR_INVALID_ARG: c_int = -1;
pub const WPD_ERR_NOT_WEBP: c_int = -2;
pub const WPD_ERR_BITSTREAM: c_int = -3;
pub const WPD_ERR_TRUNCATED: c_int = -4;
pub const WPD_ERR_UNSUPPORTED: c_int = -5;
pub const WPD_ERR_NO_MEMORY: c_int = -6;
pub const WPD_ERR_TOO_LARGE: c_int = -7;
pub const WPD_ERR_BUFFER_TOO_SMALL: c_int = -8;

const WPD_PIX_FMT_NONE: c_int = -1;
const WPD_ANIM_COMPOSITED: c_int = 0;
const WPD_ANIM_SUBFRAME: c_int = 1;

pub(crate) const ALPHA_COMPRESSION_NONE: c_int = 0;
pub(crate) const ALPHA_COMPRESSION_VP8L: c_int = 1;

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

/// `WPDFrameInfo` from `include/wpd.h`.
#[repr(C)]
pub struct WPDFrameInfo {
    pub struct_size: usize,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub width: c_int,
    pub height: c_int,
    pub duration: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
    pub complete: c_int,
}

/// `WPDOutputBuffer` from `include/wpd.h`.
#[repr(C)]
pub struct WPDOutputBuffer {
    pub struct_size: usize,
    pub plane: [WPDOutputPlane; 4],
}

/// The oldest `WPDFrameInfo` this build accepts, and equally how much of the
/// caller's struct it may touch. Appending a field leaves this where it is and
/// adds a longer extent above it, the way the frame's does, so a caller
/// compiled against the shorter struct keeps working.
impl WPDFrameInfo {
    /// A zeroed struct of this build's revision.
    pub(crate) fn zeroed() -> Self {
        WPDFrameInfo {
            struct_size: mem::size_of::<WPDFrameInfo>(),
            pos_x: 0,
            pos_y: 0,
            width: 0,
            height: 0,
            duration: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
            complete: 0,
        }
    }
}

fn frame_info_v1() -> usize {
    mem::offset_of!(WPDFrameInfo, complete) + mem::size_of::<c_int>()
}

fn output_buffer_v1() -> usize {
    mem::offset_of!(WPDOutputBuffer, plane) + mem::size_of::<[WPDOutputPlane; 4]>()
}

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
    /// Whichever lossless picture [`WPDDecoder::lossless_out`] names.
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
    vp8: Option<&'a wpd::vp8::Decoder>,
    alpha: &'a [u8],
    has_alpha: bool,
    width: c_int,
    height: c_int,
) -> Frame<'a> {
    let mut plane = [PlaneRef::borrowed(&[], 0); 4];

    if let Some(vp8) = vp8 {
        for (p, out) in vp8.picture.planes.iter().zip(plane.iter_mut()) {
            *out = PlaneRef::borrowed(&p.data[p.origin.min(p.data.len())..], p.stride);
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
    vp8l: &wpd::vp8l::Decoder,
    which: Option<Lossless>,
) -> Frame<'_> {
    match which.and_then(|which| vp8l.view(which)) {
        Some(frame) => frame,
        None => Frame::packed(&[], 0, 0, 0, Format::Argb),
    }
}

/// The decoder, with the lifetime of the file it was pointed at.
///
/// `wpd_decoder_open_borrowed` and `wpd_decoder_update` promise the caller's
/// bytes will outlive the decode; the C ABI cannot say so, so
/// [`WPDDecoderRaw`] is what crosses it and the promise is checked nowhere.
/// The safe API in [`crate::api`] hands out a real `'a` instead.
pub struct WPDDecoder<'a> {
    /// Built on the first lossy frame, as the C's `vp8_decode_init` was: a
    /// file with no VP8 chunk in it never pays for the lossy decoder.
    pub(crate) vp8: Option<Box<wpd::vp8::Decoder>>,
    pub(crate) bypass_filtering: bool,
    pub(crate) ldsp: Vp8lDsp,
    pub(crate) ydsp: YuvDsp,
    pub(crate) out_format: c_int,
    pub(crate) premultiply: c_int,
    pub(crate) options: Options,

    pub(crate) input: Input<'a>,
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) scan: Box<Scan>,
    pub(crate) animation: bool,
    pub(crate) still_done: bool,
    pub(crate) vp8_active: bool,
    pub(crate) still_lossy: bool,
    pub(crate) alpha_pending: bool,
    pub(crate) converted_rows: c_int,
    pub(crate) converted_format: c_int,
    pub(crate) still_lossless: bool,
    pub(crate) frame_index: c_int,
    pub(crate) canvas_width: c_int,
    pub(crate) canvas_height: c_int,

    pub(crate) has_alpha: bool,
    pub(crate) alpha_compression: c_int,
    pub(crate) alpha_filter: c_int,
    /// An offset, not a pointer: appending to a stream can move the bytes.
    pub(crate) alpha_data_offset: usize,
    pub(crate) alpha_data_size: usize,
    pub(crate) alpha_plane: Vec<u8>,

    pub(crate) vp8l: wpd::vp8l::Decoder,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
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
    pub(crate) anim_mode: c_int,
    pub(crate) anmf_flags: c_int,
    pub(crate) pos_x: c_int,
    pub(crate) pos_y: c_int,
    pub(crate) frame_has_alpha: bool,
    pub(crate) key_frame: bool,
    pub(crate) prev_anmf_flags: c_int,
    pub(crate) prev_width: c_int,
    pub(crate) prev_height: c_int,
    pub(crate) prev_pos_x: c_int,
    pub(crate) prev_pos_y: c_int,
    pub(crate) prev_key_frame: bool,
    pub(crate) clear_argb: [u8; 4],
    pub(crate) clear_yuva: [u8; 4],

    pub(crate) anim_loop_count: c_int,
    pub(crate) anim_frame_count: c_int,
    pub(crate) anim_background_argb: u32,
    pub(crate) frame_duration: c_int,
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

    pub(crate) ext: External,
    pub(crate) ext_active: bool,

    pub(crate) status: c_int,
    /// A fixed buffer rather than a `String`: `wpd_decoder_error` hands out a
    /// pointer into it, and the C's was good for the decoder's whole life.
    pub(crate) error: [u8; ERROR_MAX],
}

/// What the C ABI passes around, since a `*mut` cannot carry a lifetime.
pub type WPDDecoderRaw = WPDDecoder<'static>;

/// What the core's failures are called at the ABI.
pub(crate) fn status(e: wpd::error::Error) -> c_int {
    match e {
        wpd::error::Error::InvalidArgument => WPD_ERR_INVALID_ARG,
        wpd::error::Error::InvalidData => WPD_ERR_BITSTREAM,
        wpd::error::Error::NoMemory => WPD_ERR_NO_MEMORY,
        wpd::error::Error::TooLarge => WPD_ERR_TOO_LARGE,
        wpd::error::Error::Truncated => WPD_ERR_TRUNCATED,
        wpd::error::Error::NotWebp => WPD_ERR_NOT_WEBP,
        wpd::error::Error::Unsupported => WPD_ERR_UNSUPPORTED,
        wpd::error::Error::BufferTooSmall => WPD_ERR_BUFFER_TOO_SMALL,
    }
}

/// Internal failures are either a `WPDStatus` or a negated errno.
fn status_from_internal(code: c_int) -> c_int {
    match code {
        0 => WPD_OK,
        /* -EINVAL, which the image allocators raise for a degenerate size. */
        -22 => WPD_ERR_INVALID_ARG,
        _ if (WPD_ERR_BUFFER_TOO_SMALL..=WPD_ERR_INVALID_ARG).contains(&code) => code,
        _ => WPD_ERR_BITSTREAM,
    }
}

fn status_string(status: c_int) -> &'static CStr {
    match status {
        WPD_OK => c"success",
        WPD_ERR_INVALID_ARG => c"invalid argument",
        WPD_ERR_NOT_WEBP => c"not a WebP file",
        WPD_ERR_BITSTREAM => c"invalid bitstream",
        WPD_ERR_TRUNCATED => c"truncated file",
        WPD_ERR_UNSUPPORTED => c"unsupported feature",
        WPD_ERR_NO_MEMORY => c"out of memory",
        WPD_ERR_TOO_LARGE => c"image too large",
        WPD_ERR_BUFFER_TOO_SMALL => c"output buffer too small",
        _ => c"unknown error",
    }
}

fn status_text(status: c_int) -> &'static str {
    status_string(status).to_str().unwrap_or("unknown error")
}

#[no_mangle]
pub extern "C" fn wpd_status_string(status: c_int) -> *const c_char {
    status_string(status).as_ptr()
}

pub(crate) fn rl24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

pub(crate) fn rl32(b: &[u8]) -> u32 {
    rl24(b) | (b[3] as u32) << 24
}

impl<'a> WPDDecoder<'a> {
    pub(crate) fn new() -> Self {
        WPDDecoder {
            vp8: None,
            bypass_filtering: false,
            ldsp: Vp8lDsp::new(),
            ydsp: YuvDsp::new(),
            out_format: WPD_PIX_FMT_NONE,
            premultiply: 0,
            options: Options::default(),

            input: Input::new(),
            pos: 0,
            end: 0,
            scan: Box::new(Scan::new()),
            animation: false,
            still_done: false,
            vp8_active: false,
            still_lossy: false,
            alpha_pending: false,
            converted_rows: 0,
            converted_format: WPD_PIX_FMT_NONE,
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

            vp8l: wpd::vp8l::Decoder::new(),
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
            anim_mode: WPD_ANIM_COMPOSITED,
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

            ext: External([WPDOutputPlane::empty(); 4]),
            ext_active: false,

            status: WPD_OK,
            error: [0; ERROR_MAX],
        }
    }

    fn set_error(&mut self, message: &str, code: c_int) -> c_int {
        self.status = status_from_internal(code);

        let text = format!("{message} ({})", status_text(self.status));
        let bytes = text.as_bytes();
        let len = bytes.len().min(ERROR_MAX - 1);

        self.error = [0; ERROR_MAX];
        self.error[..len].copy_from_slice(&bytes[..len]);
        self.status
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
                self.vp8.as_deref(),
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

    pub(crate) fn update_canvas_size(&mut self, w: c_int, h: c_int) {
        if self.width != 0 && self.width != w {
            wpd::log::warning(&format!("Width mismatch. {} != {w}", self.width));
        }
        self.width = w;
        if self.height != 0 && self.height != h {
            wpd::log::warning(&format!("Height mismatch. {} != {h}", self.height));
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
    pub(crate) fn lossless_decode(&mut self, offset: usize, size: usize) -> c_int {
        self.lossless_canvas_in();

        let Self { vp8l, input, .. } = self;
        let ret = vp8l.decode_frame(
            wpd::vp8l::Target::Argb,
            input.chunk(offset, size),
            false,
            None,
        );

        self.lossless_canvas_out();
        match ret {
            Ok(()) => {
                self.lossless_out = Some(Lossless::Argb);
                WPD_OK
            }
            Err(e) => status(e),
        }
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
            ext_active: self.ext_active,
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
            ext,
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
                lossy_view(vp8.as_deref(), alpha_plane, *has_alpha, *width, *height)
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
                ext,
            },
            img,
        )
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
            ext,
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
                lossy_view(vp8.as_deref(), alpha_plane, *has_alpha, *width, *height)
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
                ext,
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
        self.converted_format = WPD_PIX_FMT_NONE;
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
        self.status = WPD_OK;
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
    fn capture_metadata(&mut self) -> c_int {
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
                return WPD_ERR_NO_MEMORY;
            }
            copy.extend_from_slice(&self.file_at(offset)[..bytes]);
            self.meta[i] = Some(copy);
        }
        WPD_OK
    }

    fn rescan_headers(&mut self) -> c_int {
        let base = self.input.discarded();
        let status =
            match self
                .scan
                .headers(self.input.bytes(), base, self.streaming, true)
            {
                Ok(()) => WPD_OK,
                Err(e) => crate::container::status(e),
            };
        /* Read back whatever the walk reached, error or not: a stream whose
        headers are merely incomplete keeps decoding from what has arrived. */
        let meta = self.capture_metadata();

        if status != WPD_OK {
            return status;
        }
        if meta != WPD_OK {
            return meta;
        }
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
        WPD_OK
    }

    /// No more input is coming, so a chunk list that stops short of what it
    /// promised, or that never carried an image, cannot be completed.
    fn check_final_headers(&mut self, message: &str) -> c_int {
        let hs = self.scanned();

        if hs.truncated {
            return self.set_error(message, WPD_ERR_TRUNCATED);
        }
        if hs.images == 0 && hs.frame_count == 0 {
            return self.set_error("no image data found", WPD_ERR_BITSTREAM);
        }
        WPD_OK
    }

    /// What both opens do once the bytes are in: read the headers, and undo
    /// the open if they are not a whole file's worth.
    fn opened_headers(&mut self) -> c_int {
        let mut status = self.rescan_headers();

        status = if status != WPD_OK {
            self.set_error("cannot read headers", status)
        } else {
            self.check_final_headers("file ends inside a chunk")
        };
        if status != WPD_OK {
            self.input.reset();
            self.headers_valid = false;
            return status;
        }
        self.opened = true;
        self.eos = true;
        WPD_OK
    }

    /// Opens a file the decoder takes a copy of.
    pub(crate) fn open(&mut self, data: &[u8]) -> c_int {
        self.reset();
        if let Err(e) = self.input.own(data) {
            return self.set_error("cannot buffer input", status(e));
        }
        self.opened_headers()
    }

    /// Opens a file the decoder reads in place, for as long as `'a` lasts.
    pub(crate) fn open_borrowed(&mut self, data: &'a [u8]) -> c_int {
        self.reset();
        self.input.borrow(data);
        self.opened_headers()
    }

    pub(crate) fn open_stream(&mut self) -> c_int {
        self.reset();
        self.opened = true;
        self.streaming = true;
        WPD_OK
    }

    pub(crate) fn append(&mut self, data: &[u8]) -> c_int {
        if !self.streaming || self.eos {
            return self.set_error("not an open stream", WPD_ERR_INVALID_ARG);
        }
        if data.is_empty() {
            return WPD_OK;
        }
        if self.input_mode == 2 {
            return self.set_error("cannot mix append and update", WPD_ERR_INVALID_ARG);
        }
        self.input_mode = 1;

        self.file_compact();
        if let Err(e) = self.input.append(data) {
            return self.set_error("cannot buffer input", status(e));
        }

        let status = self.rescan_headers();

        /* Headers that are merely incomplete are the normal state of a stream. */
        if status == WPD_ERR_TRUNCATED {
            return WPD_OK;
        }
        if status != WPD_OK {
            return self.set_error("cannot read headers", status);
        }
        WPD_OK
    }

    /// Replaces the stream with a longer prefix of the same file, which the
    /// decoder reads in place.
    pub(crate) fn update(&mut self, data: &'a [u8]) -> c_int {
        if !self.streaming || self.eos {
            return self.set_error("not an open stream", WPD_ERR_INVALID_ARG);
        }
        if self.input_mode == 1 {
            return self.set_error("cannot mix append and update", WPD_ERR_INVALID_ARG);
        }
        if data.len() < self.input.size() {
            return self.set_error("stream buffer shrank", WPD_ERR_INVALID_ARG);
        }
        self.input_mode = 2;
        self.input.borrow(data);

        let status = self.rescan_headers();

        if status == WPD_ERR_TRUNCATED {
            return WPD_OK;
        }
        if status != WPD_OK {
            self.input.reset();
            self.headers_valid = false;
            return self.set_error("cannot read headers", status);
        }
        WPD_OK
    }

    pub(crate) fn end_of_stream(&mut self) -> c_int {
        if !self.streaming {
            return self.set_error("not an open stream", WPD_ERR_INVALID_ARG);
        }
        self.eos = true;

        let status = self.rescan_headers();

        if status != WPD_OK {
            return self.set_error("cannot read headers", status);
        }
        self.check_final_headers("stream ended early")
    }
}

#[no_mangle]
pub extern "C" fn wpd_decoder_create() -> *mut WPDDecoderRaw {
    wpd::log::set_sink(crate::compat::forward_log);
    wpd::cpu::init();

    Box::into_raw(Box::new(WPDDecoder::new()))
}

/// # Safety
///
/// `decoder` must come from [`wpd_decoder_create`] and not have been freed.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_free(decoder: *mut WPDDecoderRaw) {
    if !decoder.is_null() {
        drop(unsafe { Box::from_raw(decoder) });
    }
}

impl WPDDecoder<'_> {
    /// The versioned C struct: check what only its encoding can get wrong,
    /// then hand the rest to [`Self::set_core_options`].
    pub(crate) fn set_options(&mut self, options: &WPDDecoderOptions) -> c_int {
        let flag = |v: c_int| v == 0 || v == 1;

        if options.struct_size < WPDDecoderOptions::v1()
            || !flag(options.bypass_filtering)
            || !flag(options.no_fancy_upsampling)
            || !flag(options.use_cropping)
            || !flag(options.use_scaling)
            || !flag(options.flip)
        {
            return self.set_error("invalid decoder options", WPD_ERR_INVALID_ARG);
        }
        self.set_core_options(options.to_core())
    }

    /// A crop that names no pixels and a scale that names no size are the two
    /// things the type cannot rule out on its own.
    pub(crate) fn set_core_options(&mut self, options: Options) -> c_int {
        let bad_crop = options
            .crop
            .is_some_and(|(l, t, w, h)| l < 0 || t < 0 || w <= 0 || h <= 0);
        let bad_scale = options
            .scale
            .is_some_and(|(w, h)| w < 0 || h < 0 || (w == 0 && h == 0));

        if bad_crop || bad_scale {
            return self.set_error("invalid decoder options", WPD_ERR_INVALID_ARG);
        }
        if self.anim_mode == WPD_ANIM_SUBFRAME && options.transforms() {
            return self.set_error(
                "cropping, scaling and flipping are defined against the canvas, \
                 which sub-frame mode does not produce",
                WPD_ERR_INVALID_ARG,
            );
        }
        self.bypass_filtering = options.bypass_filtering;
        self.options = options;
        WPD_OK
    }

    pub(crate) fn set_animation_mode(&mut self, mode: c_int) -> c_int {
        if mode != WPD_ANIM_COMPOSITED && mode != WPD_ANIM_SUBFRAME {
            return self.set_error("invalid animation mode", WPD_ERR_INVALID_ARG);
        }
        if mode == WPD_ANIM_SUBFRAME && self.options.transforms() {
            return self.set_error(
                "sub-frame mode cannot be combined with cropping, scaling or flipping",
                WPD_ERR_INVALID_ARG,
            );
        }
        /* Sub-frame mode never builds the canvas the composited one carries
        from frame to frame, so the two cannot be swapped part-way through an
        animation. wpd_decoder_rewind() clears the frame index and reopens the
        choice. */
        if mode != self.anim_mode && self.animation && self.frame_index != 0 {
            return self.set_error(
                "the animation mode cannot change mid-animation",
                WPD_ERR_INVALID_ARG,
            );
        }
        self.anim_mode = mode;
        WPD_OK
    }

    pub(crate) fn set_output_format(&mut self, format: c_int) -> c_int {
        if format != WPD_PIX_FMT_NONE && !format_valid(format) {
            return self.set_error("invalid output format", WPD_ERR_INVALID_ARG);
        }
        self.out_format = format;
        self.premultiply = c_int::from(format_is_premultiplied(format));
        WPD_OK
    }

    /// Rows already handed out live in whichever buffer was current at the
    /// time, so a new destination has to be filled from the top again.
    fn drop_converted_rows(&mut self) {
        self.converted_rows = 0;
        self.converted_format = WPD_PIX_FMT_NONE;
    }

    /// # Safety
    ///
    /// The buffer's planes must be as it declares them.
    pub(crate) unsafe fn set_output_buffer(
        &mut self,
        buffer: Option<&WPDOutputBuffer>,
    ) -> c_int {
        let Some(buffer) = buffer else {
            if self.ext_active {
                self.drop_converted_rows();
            }
            self.ext_active = false;
            self.ext = External([WPDOutputPlane::empty(); 4]);
            return WPD_OK;
        };

        if buffer.struct_size < output_buffer_v1()
            || buffer.plane[0].data.is_null()
            || buffer.plane[0].stride == 0
        {
            return self.set_error("invalid output buffer", WPD_ERR_INVALID_ARG);
        }
        for plane in &buffer.plane {
            if plane.data.is_null() != (plane.stride == 0) {
                return self.set_error("invalid output buffer", WPD_ERR_INVALID_ARG);
            }
        }
        if !self.ext_active || self.ext.0 != buffer.plane {
            self.drop_converted_rows();
        }
        self.ext = External(buffer.plane);
        self.ext_active = true;
        WPD_OK
    }
}

/// # Safety
///
/// `options`, when not null, must point to a `WPDDecoderOptions` of at least
/// its own declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_options(
    decoder: *mut WPDDecoderRaw,
    options: *const WPDDecoderOptions,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return decoder.set_error("invalid decoder options", WPD_ERR_INVALID_ARG);
    };

    decoder.set_options(options)
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_animation_mode(
    decoder: *mut WPDDecoderRaw,
    mode: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.set_animation_mode(mode),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_output_format(
    decoder: *mut WPDDecoderRaw,
    format: c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.set_output_format(format),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `buffer`, when not null, must point to a `WPDOutputBuffer` of at least its
/// own declared `struct_size` bytes, whose planes are as they were declared.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_set_output_buffer(
    decoder: *mut WPDDecoderRaw,
    buffer: *const WPDOutputBuffer,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    unsafe { decoder.set_output_buffer(buffer.as_ref()) }
}

/// The caller's bytes as a slice, with the one lifetime extension the C ABI
/// forces: `wpd_decoder_open_borrowed` and `wpd_decoder_update` promise the
/// memory outlives the decoder, and nothing on this side can check it.
///
/// # Safety
///
/// `data` must be readable for `size` bytes, and for the two borrowing entry
/// points must stay so until the decoder is reopened or freed.
unsafe fn lent<'a>(data: *const u8, size: usize) -> &'a [u8] {
    if data.is_null() || size == 0 {
        return &[];
    }
    unsafe { slice::from_raw_parts(data, size) }
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return decoder.set_error("invalid input data", WPD_ERR_INVALID_ARG);
    }
    decoder.open(unsafe { lent(data, size) })
}

/// # Safety
///
/// `data` must be readable for `size` bytes and stay unchanged until the
/// decoder is reopened or freed, which is what the header asks of it.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open_borrowed(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return decoder.set_error("invalid input data", WPD_ERR_INVALID_ARG);
    }
    decoder.open_borrowed(unsafe { lent(data, size) })
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_open_stream(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.open_stream(),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_append(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return decoder.set_error("invalid input data", WPD_ERR_INVALID_ARG);
    }
    decoder.append(unsafe { lent(data, size) })
}

/// # Safety
///
/// `data` must be readable for `size` bytes and stay valid until the next
/// update or the decoder is freed.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_update(
    decoder: *mut WPDDecoderRaw,
    data: *const u8,
    size: usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() {
        return decoder.set_error("invalid input data", WPD_ERR_INVALID_ARG);
    }
    decoder.update(unsafe { lent(data, size) })
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_end_of_stream(
    decoder: *mut WPDDecoderRaw,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.end_of_stream(),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `info`, when not null, must point to a `WPDImageInfo` of at least its own
/// declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_get_info(
    decoder: *const WPDDecoderRaw,
    info: *mut WPDImageInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(info) = (unsafe { info.as_mut() }) else {
        return decoder.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
    };

    decoder.get_info(info)
}

impl WPDDecoder<'_> {
    pub(crate) fn get_info(&mut self, info: &mut WPDImageInfo) -> c_int {
        if info.struct_size < WPDImageInfo::v1() || !self.opened {
            return self.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
        }
        if !self.headers_valid {
            return self.set_error("headers incomplete", WPD_ERR_TRUNCATED);
        }
        info_clear(info);
        info.width = self.canvas_width;
        info.height = self.canvas_height;
        info.has_alpha = c_int::from(self.info_has_alpha);
        info.is_animation = c_int::from(self.animation);
        info.frame_count = self.anim_frame_count;
        info.loop_count = self.anim_loop_count;
        info.background_argb = self.anim_background_argb;
        info.coding = match self.info_coding {
            Coding::Unknown => 0,
            Coding::Lossy => 1,
            Coding::Lossless => 2,
        };
        info.metadata = self.scanned().metadata;
        WPD_OK
    }

    pub(crate) fn rewind(&mut self) -> c_int {
        if !self.opened || !self.headers_valid {
            return self.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
        }
        /* wpd_decoder_append() is free to drop bytes the decoder has moved
        past, so the head of the file may simply no longer be there. */
        if self.input_mode == 1 {
            return self.set_error(
                "an appended stream cannot be rewound",
                WPD_ERR_UNSUPPORTED,
            );
        }
        let raw = self.scanned().raw;

        self.anim_state_reset();
        self.pos = if raw == Raw::No { 12 } else { 0 };
        self.status = WPD_OK;
        self.error = [0; ERROR_MAX];
        WPD_OK
    }

    pub(crate) fn frame_info(
        &mut self,
        index: c_int,
        info: &mut WPDFrameInfo,
    ) -> c_int {
        if info.struct_size < frame_info_v1() || !self.opened {
            return self.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
        }
        if !self.headers_valid {
            return self.set_error("headers incomplete", WPD_ERR_TRUNCATED);
        }
        /* Everything past `struct_size`, which is the caller's, survives; the
        head is the size itself. */
        let size = info.struct_size;

        *info = WPDFrameInfo {
            struct_size: size,
            pos_x: 0,
            pos_y: 0,
            width: 0,
            height: 0,
            duration: 0,
            dispose: 0,
            blend: 0,
            has_alpha: 0,
            complete: 0,
        };

        let hs = self.scanned();

        /* A still image is one frame covering the whole canvas, which is what
        libwebp's demuxer reports for it too. */
        if !self.animation {
            if index != 0 {
                return self.set_error("no such frame", WPD_ERR_INVALID_ARG);
            }
            info.width = self.canvas_width;
            info.height = self.canvas_height;
            /* The image's own alpha, not the VP8X declaration WPDImageInfo
            reports, so that this agrees with the frame decoding produces. */
            info.has_alpha = c_int::from(hs.image_has_alpha);
            info.complete = c_int::from(if hs.raw == Raw::No {
                hs.images != 0
            } else {
                self.eos
            });
            return WPD_OK;
        }

        let Ok(index) = usize::try_from(index) else {
            return self.set_error("no such frame", WPD_ERR_INVALID_ARG);
        };
        let Some(entry) = self.scan.frame(index).copied() else {
            return self.set_error("no such frame", WPD_ERR_INVALID_ARG);
        };

        info.pos_x = entry.pos_x;
        info.pos_y = entry.pos_y;
        info.width = entry.width;
        info.height = entry.height;
        info.duration = entry.duration;
        info.dispose = entry.dispose as c_int;
        info.blend = entry.blend as c_int;
        info.has_alpha = c_int::from(entry.has_alpha);
        info.complete = c_int::from(entry.complete);
        WPD_OK
    }

    /// The named metadata chunk. `Ok(None)` means the file carries none.
    pub(crate) fn metadata(&mut self, which: c_int) -> Result<Option<&[u8]>, c_int> {
        if !self.opened {
            return Err(self.set_error("invalid decoder state", WPD_ERR_INVALID_ARG));
        }
        if which <= 0 || which & (which - 1) != 0 || which >> METADATA_NB != 0 {
            return Err(self.set_error("invalid metadata type", WPD_ERR_INVALID_ARG));
        }
        Ok(self.meta[which.trailing_zeros() as usize].as_deref())
    }
}

/// # Safety
///
/// `decoder` must be live.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_rewind(decoder: *mut WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => decoder.rewind(),
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// `info`, when not null, must point to a `WPDFrameInfo` of at least its own
/// declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_frame_info(
    decoder: *const WPDDecoderRaw,
    index: c_int,
    info: *mut WPDFrameInfo,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };
    let Some(info) = (unsafe { info.as_mut() }) else {
        return decoder.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
    };

    decoder.frame_info(index, info)
}

/// # Safety
///
/// `data` and `size` must be writable.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_metadata(
    decoder: *const WPDDecoderRaw,
    which: c_int,
    data: *mut *const u8,
    size: *mut usize,
) -> c_int {
    let Some(decoder) = (unsafe { decoder.cast_mut().as_mut() }) else {
        return WPD_ERR_INVALID_ARG;
    };

    if data.is_null() || size.is_null() {
        return decoder.set_error("invalid decoder state", WPD_ERR_INVALID_ARG);
    }
    match decoder.metadata(which) {
        Err(status) => status,
        Ok(found) => {
            let (at, len) = match found {
                Some(bytes) => (bytes.as_ptr(), bytes.len()),
                None => (ptr::null(), 0),
            };

            unsafe {
                data.write(at);
                size.write(len);
            }
            WPD_OK
        }
    }
}

impl<'a> WPDDecoder<'a> {
    fn still_lossy_pending(&self, chunk_type: u32) -> bool {
        chunk_type == TAG_VP8 && !self.animation && !self.still_done
    }

    fn still_lossless_pending(&self, chunk_type: u32) -> bool {
        chunk_type == TAG_VP8L && !self.animation && !self.still_done
    }

    /// The resumable lossless path, plus the copy the container keeps of what
    /// it left behind: which picture is being filled in.
    ///
    /// Returns 1 when the image is complete, 0 when more of the chunk is
    /// needed, or a negative status.
    fn lossless_step(
        &mut self,
        offset: usize,
        avail: usize,
        size: usize,
        complete: bool,
    ) -> c_int {
        self.lossless_canvas_in();

        let Self { vp8l, input, .. } = self;
        let ret = vp8l.still_step(input.chunk(offset, avail), size, complete);

        self.lossless_canvas_out();

        let ret = match ret {
            Ok(wpd::error::Status::Done) => 1,
            Ok(wpd::error::Status::NeedMore) => 0,
            Err(e) => return status(e),
        };

        if self.vp8l.still_active() || ret == 1 {
            self.still_lossless = true;
            self.lossless_out = Some(Lossless::Still);
        }
        ret
    }

    fn lossless_peek(&mut self) -> c_int {
        match self.vp8l.still_peek() {
            Ok(()) => {
                self.lossless_out = Some(Lossless::Still);
                WPD_OK
            }
            Err(e) => status(e),
        }
    }

    fn emit_still_lossless<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<c_int, (&'static str, c_int)> {
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

        if ret < 0 {
            return Err(("cannot output frame", ret));
        }
        Ok(1)
    }

    fn emit_still_lossy<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<c_int, (&'static str, c_int)> {
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

        if ret < 0 {
            return Err(("cannot output frame", ret));
        }
        Ok(1)
    }

    /// A file with no RIFF wrapper: one image chunk, and for the lossy shape
    /// possibly an ALPH chunk ahead of it.
    fn decode_raw<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<c_int, (&'static str, c_int)> {
        let hs = self.scanned();

        if !self.eos {
            return Ok(0);
        }
        if hs.truncated {
            return Err(("raw image is truncated", WPD_ERR_TRUNCATED));
        }
        if hs.raw_image_size > c_int::MAX as usize {
            return Err(("raw image is too large", WPD_ERR_TOO_LARGE));
        }
        self.width = 0;
        self.height = 0;

        let source = if hs.raw == Raw::Lossless {
            let ret = self.lossless_decode(hs.raw_image_offset, hs.raw_image_size);

            if ret < 0 {
                return Err(("VP8L decode failed", ret));
            }
            self.still_done = true;
            self.still_lossless = true;
            self.converted_rows = self.frame_of(Source::Lossless).height;
            Source::Lossless
        } else {
            if hs.raw == Raw::AlphaAndLossy {
                if hs.raw_alpha_size == 0 {
                    return Err(("invalid ALPHA chunk", WPD_ERR_BITSTREAM));
                }
                let header = self.file_at(hs.raw_alpha_offset)[0] as c_int;

                if header & 3 > ALPHA_COMPRESSION_VP8L {
                    return Err(("unsupported ALPHA compression", WPD_ERR_UNSUPPORTED));
                }
                self.has_alpha = true;
                self.alpha_compression = header & 3;
                self.alpha_filter = header >> 2 & 3;
                self.alpha_data_offset = hs.raw_alpha_offset + 1;
                self.alpha_data_size = hs.raw_alpha_size - 1;
            }
            let ret =
                self.vp8_lossy_decode_frame(hs.raw_image_offset, hs.raw_image_size);

            if ret < 0 {
                return Err(("VP8 decode failed", ret));
            }
            self.still_done = true;
            Source::Lossy
        };
        let set = self.export_settings();
        let (t, img) = self.export_parts(source);
        let ret = export_packed(&set, t, img, out);

        if ret < 0 {
            return Err(("cannot output frame", ret));
        }
        Ok(1)
    }
}

/// # Safety
///
/// `frame` must point to a `WPDFrame` of at least its own declared
/// `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_next_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => unsafe { next_frame(decoder, frame) },
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// As [`wpd_decoder_next_frame`].
unsafe fn next_frame(decoder: &mut WPDDecoder<'_>, frame: *mut WPDFrame) -> c_int {
    if !unsafe { frame_valid(frame) } {
        return decoder.set_error("invalid frame", WPD_ERR_INVALID_ARG);
    }

    /* The handout borrows the decoder, so everything the shim needs from it
    besides the pixels is taken first, and a failure carries a message rather
    than setting one -- `set_error` wants the decoder back. */
    let ext = External(decoder.ext.0);
    let mut out = Handout::default();

    match decoder.next_picture(&mut out) {
        Ok(ret) => {
            if ret > 0 {
                unsafe { write_frame(&out, &ext, frame) };
            }
            ret
        }
        Err((message, code)) => decoder.set_error(message, code),
    }
}

impl WPDDecoder<'_> {
    /// Decodes the next frame into `out`. Returns 1 for a picture, 0 when the
    /// file is finished or the stream has not caught up, or a status.
    fn next_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
    ) -> Result<c_int, (&'static str, c_int)> {
        let decoder = self;

        if !decoder.opened {
            return Err(("no file opened", WPD_ERR_INVALID_ARG));
        }
        if !decoder.headers_valid {
            if !decoder.eos {
                return Ok(0); /* the headers have not arrived yet */
            }
            return Err(("no image data found", WPD_ERR_TRUNCATED));
        }
        if decoder.scanned().raw != Raw::No {
            return if decoder.still_done {
                Ok(0)
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
                return Err(("invalid chunk size", WPD_ERR_BITSTREAM));
            }
            let size = size as usize;
            let padded_size = size + (size & 1);

            if decoder.end - payload_pos < padded_size {
                if !decoder.eos {
                    let avail = decoder.end - payload_pos;

                    if decoder.still_lossy_pending(chunk_type) {
                        let ret = decoder.vp8_lossy_step(payload_pos, avail, size);

                        if ret < 0 {
                            return Err(("VP8 decode failed", ret));
                        }
                        if ret != 0 {
                            return decoder.emit_still_lossy(out);
                        }
                    } else if decoder.still_lossless_pending(chunk_type) {
                        let ret =
                            decoder.lossless_step(payload_pos, avail, size, false);

                        if ret < 0 {
                            return Err(("VP8L decode failed", ret));
                        }
                        if ret != 0 {
                            return decoder.emit_still_lossless(out);
                        }
                    }
                    return Ok(0); /* the rest of this chunk has not arrived yet */
                }
                return Err(("chunk runs past the end of the file", WPD_ERR_TRUNCATED));
            }
            decoder.pos += 8 + padded_size;

            match chunk_type {
                TAG_ALPH => {
                    if size == 0 {
                        return Err(("invalid ALPHA chunk size", WPD_ERR_BITSTREAM));
                    }
                    let alpha_header = decoder.file_at(payload_pos)[0] as c_int;

                    decoder.alpha_data_offset = payload_pos + 1;
                    decoder.alpha_pending = true;
                    decoder.alpha_data_size = size - 1;

                    let filter_m = (alpha_header >> 2) & 0x03;
                    let compression = alpha_header & 0x03;

                    if compression > ALPHA_COMPRESSION_VP8L {
                        wpd::log::warning("skipping unsupported ALPHA chunk");
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
                        let ret = decoder.vp8_lossy_step(payload_pos, size, size);

                        if ret == 0 {
                            WPD_ERR_BITSTREAM
                        } else {
                            ret
                        }
                    } else {
                        decoder.width = 0;
                        decoder.height = 0;
                        decoder.vp8_lossy_decode_frame(payload_pos, size)
                    };

                    if ret < 0 {
                        return Err(("VP8 decode failed", ret));
                    }
                    return decoder.emit_still_lossy(out);
                }
                TAG_VP8L => {
                    if decoder.animation || decoder.still_done {
                        continue;
                    }
                    if decoder.vp8l.still_active() {
                        let mut ret =
                            decoder.lossless_step(payload_pos, size, size, true);

                        if ret == 0 {
                            ret = WPD_ERR_BITSTREAM;
                        }
                        if ret < 0 {
                            return Err(("VP8L decode failed", ret));
                        }
                        return decoder.emit_still_lossless(out);
                    }
                    decoder.width = 0;
                    decoder.height = 0;

                    let ret = decoder.lossless_decode(payload_pos, size);

                    if ret < 0 {
                        return Err(("VP8L decode failed", ret));
                    }
                    decoder.still_done = true;

                    let set = decoder.export_settings();
                    let height = decoder.frame_of(Source::Lossless).height;

                    decoder.still_lossless = true;
                    decoder.converted_rows = height;

                    let (t, img) = decoder.export_parts(Source::Lossless);
                    let ret = export_packed(&set, t, img, out);

                    if ret < 0 {
                        return Err(("cannot output frame", ret));
                    }
                    return Ok(1);
                }
                TAG_ANMF => {
                    if !decoder.animation
                        || decoder.canvas_width == 0
                        || decoder.canvas_height == 0
                    {
                        return Err((
                            "ANMF chunk without animation header",
                            WPD_ERR_BITSTREAM,
                        ));
                    }
                    let ret = decoder.decode_anmf(payload_pos, size);

                    if ret < 0 {
                        return Err(("animation frame decode failed", ret));
                    }
                    let set = decoder.export_settings();
                    let source = match (decoder.anim_mode, decoder.subframe_out) {
                        (WPD_ANIM_SUBFRAME, Some(which)) => which,
                        (WPD_ANIM_SUBFRAME, None) => Source::None,
                        _ => Source::Canvas,
                    };
                    let (t, img) = decoder.export_parts(source);
                    let ret = export_packed(&set, t, img, out);

                    if ret < 0 {
                        return Err(("cannot output frame", ret));
                    }
                    return Ok(1);
                }
                _ => {}
            }
        }

        Ok(0)
    }
}

/// # Safety
///
/// `frame` must be as [`wpd_decoder_next_frame`] requires, and `rows_valid`,
/// when not null, writable.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_partial_frame(
    decoder: *mut WPDDecoderRaw,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> c_int {
    match unsafe { decoder.as_mut() } {
        Some(decoder) => unsafe { partial_frame(decoder, frame, rows_valid) },
        None => WPD_ERR_INVALID_ARG,
    }
}

/// # Safety
///
/// As [`wpd_decoder_partial_frame`].
unsafe fn partial_frame(
    decoder: &mut WPDDecoder<'_>,
    frame: *mut WPDFrame,
    rows_valid: *mut c_int,
) -> c_int {
    if !unsafe { frame_valid(frame) } {
        return decoder.set_error("invalid frame", WPD_ERR_INVALID_ARG);
    }

    let ext = External(decoder.ext.0);
    let mut out = Handout::default();
    let mut rows = 0;

    unsafe { frame_clear(frame) };

    let ret = match decoder.partial_picture(&mut out, &mut rows) {
        Ok(had_picture) => {
            if had_picture {
                unsafe { write_frame(&out, &ext, frame) };
            }
            WPD_OK
        }
        Err((message, code)) => decoder.set_error(message, code),
    };

    if !rows_valid.is_null() {
        unsafe { rows_valid.write(rows) };
    }
    ret
}

impl WPDDecoder<'_> {
    /// As much of the frame in progress as is finished. Returns whether a
    /// picture was produced, and fills `rows` in with how many of its rows
    /// are valid.
    fn partial_picture<'o>(
        &'o mut self,
        out: &mut Handout<'o>,
        rows_valid: &mut c_int,
    ) -> Result<bool, (&'static str, c_int)> {
        let decoder = self;

        if !decoder.opened {
            return Err(("no file opened", WPD_ERR_INVALID_ARG));
        }
        let set = decoder.export_settings();

        if decoder.still_lossless && decoder.vp8l.still_active() {
            let ret = decoder.lossless_peek();

            if ret < 0 {
                return Err(("VP8L decode failed", ret));
            }
        }

        fn lossless_rows(d: &WPDDecoder<'_>) -> c_int {
            if d.vp8l.still_active() {
                d.vp8l.still_rows_out()
            } else {
                d.frame_of(Source::Lossless).height
            }
        }
        fn lossy_rows(d: &WPDDecoder<'_>) -> c_int {
            match (d.vp8_active, d.vp8.as_deref()) {
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
            let ret = export_packed(&set, t, img, out);

            if ret < 0 {
                return Err(("cannot output frame", ret));
            }
            *rows_valid = out.height;
            return Ok(true);
        }

        if decoder.still_lossless {
            let upto = lossless_rows(decoder);
            let done = decoder.converted_rows;
            let (t, img) = decoder.row_parts(Source::Lossless);
            let ret = export_still_lossless(&set, t, &img, out, upto);

            if ret < 0 {
                return Err(("cannot output frame", ret));
            }
            *rows_valid = upto.max(done);
            return Ok(true);
        }
        if !decoder.still_lossy {
            return Ok(false);
        }

        let mut rows = lossy_rows(decoder);

        if !format_is_packed(decoder.out_format) {
            let have = decoder.frame_of(Source::Lossy).format as c_int;
            let format = if decoder.out_format == WPD_PIX_FMT_NONE {
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

            let planar = have != Format::Yuva420p as c_int && format != have;

            if planar {
                let want_alpha = format == Format::Yuva420p as c_int;
                let WPDDecoder {
                    ydsp,
                    output,
                    vp8,
                    alpha_plane,
                    has_alpha,
                    width,
                    height,
                    ..
                } = &mut *decoder;
                let src = lossy_view(
                    vp8.as_deref(),
                    alpha_plane,
                    *has_alpha,
                    *width,
                    *height,
                );
                let ret = ensure_yuva_rows(ydsp, output, &src, want_alpha, first, rows);

                if ret < 0 {
                    return Err(("cannot output frame", ret));
                }
            }

            decoder.converted_rows = rows;
            decoder.converted_format = format;

            let ret = {
                let WPDDecoder {
                    ext,
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
                    lossy_view(vp8.as_deref(), alpha_plane, *has_alpha, *width, *height)
                };

                if set.ext_active {
                    export_external_planar_rows(
                        &set, ext, &plane, format, out, first, rows,
                    )
                } else {
                    export_own(&set, plane, format, out);
                    WPD_OK
                }
            };

            if ret < 0 {
                return Err(("cannot output frame", ret));
            }
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
        let ret = export_still_packed(&set, t, &img, out, rows);

        if ret < 0 {
            return Err(("cannot output frame", ret));
        }
        *rows_valid = rows.max(done);
        Ok(true)
    }
}

/// # Safety
///
/// `decoder` must be live or null.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_status(decoder: *const WPDDecoderRaw) -> c_int {
    match unsafe { decoder.as_ref() } {
        Some(decoder) => decoder.status,
        None => WPD_ERR_INVALID_ARG,
    }
}

impl WPDDecoder<'_> {
    /// The next frame, written into a struct this build owns.
    pub(crate) fn next_frame(&mut self, frame: &mut WPDFrame) -> c_int {
        unsafe { next_frame(self, frame) }
    }

    /// As [`Self::next_frame`], for the frame in progress.
    pub(crate) fn partial_frame(
        &mut self,
        frame: &mut WPDFrame,
        rows_valid: &mut c_int,
    ) -> c_int {
        unsafe { partial_frame(self, frame, rows_valid) }
    }

    /// The last failure's message, which says more than the status does.
    pub(crate) fn error_message(&self) -> &str {
        if self.error[0] == 0 {
            return "unknown decoder error";
        }
        let end = self.error.iter().position(|&b| b == 0).unwrap_or(0);

        std::str::from_utf8(&self.error[..end]).unwrap_or("")
    }
}

/// # Safety
///
/// As [`wpd_decoder_status`]. The string belongs to the decoder and stays
/// valid until its next failure.
#[no_mangle]
pub unsafe extern "C" fn wpd_decoder_error(
    decoder: *const WPDDecoderRaw,
) -> *const c_char {
    match unsafe { decoder.as_ref() } {
        Some(decoder) if decoder.error[0] != 0 => decoder.error.as_ptr().cast(),
        _ => c"unknown decoder error".as_ptr(),
    }
}

/// The planes `wpd_decode` copies out, which is what the frame's format says
/// it has.
fn frame_planes(format: c_int) -> usize {
    match Format::from_raw(format) {
        Some(Format::Yuva420p) => 4,
        Some(Format::Yuv420p) => 3,
        _ => 1,
    }
}

/// The memory behind a frame `wpd_decode` handed out, released by
/// `wpd_frame_free`.
struct WPDFrameOwner {
    plane: [Vec<u8>; 4],
}

/// Runs a one-shot decode of `data` into `frame`, leaving the decoder for the
/// caller to take what it needs out of.
///
/// # Safety
///
/// As [`wpd_decode`].
unsafe fn decode_once(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> (*mut WPDDecoderRaw, c_int) {
    let decoder = wpd_decoder_create();

    if decoder.is_null() {
        return (ptr::null_mut(), WPD_ERR_NO_MEMORY);
    }
    let mut status = if options.is_null() {
        WPD_OK
    } else {
        unsafe { wpd_decoder_set_options(decoder, options) }
    };

    if status == WPD_OK {
        status = unsafe { wpd_decoder_set_output_format(decoder, format) };
    }
    if status == WPD_OK && !buffer.is_null() {
        status = unsafe { wpd_decoder_set_output_buffer(decoder, buffer) };
    }
    if status == WPD_OK {
        status = unsafe { wpd_decoder_open_borrowed(decoder, data, size) };
    }
    let ret = if status == WPD_OK {
        unsafe { wpd_decoder_next_frame(decoder, frame) }
    } else {
        status
    };

    (decoder, ret)
}

/// # Safety
///
/// `data` must be readable for `size` bytes, and `frame` must point to a
/// `WPDFrame` of at least its own declared `struct_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn wpd_decode_into(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    buffer: *const WPDOutputBuffer,
    frame: *mut WPDFrame,
) -> c_int {
    if data.is_null() || buffer.is_null() || !unsafe { frame_valid(frame) } {
        return WPD_ERR_INVALID_ARG;
    }
    if !unsafe { (*frame).private_data }.is_null() {
        unsafe { wpd_frame_free(frame) };
    }
    let (decoder, ret) =
        unsafe { decode_once(data, size, format, options, buffer, frame) };

    if decoder.is_null() {
        return ret;
    }
    unsafe { wpd_decoder_free(decoder) };

    match ret {
        0 => WPD_ERR_BITSTREAM,
        ret if ret < 0 => ret,
        _ => WPD_OK,
    }
}

/// # Safety
///
/// As [`wpd_decode_into`].
#[no_mangle]
pub unsafe extern "C" fn wpd_decode(
    data: *const u8,
    size: usize,
    format: c_int,
    options: *const WPDDecoderOptions,
    frame: *mut WPDFrame,
) -> c_int {
    if data.is_null() || !unsafe { frame_valid(frame) } {
        return WPD_ERR_INVALID_ARG;
    }
    if !unsafe { (*frame).private_data }.is_null() {
        unsafe { wpd_frame_free(frame) };
    }
    let mut decoded = WPDFrame {
        struct_size: mem::size_of::<WPDFrame>(),
        data: [ptr::null(); 4],
        stride: [0; 4],
        width: 0,
        height: 0,
        format: WPD_PIX_FMT_NONE,
        duration: 0,
        timestamp: 0,
        private_data: ptr::null_mut(),
        pos_x: 0,
        pos_y: 0,
        dispose: 0,
        blend: 0,
        has_alpha: 0,
    };
    let (decoder, ret) =
        unsafe { decode_once(data, size, format, options, ptr::null(), &mut decoded) };

    if decoder.is_null() {
        return ret;
    }
    if ret <= 0 {
        unsafe { wpd_decoder_free(decoder) };
        return if ret < 0 { ret } else { WPD_ERR_BITSTREAM };
    }

    let owner = Box::new(WPDFrameOwner {
        plane: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    });
    let planes = frame_planes(decoded.format);

    unsafe { frame_clear(frame) };
    frame_copy(frame, &decoded);

    let out = unsafe { &mut *frame };

    out.private_data = Box::into_raw(owner).cast::<c_void>();

    let owner = unsafe { &mut *out.private_data.cast::<WPDFrameOwner>() };
    let mut status = WPD_OK;

    for p in 0..planes {
        let shift = u32::from(p == 1 || p == 2);
        let w = if planes == 1 {
            decoded.width as usize * format_bpp(decoded.format) as usize
        } else {
            wpd::image::ceil_rshift(decoded.width, shift) as usize
        };
        let h = wpd::image::ceil_rshift(decoded.height, shift) as usize;
        let Some(bytes) = w.checked_mul(h) else {
            status = WPD_ERR_TOO_LARGE;
            break;
        };

        owner.plane[p] = vec![0u8; bytes];
        for y in 0..h {
            let src = unsafe { decoded.data[p].offset(y as isize * decoded.stride[p]) };

            unsafe {
                ptr::copy_nonoverlapping(src, owner.plane[p][y * w..].as_mut_ptr(), w);
            }
        }
        out.data[p] = owner.plane[p].as_ptr();
        out.stride[p] = w as isize;
    }
    unsafe { wpd_decoder_free(decoder) };

    if status != WPD_OK {
        unsafe { wpd_frame_free(frame) };
        return status;
    }
    WPD_OK
}

/// Copies past `struct_size` rather than assigning: the caller's frame may be
/// a newer, longer revision of the struct, and its own size has to survive.
fn frame_copy(dst: *mut WPDFrame, src: &WPDFrame) {
    let head = mem::size_of::<usize>();
    let extent = unsafe { crate::export::frame_extent(dst) }
        .min(unsafe { crate::export::frame_extent(src) });

    unsafe {
        ptr::copy_nonoverlapping(
            (src as *const WPDFrame).cast::<u8>().add(head),
            dst.cast::<u8>().add(head),
            extent - head,
        );
    }
}

/// # Safety
///
/// `frame`, when not null, must be one [`wpd_decode`] filled in, or a frame
/// that owns nothing.
#[no_mangle]
pub unsafe extern "C" fn wpd_frame_free(frame: *mut WPDFrame) {
    if !unsafe { frame_valid(frame) } {
        return;
    }
    let owner = unsafe { (*frame).private_data };

    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner.cast::<WPDFrameOwner>()) });
    }
    unsafe { frame_clear(frame) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ABI's table of descriptions and the core's are written out
    /// separately, because one is NUL-terminated and the other is not. This is
    /// what says they still describe the same failures.
    #[test]
    fn every_core_failure_crosses_the_abi_under_its_own_name() {
        for e in [
            wpd::error::Error::InvalidArgument,
            wpd::error::Error::InvalidData,
            wpd::error::Error::NoMemory,
            wpd::error::Error::TooLarge,
            wpd::error::Error::Truncated,
            wpd::error::Error::NotWebp,
            wpd::error::Error::Unsupported,
            wpd::error::Error::BufferTooSmall,
        ] {
            assert_eq!(status_text(status(e)), e.message(), "{e:?}");
        }
    }
}
