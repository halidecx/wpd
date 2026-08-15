//! The lossless (VP8L) decoder.
//!
//! A port of `src/vp8l.c`: the frame header and its transform list, the prefix
//! codes each meta-block is coded with, the pixel loop, and the transforms that
//! turn the decoded residual back into ARGB.
//!
//! Three things are shaped differently from the C.
//!
//! A picture is a flat `Vec<u32>` rather than a byte plane addressed through a
//! `linesize`. The pixel loop already walked it linearly, the predictors
//! already took `uint32_t *`, and the transforms only ever look at whole
//! pixels, so `u32` is the unit the whole module wants — and it takes the
//! per-byte index arithmetic, and its bounds checks, out of the hot loop. A
//! pixel is stored in native byte order over the memory the C laid out as
//! `[A, R, G, B]`, which is what the assembly and the C ABI both still see.
//!
//! The bit reader holds an offset into the chunk rather than a pointer to it,
//! so `br_extend` has no counterpart: a streaming append that reallocates the
//! buffer cannot invalidate anything the reader tracks. See [`bitreader`].
//!
//! Caller-owned memory — the alpha plane an ALPH chunk can be written straight
//! into — is an argument to [`Decoder::decode_frame`] rather than a pointer
//! kept in the context. Nothing borrowed outlives the call that passed it in.

pub mod bitreader;
pub mod entropy;
pub mod huffman;
pub mod transform;

use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{check_image_size, Error, Result, Status};
use bitreader::BitReader;
use huffman::{Plan, Reader};

const HUFFMAN_CODES_PER_META_CODE: usize = 5;
const NUM_LITERAL_CODES: u32 = 256;
const NUM_LENGTH_CODES: u32 = 24;
const NUM_DISTANCE_CODES: u32 = 40;
const NUM_SHORT_DISTANCES: u32 = 120;

/// How many finished rows the resumable path hands out at a time.
const ROW_BATCH: i32 = 16;

/// The slack `WPD_FILE_PADDING` leaves past the last pixel, in pixels.
const PADDING: usize = 16;

/// What one image's prefix-code tables are assumed to fit in.
const ARENA_CHUNK: usize = 4096;

const HUFF_IDX_GREEN: usize = 0;
const HUFF_IDX_RED: usize = 1;
const HUFF_IDX_BLUE: usize = 2;
const HUFF_IDX_ALPHA: usize = 3;
const HUFF_IDX_DIST: usize = 4;

const ROLE_ARGB: usize = 0;
const ROLE_ENTROPY: usize = 1;
const ROLE_PREDICTOR: usize = 2;
const ROLE_COLOR: usize = 3;
const ROLE_PALETTE: usize = 4;
const ROLE_NB: usize = 5;

const ALPHABET_SIZES: [u32; HUFFMAN_CODES_PER_META_CODE] = [
    NUM_LITERAL_CODES + NUM_LENGTH_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_LITERAL_CODES,
    NUM_DISTANCE_CODES,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transform {
    Predictor,
    Color,
    SubtractGreen,
    ColorIndexing,
}

impl Transform {
    fn from_bits(v: u32) -> Self {
        match v {
            0 => Self::Predictor,
            1 => Self::Color,
            2 => Self::SubtractGreen,
            _ => Self::ColorIndexing,
        }
    }
}

/// Which of the module's output pictures a decode fills in.
///
/// Both are kept across calls and resized on use, so a lossy animation with
/// alpha alternates between them without reallocating either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Argb,
    Alpha,
}

/// The alpha plane an ALPH chunk can be written straight into.
///
/// This is caller memory, so it arrives per call rather than being kept in the
/// decoder — see the module documentation.
pub struct AlphaDst<'a> {
    pub data: &'a mut [u8],
    pub stride: usize,
}

/// A picture, one pixel per `u32`, rows contiguous at `stride`.
#[derive(Default)]
pub struct Picture {
    pub data: Vec<u32>,
    pub stride: usize,
    pub width: i32,
    pub height: i32,
}

impl Picture {
    /// Sizes the picture to `w` by `h` and zeroes it, keeping the allocation
    /// when it is already large enough, as `image_alloc_plane` does.
    fn alloc(&mut self, w: i32, h: i32) -> Result<()> {
        if w <= 0 || h <= 0 {
            return Err(Error::TooLarge);
        }
        let size = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_add(PADDING))
            .ok_or(Error::TooLarge)?;

        if self.data.len() < size {
            /* Growing already zeroes what it adds, and dropping what was
            there first means it zeroes the whole buffer exactly once. */
            self.data.clear();
            self.data
                .try_reserve_exact(size)
                .map_err(|_| Error::NoMemory)?;
            self.data.resize(size, 0);
        } else {
            self.data[..size].fill(0);
        }
        self.stride = w as usize;
        self.width = w;
        self.height = h;
        Ok(())
    }

    fn release(&mut self) {
        *self = Self::default();
    }
}

/// Where the pixel loop left off, so a chunk that ran out mid-image can be
/// picked up once more of it has arrived.
#[derive(Clone, Copy, Default)]
pub struct Resume {
    pub pos: usize,
    pub cached: usize,
    pub x: i32,
    pub y: i32,
    pub hg: usize,
    pub rows_done: i32,
}

/// One meta prefix code: the five trees a pixel is read with, and the shortcut
/// for a group whose three non-green channels never vary.
#[derive(Clone, Copy, Default)]
pub struct HTreeGroup {
    pub trees: [Reader; HUFFMAN_CODES_PER_META_CODE],
    pub trivial_literal: bool,
    pub literal: [u8; 4],
}

/// One entropy-coded image: the ARGB picture itself, or one of the sub-images
/// a transform is described by.
#[derive(Default)]
struct ImageContext {
    storage: Picture,
    color_cache: Vec<u32>,
    color_cache_bits: u32,
    groups: Vec<HTreeGroup>,
    arena: Vec<u32>,
    size_reduction: u32,
}

impl ImageContext {
    /// Drops what a decode derived without giving the buffers back: an
    /// animation runs this between frames, and every one of them would
    /// otherwise re-ask the allocator for the same sizes.
    fn clear(&mut self) {
        self.color_cache.clear();
        self.color_cache_bits = 0;
        self.groups.clear();
        self.arena.clear();
        self.size_reduction = 0;
        self.storage.width = 0;
        self.storage.height = 0;
        self.storage.stride = 0;
    }
}

/// Sizes a reused scratch buffer, falling back to an error rather than an
/// abort when the allocator cannot manage it.
fn grow<T: Copy>(buf: &mut Vec<T>, len: usize, fill: T) -> Result<()> {
    if buf.len() < len {
        buf.try_reserve(len - buf.len())
            .map_err(|_| Error::NoMemory)?;
        buf.resize(len, fill);
    }
    Ok(())
}

pub struct Decoder {
    dsp: Vp8lDsp,
    gb: BitReader,

    pub width: i32,
    pub height: i32,
    pub has_alpha: bool,

    reduced_width: i32,
    transforms: [Transform; 4],
    nb_transforms: usize,
    nb_huffman_groups: usize,
    image: [ImageContext; ROLE_NB],

    alpha_dst_used: bool,

    argb: Picture,
    alpha_argb: Picture,
    out: Picture,
    /// Which picture the resumable path is filling in, once it has one.
    staged: bool,
    /// The row above the batch, as the predictor transform left it, plus the
    /// batch's first row while it is being predicted against it.
    scratch: Vec<u32>,
    /// Reused across prefix codes: the symbols sorted by code length, and the
    /// length list they were sorted from.
    sorted: Vec<u16>,
    lengths: Vec<u8>,

    active: bool,
    next_try: usize,
    resume: Resume,
    rows_out: i32,
    peeked: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            dsp: Vp8lDsp::new(),
            gb: BitReader::default(),
            width: 0,
            height: 0,
            has_alpha: false,
            reduced_width: 0,
            transforms: [Transform::Predictor; 4],
            nb_transforms: 0,
            nb_huffman_groups: 0,
            image: Default::default(),
            alpha_dst_used: false,
            argb: Picture::default(),
            alpha_argb: Picture::default(),
            out: Picture::default(),
            staged: false,
            scratch: Vec::new(),
            sorted: Vec::new(),
            lengths: Vec::new(),
            active: false,
            next_try: 0,
            resume: Resume::default(),
            rows_out: 0,
            peeked: false,
        }
    }

    /// Drops everything derived from a file, keeping the decode buffers, which
    /// are sized on use and reused.
    pub fn reset(&mut self) {
        for img in &mut self.image {
            img.clear();
        }
        self.active = false;
        self.next_try = 0;
        self.peeked = false;
        self.staged = false;
        self.width = 0;
        self.height = 0;
        self.has_alpha = false;
    }

    /// [`Self::reset`], and the decode buffers too, for when the file is closed
    /// and nothing is looking at the pictures any more.
    pub fn release(&mut self) {
        self.reset();
        for img in &mut self.image {
            *img = ImageContext::default();
        }
        self.argb.release();
        self.alpha_argb.release();
        self.out.release();
        self.scratch = Vec::new();
        self.sorted = Vec::new();
        self.lengths = Vec::new();
    }

    /// The canvas the container has already committed to. A lossless frame
    /// header carries its own dimensions, an alpha chunk does not, and the two
    /// have to agree.
    pub fn set_canvas(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
    }

    pub fn alpha_dst_used(&self) -> bool {
        self.alpha_dst_used
    }

    pub fn still_active(&self) -> bool {
        self.active
    }

    pub fn still_rows_out(&self) -> i32 {
        self.rows_out
    }

    /// The picture a [`Self::decode_frame`] filled in.
    pub fn picture(&self, target: Target) -> &Picture {
        match target {
            Target::Argb => &self.argb,
            Target::Alpha => &self.alpha_argb,
        }
    }

    /// The picture the resumable path is filling in, once it has started one.
    pub fn still_picture(&self) -> Option<&Picture> {
        if !self.staged {
            return None;
        }
        Some(if self.peeked { &self.out } else { &self.argb })
    }

    fn picture_mut(&mut self, role: usize, target: Target) -> &mut Picture {
        if role != ROLE_ARGB {
            return &mut self.image[role].storage;
        }
        match target {
            Target::Argb => &mut self.argb,
            Target::Alpha => &mut self.alpha_argb,
        }
    }

    fn update_canvas_size(&mut self, w: i32, h: i32) {
        if self.width != 0 && self.width != w {
            crate::log::warning(&format!("Width mismatch. {} != {}", self.width, w));
        }
        self.width = w;
        if self.height != 0 && self.height != h {
            crate::log::warning(&format!("Height mismatch. {} != {}", self.height, h));
        }
        self.height = h;
    }

    /// The block size a transform sub-image is coded at, and how many blocks
    /// of it cover the picture.
    fn parse_block_size(&mut self, buf: &[u8]) -> (u32, i32, i32) {
        let bits = self.gb.bits(buf, 3) + 2;
        let w = ceil_shift(self.reduced_width, bits);
        let h = ceil_shift(self.height, bits);

        (bits, w, h)
    }

    fn decode_entropy_image(&mut self, buf: &[u8]) -> Result<()> {
        let (block_bits, blocks_w, blocks_h) = self.parse_block_size(buf);

        self.decode_entropy_coded_image(
            ROLE_ENTROPY,
            Target::Argb,
            buf,
            blocks_w,
            blocks_h,
        )?;

        let img = &mut self.image[ROLE_ENTROPY];

        img.size_reduction = block_bits;

        let mut max = 0u32;

        for y in 0..img.storage.height as usize {
            let row = &img.storage.data[y * img.storage.stride..]
                [..img.storage.width as usize];

            for px in row {
                let b = px.to_ne_bytes();

                max = max.max(u32::from(b[1]) << 8 | u32::from(b[2]));
            }
        }
        self.nb_huffman_groups = max as usize + 1;
        Ok(())
    }

    fn parse_transform_predictor(&mut self, buf: &[u8]) -> Result<()> {
        let (block_bits, blocks_w, blocks_h) = self.parse_block_size(buf);

        self.decode_entropy_coded_image(
            ROLE_PREDICTOR,
            Target::Argb,
            buf,
            blocks_w,
            blocks_h,
        )?;
        self.image[ROLE_PREDICTOR].size_reduction = block_bits;
        Ok(())
    }

    fn parse_transform_color(&mut self, buf: &[u8]) -> Result<()> {
        let (block_bits, blocks_w, blocks_h) = self.parse_block_size(buf);

        self.decode_entropy_coded_image(
            ROLE_COLOR,
            Target::Argb,
            buf,
            blocks_w,
            blocks_h,
        )?;
        self.image[ROLE_COLOR].size_reduction = block_bits;
        Ok(())
    }

    fn parse_transform_color_indexing(&mut self, buf: &[u8]) -> Result<()> {
        let index_size = self.gb.bits(buf, 8) as i32 + 1;
        let width_bits = match index_size {
            ..=2 => 3,
            3..=4 => 2,
            5..=16 => 1,
            _ => 0,
        };

        self.decode_entropy_coded_image(
            ROLE_PALETTE,
            Target::Argb,
            buf,
            index_size,
            1,
        )?;

        let img = &mut self.image[ROLE_PALETTE];

        img.size_reduction = width_bits;
        if width_bits > 0 {
            self.reduced_width = ceil_shift(self.width, width_bits);
        }

        /* The palette is stored as deltas along the row. */
        let row = &mut img.storage.data[..img.storage.width as usize];

        for i in 1..row.len() {
            row[i] = crate::dsp::vp8l::add_pixels(row[i], row[i - 1]);
        }
        Ok(())
    }

    fn decode_entropy_coded_image(
        &mut self,
        role: usize,
        target: Target,
        buf: &[u8],
        w: i32,
        h: i32,
    ) -> Result<()> {
        self.read_image_header(role, target, buf, w, h)?;
        self.decode_pixels(role, target, buf, false)?;
        Ok(())
    }

    /// The prefix codes one entropy-coded image is written with, and the
    /// sub-image saying which of them applies where.
    fn read_image_header(
        &mut self,
        role: usize,
        target: Target,
        buf: &[u8],
        w: i32,
        h: i32,
    ) -> Result<()> {
        self.picture_mut(role, target).alloc(w, h)?;

        let cache_bits = if self.gb.bit(buf) != 0 {
            let bits = self.gb.bits(buf, 4);

            if !(1..=11).contains(&bits) {
                crate::log::error(&format!("invalid color cache bits: {bits}"));
                return Err(Error::InvalidData);
            }
            bits
        } else {
            0
        };

        {
            let img = &mut self.image[role];

            img.color_cache_bits = cache_bits;
            img.color_cache.clear();
            if cache_bits > 0 {
                let n = 1usize << cache_bits;

                img.color_cache
                    .try_reserve(n)
                    .map_err(|_| Error::NoMemory)?;
                img.color_cache.resize(n, 0);
            }
        }

        let mut nb_groups = 1usize;

        if role == ROLE_ARGB && self.gb.bit(buf) != 0 {
            self.decode_entropy_image(buf)?;
            nb_groups = self.nb_huffman_groups;
        }

        let mut max_alphabet_size = ALPHABET_SIZES[HUFF_IDX_GREEN] as usize;

        if cache_bits > 0 {
            max_alphabet_size += 1 << cache_bits;
        }

        let Decoder {
            gb,
            image,
            sorted,
            lengths,
            ..
        } = self;

        grow(sorted, max_alphabet_size, 0u16)?;
        grow(lengths, max_alphabet_size, 0u8)?;

        let img = &mut image[role];

        img.groups.clear();
        img.groups
            .try_reserve(nb_groups)
            .map_err(|_| Error::NoMemory)?;
        img.groups.resize(nb_groups, HTreeGroup::default());
        img.arena.clear();
        /* The C grew the arena in chunks of this size; asking for one up front
        keeps the per-code `try_reserve` from reallocating and copying. */
        img.arena
            .try_reserve(ARENA_CHUNK)
            .map_err(|_| Error::NoMemory)?;

        #[allow(clippy::needless_range_loop)]
        for i in 0..nb_groups {
            for j in 0..HUFFMAN_CODES_PER_META_CODE {
                let extra = if j == HUFF_IDX_GREEN && cache_bits > 0 {
                    1usize << cache_bits
                } else {
                    0
                };
                let alphabet_size = ALPHABET_SIZES[j] as usize + extra;
                let lengths = &mut lengths[..alphabet_size];
                let mut plan = Plan::default();

                lengths.fill(0);
                if gb.bit(buf) != 0 {
                    huffman::read_simple_code(gb, buf, &mut plan, lengths);
                } else {
                    huffman::read_normal_code(gb, buf, &mut plan, lengths)?;
                }
                img.groups[i].trees[j] =
                    huffman::build(&mut img.arena, &mut plan, lengths, sorted)?;
            }

            let hg = &mut img.groups[i];

            hg.trivial_literal = hg.trees[HUFF_IDX_RED].mask == 0
                && hg.trees[HUFF_IDX_BLUE].mask == 0
                && hg.trees[HUFF_IDX_ALPHA].mask == 0;
            if hg.trivial_literal {
                for (slot, tree) in
                    [(0, HUFF_IDX_ALPHA), (1, HUFF_IDX_RED), (3, HUFF_IDX_BLUE)]
                {
                    hg.literal[slot] = hg.trees[tree].tree(&img.arena).only_symbol();
                }
            }
        }
        Ok(())
    }

    /// The pixel loop, over whichever image the role names.
    ///
    /// The picture and the prefix codes come from different places depending on
    /// the role — the ARGB picture is one the caller keeps, a sub-image's is
    /// owned by its own context — so the pieces are split out here and the loop
    /// itself takes them as arguments.
    fn decode_pixels(
        &mut self,
        role: usize,
        target: Target,
        buf: &[u8],
        resumable: bool,
    ) -> Result<Status> {
        let Decoder {
            gb,
            image,
            argb,
            alpha_argb,
            reduced_width,
            resume,
            ..
        } = self;

        if role != ROLE_ARGB {
            let ImageContext {
                storage,
                color_cache,
                color_cache_bits,
                groups,
                arena,
                ..
            } = &mut image[role];

            return entropy::decode_pixels(entropy::Args {
                gb,
                buf,
                pic: storage,
                groups,
                arena,
                cache: color_cache,
                cache_bits: *color_cache_bits,
                reduced_width: None,
                entropy: None,
                st: resume,
                resumable,
            });
        }

        let (head, tail) = image.split_at_mut(ROLE_ENTROPY);
        let ImageContext {
            color_cache,
            color_cache_bits,
            groups,
            arena,
            ..
        } = &mut head[ROLE_ARGB];
        let ent = &tail[0];
        let pic = match target {
            Target::Argb => argb,
            Target::Alpha => alpha_argb,
        };

        entropy::decode_pixels(entropy::Args {
            gb,
            buf,
            pic,
            groups,
            arena,
            cache: color_cache,
            cache_bits: *color_cache_bits,
            reduced_width: Some(*reduced_width),
            entropy: (ent.size_reduction > 0).then(|| entropy::Entropy {
                data: &ent.storage.data,
                stride: ent.storage.stride,
                bits: ent.size_reduction,
            }),
            st: resume,
            resumable,
        })
    }

    fn read_frame_header(
        &mut self,
        target: Target,
        buf: &[u8],
        is_alpha_chunk: bool,
    ) -> Result<()> {
        self.gb = BitReader::new(buf);

        let (w, h) = if is_alpha_chunk {
            if self.width == 0 || self.height == 0 {
                return Err(Error::InvalidData);
            }
            (self.width, self.height)
        } else {
            if self.gb.bits(buf, 8) != 0x2F {
                crate::log::error("Invalid WebP Lossless signature");
                return Err(Error::InvalidData);
            }
            let w = self.gb.bits(buf, 14) as i32 + 1;
            let h = self.gb.bits(buf, 14) as i32 + 1;

            self.update_canvas_size(w, h);
            check_image_size(self.width, self.height)?;

            self.has_alpha = self.gb.bit(buf) != 0;
            if self.gb.bits(buf, 3) != 0 {
                crate::log::error("Invalid WebP Lossless version");
                return Err(Error::InvalidData);
            }
            (w, h)
        };

        self.nb_transforms = 0;
        self.reduced_width = self.width;

        let mut used = 0u32;

        while self.gb.bit(buf) != 0 {
            let coded = self.gb.bits(buf, 2);
            let transform = Transform::from_bits(coded);
            let bit = 1u32 << coded;

            if used & bit != 0 {
                crate::log::error(&format!(
                    "Transform {transform:?} used more than once"
                ));
                return Err(Error::InvalidData);
            }
            used |= bit;
            if self.nb_transforms == self.transforms.len() {
                return Err(Error::InvalidData);
            }
            self.transforms[self.nb_transforms] = transform;
            self.nb_transforms += 1;

            match transform {
                Transform::Predictor => self.parse_transform_predictor(buf)?,
                Transform::Color => self.parse_transform_color(buf)?,
                Transform::ColorIndexing => self.parse_transform_color_indexing(buf)?,
                Transform::SubtractGreen => {}
            }
        }

        self.read_image_header(ROLE_ARGB, target, buf, w, h)
    }

    /// Decodes a whole frame in one call.
    pub fn decode_frame(
        &mut self,
        target: Target,
        buf: &[u8],
        is_alpha_chunk: bool,
        alpha_dst: Option<AlphaDst<'_>>,
    ) -> Result<()> {
        self.alpha_dst_used = false;

        let mut ret = self
            .read_frame_header(target, buf, is_alpha_chunk)
            .and_then(|()| self.decode_pixels(ROLE_ARGB, target, buf, false))
            .map(|_| ());

        if ret.is_ok() {
            ret = self.apply_transforms(target, alpha_dst);
        }

        let pic = self.picture_mut(ROLE_ARGB, target);

        pic.stride = pic.width.max(0) as usize;
        for img in &mut self.image {
            img.clear();
        }
        ret
    }

    fn apply_transforms(
        &mut self,
        target: Target,
        mut alpha_dst: Option<AlphaDst<'_>>,
    ) -> Result<()> {
        for i in (0..self.nb_transforms).rev() {
            match self.transforms[i] {
                Transform::Predictor => self.apply_predictor(target)?,
                Transform::Color => self.apply_color(target),
                Transform::SubtractGreen => self.apply_subtract_green(target),
                Transform::ColorIndexing => {
                    match alpha_dst.take() {
                        Some(dst) if self.nb_transforms == 1 => {
                            self.apply_color_indexing_alpha(target, dst)
                        }
                        dst => {
                            alpha_dst = dst;
                            self.apply_color_indexing(target);
                        }
                    };
                }
            }
        }
        Ok(())
    }

    fn apply_predictor(&mut self, target: Target) -> Result<()> {
        let Decoder {
            dsp,
            image,
            argb,
            alpha_argb,
            reduced_width,
            ..
        } = self;
        let pic = match target {
            Target::Argb => argb,
            Target::Alpha => alpha_argb,
        };
        let modes = &image[ROLE_PREDICTOR];

        transform::predictor_rows(
            dsp,
            &mut pic.data,
            0,
            pic.stride,
            *reduced_width as usize,
            &modes.storage.data,
            modes.storage.stride,
            modes.size_reduction,
            0,
            pic.height,
            None,
        )
    }

    fn apply_color(&mut self, target: Target) {
        let Decoder {
            dsp,
            image,
            argb,
            alpha_argb,
            reduced_width,
            ..
        } = self;
        let pic = match target {
            Target::Argb => argb,
            Target::Alpha => alpha_argb,
        };
        let mult = &image[ROLE_COLOR];

        transform::color_rows(
            dsp,
            &mut pic.data,
            0,
            pic.stride,
            *reduced_width as usize,
            &mult.storage.data,
            mult.storage.stride,
            mult.size_reduction,
            0,
            pic.height,
        );
    }

    fn apply_subtract_green(&mut self, target: Target) {
        let width = self.reduced_width as usize;
        let pic = self.picture_mut(ROLE_ARGB, target);

        transform::subtract_green_rows(&mut pic.data, 0, pic.stride, width, pic.height);
    }

    fn apply_color_indexing(&mut self, target: Target) {
        let Decoder {
            dsp,
            image,
            argb,
            alpha_argb,
            reduced_width,
            ..
        } = self;
        let pic = match target {
            Target::Argb => argb,
            Target::Alpha => alpha_argb,
        };
        let pal = &image[ROLE_PALETTE];
        let width = pic.width as usize;
        let height = pic.height;
        let src_stride = pic.stride;

        transform::color_indexing_rows(
            dsp,
            &mut pic.data,
            0,
            width,
            src_stride,
            width,
            height,
            &pal.storage.data[..pal.storage.width as usize],
            pal.size_reduction,
            height as usize * width > 300,
        );
        if pal.size_reduction > 0 {
            pic.stride = width;
            *reduced_width = pic.width;
        }
    }

    fn apply_color_indexing_alpha(&mut self, target: Target, dst: AlphaDst<'_>) {
        let Decoder {
            image,
            argb,
            alpha_argb,
            reduced_width,
            alpha_dst_used,
            ..
        } = self;
        let pic = match target {
            Target::Argb => argb,
            Target::Alpha => alpha_argb,
        };
        let pal = &image[ROLE_PALETTE];

        transform::color_indexing_alpha(
            &pic.data,
            pic.stride,
            pic.width as usize,
            pic.height,
            &pal.storage.data[..pal.storage.width as usize],
            pal.size_reduction,
            dst,
        );
        pic.stride = pic.width as usize;
        *alpha_dst_used = true;
        *reduced_width = pic.width;
    }
}

/// `(v + (1 << s) - 1) >> s`, which is how many blocks of `1 << s` cover `v`.
fn ceil_shift(v: i32, s: u32) -> i32 {
    (v + (1 << s) - 1) >> s
}

/// The resumable still-image path.
impl Decoder {
    fn still_alloc(&mut self) -> Result<()> {
        self.out.alloc(self.width, self.height)?;

        let scratch = 2 * self.width as usize + 1;

        if self.scratch.len() < scratch {
            let more = scratch - self.scratch.len();

            self.scratch
                .try_reserve(more)
                .map_err(|_| Error::NoMemory)?;
            self.scratch.resize(scratch, 0);
        }
        Ok(())
    }

    /// Decodes as much of a still lossless image as the buffered bytes allow.
    /// [`Status::Done`] means the whole image is out.
    pub fn still_step(
        &mut self,
        payload: &[u8],
        size: usize,
        complete: bool,
    ) -> Result<Status> {
        let avail = payload.len();

        if !self.active {
            let first = (size / 16).max(16);

            if avail < first || (!complete && avail < self.next_try) {
                return Ok(Status::NeedMore);
            }
            for img in &mut self.image {
                img.clear();
            }
            self.width = 0;
            self.height = 0;

            let mut ret = self.read_frame_header(Target::Argb, payload, false);

            if ret.is_ok() && self.gb.is_eos(payload) {
                ret = Err(Error::InvalidData);
            }
            if let Err(e) = ret {
                for img in &mut self.image {
                    img.clear();
                }
                if complete {
                    return Err(e);
                }
                self.next_try = 2 * avail;
                return Ok(Status::NeedMore);
            }
            self.resume = Resume::default();
            self.rows_out = 0;
            self.peeked = false;
            self.active = true;
            self.staged = true;
        }

        let status = self.decode_pixels(ROLE_ARGB, Target::Argb, payload, true)?;

        if status == Status::NeedMore && complete {
            return Err(Error::InvalidData);
        }

        let mut rows = self.resume.rows_done;

        if status == Status::NeedMore {
            rows -= rows % ROW_BATCH;
        }
        if self.peeked && rows > self.rows_out {
            self.transform_rows(self.rows_out, rows)?;
            self.rows_out = rows;
        }
        if status == Status::NeedMore {
            return Ok(Status::NeedMore);
        }

        /* Nobody looked, so the image can be transformed where it lies. */
        let ret = if self.peeked {
            Ok(())
        } else {
            self.apply_transforms(Target::Argb, None)
        };

        self.argb.stride = self.argb.width.max(0) as usize;
        for img in &mut self.image {
            img.clear();
        }
        self.active = false;
        ret.map(|()| Status::Done)
    }

    /// Switches the in-progress image over to handing rows out as they finish,
    /// which needs somewhere to put them: backward references keep reading the
    /// untransformed pixels for as long as the image is being decoded.
    pub fn still_peek(&mut self) -> Result<()> {
        if !self.peeked {
            self.still_alloc()?;
            self.peeked = true;
        }

        let rows = self.resume.rows_done - self.resume.rows_done % ROW_BATCH;

        if rows > self.rows_out {
            self.transform_rows(self.rows_out, rows)?;
            self.rows_out = rows;
        }
        Ok(())
    }

    /// Copies the rows the pixel loop has finished into the staging picture and
    /// transforms them there.
    ///
    /// The predictor needs the row above the batch as *it* left it, not as the
    /// transforms that follow it left it, so that one row is kept in
    /// [`Self::scratch`] and the batch's first row is predicted alongside it
    /// before the rest run in place.
    fn transform_rows(&mut self, y0: i32, y1: i32) -> Result<()> {
        let Decoder {
            dsp,
            image,
            argb,
            out,
            scratch,
            reduced_width,
            transforms,
            nb_transforms,
            width,
            ..
        } = self;
        let packed = *reduced_width;
        let packed_row = packed.max(0) as usize;
        let stride = out.stride;
        let src_stride = argb.stride;
        let base = y0 as usize * stride;

        for i in 0..(y1 - y0) as usize {
            let src = (y0 as usize + i) * src_stride;

            out.data[base + i * stride..][..packed_row]
                .copy_from_slice(&argb.data[src..][..packed_row]);
        }

        let mut ret = Ok(());

        for i in (0..*nb_transforms).rev() {
            if ret.is_err() {
                break;
            }
            match transforms[i] {
                Transform::Predictor => {
                    let modes = &image[ROLE_PREDICTOR];
                    let w = *reduced_width as usize;

                    ret = predict_batch(
                        dsp,
                        &mut out.data,
                        scratch,
                        base,
                        stride,
                        w,
                        *width as usize,
                        modes,
                        y0,
                        y1,
                    );
                    if ret.is_ok() {
                        let last = base + (y1 - 1 - y0) as usize * stride;

                        scratch[..w].copy_from_slice(&out.data[last..][..w]);
                    }
                }
                Transform::Color => {
                    let mult = &image[ROLE_COLOR];

                    transform::color_rows(
                        dsp,
                        &mut out.data,
                        base,
                        stride,
                        *reduced_width as usize,
                        &mult.storage.data,
                        mult.storage.stride,
                        mult.size_reduction,
                        y0,
                        y1,
                    );
                }
                Transform::SubtractGreen => {
                    transform::subtract_green_rows(
                        &mut out.data,
                        base,
                        stride,
                        *reduced_width as usize,
                        y1 - y0,
                    );
                }
                Transform::ColorIndexing => {
                    let pal = &image[ROLE_PALETTE];

                    transform::color_indexing_rows(
                        dsp,
                        &mut out.data,
                        base,
                        stride,
                        stride,
                        out.width as usize,
                        y1 - y0,
                        &pal.storage.data[..pal.storage.width as usize],
                        pal.size_reduction,
                        out.height as usize * out.width as usize > 300,
                    );
                    *reduced_width = *width;
                }
            }
        }
        *reduced_width = packed;
        ret
    }
}

/// One batch of predictor rows.
///
/// `scratch` holds the row above the batch in its first `width` entries; the
/// batch's first row is copied in behind it so the two are adjacent, predicted
/// there, and copied back. Everything after that row has the row it needs
/// immediately above it in `plane` already.
#[allow(clippy::too_many_arguments)]
fn predict_batch(
    dsp: &Vp8lDsp,
    plane: &mut [u32],
    scratch: &mut [u32],
    base: usize,
    stride: usize,
    width: usize,
    full_width: usize,
    modes: &ImageContext,
    y0: i32,
    y1: i32,
) -> Result<()> {
    if width == 0 || y1 <= y0 {
        return Ok(());
    }
    if y0 == 0 {
        return transform::predictor_rows(
            dsp,
            plane,
            base,
            stride,
            width,
            &modes.storage.data,
            modes.storage.stride,
            modes.size_reduction,
            y0,
            y1,
            None,
        );
    }

    scratch[full_width..][..width].copy_from_slice(&plane[base..][..width]);
    transform::predictor_rows(
        dsp,
        scratch,
        full_width,
        stride,
        width,
        &modes.storage.data,
        modes.storage.stride,
        modes.size_reduction,
        y0,
        y0 + 1,
        Some(0),
    )?;
    plane[base..][..width].copy_from_slice(&scratch[full_width..][..width]);

    transform::predictor_rows(
        dsp,
        plane,
        base + stride,
        stride,
        width,
        &modes.storage.data,
        modes.storage.stride,
        modes.size_reduction,
        y0 + 1,
        y1,
        Some(base),
    )
}
