//! The RIFF container: walking the chunk list of a file that may still be
//! arriving, without decoding any of it.
//!
//! The scanner works in stream offsets rather than pointers, so the buffer it
//! is handed can move, grow or lose its head between calls. [`Scan::headers`]
//! takes the window that is currently buffered along with the stream offset it
//! starts at, and resumes from wherever the last call stopped, which is what
//! keeps feeding a stream one piece at a time linear rather than quadratic.
//!
//! Nothing here indexes without a bound: every read goes through the helpers
//! below, which return zero past the end of the window. The C could prove its
//! reads in range by construction; a Rust bounds check that fires on damaged
//! input would be a denial of service the C did not have, so the reads simply
//! cannot leave the slice.

use crate::error::{Error, Result};
use crate::log;

/// ICCP, EXIF and XMP, in `WPDMetadata` bit order.
pub const METADATA_NB: usize = 3;

/// Far above any animation a player would sit through, and low enough that the
/// table cannot be made to eat memory by a file that is all ANMF headers. A
/// file past it still decodes; the table simply stops growing.
pub const MAX_FRAMES: usize = 1 << 20;

pub const ANMF_FLAG_DISPOSE: u8 = 1 << 0;
pub const ANMF_FLAG_NO_BLEND: u8 = 1 << 1;

const VP8X_FLAG_XMP: u8 = 0x04;
const VP8X_FLAG_EXIF: u8 = 0x08;
const VP8X_FLAG_ICCP: u8 = 0x20;
const VP8X_FLAG_ALPHA: u8 = 0x10;

const TAG_RIFF: u32 = u32::from_le_bytes(*b"RIFF");
const TAG_WEBP: u32 = u32::from_le_bytes(*b"WEBP");
const TAG_VP8: u32 = u32::from_le_bytes(*b"VP8 ");
const TAG_VP8L: u32 = u32::from_le_bytes(*b"VP8L");
const TAG_VP8X: u32 = u32::from_le_bytes(*b"VP8X");
const TAG_ALPH: u32 = u32::from_le_bytes(*b"ALPH");
const TAG_ANIM: u32 = u32::from_le_bytes(*b"ANIM");
const TAG_ANMF: u32 = u32::from_le_bytes(*b"ANMF");

/// The order of the `WPDMetadata` bits, so a bit indexes these tables.
const META_TAG: [u32; METADATA_NB] = [
    u32::from_le_bytes(*b"ICCP"),
    u32::from_le_bytes(*b"EXIF"),
    u32::from_le_bytes(*b"XMP "),
];

const META_VP8X_FLAG: [u8; METADATA_NB] =
    [VP8X_FLAG_ICCP, VP8X_FLAG_EXIF, VP8X_FLAG_XMP];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Coding {
    #[default]
    Unknown,
    Lossy,
    Lossless,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dispose {
    #[default]
    None,
    Background,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blend {
    #[default]
    Alpha,
    None,
}

/// Which of the three shapes a file with no RIFF wrapper turned out to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Raw {
    #[default]
    No,
    Lossless,
    Lossy,
    AlphaAndLossy,
}

/// One ANMF header, as the scanner reads it without decoding anything.
#[derive(Clone, Copy, Default)]
pub struct FrameEntry {
    pub pos_x: i32,
    pub pos_y: i32,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub dispose: Dispose,
    pub blend: Blend,
    pub has_alpha: bool,
    pub complete: bool,
}

/// What a scan found. The scanner's own state — where it stopped, the frame
/// table, how far into an ANMF the alpha walk has gone — stays in [`Scan`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Info {
    pub end: usize,
    pub width: i32,
    pub height: i32,
    pub has_alpha: bool,
    /// Alpha the image chunks themselves carry, without the VP8X declaration
    /// folded in, which is what a decoded frame reports.
    pub image_has_alpha: bool,
    pub animation: bool,
    pub images: i32,
    pub frame_count: i32,
    pub loop_count: i32,
    pub background_argb: u32,
    pub coding: Coding,
    pub truncated: bool,
    pub metadata: i32,
    pub meta_offset: [usize; METADATA_NB],
    pub meta_size: [u32; METADATA_NB],
    pub raw: Raw,
    pub raw_image_offset: usize,
    pub raw_image_size: usize,
    pub raw_alpha_offset: usize,
    pub raw_alpha_size: usize,
}

#[derive(Default)]
pub struct Scan {
    pos: usize,
    riff_end: u64,
    info: Info,
    vp8x: bool,
    /// The frame table, built only when `collect_frames` asks for it, so that
    /// reading a file's information keeps its promise not to allocate.
    /// `nb_frames` counts the frames whose payload is all here; a frame whose
    /// ANMF header has arrived but whose payload has not occupies the slot
    /// past them and is rebuilt on every rescan until it completes.
    collect_frames: bool,
    partial_frame: bool,
    frames_capped: bool,
    nb_frames: usize,
    /// How far the alpha scan has walked into the sub-chunk list starting at
    /// `anmf_scan_at`, so an ANMF arriving in pieces is not re-walked from its
    /// first sub-chunk on every delivery. The offset is never zero, so a scan
    /// that has moved on to another ANMF invalidates this by itself.
    anmf_scan_at: usize,
    anmf_scan_pos: usize,
    anmf_scan_done: bool,
    anmf_scan_alpha: bool,
    frames: Vec<FrameEntry>,
}

#[inline]
fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

fn rl16(b: &[u8], at: usize) -> u32 {
    u32::from(byte(b, at)) | u32::from(byte(b, at + 1)) << 8
}

fn rl24(b: &[u8], at: usize) -> u32 {
    rl16(b, at) | u32::from(byte(b, at + 2)) << 16
}

fn rl32(b: &[u8], at: usize) -> u32 {
    rl24(b, at) | u32::from(byte(b, at + 3)) << 24
}

/// `len` bytes of `b` from `from`, clipped to what is there.
fn window(b: &[u8], from: usize, len: usize) -> &[u8] {
    let from = from.min(b.len());
    let to = from.saturating_add(len).min(b.len());

    &b[from..to]
}

impl Scan {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts the scanner back to before any file, keeping the frame table's
    /// allocation, which the next file sizes on use and reuses.
    pub fn reset(&mut self) {
        let mut frames = core::mem::take(&mut self.frames);

        frames.clear();
        *self = Self {
            frames,
            ..Self::default()
        };
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The `index`th ANMF header, or `None` when there is no such frame. A
    /// frame whose header has arrived but whose payload has not is the last
    /// one and reports itself incomplete.
    pub fn frame(&self, index: usize) -> Option<&FrameEntry> {
        if index >= self.nb_frames + usize::from(self.partial_frame) {
            return None;
        }
        self.frames.get(index)
    }

    fn still_header(&mut self, tag: u32, p: &[u8], size: usize) {
        if tag == TAG_VP8L {
            self.info.coding = Coding::Lossless;
            if p.len() >= 5 && p[0] == 0x2f {
                let bits = rl32(p, 1);

                if bits >> 29 != 0 {
                    return;
                }
                self.info.width = (bits & 0x3fff) as i32 + 1;
                self.info.height = ((bits >> 14) & 0x3fff) as i32 + 1;
                self.info.image_has_alpha |= bits >> 28 & 1 != 0;
                self.info.has_alpha |= bits >> 28 & 1 != 0;
            }
        } else {
            self.info.coding = Coding::Lossy;
            if p.len() >= 10
                && size >= 10
                && p[3] == 0x9d
                && p[4] == 0x01
                && p[5] == 0x2a
            {
                let bits = rl24(p, 0);

                if bits & 1 != 0
                    || (bits >> 1) & 7 > 3
                    || bits & 0x10 == 0
                    || (bits >> 5) as usize > size - 10
                {
                    return;
                }
                self.info.width = (rl16(p, 6) & 0x3fff) as i32;
                self.info.height = (rl16(p, 8) & 0x3fff) as i32;
            }
        }
    }

    /// Walks an ANMF's sub-chunks for the one frame field the 16-byte ANMF
    /// header does not carry, and reports what it made of the alpha. Stops at
    /// the image chunk either way, and says nothing when the payload has not
    /// all arrived. Resumes where the last delivery of the same ANMF left off,
    /// so a frame that arrives in many pieces costs one walk of its sub-chunks
    /// in total rather than one per piece; only a sub-chunk stepped over whole
    /// advances that mark.
    fn anmf_alpha(&mut self, p: &[u8]) -> bool {
        let at = self.pos + 24;

        if self.anmf_scan_at != at || self.anmf_scan_pos > p.len() {
            self.anmf_scan_at = at;
            self.anmf_scan_pos = 0;
            self.anmf_scan_done = false;
            self.anmf_scan_alpha = false;
        }
        if self.anmf_scan_done {
            return self.anmf_scan_alpha;
        }

        let mut at = self.anmf_scan_pos;

        while p.len() - at >= 8 {
            let tag = rl32(p, at);
            let size = rl32(p, at + 4);

            if size == u32::MAX {
                return false;
            }
            let padded = size as usize + (size & 1) as usize;

            at += 8;
            if p.len() - at < padded {
                return false;
            }
            if tag == TAG_ALPH {
                self.anmf_scan_alpha = true;
            } else if tag == TAG_VP8L {
                if size >= 5 && byte(p, at) == 0x2f {
                    self.anmf_scan_alpha = rl32(p, at + 1) >> 28 & 1 != 0;
                }
            } else if tag != TAG_VP8 {
                at += padded;
                self.anmf_scan_pos = at;
                continue;
            }
            self.anmf_scan_done = true;
            return self.anmf_scan_alpha;
        }
        false
    }

    /// Records the ANMF whose payload starts `p`, of which only what is
    /// buffered is here. `complete` says whether the scan is stepping past the
    /// whole padded chunk, which is the only thing that may promote the entry:
    /// a frame still arriving takes the slot past the complete ones and is
    /// rewritten by the next scan, so the table never double-counts. Deriving
    /// it from the buffered length instead would count an odd-sized chunk
    /// twice, once when every byte but its pad has landed and again once the
    /// scan finally walks over it.
    fn anmf(&mut self, p: &[u8], complete: bool) -> Result<()> {
        if p.len() < 16 {
            return Ok(());
        }
        if self.nb_frames >= MAX_FRAMES {
            if !self.frames_capped {
                log::warning(&format!("frame table capped at {MAX_FRAMES} entries\n"));
            }
            self.frames_capped = true;
            return Ok(());
        }

        let mut entry = FrameEntry {
            pos_x: rl24(p, 0) as i32 * 2,
            pos_y: rl24(p, 3) as i32 * 2,
            width: rl24(p, 6) as i32 + 1,
            height: rl24(p, 9) as i32 + 1,
            duration: rl24(p, 12) as i32,
            dispose: if p[15] & ANMF_FLAG_DISPOSE != 0 {
                Dispose::Background
            } else {
                Dispose::None
            },
            blend: if p[15] & ANMF_FLAG_NO_BLEND != 0 {
                Blend::None
            } else {
                Blend::Alpha
            },
            has_alpha: false,
            complete,
        };

        entry.has_alpha = self.anmf_alpha(&p[16..]);

        if self.frames.len() == self.nb_frames {
            self.frames.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.frames.push(entry);
        } else {
            self.frames[self.nb_frames] = entry;
        }
        if complete {
            self.nb_frames += 1;
        } else {
            self.partial_frame = true;
        }
        Ok(())
    }

    fn raw_headers(&mut self, data: &[u8], partial: bool) -> Result<()> {
        let size = data.len();

        self.info.truncated = false;
        if size == 0 {
            return Err(Error::Truncated);
        }
        if data[0] == 0x2f {
            self.info.raw = Raw::Lossless;
            self.info.raw_image_offset = 0;
            self.info.raw_image_size = size;
            if size < 5 {
                return Err(Error::Truncated);
            }
            self.still_header(TAG_VP8L, data, size);
        } else if size >= 6 && data[3] == 0x9d && data[4] == 0x01 && data[5] == 0x2a {
            /* A bare stream declares no payload length, so until the caller
            says the stream has ended the keyframe header's own first partition
            is the only length to measure it against. */
            self.info.raw = Raw::Lossy;
            self.info.raw_image_offset = 0;
            self.info.raw_image_size = size;
            if size < 10 {
                return Err(Error::Truncated);
            }
            let mut payload = 10 + (rl24(data, 0) >> 5) as usize;

            if !partial || payload < size {
                payload = size;
            }
            self.still_header(TAG_VP8, data, payload);
            if self.info.width != 0 && payload > size {
                self.info.truncated = true;
            }
        } else if size >= 4 && rl32(data, 0) == TAG_ALPH {
            self.info.raw = Raw::AlphaAndLossy;
            if size < 8 {
                return Err(Error::Truncated);
            }
            let alpha_size = rl32(data, 4);

            if alpha_size == u32::MAX {
                return Err(Error::InvalidData);
            }
            let padded = u64::from(alpha_size) + u64::from(alpha_size & 1);

            if padded > (size - 8) as u64 || (size - 8) as u64 - padded < 8 {
                return Err(Error::Truncated);
            }
            let image_header = 8 + padded as usize;

            if rl32(data, image_header) != TAG_VP8 {
                return Err(Error::InvalidData);
            }
            let image_size = rl32(data, image_header + 4) as usize;
            let mut have = image_size;

            if image_size > size - image_header - 8 {
                self.info.truncated = true;
                if !partial {
                    return Err(Error::Truncated);
                }
                have = size - image_header - 8;
            }
            self.info.raw_alpha_offset = 8;
            self.info.raw_alpha_size = alpha_size as usize;
            self.info.raw_image_offset = image_header + 8;
            self.info.raw_image_size = have;
            self.info.has_alpha = true;
            self.info.image_has_alpha = true;
            if have < 10 {
                return Err(Error::Truncated);
            }
            let image = window(data, self.info.raw_image_offset, have);

            self.still_header(TAG_VP8, image, image_size);
        } else {
            return Err(if size < 12 && partial {
                Error::Truncated
            } else {
                Error::NotWebp
            });
        }
        self.info.frame_count = 1;
        self.info.images = 1;
        self.info.end = size;
        if self.info.width != 0 && self.info.height != 0 {
            Ok(())
        } else {
            Err(Error::InvalidData)
        }
    }

    /// Walks the chunk list without decoding anything, so it is safe to run on
    /// the caller's memory before the file is copied. `base` is the stream
    /// offset `buf` starts at, once earlier bytes have been dropped;
    /// `partial` says more input may still be coming, and `collect_frames`
    /// asks for the ANMF table, which is the only thing here that allocates.
    pub fn headers(
        &mut self,
        buf: &[u8],
        base: usize,
        partial: bool,
        collect_frames: bool,
    ) -> Result<()> {
        let size = base + buf.len();
        let mut partial_still = false;

        self.collect_frames = collect_frames;
        self.info.truncated = false;
        self.partial_frame = false;

        if self.pos == 0 {
            if (4..12).contains(&size) && rl32(buf, 0) == TAG_RIFF {
                return Err(Error::Truncated);
            }
            if size < 12 || rl32(buf, 0) != TAG_RIFF || rl32(buf, 8) != TAG_WEBP {
                return self.raw_headers(buf, partial);
            }
            self.riff_end = u64::from(rl32(buf, 4)) + 8;
            self.pos = 12;
        }

        self.info.end = size;
        if self.riff_end < size as u64 {
            self.info.end = self.riff_end as usize;
        } else if self.riff_end > size as u64 {
            self.info.truncated = true;
        }

        while self.pos + 8 <= self.info.end {
            let at = self.pos - base;
            let tag = rl32(buf, at);
            let size = rl32(buf, at + 4);

            if size == u32::MAX {
                self.info.truncated = true;
                break;
            }
            let padded = size as usize + (size & 1) as usize;

            if self.info.end - (self.pos + 8) < padded {
                let avail = self.info.end - (self.pos + 8);

                self.info.truncated = true;
                if self.collect_frames && tag == TAG_ANMF {
                    self.anmf(window(buf, at + 8, avail), false)?;
                }
                if partial
                    && self.info.images == 0
                    && (tag == TAG_VP8 || tag == TAG_VP8L)
                {
                    let (width, height) = (self.info.width, self.info.height);

                    partial_still = true;
                    self.still_header(tag, window(buf, at + 8, avail), size as usize);
                    if self.vp8x && width != 0 && height != 0 {
                        self.info.width = width;
                        self.info.height = height;
                    }
                }
                break;
            }

            match tag {
                TAG_VP8X => {
                    self.vp8x = true;
                    if size >= 10 {
                        let flags = byte(buf, at + 8);

                        self.info.has_alpha |= flags & VP8X_FLAG_ALPHA != 0;
                        for (i, &bit) in META_VP8X_FLAG.iter().enumerate() {
                            if flags & bit != 0 {
                                self.info.metadata |= 1 << i;
                            }
                        }
                        self.info.width = rl24(buf, at + 12) as i32 + 1;
                        self.info.height = rl24(buf, at + 15) as i32 + 1;
                        if u64::from(self.info.width as u32)
                            * u64::from(self.info.height as u32)
                            >= 1 << 32
                        {
                            return Err(Error::TooLarge);
                        }
                    }
                }
                TAG_ALPH => {
                    self.info.has_alpha = true;
                    self.info.image_has_alpha = true;
                }
                TAG_ANIM => {
                    self.info.animation = true;
                    if size >= 6 {
                        self.info.background_argb = rl32(buf, at + 8);
                        self.info.loop_count = rl16(buf, at + 12) as i32;
                    }
                }
                TAG_ANMF => {
                    self.info.frame_count = self.info.frame_count.saturating_add(1);
                    if self.collect_frames {
                        self.anmf(window(buf, at + 8, size as usize), true)?;
                    }
                }
                TAG_VP8 | TAG_VP8L => {
                    let first = self.info.images == 0;

                    self.info.images = self.info.images.saturating_add(1);
                    if first {
                        let (width, height) = (self.info.width, self.info.height);

                        self.still_header(
                            tag,
                            window(buf, at + 8, size as usize),
                            size as usize,
                        );
                        if self.vp8x && width != 0 && height != 0 {
                            self.info.width = width;
                            self.info.height = height;
                        }
                    }
                }
                _ => {
                    for (i, &meta) in META_TAG.iter().enumerate() {
                        if tag != meta {
                            continue;
                        }
                        self.info.metadata |= 1 << i;
                        if self.info.meta_offset[i] == 0 && size != 0 {
                            self.info.meta_offset[i] = self.pos + 8;
                            self.info.meta_size[i] = size;
                        }
                    }
                }
            }
            self.pos += 8 + padded;
        }

        /* An animation may mix lossy and lossless frames, which libwebp
        reports as an undefined coding; only the first still's coding is
        meaningful. */
        if self.info.animation {
            self.info.coding = Coding::Unknown;
        } else {
            self.info.frame_count = i32::from(self.info.images != 0 || partial_still);
        }

        if self.info.width == 0 || self.info.height == 0 {
            return Err(if self.info.truncated {
                Error::Truncated
            } else {
                Error::InvalidData
            });
        }
        Ok(())
    }
}

/// Reads what a file declares about itself without decoding it, and without
/// allocating: the frame table is what costs memory, and this never asks for
/// one.
pub fn get_info(data: &[u8]) -> Result<Info> {
    let mut scan = Scan::new();

    scan.headers(data, 0, true, false)?;
    Ok(scan.info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff(payload: &[u8]) -> Vec<u8> {
        let mut file = Vec::new();

        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(payload.len() as u32 + 4).to_le_bytes());
        file.extend_from_slice(b"WEBP");
        file.extend_from_slice(payload);
        file
    }

    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();

        out.extend_from_slice(tag);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() & 1 != 0 {
            out.push(0);
        }
        out
    }

    /// A VP8L frame header for `w` by `h`, with no pixel data behind it.
    fn vp8l_header(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let bits = (w - 1) | (h - 1) << 14 | u32::from(alpha) << 28;
        let mut body = vec![0x2f];

        body.extend_from_slice(&bits.to_le_bytes());
        body
    }

    #[test]
    fn a_lossless_still_reports_its_own_size() {
        let file = riff(&chunk(b"VP8L", &vp8l_header(17, 5, true)));
        let info = get_info(&file).unwrap();

        assert_eq!((info.width, info.height), (17, 5));
        assert!(info.has_alpha);
        assert_eq!(info.coding, Coding::Lossless);
        assert_eq!(info.frame_count, 1);
        assert!(!info.animation);
    }

    #[test]
    fn something_that_is_not_a_webp_says_so() {
        assert_eq!(get_info(b"not a webp file at all"), Err(Error::NotWebp));
    }

    #[test]
    fn a_riff_header_alone_is_merely_truncated() {
        assert_eq!(get_info(b"RIFF\0\0\0\0"), Err(Error::Truncated));
    }

    #[test]
    fn the_canvas_the_vp8x_declares_outranks_the_frame_header() {
        let mut payload = chunk(b"VP8X", &[0x10, 0, 0, 0, 99, 0, 0, 49, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(17, 5, false)));
        let info = get_info(&riff(&payload)).unwrap();

        assert_eq!((info.width, info.height), (100, 50));
        /* The VP8X alpha flag is the one WPDImageInfo reports, but a frame
        still says only what its own header carried. */
        assert!(info.has_alpha);
        assert!(!info.image_has_alpha);
    }

    #[test]
    fn metadata_is_found_by_tag_as_well_as_by_flag() {
        let mut payload = chunk(b"VP8X", &[0x28, 0, 0, 0, 16, 0, 0, 16, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(17, 17, false)));
        payload.extend_from_slice(&chunk(b"EXIF", b"exif"));
        let info = get_info(&riff(&payload)).unwrap();

        assert_eq!(info.metadata, 0b111 & !0b100);
        assert_eq!(info.meta_size[1], 4);
        assert_ne!(info.meta_offset[1], 0);
        assert_eq!(info.meta_offset[0], 0);
    }

    #[test]
    fn a_stream_delivered_in_pieces_scans_once() {
        let mut payload = chunk(b"VP8X", &[0x02, 0, 0, 0, 16, 0, 0, 16, 0, 0]);

        payload.extend_from_slice(&chunk(b"ANIM", &[0, 0, 0, 0xff, 3, 0]));
        for _ in 0..4 {
            let mut anmf = vec![0u8; 16];

            anmf[6] = 15;
            anmf[9] = 15;
            anmf[12] = 40;
            anmf.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
            payload.extend_from_slice(&chunk(b"ANMF", &anmf));
        }
        let file = riff(&payload);
        let mut scan = Scan::new();

        for split in 1..file.len() {
            let _ = scan.headers(&file[..split], 0, true, true);
        }
        scan.headers(&file, 0, false, true).unwrap();

        assert!(scan.info().animation);
        assert_eq!(scan.info().frame_count, 4);
        assert_eq!(scan.info().loop_count, 3);
        assert_eq!(scan.info().background_argb, 0xff00_0000);
        for i in 0..4 {
            let frame = scan.frame(i).unwrap();

            assert_eq!((frame.width, frame.height), (16, 16));
            assert_eq!(frame.duration, 40);
            assert!(frame.complete);
        }
        assert!(scan.frame(4).is_none());
    }

    #[test]
    fn a_chunk_declaring_more_than_arrived_reads_only_what_is_there() {
        let mut payload = chunk(b"VP8L", &vp8l_header(17, 5, false));

        payload[4] = 0xff;
        payload[5] = 0xff;
        let file = riff(&payload);

        /* A stream may still be carrying the rest of it, so the header that
        did arrive is read; a file that has ended cannot be completed. */
        let mut scan = Scan::new();

        scan.headers(&file, 0, true, true).unwrap();
        assert_eq!((scan.info().width, scan.info().height), (17, 5));
        assert!(scan.info().truncated);

        let mut scan = Scan::new();

        assert_eq!(scan.headers(&file, 0, false, true), Err(Error::Truncated));
    }

    #[test]
    fn every_prefix_of_every_test_file_is_scanned_without_panicking() {
        let mut payload = chunk(b"VP8X", &[0x12, 0, 0, 0, 16, 0, 0, 16, 0, 0]);

        payload.extend_from_slice(&chunk(b"ANIM", &[0, 0, 0, 0xff, 3, 0]));
        payload.extend_from_slice(&chunk(b"ALPH", &[0, 1, 2]));
        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, true)));
        payload.extend_from_slice(&chunk(b"XMP ", b"xmp"));
        let mut file = riff(&payload);

        for split in 0..file.len() {
            let _ = get_info(&file[..split]);
        }
        /* And with every single byte corrupted, since a size field that says
        far more than is there is the shape that reaches furthest. */
        for i in 0..file.len() {
            let saved = file[i];

            for byte in [0x00u8, 0x01, 0x7f, 0x80, 0xff] {
                file[i] = byte;
                let _ = get_info(&file);
            }
            file[i] = saved;
        }
    }
}
