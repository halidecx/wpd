use crate::bits;
use crate::error::{Error, Result};
use crate::log;

pub const METADATA_NB: usize = 3;

pub const MAX_FRAMES: usize = 1 << 20;

pub const ANMF_FLAG_DISPOSE: u8 = 1 << 0;
pub const ANMF_FLAG_NO_BLEND: u8 = 1 << 1;

const VP8X_FLAG_ANIM: u8 = 0x02;
const VP8X_FLAG_XMP: u8 = 0x04;
const VP8X_FLAG_EXIF: u8 = 0x08;
const VP8X_FLAG_ICCP: u8 = 0x20;
const VP8X_FLAG_ALPHA: u8 = 0x10;

const VP8X_FLAGS_VALID: u8 =
    VP8X_FLAG_ANIM | VP8X_FLAG_XMP | VP8X_FLAG_EXIF | VP8X_FLAG_ALPHA | VP8X_FLAG_ICCP;

const VP8X_CHUNK_SIZE: u32 = 10;

const ANIM_CHUNK_SIZE: u32 = 6;

const TAG_RIFF: u32 = u32::from_le_bytes(*b"RIFF");
const TAG_WEBP: u32 = u32::from_le_bytes(*b"WEBP");
pub(crate) const TAG_VP8: u32 = u32::from_le_bytes(*b"VP8 ");
pub(crate) const TAG_VP8L: u32 = u32::from_le_bytes(*b"VP8L");
const TAG_VP8X: u32 = u32::from_le_bytes(*b"VP8X");
pub(crate) const TAG_ALPH: u32 = u32::from_le_bytes(*b"ALPH");
const TAG_ANIM: u32 = u32::from_le_bytes(*b"ANIM");
pub(crate) const TAG_ANMF: u32 = u32::from_le_bytes(*b"ANMF");

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

impl Coding {
    pub fn name(self) -> &'static str {
        match self {
            Coding::Unknown => "unknown",
            Coding::Lossy => "lossy",
            Coding::Lossless => "lossless",
        }
    }
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Raw {
    #[default]
    No,
    Lossless,
    Lossy,
    AlphaAndLossy,
}

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Info {
    pub end: usize,
    pub width: i32,
    pub height: i32,
    pub has_alpha: bool,
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
    vp8x_flags: u8,
    anim_chunk: bool,
    still_chunk: bool,
    collect_frames: bool,
    partial_frame: bool,
    frames_capped: bool,
    nb_frames: usize,
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
    bits::rl16(&bits::quad(b, at))
}

fn rl24(b: &[u8], at: usize) -> u32 {
    bits::rl24(&bits::quad(b, at))
}

fn rl32(b: &[u8], at: usize) -> u32 {
    bits::rl32(&bits::quad(b, at))
}

fn window(b: &[u8], from: usize, len: usize) -> &[u8] {
    let from = from.min(b.len());
    let to = from.saturating_add(len).min(b.len());

    &b[from..to]
}

impl Scan {
    pub fn new() -> Self {
        Self::default()
    }

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

    fn anmf(&mut self, p: &[u8], complete: bool) -> Result<()> {
        if p.len() < 16 {
            return Ok(());
        }
        if self.nb_frames >= MAX_FRAMES {
            if !self.frames_capped {
                log::warning_args(format_args!(
                    "frame table capped at {MAX_FRAMES} entries\n"
                ));
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

    fn still_chunk_allowed(&mut self) -> Result<()> {
        if self.vp8x_flags & VP8X_FLAG_ANIM != 0 || self.anim_chunk {
            log::error("image chunk outside a frame of an animation");
            return Err(Error::InvalidData);
        }
        self.still_chunk = true;
        Ok(())
    }

    fn frame_bounds(&self, p: &[u8]) -> Result<()> {
        if p.len() < 16 {
            log::error("ANMF chunk is too short");
            return Err(Error::InvalidData);
        }
        if self.info.width == 0 || self.info.height == 0 {
            return Ok(());
        }

        let x = rl24(p, 0) as i32 * 2;
        let y = rl24(p, 3) as i32 * 2;
        let w = rl24(p, 6) as i32 + 1;
        let h = rl24(p, 9) as i32 + 1;
        let exact = self.vp8x_flags & VP8X_FLAG_ANIM == 0;
        let fits = if exact {
            x == 0 && y == 0 && w == self.info.width && h == self.info.height
        } else {
            x + w <= self.info.width && y + h <= self.info.height
        };

        if !fits {
            log::error_args(format_args!(
                "frame ({w}x{h} at {x}x{y}) does not fit the canvas ({}x{})",
                self.info.width, self.info.height
            ));
            return Err(Error::InvalidData);
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
                    self.still_chunk_allowed()?;

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
                    if self.vp8x || size != VP8X_CHUNK_SIZE {
                        log::error("invalid VP8X chunk");
                        return Err(Error::InvalidData);
                    }
                    self.vp8x = true;

                    let flags = byte(buf, at + 8);

                    if flags & !VP8X_FLAGS_VALID != 0 {
                        log::error_args(format_args!(
                            "VP8X sets reserved flag bits (0x{flags:02x})"
                        ));
                        return Err(Error::InvalidData);
                    }
                    self.vp8x_flags = flags;
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
                TAG_ALPH => {
                    self.still_chunk_allowed()?;
                    self.info.has_alpha = true;
                    self.info.image_has_alpha = true;
                }
                TAG_ANIM => {
                    if size < ANIM_CHUNK_SIZE {
                        log::error("ANIM chunk is too short");
                        return Err(Error::InvalidData);
                    }
                    if self.still_chunk {
                        log::error("ANIM chunk after a still image chunk");
                        return Err(Error::InvalidData);
                    }
                    /* Match libwebp: keep the first ANIM and skip later ones. */
                    if !self.anim_chunk {
                        self.anim_chunk = true;
                        self.info.animation = true;
                        self.info.background_argb = rl32(buf, at + 8);
                        self.info.loop_count = rl16(buf, at + 12) as i32;
                    }
                }
                TAG_ANMF => {
                    if !self.anim_chunk {
                        log::error("ANMF chunk before the ANIM header");
                        return Err(Error::InvalidData);
                    }
                    /* Match libwebp: a non-animation ANMF must cover the canvas. */
                    if self.vp8x_flags & VP8X_FLAG_ANIM == 0
                        && self.info.frame_count > 0
                    {
                        log::error("more than one frame without the animation flag");
                        return Err(Error::InvalidData);
                    }
                    self.frame_bounds(window(buf, at + 8, size as usize))?;
                    self.info.frame_count = self.info.frame_count.saturating_add(1);
                    if self.collect_frames {
                        self.anmf(window(buf, at + 8, size as usize), true)?;
                    }
                }
                TAG_VP8 | TAG_VP8L => {
                    self.still_chunk_allowed()?;

                    let first = self.info.images == 0;

                    self.info.images = self.info.images.saturating_add(1);
                    if first {
                        let (width, height) = (self.info.width, self.info.height);

                        self.info.width = 0;
                        self.info.height = 0;
                        self.still_header(
                            tag,
                            window(buf, at + 8, size as usize),
                            size as usize,
                        );
                        let (image_w, image_h) = (self.info.width, self.info.height);

                        if self.vp8x && width != 0 && height != 0 {
                            self.info.width = width;
                            self.info.height = height;
                            /* Match libwebp: conflicting still dimensions are invalid. */
                            if image_w != 0
                                && image_h != 0
                                && (image_w != width || image_h != height)
                            {
                                log::error_args(format_args!(
                                    "VP8X canvas {width}x{height} does not match \
                                     the image's {image_w}x{image_h}"
                                ));
                                return Err(Error::InvalidData);
                            }
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

        /* Match libwebp: mixed animation coding is undefined. */
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
        if !partial
            && !self.info.truncated
            && self.vp8x_flags & VP8X_FLAG_ANIM != 0
            && !self.anim_chunk
        {
            log::error("VP8X declares an animation with no ANIM chunk");
            return Err(Error::InvalidData);
        }
        Ok(())
    }
}

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
    fn the_vp8x_alpha_flag_is_reported_without_the_frame_carrying_it() {
        let mut payload = chunk(b"VP8X", &[0x10, 0, 0, 0, 99, 0, 0, 49, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(100, 50, false)));
        let info = get_info(&riff(&payload)).unwrap();

        assert_eq!((info.width, info.height), (100, 50));
        assert!(info.has_alpha);
        assert!(!info.image_has_alpha);
    }

    #[test]
    fn a_still_whose_two_declared_canvases_disagree_is_refused() {
        let mut payload = chunk(b"VP8X", &[0x00, 0, 0, 0, 99, 0, 0, 49, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(17, 5, false)));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
    }

    #[test]
    fn a_vp8x_that_is_not_ten_bytes_is_refused() {
        for body in [
            &[0u8, 0, 0, 0, 15, 0, 0, 15, 0][..],
            &[0, 0, 0, 0, 15, 0, 0, 15, 0, 0, 0][..],
        ] {
            let mut payload = chunk(b"VP8X", body);

            payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
            assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
        }
    }

    #[test]
    fn a_reserved_vp8x_flag_bit_is_refused() {
        for flags in [0x01u8, 0x40, 0x80] {
            let mut payload = chunk(b"VP8X", &[flags, 0, 0, 0, 15, 0, 0, 15, 0, 0]);

            payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
            assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
        }
    }

    #[test]
    fn the_animation_flag_and_the_anim_chunk_have_to_agree() {
        let mut payload = chunk(b"VP8X", &[0x02, 0, 0, 0, 15, 0, 0, 15, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));

        let payload = chunk(b"VP8X", &[0x02, 0, 0, 0, 15, 0, 0, 15, 0, 0]);
        let mut scan = Scan::new();

        assert_eq!(
            scan.headers(&riff(&payload), 0, false, false),
            Err(Error::InvalidData)
        );

        let mut payload = chunk(b"VP8X", &[0x02, 0, 0, 0, 15, 0, 0, 15, 0, 0]);
        let mut anmf = vec![0u8; 16];

        anmf[6] = 15;
        anmf[9] = 15;
        anmf.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
        payload.extend_from_slice(&chunk(b"ANMF", &anmf));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
    }

    #[test]
    fn an_anim_chunk_after_a_still_is_refused() {
        let mut payload = chunk(b"VP8X", &[0x00, 0, 0, 0, 15, 0, 0, 15, 0, 0]);

        payload.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
        payload.extend_from_slice(&chunk(b"ANIM", &[0, 0, 0, 0xff, 0, 0]));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));

        let mut anmf = vec![0u8; 16];

        anmf[6] = 15;
        anmf[9] = 15;
        anmf.extend_from_slice(&chunk(b"VP8L", &vp8l_header(16, 16, false)));
        payload.extend_from_slice(&chunk(b"ANMF", &anmf));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));

        let mut payload = chunk(b"VP8X", &[0x10, 0, 0, 0, 15, 0, 0, 15, 0, 0]);

        payload.extend_from_slice(&chunk(b"ALPH", &[0, 0, 0, 0]));
        payload.extend_from_slice(&chunk(b"ANIM", &[0, 0, 0, 0xff, 0, 0]));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
    }

    #[test]
    fn a_frame_hanging_off_the_canvas_is_refused_by_the_scan() {
        let mut payload = chunk(b"VP8X", &[0x02, 0, 0, 0, 15, 0, 0, 15, 0, 0]);

        payload.extend_from_slice(&chunk(b"ANIM", &[0, 0, 0, 0xff, 0, 0]));

        let mut anmf = vec![0u8; 16];

        anmf[0] = 4;
        anmf[6] = 8;
        anmf[9] = 15;
        anmf.extend_from_slice(&chunk(b"VP8L", &vp8l_header(9, 16, false)));
        payload.extend_from_slice(&chunk(b"ANMF", &anmf));
        assert_eq!(get_info(&riff(&payload)), Err(Error::InvalidData));
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
