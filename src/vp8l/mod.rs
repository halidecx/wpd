pub mod bitreader;
pub mod entropy;
pub mod huffman;
pub mod transform;

use zerocopy::IntoBytes;

use crate::dsp::filters::UnfilterFn;
use crate::dsp::vp8l::Vp8lDsp;
use crate::error::{check_image_size, Error, Result, Status};
use crate::image::Format;
use crate::picture::{Frame, FrameMut, PlaneMut};
use bitreader::BitReader;
use huffman::{Plan, Reader};

const HUFFMAN_CODES_PER_META_CODE: usize = 5;
const NUM_LITERAL_CODES: u32 = 256;
const NUM_LENGTH_CODES: u32 = 24;
const NUM_DISTANCE_CODES: u32 = 40;
const NUM_SHORT_DISTANCES: u32 = 120;

const ROW_BATCH: i32 = 16;

/// Below this many pixels an image's transforms run after its pixels on the
/// one thread, rather than beside them on a second. Measured on crops of a
/// photo: 128x128 loses 4% to the handoff, 160x160 gains 8%, 200x200 23%.
const PIPELINE_PIXELS: usize = 192 * 192;

/// The payload goes to the entropy decoder in about this many pieces, and the
/// rows each piece finishes go to the transform thread together.
const PIPELINE_PIECES: usize = 32;

/// Pieces are never smaller than this many bytes.
const PIPELINE_MIN_STEP: usize = 4096;

/// An alpha plane's rows go from residuals to the plane this many at a time,
/// and cross to the transform thread, when there is one, in bands of at most
/// this many that are handed back and reused.
const ALPHA_BAND_ROWS: i32 = 32;

const PADDING: usize = 16;

const ARENA_CHUNK: usize = 4096;

/// Bits that can index the root of a green table. Green has the most
/// symbols, and a code too long for the root takes a second lookup behind a
/// branch that mispredicts. A group gets a root of at most a quarter as many
/// entries as it covers pixels, on average, so that small images and images
/// of many groups do not spend longer building tables than using them.
const GREEN_TABLE_BITS: u32 = 11;

/// Pixels a group has to cover, on average, before it gets the table
/// `huffman::build_packed` makes. A table takes about as long to build as it
/// saves over 200 literals.
const PACKED_MIN_PIXELS: usize = 1024;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Transform {
    #[default]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Argb,
    Alpha,
}

fn target_picture<'p>(
    target: Target,
    argb: &'p mut Picture,
    alpha_argb: &'p mut Picture,
) -> &'p mut Picture {
    match target {
        Target::Argb => argb,
        Target::Alpha => alpha_argb,
    }
}

pub struct AlphaDst<'a> {
    pub data: &'a mut [u8],
    pub stride: usize,
    /// How the plane is unfiltered, for a decode that can do it as the rows
    /// come; `alpha_unfiltered` says whether it did.
    pub unfilter: Option<Unfilter>,
}

/// An alpha plane's unfiltering: its first row by `first` alone, every row
/// after that by `rest` against the row above.
#[derive(Clone, Copy)]
pub struct Unfilter {
    pub first: UnfilterFn,
    pub rest: UnfilterFn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Output {
    Argb,
    Still,
}

#[derive(Default)]
pub struct Picture {
    pub data: Vec<u32>,
    pub stride: usize,
    pub width: i32,
    pub height: i32,
}

impl Picture {
    pub fn frame(&self) -> Frame<'_> {
        Frame::packed(
            self.data.as_bytes(),
            self.stride * 4,
            self.width,
            self.height,
            Format::Argb,
        )
    }

    pub fn frame_mut(&mut self) -> FrameMut<'_> {
        let (width, height, stride) = (self.width, self.height, self.stride * 4);
        let plane = [
            PlaneMut::borrowed(self.data.as_mut_bytes(), stride),
            PlaneMut::borrowed(&mut [], 0),
            PlaneMut::borrowed(&mut [], 0),
            PlaneMut::borrowed(&mut [], 0),
        ];

        FrameMut::borrowed(plane, width, height, Format::Argb, false)
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.data.is_empty()
    }

    fn alloc(&mut self, w: i32, h: i32) -> Result<()> {
        if w <= 0 || h <= 0 {
            return Err(Error::TooLarge);
        }
        let size = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_add(PADDING))
            .ok_or(Error::TooLarge)?;

        if self.data.len() < size {
            self.data = Vec::new();
            self.data = crate::picture::try_zeroed(size)?;
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

#[derive(Clone, Copy, Default)]
pub struct Resume {
    pub pos: usize,
    pub cached: usize,
    pub x: i32,
    pub y: i32,
    pub hg: usize,
    pub rows_done: i32,
}

#[derive(Clone, Copy, Default)]
pub struct HTreeGroup {
    pub trees: [Reader; HUFFMAN_CODES_PER_META_CODE],
    pub trivial_literal: bool,
    /// The literal pixel, if `trivial_literal`, and else its alpha, if the
    /// alpha code has a single symbol.
    pub literal: [u8; 4],
    /// Where `huffman::build_packed` put the group's table in the arena.
    pub packed: Option<u32>,
    /// Where `huffman::build_fused` put the group's table in the arena.
    pub fused: Option<u32>,
}

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

fn grow<T: Copy>(buf: &mut Vec<T>, len: usize, fill: T) -> Result<()> {
    if buf.len() < len {
        buf.try_reserve(len - buf.len())
            .map_err(|_| Error::NoMemory)?;
        buf.resize(len, fill);
    }
    Ok(())
}

#[derive(Default)]
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
    nb_huffman_group_codes: usize,
    huffman_groups_mapped: bool,
    huffman_group_map: Vec<u32>,
    image: [ImageContext; ROLE_NB],

    alpha_dst_used: bool,
    alpha_unfiltered: bool,

    argb: Picture,
    alpha_argb: Picture,
    out: Picture,
    indices: Vec<u8>,
    staged: bool,
    scratch: Vec<u32>,
    green: Vec<u8>,
    sorted: Vec<u16>,
    lengths: Vec<u8>,

    active: bool,
    next_try: usize,
    resume: Resume,
    rows_out: i32,
    peeked: bool,

    /// The threads a decode may use, counting the calling one.
    pub threads: usize,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

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
        self.resume = Resume::default();
        self.rows_out = 0;
        self.reduced_width = 0;
        self.nb_huffman_groups = 0;
        self.nb_huffman_group_codes = 0;
        self.huffman_groups_mapped = false;
    }

    pub fn release(&mut self) {
        self.reset();
        for img in &mut self.image {
            *img = ImageContext::default();
        }
        self.argb.release();
        self.alpha_argb.release();
        self.out.release();
        self.indices = Vec::new();
        self.scratch = Vec::new();
        self.green = Vec::new();
        self.sorted = Vec::new();
        self.lengths = Vec::new();
        self.huffman_group_map = Vec::new();
    }

    pub fn release_alpha_canvas(&mut self) {
        self.alpha_argb.release();
    }

    pub fn set_canvas(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
    }

    pub fn alpha_dst_used(&self) -> bool {
        self.alpha_dst_used
    }

    /// Whether the last alpha decode unfiltered its plane as well.
    pub fn alpha_unfiltered(&self) -> bool {
        self.alpha_unfiltered
    }

    pub fn still_active(&self) -> bool {
        self.active
    }

    pub fn still_rows_out(&self) -> i32 {
        self.rows_out
    }

    pub fn picture(&self, target: Target) -> &Picture {
        match target {
            Target::Argb => &self.argb,
            Target::Alpha => &self.alpha_argb,
        }
    }

    pub fn still_picture(&self) -> Option<&Picture> {
        if !self.staged {
            return None;
        }
        Some(if self.peeked { &self.out } else { &self.argb })
    }

    pub fn picture_out_mut(&mut self, target: Target) -> &mut Picture {
        match target {
            Target::Argb => &mut self.argb,
            Target::Alpha => &mut self.alpha_argb,
        }
    }

    pub fn still_picture_mut(&mut self) -> Option<&mut Picture> {
        if !self.staged {
            return None;
        }
        Some(if self.peeked {
            &mut self.out
        } else {
            &mut self.argb
        })
    }

    pub fn view(&self, which: Output) -> Option<Frame<'_>> {
        let pic = match which {
            Output::Argb => self.picture(Target::Argb),
            Output::Still => self.still_picture()?,
        };

        (!pic.is_empty()).then(|| pic.frame())
    }

    pub fn view_mut(&mut self, which: Output) -> Option<FrameMut<'_>> {
        let pic = match which {
            Output::Argb => self.picture_out_mut(Target::Argb),
            Output::Still => self.still_picture_mut()?,
        };

        if pic.is_empty() {
            return None;
        }
        Some(pic.frame_mut())
    }

    fn picture_mut(&mut self, role: usize, target: Target) -> &mut Picture {
        if role != ROLE_ARGB {
            return &mut self.image[role].storage;
        }
        target_picture(target, &mut self.argb, &mut self.alpha_argb)
    }

    fn update_canvas_size(&mut self, w: i32, h: i32) {
        if self.width != 0 && self.width != w {
            crate::log::warning_args(format_args!(
                "Width mismatch. {} != {}",
                self.width, w
            ));
        }
        self.width = w;
        if self.height != 0 && self.height != h {
            crate::log::warning_args(format_args!(
                "Height mismatch. {} != {}",
                self.height, h
            ));
        }
        self.height = h;
    }

    fn parse_block_size(&mut self, buf: &[u8]) -> (u32, i32, i32) {
        let bits = self.gb.bits(buf, 3) + 2;
        let w = ceil_shift(self.reduced_width, bits);
        let h = ceil_shift(self.height, bits);

        (bits, w, h)
    }

    fn parse_subimage(&mut self, role: usize, buf: &[u8]) -> Result<()> {
        let (block_bits, blocks_w, blocks_h) = self.parse_block_size(buf);

        self.decode_entropy_coded_image(role, Target::Argb, buf, blocks_w, blocks_h)?;
        self.image[role].size_reduction = block_bits;
        Ok(())
    }

    fn decode_entropy_image(&mut self, buf: &[u8]) -> Result<()> {
        self.parse_subimage(ROLE_ENTROPY, buf)?;

        const UNUSED: u32 = u32::MAX;
        let img = &mut self.image[ROLE_ENTROPY];
        let mut max = 0;

        for y in 0..img.storage.height as usize {
            let row = &img.storage.data[y * img.storage.stride..]
                [..img.storage.width as usize];

            for px in row {
                max = max.max(entropy::group_index(*px));
            }
        }
        let nb_codes = max as usize + 1;

        self.nb_huffman_group_codes = nb_codes;
        self.nb_huffman_groups = nb_codes;
        self.huffman_groups_mapped = false;

        let pixels = (img.storage.width as usize) * (img.storage.height as usize);

        /* Match libwebp's threshold: compact sparse ids and avoid reserving
         * all decoding tables up front when an image asks for thousands. */
        if nb_codes <= 1000 && nb_codes <= pixels {
            return Ok(());
        }

        grow(&mut self.huffman_group_map, nb_codes, UNUSED)?;
        self.huffman_group_map[..nb_codes].fill(UNUSED);

        let mut nb_groups = 0u32;

        for y in 0..img.storage.height as usize {
            let row = &mut img.storage.data[y * img.storage.stride..]
                [..img.storage.width as usize];

            for px in row {
                let old = entropy::group_index(*px) as usize;
                let mapped = &mut self.huffman_group_map[old];

                if *mapped == UNUSED {
                    *mapped = nb_groups;
                    nb_groups += 1;
                }
                entropy::set_group_index(px, *mapped);
            }
        }
        self.nb_huffman_groups = nb_groups as usize;
        self.huffman_groups_mapped = true;
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
        self.picture_mut(role, target).alloc(w, h)?;
        self.read_image_header(role, buf, w as usize * h as usize)?;
        self.decode_pixels(role, target, buf, false)?;
        Ok(())
    }

    fn read_image_header(
        &mut self,
        role: usize,
        buf: &[u8],
        pixels: usize,
    ) -> Result<()> {
        let cache_bits = if self.gb.bit(buf) != 0 {
            let bits = self.gb.bits(buf, 4);

            if !(1..=11).contains(&bits) {
                crate::log::error_args(format_args!(
                    "invalid color cache bits: {bits}"
                ));
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
        let mut nb_group_codes = 1usize;

        if role == ROLE_ARGB {
            self.huffman_groups_mapped = false;
            if self.gb.bit(buf) != 0 {
                self.decode_entropy_image(buf)?;
                nb_groups = self.nb_huffman_groups;
                nb_group_codes = self.nb_huffman_group_codes;
            }
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
            huffman_groups_mapped,
            huffman_group_map,
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
        img.arena
            .try_reserve(ARENA_CHUNK)
            .map_err(|_| Error::NoMemory)?;

        let green_bits = (pixels / nb_groups)
            .max(1)
            .ilog2()
            .saturating_sub(2)
            .clamp(huffman::TABLE_BITS, GREEN_TABLE_BITS);

        #[allow(clippy::needless_range_loop)]
        for code in 0..nb_group_codes {
            let group = if role == ROLE_ARGB && *huffman_groups_mapped {
                (huffman_group_map[code] != u32::MAX)
                    .then_some(huffman_group_map[code] as usize)
            } else {
                Some(code)
            };

            for j in 0..HUFFMAN_CODES_PER_META_CODE {
                let extra = if j == HUFF_IDX_GREEN && cache_bits > 0 {
                    1usize << cache_bits
                } else {
                    0
                };
                let alphabet_size = ALPHABET_SIZES[j] as usize + extra;
                let lengths = &mut lengths[..alphabet_size];
                let mut plan = if j == HUFF_IDX_GREEN {
                    Plan::with_root_bits(green_bits)
                } else {
                    Plan::default()
                };

                lengths.fill(0);
                if gb.bit(buf) != 0 {
                    huffman::read_simple_code(gb, buf, &mut plan, lengths);
                } else {
                    huffman::read_normal_code(gb, buf, &mut plan, lengths)?;
                }
                /* Match libwebp: reject Huffman tables read past the chunk. */
                if gb.is_eos(buf) {
                    crate::log::error("prefix code runs past the end of the data");
                    return Err(Error::InvalidData);
                }
                if let Some(group) = group {
                    img.groups[group].trees[j] =
                        huffman::build(&mut img.arena, &mut plan, lengths, sorted)?;
                } else {
                    huffman::validate(&mut plan, lengths, sorted)?;
                }
            }

            let Some(group) = group else {
                continue;
            };
            let hg = &mut img.groups[group];

            let opaque = hg.trees[HUFF_IDX_ALPHA].mask == 0;

            hg.trivial_literal = hg.trees[HUFF_IDX_RED].mask == 0
                && hg.trees[HUFF_IDX_BLUE].mask == 0
                && opaque;
            if hg.trivial_literal {
                for (slot, tree) in
                    [(0, HUFF_IDX_ALPHA), (1, HUFF_IDX_RED), (3, HUFF_IDX_BLUE)]
                {
                    hg.literal[slot] = hg.trees[tree].tree(&img.arena).only_symbol();
                }
            } else if pixels / PACKED_MIN_PIXELS >= nb_groups {
                let packed = huffman::build_packed(
                    &mut img.arena,
                    [
                        hg.trees[HUFF_IDX_RED],
                        hg.trees[HUFF_IDX_BLUE],
                        hg.trees[HUFF_IDX_ALPHA],
                    ],
                )?;

                hg.packed = Some(packed);
                if opaque {
                    hg.literal[0] =
                        hg.trees[HUFF_IDX_ALPHA].tree(&img.arena).only_symbol();
                    hg.fused = huffman::build_fused(
                        &mut img.arena,
                        hg.trees[HUFF_IDX_GREEN],
                        packed,
                    )?;
                }
            }
        }
        Ok(())
    }

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

        let pic = target_picture(target, argb, alpha_argb);

        decode_argb(gb, buf, image, pic, *reduced_width, resume, resumable)
    }

    fn read_frame_header(
        &mut self,
        buf: &[u8],
        is_alpha_chunk: bool,
    ) -> Result<(i32, i32)> {
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
                crate::log::error_args(format_args!(
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
                Transform::Predictor => self.parse_subimage(ROLE_PREDICTOR, buf)?,
                Transform::Color => self.parse_subimage(ROLE_COLOR, buf)?,
                Transform::ColorIndexing => self.parse_transform_color_indexing(buf)?,
                Transform::SubtractGreen => {}
            }
        }

        self.read_image_header(
            ROLE_ARGB,
            buf,
            self.reduced_width as usize * self.height as usize,
        )?;
        Ok((w, h))
    }

    fn alpha_is_8b(&self) -> bool {
        self.nb_transforms == 1
            && self.transforms[0] == Transform::ColorIndexing
            && self.image[ROLE_ARGB].color_cache_bits == 0
            && self.image[ROLE_ARGB]
                .groups
                .iter()
                .all(|hg| hg.trivial_literal)
    }

    pub fn decode_frame(
        &mut self,
        target: Target,
        buf: &[u8],
        is_alpha_chunk: bool,
        alpha_dst: Option<AlphaDst<'_>>,
    ) -> Result<()> {
        self.alpha_dst_used = false;
        self.alpha_unfiltered = false;

        let ret = self.decode_frame_inner(target, buf, is_alpha_chunk, alpha_dst);

        if self.alpha_dst_used {
            self.alpha_argb.release();
        } else {
            let pic = self.picture_mut(ROLE_ARGB, target);

            pic.stride = pic.width.max(0) as usize;
        }
        for img in &mut self.image {
            img.clear();
        }
        ret
    }

    fn decode_frame_inner(
        &mut self,
        target: Target,
        buf: &[u8],
        is_alpha_chunk: bool,
        alpha_dst: Option<AlphaDst<'_>>,
    ) -> Result<()> {
        let (w, h) = self.read_frame_header(buf, is_alpha_chunk)?;
        let alpha_dst = match alpha_dst {
            Some(dst) if self.alpha_is_8b() => {
                return self.decode_alpha_8b(buf, dst);
            }
            dst => dst,
        };

        self.picture_mut(ROLE_ARGB, target).alloc(w, h)?;
        let alpha_dst = if self.pipelines() {
            match (target, alpha_dst) {
                (Target::Argb, _) => {
                    self.still_alloc()?;
                    self.resume = Resume::default();
                    self.pipeline(buf, 0)?;
                    /* The transformed image is in `out`; the pixels it came
                     * from are spent. */
                    std::mem::swap(&mut self.argb, &mut self.out);
                    return Ok(());
                }
                (Target::Alpha, Some(dst)) => return self.alpha_pipelined(buf, dst),
                (Target::Alpha, None) => None,
            }
        } else {
            alpha_dst
        };
        self.decode_pixels(ROLE_ARGB, target, buf, false)?;
        match alpha_dst {
            Some(dst)
                if !self.transforms[..self.nb_transforms]
                    .contains(&Transform::ColorIndexing) =>
            {
                self.alpha_rows(dst)
            }
            dst => self.apply_transforms(target, dst),
        }
    }

    fn decode_alpha_8b(&mut self, buf: &[u8], dst: AlphaDst<'_>) -> Result<()> {
        let width = self.reduced_width.max(0) as usize;
        let height = self.height;
        let total = width
            .checked_mul(height.max(0) as usize)
            .ok_or(Error::TooLarge)?;

        grow(&mut self.indices, total, 0u8)?;

        {
            let Decoder {
                gb, image, indices, ..
            } = self;
            let (head, tail) = image.split_at_mut(ROLE_ENTROPY);
            let ent = &tail[0];

            entropy::decode_alpha_pixels(entropy::AlphaArgs {
                gb,
                buf,
                pixels: &mut indices[..total],
                width,
                groups: &head[ROLE_ARGB].groups,
                arena: &head[ROLE_ARGB].arena,
                entropy: (ent.size_reduction > 0).then(|| entropy::Entropy {
                    data: &ent.storage.data,
                    stride: ent.storage.stride,
                    bits: ent.size_reduction,
                }),
            })?;
        }

        let pal = &self.image[ROLE_PALETTE];

        transform::color_indexing_alpha(
            &self.indices[..total],
            width,
            self.width.max(0) as usize,
            height,
            &pal.storage.data[..pal.storage.width as usize],
            pal.size_reduction,
            dst,
        );
        self.reduced_width = self.width;
        self.alpha_dst_used = true;
        Ok(())
    }

    fn apply_transforms(
        &mut self,
        target: Target,
        mut alpha_dst: Option<AlphaDst<'_>>,
    ) -> Result<()> {
        // Palette expansion changes the row layout and keeps its whole-image path.
        if self.nb_transforms > 1
            && !self.transforms[..self.nb_transforms]
                .contains(&Transform::ColorIndexing)
        {
            return self.apply_transform_batches(target);
        }
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

    fn apply_transform_batches(&mut self, target: Target) -> Result<()> {
        let width = self.reduced_width as usize;
        let has_predictor =
            self.transforms[..self.nb_transforms].contains(&Transform::Predictor);
        if has_predictor {
            grow(&mut self.scratch, 2 * width + 1, 0)?;
        }
        let pic = target_picture(target, &mut self.argb, &mut self.alpha_argb);
        let stride = pic.stride;
        let mut y0 = 0;

        while y0 < pic.height {
            let y1 = (y0 + ROW_BATCH).min(pic.height);
            let base = y0 as usize * stride;
            for i in (0..self.nb_transforms).rev() {
                match self.transforms[i] {
                    Transform::Predictor => {
                        predict_batch(
                            &self.dsp,
                            &mut pic.data,
                            &mut self.scratch,
                            base,
                            stride,
                            width,
                            width,
                            &self.image[ROLE_PREDICTOR],
                            y0,
                            y1,
                        )?;
                        // Later transforms must not change the predictor's upper row.
                        if y1 < pic.height {
                            let last = (y1 - 1) as usize * stride;
                            self.scratch[..width]
                                .copy_from_slice(&pic.data[last..][..width]);
                        }
                    }
                    Transform::Color => {
                        let mult = &self.image[ROLE_COLOR];
                        transform::color_rows(
                            &self.dsp,
                            &mut pic.data,
                            base,
                            stride,
                            width,
                            &mult.storage.data,
                            mult.storage.stride,
                            mult.size_reduction,
                            y0,
                            y1,
                        );
                    }
                    Transform::SubtractGreen => {
                        transform::subtract_green_rows(
                            &self.dsp,
                            &mut pic.data,
                            base,
                            stride,
                            width,
                            y1 - y0,
                        );
                    }
                    Transform::ColorIndexing => unreachable!(),
                }
            }
            y0 = y1;
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
        let pic = target_picture(target, argb, alpha_argb);
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
        let pic = target_picture(target, argb, alpha_argb);
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
        let Decoder {
            dsp,
            argb,
            alpha_argb,
            reduced_width,
            ..
        } = self;
        let width = *reduced_width as usize;
        let pic = target_picture(target, argb, alpha_argb);

        transform::subtract_green_rows(
            dsp,
            &mut pic.data,
            0,
            pic.stride,
            width,
            pic.height,
        );
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
        let pic = target_picture(target, argb, alpha_argb);
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
        let pic = target_picture(target, argb, alpha_argb);
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

fn ceil_shift(v: i32, s: u32) -> i32 {
    (v + (1 << s) - 1) >> s
}

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

            let mut ret = self
                .read_frame_header(payload, false)
                .and_then(|(w, h)| self.argb.alloc(w, h));

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

        let ret = self.still_rows(payload, complete);

        if ret.is_err() {
            self.still_abandon();
        }
        ret
    }

    /* A packed image decodes with `argb.stride` narrowed to the packed width,
     * and only a finished image widens it again. An error in between would
     * leave a live still whose stride no longer matches its width, which is
     * what every reader of the picture assumes. A failed image is over, so
     * put the picture back in order and let go of it. */
    fn still_abandon(&mut self) {
        self.argb.stride = self.argb.width.max(0) as usize;
        for img in &mut self.image {
            img.clear();
        }
        self.active = false;
    }

    fn still_rows(&mut self, payload: &[u8], complete: bool) -> Result<Status> {
        if complete && self.pipelines() {
            return self.still_rows_pipelined(payload);
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

    /// Whether an image's transforms are worth a thread of their own. The
    /// predictor and colour transforms are; subtracting green or expanding a
    /// palette costs less than handing the rows over does.
    fn pipelines(&self) -> bool {
        self.threads > 1
            && self.transforms[..self.nb_transforms]
                .iter()
                .any(|t| matches!(t, Transform::Predictor | Transform::Color))
            && (self.width.max(0) as usize) * (self.height.max(0) as usize)
                >= PIPELINE_PIXELS
    }

    pub fn still_peek(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
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
        let stride = out.stride;
        let base = y0 as usize * stride;

        copy_packed_rows(&mut out.data[base..], stride, argb, *reduced_width, y0, y1);

        let xf = Inverse {
            dsp,
            side: &image[ROLE_PREDICTOR..],
            list: &transforms[..*nb_transforms],
            packed: *reduced_width,
            width: *width,
            out_width: out.width,
            out_height: out.height,
        };

        xf.rows(&mut out.data, scratch, base, stride, y0, y1)
    }

    /// A still whose whole payload is here: the rows the pipeline finishes
    /// land in `out`, as they would have had the still been peeked at.
    fn still_rows_pipelined(&mut self, payload: &[u8]) -> Result<Status> {
        if !self.peeked {
            self.still_alloc()?;
            self.peeked = true;
        }
        self.rows_out = self.pipeline(payload, self.rows_out)?;
        self.argb.stride = self.argb.width.max(0) as usize;
        for img in &mut self.image {
            img.clear();
        }
        self.active = false;
        Ok(Status::Done)
    }

    /// Decodes the main image from row `first` on with two threads: this one
    /// runs the entropy decoder and copies each run of rows it finishes to
    /// `out`, and the other runs the inverse transforms there. Returns the
    /// rows done, which is all of them.
    fn pipeline(&mut self, payload: &[u8], first: i32) -> Result<i32> {
        let Decoder {
            dsp,
            gb,
            image,
            argb,
            out,
            scratch,
            resume,
            reduced_width,
            transforms,
            nb_transforms,
            width,
            threads,
            ..
        } = self;
        let (coded, side) = image.split_at_mut(ROLE_PREDICTOR);
        let stride = out.stride;
        let packed = *reduced_width;
        let xf = Inverse {
            dsp,
            side,
            list: &transforms[..*nb_transforms],
            packed,
            width: *width,
            out_width: out.width,
            out_height: out.height,
        };
        let (tx, rx) = std::sync::mpsc::channel::<(i32, i32, &mut [u32])>();
        let (xf_ret, dec_ret) = crate::task::join(
            *threads,
            move || {
                let mut ret = Ok(());

                for (y0, y1, band) in rx {
                    if ret.is_ok() {
                        ret = xf.rows(band, scratch, 0, stride, y0, y1);
                    }
                }
                ret
            },
            || {
                let tx = tx;
                let mut rest = &mut out.data[first as usize * stride..];
                let pieces = Pieces {
                    gb,
                    payload,
                    coded,
                    pic: argb,
                    packed,
                    resume,
                };

                pieces.decode(first, |y0, y1, pic| {
                    let (band, tail) = std::mem::take(&mut rest)
                        .split_at_mut((y1 - y0) as usize * stride);

                    rest = tail;
                    copy_packed_rows(band, stride, pic, packed, y0, y1);
                    /* The receiver outlives this loop, so the send cannot fail. */
                    let _ = tx.send((y0, y1, band));
                    Ok(())
                })
            },
        );

        /* The pixels failing is what a single pass would have reported. */
        let rows = dec_ret?;

        xf_ret.map(|()| rows)
    }

    /// An alpha plane decoded with two threads, as `pipeline` decodes a still,
    /// except that the transformed rows go through a few recycled bands, and
    /// the thread that transforms them also takes their green to `dst` and
    /// unfilters it there.
    fn alpha_pipelined(&mut self, payload: &[u8], dst: AlphaDst<'_>) -> Result<()> {
        let w = self.width.max(0) as usize;

        grow(&mut self.scratch, 2 * w + 1, 0)?;
        grow(&mut self.green, 2 * (w + 1), 0)?;
        self.resume = Resume::default();

        let by_green = self.green_alpha();
        let Decoder {
            dsp,
            gb,
            image,
            alpha_argb,
            scratch,
            green,
            resume,
            reduced_width,
            transforms,
            nb_transforms,
            width,
            height,
            threads,
            ..
        } = self;
        let (coded, side) = image.split_at_mut(ROLE_PREDICTOR);
        let packed = *reduced_width;
        let unfilter = dst.unfilter;
        let mut rows = AlphaRows {
            extract_green: dsp.extract_green,
            xf: Inverse {
                dsp,
                side,
                list: &transforms[..*nb_transforms],
                packed,
                width: *width,
                out_width: *width,
                out_height: *height,
            },
            dst,
            green,
            by_green,
            rest: 0,
        };
        let (tx, rx) = std::sync::mpsc::channel::<(i32, i32, Vec<u32>)>();
        let (back_tx, back_rx) = std::sync::mpsc::channel::<Vec<u32>>();
        let (xf_ret, dec_ret) = crate::task::join(
            *threads,
            move || {
                let mut ret = Ok(());

                for (y0, y1, mut band) in rx {
                    if ret.is_ok() {
                        ret = rows.rows(&mut band, w, scratch, y0, y1);
                    }
                    /* The decoding side may be gone already; the band then
                     * has nowhere to go but away. */
                    let _ = back_tx.send(band);
                }
                ret
            },
            || {
                let tx = tx;
                let pieces = Pieces {
                    gb,
                    payload,
                    coded,
                    pic: alpha_argb,
                    packed,
                    resume,
                };

                pieces.decode(0, |y0, y1, pic| {
                    let mut y = y0;

                    while y < y1 {
                        let end = (y + ALPHA_BAND_ROWS).min(y1);
                        let len = (end - y) as usize * w;
                        let mut band = back_rx.try_recv().unwrap_or_default();

                        band.clear();
                        band.try_reserve(len).map_err(|_| Error::NoMemory)?;
                        band.resize(len, 0);
                        copy_packed_rows(&mut band, w, pic, packed, y, end);
                        let _ = tx.send((y, end, band));
                        y = end;
                    }
                    Ok(())
                })
            },
        );

        dec_ret?;
        xf_ret?;
        self.alpha_unfiltered = unfilter.is_some();
        self.alpha_dst_used = true;
        Ok(())
    }

    /// An alpha plane whose pixels are all decoded, taken to `dst` a band of
    /// rows at a time, so each band is transformed, has its green taken and
    /// is unfiltered while it is still in cache.
    fn alpha_rows(&mut self, dst: AlphaDst<'_>) -> Result<()> {
        let w = self.width.max(0) as usize;

        grow(&mut self.scratch, 2 * w + 1, 0)?;
        grow(&mut self.green, 2 * (w + 1), 0)?;

        let by_green = self.green_alpha();
        let Decoder {
            dsp,
            image,
            alpha_argb,
            scratch,
            green,
            reduced_width,
            transforms,
            nb_transforms,
            width,
            height,
            ..
        } = self;
        let unfilter = dst.unfilter;
        let mut rows = AlphaRows {
            extract_green: dsp.extract_green,
            xf: Inverse {
                dsp,
                side: &image[ROLE_PREDICTOR..],
                list: &transforms[..*nb_transforms],
                packed: *reduced_width,
                width: *width,
                out_width: *width,
                out_height: *height,
            },
            dst,
            green,
            by_green,
            rest: 0,
        };
        let stride = alpha_argb.stride;
        let mut y0 = 0;

        while y0 < *height {
            let y1 = (y0 + ALPHA_BAND_ROWS).min(*height);

            rows.rows(
                &mut alpha_argb.data[y0 as usize * stride..],
                stride,
                scratch,
                y0,
                y1,
            )?;
            y0 = y1;
        }
        self.alpha_unfiltered = unfilter.is_some();
        self.alpha_dst_used = true;
        Ok(())
    }

    /// Whether an alpha image's green can be inverse predicted on its own,
    /// for as long as its pixels' other channels stay the same: it has to be
    /// the predictor that is undone first, since the transforms after it
    /// leave green alone and can be skipped, but not those before it.
    fn green_alpha(&self) -> bool {
        match self.transforms[..self.nb_transforms].split_last() {
            Some((Transform::Predictor, rest)) => rest
                .iter()
                .all(|t| matches!(t, Transform::Color | Transform::SubtractGreen)),
            _ => false,
        }
    }
}

/// An alpha image's rows on their way from residuals to the plane: through
/// the inverse transforms, then their green taken to the plane and
/// unfiltered there.
///
/// The select predictor is the only one to look across channels, and an
/// alpha image's other channels are usually the same in every pixel, where
/// they make no difference to it. While they are, the rows skip the full
/// transforms, and green is inverse predicted by itself.
struct AlphaRows<'a> {
    xf: Inverse<'a>,
    extract_green: fn(&mut [u8], &[u8]),
    dst: AlphaDst<'a>,
    /// Two rows of predicted green, each one longer than a row.
    green: &'a mut [u8],
    by_green: bool,
    /// The channels other than green that every pixel so far has.
    rest: u32,
}

impl AlphaRows<'_> {
    /// Rows y0..y1 in order, from their residuals in `band`, `stride` apart.
    /// `scratch` carries the last row predicted in full from one call to the
    /// next.
    fn rows(
        &mut self,
        band: &mut [u32],
        stride: usize,
        scratch: &mut [u32],
        y0: i32,
        y1: i32,
    ) -> Result<()> {
        let w = self.xf.width as usize;
        let mut y = y0;

        while self.by_green && y < y1 {
            let res = &band[(y - y0) as usize * stride..][..w];

            if !self.green_row(res, y)? {
                self.by_green = false;
                if y > 0 {
                    /* The full predictor picks up from the row above,
                     * rebuilt whole. */
                    let above = &self.green[(y as usize - 1) % 2 * (w + 1)..][..w];

                    for (px, &g) in scratch[..w].iter_mut().zip(above) {
                        let mut b = self.rest.to_ne_bytes();

                        b[2] = g;
                        *px = u32::from_ne_bytes(b);
                    }
                }
                break;
            }
            y += 1;
        }
        if y == y1 {
            return Ok(());
        }

        let base = (y - y0) as usize * stride;

        self.xf.rows(band, scratch, base, stride, y, y1)?;
        for (i, y) in (y..y1).enumerate() {
            let row = &band[base + i * stride..][..w];
            let at = y as usize * self.dst.stride;

            (self.extract_green)(&mut self.dst.data[at..at + w], row.as_bytes());
            self.unfilter(y);
        }
        Ok(())
    }

    /// Predicts row y's green from its residuals, and takes it to the plane,
    /// unless a pixel's other channels would differ from the rest.
    fn green_row(&mut self, res: &[u32], y: i32) -> Result<bool> {
        let w = res.len();
        let (a, b) = self.green[..2 * (w + 1)].split_at_mut(w + 1);
        let (above, here) = if y % 2 == 0 { (b, a) } else { (a, b) };
        let here = &mut here[..w];
        let modes = self.xf.side(ROLE_PREDICTOR);

        if y == 0 {
            let black = u32::from_ne_bytes([0xFF, 0, 0, 0]);
            let first =
                crate::dsp::vp8l::add_pixels(res[0], black) & transform::NOT_GREEN;

            if res[1..].iter().fold(0, |acc, &px| acc | px) & transform::NOT_GREEN != 0
            {
                return Ok(false);
            }
            /* Black, what mode 0 predicts, has to match as well. */
            if first != black && Self::uses_black(modes) {
                return Ok(false);
            }
            self.rest = first;
            transform::predict_green_first_row(self.xf.dsp, res, here);
        } else {
            if res.iter().fold(0, |acc, &px| acc | px) & transform::NOT_GREEN != 0 {
                return Ok(false);
            }

            let bits = modes.size_reduction;
            let storage = &modes.storage;

            transform::predict_green_row(
                self.xf.dsp,
                &storage.data[(y >> bits) as usize * storage.stride..],
                bits,
                res,
                above,
                here,
            )?;
        }

        let at = y as usize * self.dst.stride;

        self.dst.data[at..at + w].copy_from_slice(here);
        self.unfilter(y);
        Ok(true)
    }

    fn uses_black(modes: &ImageContext) -> bool {
        let storage = &modes.storage;
        let w = storage.width.max(0) as usize;

        (0..storage.height.max(0) as usize).any(|y| {
            storage.data[y * storage.stride..][..w]
                .iter()
                .any(|m| m.to_ne_bytes()[2] == 0)
        })
    }

    fn unfilter(&mut self, y: i32) {
        let Some(u) = self.dst.unfilter else {
            return;
        };
        let w = self.xf.width as usize;
        let stride = self.dst.stride;

        if y == 0 {
            (u.first)(None, &mut self.dst.data[..w]);
        } else {
            let at = (y as usize - 1) * stride;
            let (above, here) = self.dst.data[at..].split_at_mut(stride);

            (u.rest)(Some(&above[..w]), &mut here[..w]);
        }
    }
}

/// The entropy decoder run over growing prefixes of a complete payload, so it
/// stops every so often with rows finished that another thread can take. The
/// prefixes are only stopping points; the pixels are the ones a single pass
/// would decode.
struct Pieces<'a> {
    gb: &'a mut BitReader,
    payload: &'a [u8],
    coded: &'a mut [ImageContext],
    pic: &'a mut Picture,
    packed: i32,
    resume: &'a mut Resume,
}

impl Pieces<'_> {
    /// Decodes on from row `first`, calling `done` with each run of rows
    /// finished. Returns the rows done, which is all of them.
    fn decode(
        self,
        first: i32,
        mut done: impl FnMut(i32, i32, &Picture) -> Result<()>,
    ) -> Result<i32> {
        let Pieces {
            gb,
            payload,
            coded,
            pic,
            packed,
            resume,
        } = self;
        let mut sent = first;
        let start = payload.len() - gb.left(payload);
        let step = ((payload.len() - start) / PIPELINE_PIECES).max(PIPELINE_MIN_STEP);
        let mut end = start;

        loop {
            /* The last piece's rows are transformed with nothing left to
             * decode beside them, so the pieces shrink toward the end. */
            let step = step.min(((payload.len() - end) / 4).max(PIPELINE_MIN_STEP));

            end = (end + step).min(payload.len());

            let last = end == payload.len();
            let status =
                decode_argb(gb, &payload[..end], coded, pic, packed, resume, true)?;

            if status == Status::NeedMore && last {
                crate::log::error("image data runs past the end of the chunk");
                return Err(Error::InvalidData);
            }

            let rows = resume.rows_done;

            if rows > sent {
                done(sent, rows, pic)?;
                sent = rows;
            }
            if status == Status::Done {
                return Ok(sent);
            }
        }
    }
}

/// Copies rows y0..y1 of the entropy-coded pixels, `packed` wide, to the start
/// of `dst`, whose rows are `stride` apart.
fn copy_packed_rows(
    dst: &mut [u32],
    stride: usize,
    src: &Picture,
    packed: i32,
    y0: i32,
    y1: i32,
) {
    let packed = packed.max(0) as usize;

    for (i, y) in (y0..y1).enumerate() {
        dst[i * stride..][..packed]
            .copy_from_slice(&src.data[y as usize * src.stride..][..packed]);
    }
}

/// Decodes the main image's pixels. `coded` is the argb and entropy images'
/// contexts, the two the entropy decoder reads.
fn decode_argb(
    gb: &mut BitReader,
    buf: &[u8],
    coded: &mut [ImageContext],
    pic: &mut Picture,
    reduced_width: i32,
    resume: &mut Resume,
    resumable: bool,
) -> Result<Status> {
    let (head, tail) = coded.split_at_mut(ROLE_ENTROPY);
    let ImageContext {
        color_cache,
        color_cache_bits,
        groups,
        arena,
        ..
    } = &mut head[ROLE_ARGB];
    let ent = &tail[0];

    entropy::decode_pixels(entropy::Args {
        gb,
        buf,
        pic,
        groups,
        arena,
        cache: color_cache,
        cache_bits: *color_cache_bits,
        reduced_width: Some(reduced_width),
        entropy: (ent.size_reduction > 0).then(|| entropy::Entropy {
            data: &ent.storage.data,
            stride: ent.storage.stride,
            bits: ent.size_reduction,
        }),
        st: resume,
        resumable,
    })
}

/// What the inverse transforms read besides the pixels they rewrite, apart
/// from the decoder so rows can be transformed on another thread.
struct Inverse<'a> {
    dsp: &'a Vp8lDsp,
    /// The predictor, colour and palette images, from ROLE_PREDICTOR on.
    side: &'a [ImageContext],
    list: &'a [Transform],
    packed: i32,
    width: i32,
    out_width: i32,
    out_height: i32,
}

impl Inverse<'_> {
    fn side(&self, role: usize) -> &ImageContext {
        &self.side[role - ROLE_PREDICTOR]
    }

    /// Runs the inverse transforms over rows y0..y1 of `plane`, whose row y0
    /// starts at `base`. `scratch` carries the last predicted row from one
    /// batch to the next.
    fn rows(
        &self,
        plane: &mut [u32],
        scratch: &mut [u32],
        base: usize,
        stride: usize,
        y0: i32,
        y1: i32,
    ) -> Result<()> {
        let dsp = self.dsp;
        let mut reduced = self.packed;

        for &transform in self.list.iter().rev() {
            match transform {
                Transform::Predictor => {
                    let modes = self.side(ROLE_PREDICTOR);
                    let w = reduced as usize;

                    predict_batch(
                        dsp,
                        plane,
                        scratch,
                        base,
                        stride,
                        w,
                        self.width as usize,
                        modes,
                        y0,
                        y1,
                    )?;

                    let last = base + (y1 - 1 - y0) as usize * stride;

                    scratch[..w].copy_from_slice(&plane[last..][..w]);
                }
                Transform::Color => {
                    let mult = self.side(ROLE_COLOR);

                    transform::color_rows(
                        dsp,
                        plane,
                        base,
                        stride,
                        reduced as usize,
                        &mult.storage.data,
                        mult.storage.stride,
                        mult.size_reduction,
                        y0,
                        y1,
                    );
                }
                Transform::SubtractGreen => {
                    transform::subtract_green_rows(
                        dsp,
                        plane,
                        base,
                        stride,
                        reduced as usize,
                        y1 - y0,
                    );
                }
                Transform::ColorIndexing => {
                    let pal = self.side(ROLE_PALETTE);

                    transform::color_indexing_rows(
                        dsp,
                        plane,
                        base,
                        stride,
                        stride,
                        self.out_width as usize,
                        y1 - y0,
                        &pal.storage.data[..pal.storage.width as usize],
                        pal.size_reduction,
                        self.out_height as usize * self.out_width as usize > 300,
                    );
                    reduced = self.width;
                }
            }
        }
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(miri))]
    fn sparse_huffman_image(group: Option<u32>) -> Vec<u8> {
        fn put(bits: &mut Vec<u8>, value: u32, count: u32) {
            for i in 0..count {
                bits.push(((value >> i) & 1) as u8);
            }
        }

        fn simple_tree(bits: &mut Vec<u8>, symbol: u32) {
            put(bits, 1, 1);
            put(bits, 0, 1);
            put(bits, u32::from(symbol > 1), 1);
            put(bits, symbol, if symbol > 1 { 8 } else { 1 });
        }

        let mut bits = Vec::new();

        put(&mut bits, 0x2f, 8);
        put(&mut bits, 0, 14);
        put(&mut bits, 0, 14);
        put(&mut bits, 1, 1);
        put(&mut bits, 0, 3);
        put(&mut bits, 0, 1);
        put(&mut bits, 0, 1);
        put(&mut bits, u32::from(group.is_some()), 1);
        if let Some(group) = group {
            put(&mut bits, 0, 3);
            put(&mut bits, 0, 1);
            for symbol in [group & 255, group >> 8, 0, 255, 0] {
                simple_tree(&mut bits, symbol);
            }
        }
        for _ in 0..=group.unwrap_or(0) {
            for _ in 0..HUFFMAN_CODES_PER_META_CODE {
                simple_tree(&mut bits, 0);
            }
        }

        let mut data = vec![0u8; bits.len().div_ceil(8)];

        for (i, bit) in bits.into_iter().enumerate() {
            data[i / 8] |= bit << (i % 8);
        }
        data
    }

    #[test]
    #[cfg(not(miri))]
    fn sparse_huffman_groups_are_stored_densely() {
        let sparse_data = sparse_huffman_image(Some(u16::MAX.into()));
        let mut parsed = Decoder::new();
        let mut baseline = Decoder::new();
        let mut sparse = Decoder::new();

        parsed.read_frame_header(&sparse_data, false).unwrap();
        baseline
            .decode_frame(Target::Argb, &sparse_huffman_image(Some(0)), false, None)
            .unwrap();
        sparse
            .decode_frame(Target::Argb, &sparse_data, false, None)
            .unwrap();

        assert_eq!(
            sparse.picture(Target::Argb).data,
            baseline.picture(Target::Argb).data
        );
        assert_eq!(parsed.nb_huffman_groups, 1);
        assert_eq!(parsed.nb_huffman_group_codes, 1 << 16);
        assert!(parsed.huffman_groups_mapped);
        assert_eq!(parsed.image[ROLE_ARGB].groups.len(), 1);
        assert_eq!(parsed.huffman_group_map[u16::MAX as usize], 0);
    }

    #[test]
    #[cfg(not(miri))]
    fn reused_decoder_discards_the_previous_huffman_mapping() {
        let mut decoder = Decoder::new();
        let sparse = sparse_huffman_image(Some(u16::MAX.into()));
        let plain = sparse_huffman_image(None);

        for stale in [u32::MAX, 1] {
            decoder
                .decode_frame(Target::Argb, &sparse, false, None)
                .unwrap();
            assert!(decoder.huffman_groups_mapped);
            decoder.huffman_group_map[0] = stale;
            decoder
                .decode_frame(Target::Argb, &plain, false, None)
                .unwrap();
            assert!(!decoder.huffman_groups_mapped);
            assert_eq!(decoder.picture(Target::Argb).data[0], 0);
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn animation_reuses_slots_after_a_sparse_huffman_frame() {
        fn chunk(dst: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
            dst.extend_from_slice(tag);
            dst.extend_from_slice(&(data.len() as u32).to_le_bytes());
            dst.extend_from_slice(data);
            if data.len() & 1 != 0 {
                dst.push(0);
            }
        }

        let mut body = b"WEBP".to_vec();
        chunk(&mut body, b"VP8X", &[0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        chunk(&mut body, b"ANIM", &[0; 6]);
        for group in [Some(u16::MAX.into()), None, None, None, None, None] {
            let mut frame = vec![0; 16];
            frame[12] = 1;
            frame[15] = 2;
            chunk(&mut frame, b"VP8L", &sparse_huffman_image(group));
            chunk(&mut body, b"ANMF", &frame);
        }
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&body);
        for n_threads in [1, 2, 4] {
            let mut decoder = crate::api::Decoder::new();
            decoder
                .set_options(crate::options::Options {
                    n_threads,
                    ..Default::default()
                })
                .unwrap();
            decoder.open(&data).unwrap();
            for _ in 0..6 {
                let picture = decoder.next_frame().unwrap().unwrap();
                assert_eq!(picture.row(0, 0), &[0; 4]);
            }
            assert!(decoder.next_frame().unwrap().is_none());
        }
    }

    #[test]
    fn transform_batches_preserve_predictor_rows_in_every_transform_order() {
        use Transform::{Color, Predictor, SubtractGreen};
        let orders = [
            [Predictor, Color, SubtractGreen],
            [Predictor, SubtractGreen, Color],
            [Color, Predictor, SubtractGreen],
            [Color, SubtractGreen, Predictor],
            [SubtractGreen, Predictor, Color],
            [SubtractGreen, Color, Predictor],
        ];
        for order in orders {
            for count in [2, 3] {
                for (width, height) in
                    [(1, 33), (2, 17), (17, 15), (65, 16), (65, 17), (65, 33)]
                {
                    for target in [Target::Argb, Target::Alpha] {
                        let make = || {
                            let mut d = Decoder::new();
                            d.reduced_width = width;
                            d.transforms[..3].copy_from_slice(&order);
                            d.nb_transforms = count;
                            let pic =
                                target_picture(target, &mut d.argb, &mut d.alpha_argb);
                            pic.alloc(width, height).unwrap();
                            let mut v = 42u32;
                            for px in &mut pic.data {
                                v = v.wrapping_mul(1664525).wrapping_add(1013904223);
                                *px = v;
                            }
                            for role in [ROLE_PREDICTOR, ROLE_COLOR] {
                                let img = &mut d.image[role];
                                img.size_reduction = 2;
                                img.storage
                                    .alloc(ceil_shift(width, 2), ceil_shift(height, 2))
                                    .unwrap();
                                for (i, px) in img.storage.data.iter_mut().enumerate() {
                                    *px = if role == ROLE_PREDICTOR {
                                        u32::from_ne_bytes([0, 0, (i % 14) as u8, 0])
                                    } else {
                                        (i as u32).wrapping_mul(0x7313_fa19)
                                    };
                                }
                            }
                            d
                        };
                        let mut actual = make();
                        let mut expected = make();
                        for &t in order[..count].iter().rev() {
                            match t {
                                Predictor => expected.apply_predictor(target).unwrap(),
                                Color => expected.apply_color(target),
                                SubtractGreen => expected.apply_subtract_green(target),
                                _ => unreachable!(),
                            }
                        }
                        actual.apply_transforms(target, None).unwrap();
                        let a = target_picture(
                            target,
                            &mut actual.argb,
                            &mut actual.alpha_argb,
                        );
                        let b = target_picture(
                            target,
                            &mut expected.argb,
                            &mut expected.alpha_argb,
                        );
                        assert_eq!(
                            a.data, b.data,
                            "{order:?} {count} {width}x{height} {target:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn inverse_transforms_over_bands_match_the_whole_image() {
        use Transform::{Color, Predictor, SubtractGreen};
        let (width, height) = (37, 41);
        let make = || {
            let mut d = Decoder::new();
            d.width = width;
            d.height = height;
            d.reduced_width = width;
            d.transforms[..3].copy_from_slice(&[SubtractGreen, Predictor, Color]);
            d.nb_transforms = 3;
            d.argb.alloc(width, height).unwrap();
            let mut v = 7u32;
            for px in &mut d.argb.data {
                v = v.wrapping_mul(1664525).wrapping_add(1013904223);
                *px = v;
            }
            for role in [ROLE_PREDICTOR, ROLE_COLOR] {
                let img = &mut d.image[role];
                img.size_reduction = 2;
                img.storage
                    .alloc(ceil_shift(width, 2), ceil_shift(height, 2))
                    .unwrap();
                for (i, px) in img.storage.data.iter_mut().enumerate() {
                    *px = if role == ROLE_PREDICTOR {
                        u32::from_ne_bytes([0, 0, (i % 14) as u8, 0])
                    } else {
                        (i as u32).wrapping_mul(0x7313_fa19)
                    };
                }
            }
            d
        };
        let mut expected = make();
        expected.apply_transforms(Target::Argb, None).unwrap();

        /* Bands of every size the pipeline might hand over, each copied out
         * of the pixels and transformed where it lands, as the second
         * thread does. */
        for step in [1, 2, 5, 16, 41] {
            let mut d = make();
            d.still_alloc().unwrap();
            let Decoder {
                dsp,
                image,
                argb,
                out,
                scratch,
                transforms,
                nb_transforms,
                ..
            } = &mut d;
            let stride = out.stride;
            let xf = Inverse {
                dsp,
                side: &image[ROLE_PREDICTOR..],
                list: &transforms[..*nb_transforms],
                packed: width,
                width,
                out_width: width,
                out_height: height,
            };
            let mut rest = &mut out.data[..];
            let mut y0 = 0;
            while y0 < height {
                let y1 = (y0 + step).min(height);
                let (band, tail) =
                    std::mem::take(&mut rest).split_at_mut((y1 - y0) as usize * stride);
                rest = tail;
                copy_packed_rows(band, stride, argb, width, y0, y1);
                xf.rows(band, scratch, 0, stride, y0, y1).unwrap();
                y0 = y1;
            }
            let n = (width * height) as usize;
            assert_eq!(out.data[..n], expected.argb.data[..n], "bands of {step}");
        }
    }

    #[test]
    fn alpha_rows_by_green_match_the_full_transforms() {
        use Transform::{Color, Predictor, SubtractGreen};

        fn prefix(_: Option<&[u8]>, row: &mut [u8]) {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
        }

        fn down(above: Option<&[u8]>, row: &mut [u8]) {
            for (px, &up) in row.iter_mut().zip(above.unwrap()) {
                *px = px.wrapping_add(up);
            }
        }

        let (width, height) = (45, 75);
        let w = width as usize;
        let lists: [&[Transform]; 4] = [
            &[Predictor],
            &[SubtractGreen, Predictor],
            &[Color, SubtractGreen, Predictor],
            &[Predictor, Color],
        ];
        let unfilters = [
            None,
            Some(Unfilter {
                first: prefix,
                rest: down,
            }),
        ];

        for list in lists {
            for first_black in [false, true] {
                for mode_0 in [false, true] {
                    /* A residual with more than green, where the rows have to
                     * be predicted in full from then on. */
                    for stray in [
                        None,
                        Some((0, 5)),
                        Some((31, 44)),
                        Some((32, 0)),
                        Some((50, 3)),
                    ] {
                        let make = || {
                            let mut d = Decoder::new();
                            d.width = width;
                            d.height = height;
                            d.reduced_width = width;
                            d.transforms[..list.len()].copy_from_slice(list);
                            d.nb_transforms = list.len();
                            d.alpha_argb.alloc(width, height).unwrap();

                            let stride = d.alpha_argb.stride;
                            let mut v = 99u32;

                            for y in 0..height as usize {
                                for px in &mut d.alpha_argb.data[y * stride..][..w] {
                                    v = v
                                        .wrapping_mul(1664525)
                                        .wrapping_add(1013904223);
                                    /* Small steps, for ties and clamping. */
                                    let g = ((v >> 24) as u8 & 3).wrapping_sub(1);

                                    *px = u32::from_ne_bytes([0, 0, g, 0]);
                                }
                            }
                            /* As an encoder leaves it: an alpha of 0 less black's. */
                            if !first_black {
                                d.alpha_argb.data[0] |=
                                    u32::from_ne_bytes([1, 0, 0, 0]);
                            }
                            if let Some((y, x)) = stray {
                                d.alpha_argb.data[y * stride + x] |=
                                    u32::from_ne_bytes([0, 1, 0, 0]);
                            }
                            for role in [ROLE_PREDICTOR, ROLE_COLOR] {
                                let img = &mut d.image[role];
                                img.size_reduction = 2;
                                img.storage
                                    .alloc(ceil_shift(width, 2), ceil_shift(height, 2))
                                    .unwrap();
                                for (i, px) in img.storage.data.iter_mut().enumerate() {
                                    /* Half select, the one predictor a mode 0
                                     * tile's black would lead astray. */
                                    let mode = match i {
                                        30 if mode_0 => 0,
                                        _ if i % 2 == 0 => 11,
                                        _ => 1 + i % 13,
                                    };

                                    *px = if role == ROLE_PREDICTOR {
                                        u32::from_ne_bytes([0, 0, mode as u8, 0])
                                    } else {
                                        (i as u32).wrapping_mul(0x7313_fa19)
                                    };
                                }
                            }
                            d
                        };

                        for unfilter in unfilters {
                            let mut expected = make();
                            let mut want = vec![0u8; w * height as usize];

                            expected.apply_transforms(Target::Alpha, None).unwrap();
                            for (y, row) in want.chunks_exact_mut(w).enumerate() {
                                let stride = expected.alpha_argb.stride;

                                for (a, &px) in row
                                    .iter_mut()
                                    .zip(&expected.alpha_argb.data[y * stride..][..w])
                                {
                                    *a = px.to_ne_bytes()[2];
                                }
                            }
                            if let Some(u) = unfilter {
                                (u.first)(None, &mut want[..w]);
                                for y in 1..height as usize {
                                    let (above, here) =
                                        want[(y - 1) * w..].split_at_mut(w);

                                    (u.rest)(Some(above), &mut here[..w]);
                                }
                            }

                            let mut actual = make();
                            let mut got = vec![0u8; w * height as usize];

                            actual
                                .alpha_rows(AlphaDst {
                                    data: &mut got,
                                    stride: w,
                                    unfilter,
                                })
                                .unwrap();
                            assert_eq!(
                                got,
                                want,
                                "{list:?} first black {first_black}, mode 0 {mode_0}, \
                                 stray {stray:?}, unfilter {}",
                                unfilter.is_some()
                            );
                        }
                    }
                }
            }
        }
    }

    const WIDE: &[u8] = &[
        0x2f, 0x31, 0x1a, 0x8e, 0x1a, 0x8e, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0x40, 0x3e, 0x3e, 0x3e, 0x2f, 0x03,
    ];

    fn too_big_for_miri() -> bool {
        cfg!(miri)
    }

    #[test]
    fn a_reset_forgets_how_far_the_last_image_got() {
        if too_big_for_miri() {
            return;
        }

        let mut dec = Decoder::new();

        dec.set_canvas(39, 16);
        dec.decode_frame(Target::Argb, WIDE, false, None).unwrap();

        dec.reset();
        dec.set_canvas(39, 16);

        assert_eq!(dec.resume.rows_done, 0);
        assert_eq!(dec.rows_out, 0);
        assert_eq!(dec.reduced_width, 0);
    }

    #[test]
    fn an_error_mid_image_leaves_the_stride_matching_the_width() {
        if too_big_for_miri() {
            return;
        }

        let file = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/wpd-test-data/palette2bpp_rgb.webp"
        ))
        .unwrap();
        let payload = &file[20..];
        let mut failed = 0;

        /* Damage past the headers, so the image starts and then breaks. */
        for at in (payload.len() / 2..payload.len()).step_by(97) {
            let mut bad = payload.to_vec();

            for b in &mut bad[at..] {
                *b = !*b;
            }

            let mut dec = Decoder::new();

            dec.set_canvas(300, 200);
            if dec.still_step(&bad, bad.len(), true).is_err() {
                failed += 1;
                assert!(!dec.still_active());
            }
            assert_eq!(dec.argb.stride, dec.argb.width.max(0) as usize);
        }
        assert!(failed > 0, "no damaged copy failed mid-image");
    }

    #[test]
    fn peeking_with_no_image_in_progress_does_nothing() {
        if too_big_for_miri() {
            return;
        }

        let mut dec = Decoder::new();

        dec.set_canvas(39, 16);
        dec.decode_frame(Target::Argb, WIDE, false, None).unwrap();
        dec.set_canvas(39, 16);

        assert!(!dec.still_active());
        dec.still_peek().unwrap();
    }

    #[test]
    fn peeking_before_a_frame_header_does_nothing() {
        if too_big_for_miri() {
            return;
        }

        let mut dec = Decoder::new();

        dec.set_canvas(39, 16);

        assert_eq!(
            dec.still_step(&WIDE[..11], WIDE.len(), false),
            Ok(Status::NeedMore)
        );
        assert!(!dec.still_active());

        dec.still_peek().unwrap();

        assert_eq!(dec.still_step(WIDE, WIDE.len(), true), Ok(Status::Done));
    }
}
