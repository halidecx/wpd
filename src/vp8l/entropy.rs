use super::bitreader::{BitReader, TAIL_MARGIN};
use super::huffman::Tree;
use super::{
    HTreeGroup, Picture, Resume, HUFFMAN_CODES_PER_META_CODE, HUFF_IDX_ALPHA,
    HUFF_IDX_BLUE, HUFF_IDX_DIST, HUFF_IDX_GREEN, HUFF_IDX_RED, NUM_LENGTH_CODES,
    NUM_LITERAL_CODES, NUM_SHORT_DISTANCES,
};
use crate::error::{Error, Result, Status};

pub struct Entropy<'a> {
    pub data: &'a [u32],
    pub stride: usize,
    pub bits: u32,
}

#[inline(always)]
pub fn group_index(pixel: u32) -> u32 {
    let b = pixel.to_ne_bytes();

    u32::from(b[1]) << 8 | u32::from(b[2])
}

struct GroupMap<'a> {
    entropy: Option<Entropy<'a>>,
    bits: u32,
    mask: i32,
}

impl<'a> GroupMap<'a> {
    fn new(entropy: Option<Entropy<'a>>, groups: &[HTreeGroup]) -> Self {
        let bits = match (&entropy, groups.len() > 1) {
            (Some(e), true) => e.bits,
            _ => 0,
        };
        let mask = if bits != 0 { (1 << bits) - 1 } else { !0 };

        Self {
            entropy,
            bits,
            mask,
        }
    }

    #[inline(always)]
    fn at(&self, x: i32, y: i32) -> usize {
        match &self.entropy {
            Some(e) if self.bits != 0 => {
                let off = (y >> e.bits) as usize * e.stride + (x >> e.bits) as usize;

                group_index(e.data[off]) as usize
            }
            _ => 0,
        }
    }
}

pub struct Args<'a, 'e> {
    pub gb: &'a mut BitReader,
    pub buf: &'a [u8],
    pub pic: &'a mut Picture,
    pub groups: &'a [HTreeGroup],
    pub arena: &'a [u32],
    pub cache: &'a mut [u32],
    pub cache_bits: u32,
    pub reduced_width: Option<i32>,
    pub entropy: Option<Entropy<'e>>,
    pub st: &'a mut Resume,
    pub resumable: bool,
}

const LZ77_DISTANCE_OFFSETS: [[i8; 2]; NUM_SHORT_DISTANCES as usize] = [
    [0, 1],
    [1, 0],
    [1, 1],
    [-1, 1],
    [0, 2],
    [2, 0],
    [1, 2],
    [-1, 2],
    [2, 1],
    [-2, 1],
    [2, 2],
    [-2, 2],
    [0, 3],
    [3, 0],
    [1, 3],
    [-1, 3],
    [3, 1],
    [-3, 1],
    [2, 3],
    [-2, 3],
    [3, 2],
    [-3, 2],
    [0, 4],
    [4, 0],
    [1, 4],
    [-1, 4],
    [4, 1],
    [-4, 1],
    [3, 3],
    [-3, 3],
    [2, 4],
    [-2, 4],
    [4, 2],
    [-4, 2],
    [0, 5],
    [3, 4],
    [-3, 4],
    [4, 3],
    [-4, 3],
    [5, 0],
    [1, 5],
    [-1, 5],
    [5, 1],
    [-5, 1],
    [2, 5],
    [-2, 5],
    [5, 2],
    [-5, 2],
    [4, 4],
    [-4, 4],
    [3, 5],
    [-3, 5],
    [5, 3],
    [-5, 3],
    [0, 6],
    [6, 0],
    [1, 6],
    [-1, 6],
    [6, 1],
    [-6, 1],
    [2, 6],
    [-2, 6],
    [6, 2],
    [-6, 2],
    [4, 5],
    [-4, 5],
    [5, 4],
    [-5, 4],
    [3, 6],
    [-3, 6],
    [6, 3],
    [-6, 3],
    [0, 7],
    [7, 0],
    [1, 7],
    [-1, 7],
    [5, 5],
    [-5, 5],
    [7, 1],
    [-7, 1],
    [4, 6],
    [-4, 6],
    [6, 4],
    [-6, 4],
    [2, 7],
    [-2, 7],
    [7, 2],
    [-7, 2],
    [3, 7],
    [-3, 7],
    [7, 3],
    [-7, 3],
    [5, 6],
    [-5, 6],
    [6, 5],
    [-6, 5],
    [8, 0],
    [4, 7],
    [-4, 7],
    [7, 4],
    [-7, 4],
    [8, 1],
    [8, 2],
    [6, 6],
    [-6, 6],
    [8, 3],
    [5, 7],
    [-5, 7],
    [7, 5],
    [-7, 5],
    [8, 4],
    [6, 7],
    [-6, 7],
    [7, 6],
    [-7, 6],
    [8, 5],
    [7, 7],
    [-7, 7],
    [8, 6],
    [8, 7],
];

#[inline(always)]
fn cache_slot(value: u32, bits: u32) -> usize {
    (0x1E35_A7BDu32.wrapping_mul(value) >> (32 - bits)) as usize
}

#[inline(always)]
fn cache_value(pixel: u32) -> u32 {
    u32::from_be_bytes(pixel.to_ne_bytes())
}

#[inline(always)]
fn cache_fill(
    cache: &mut [u32],
    bits: u32,
    pixels: &[u32],
    from: usize,
    to: usize,
) -> usize {
    if from >= to {
        return from;
    }
    for &px in &pixels[from..to] {
        let v = cache_value(px);

        cache[cache_slot(v, bits)] = v;
    }
    to
}

fn copy_block<T: Copy>(pixels: &mut [T], pos: usize, dist: usize, length: usize) {
    if dist >= length {
        let (done, rest) = pixels.split_at_mut(pos);

        rest[..length].copy_from_slice(&done[pos - dist..][..length]);
        return;
    }
    if dist == 1 {
        let v = pixels[pos - 1];

        pixels[pos..][..length].fill(v);
        return;
    }

    let mut i = 0;

    while i < length {
        let step = dist.min(length - i);
        let (done, rest) = pixels.split_at_mut(pos + i);

        rest[..step].copy_from_slice(&done[pos + i - dist..][..step]);
        i += step;
    }
}

#[inline(always)]
fn resolve<'a>(
    hg: &HTreeGroup,
    arena: &'a [u32],
) -> [Tree<'a>; HUFFMAN_CODES_PER_META_CODE] {
    [
        hg.trees[0].tree(arena),
        hg.trees[1].tree(arena),
        hg.trees[2].tree(arena),
        hg.trees[3].tree(arena),
        hg.trees[4].tree(arena),
    ]
}

pub fn decode_pixels(args: Args<'_, '_>) -> Result<Status> {
    if args.resumable {
        run::<true>(args)
    } else {
        run::<false>(args)
    }
}

fn run<const RESUMABLE: bool>(args: Args<'_, '_>) -> Result<Status> {
    let Args {
        gb,
        buf,
        pic,
        groups,
        arena,
        cache,
        cache_bits,
        reduced_width,
        entropy,
        st,
        ..
    } = args;

    let mut width = pic.width.max(0) as usize;

    if let Some(reduced) = reduced_width {
        let reduced = reduced.max(0) as usize;

        if reduced < width {
            width = reduced;
            pic.stride = width;
        }
    }

    let total = width * pic.height.max(0) as usize;
    let pixels = &mut pic.data[..total];
    let multi_group = groups.len() > 1;
    let map = GroupMap::new(entropy, groups);
    let huff_mask = map.mask;

    let mut pos = 0usize;
    let mut cached = 0usize;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut hgi = 0usize;

    if RESUMABLE {
        pos = st.pos;
        cached = st.cached;
        x = st.x;
        y = st.y;
        hgi = st.hg;
    }

    let mut hg = &groups[hgi];
    let mut trees = resolve(hg, arena);
    let mut snap = *gb;
    let mut near = false;

    macro_rules! suspend {
        () => {{
            *gb = snap;
            st.pos = pos;
            st.cached = cached;
            st.x = x;
            st.y = y;
            st.hg = hgi;
            st.rows_done = y;
            return Ok(Status::NeedMore);
        }};
    }

    while pos < total {
        if !RESUMABLE && gb.is_eos(buf) {
            return Err(Error::InvalidData);
        }
        if RESUMABLE {
            near = gb.left(buf) <= TAIL_MARGIN;
            if near {
                snap = *gb;
            }
        }

        if x & huff_mask == 0 {
            hgi = map.at(x, y);
            hg = &groups[hgi];
            trees = resolve(hg, arena);
        }
        gb.fill(buf);

        let v = trees[HUFF_IDX_GREEN].read(gb);

        if v < NUM_LITERAL_CODES {
            let mut px;

            if hg.trivial_literal {
                if RESUMABLE && near && gb.is_eos(buf) {
                    suspend!();
                }
                px = hg.literal;
                px[2] = v as u8;
            } else {
                let r = trees[HUFF_IDX_RED].read(gb);

                gb.fill(buf);

                let b = trees[HUFF_IDX_BLUE].read(gb);
                let a = trees[HUFF_IDX_ALPHA].read(gb);

                if RESUMABLE && near && gb.is_eos(buf) {
                    suspend!();
                }
                px = [a as u8, r as u8, v as u8, b as u8];
            }
            pixels[pos] = u32::from_ne_bytes(px);
            pos += 1;
            x += 1;
            if x == width as i32 {
                x = 0;
                y += 1;
                if cache_bits != 0 {
                    cached = cache_fill(cache, cache_bits, pixels, cached, pos);
                }
            }
        } else if v < NUM_LITERAL_CODES + NUM_LENGTH_CODES {
            let prefix = v - NUM_LITERAL_CODES;
            let length = extend(gb, buf, prefix);
            let prefix = trees[HUFF_IDX_DIST].read(gb);

            gb.fill(buf);
            if prefix > 39 {
                crate::log::error_args(format_args!(
                    "distance prefix code too large: {prefix}"
                ));
                return Err(Error::InvalidData);
            }

            let coded = extend(gb, buf, prefix);

            if RESUMABLE && near && gb.is_eos(buf) {
                suspend!();
            }

            let distance = if coded <= NUM_SHORT_DISTANCES {
                let [xi, yi] = LZ77_DISTANCE_OFFSETS[coded as usize - 1];

                (i32::from(xi) + i32::from(yi) * width as i32).max(1) as usize
            } else {
                (coded - NUM_SHORT_DISTANCES) as usize
            };
            let length = length as usize;

            if distance > pos || length > total - pos {
                return Err(Error::InvalidData);
            }

            copy_block(pixels, pos, distance, length);
            pos += length;
            x += length as i32;
            while x >= width as i32 {
                x -= width as i32;
                y += 1;
            }
            if multi_group && x & huff_mask != 0 {
                hgi = map.at(x, y);
                hg = &groups[hgi];
                trees = resolve(hg, arena);
            }
            if cache_bits != 0 {
                cached = cache_fill(cache, cache_bits, pixels, cached, pos);
            }
        } else {
            let slot = (v - (NUM_LITERAL_CODES + NUM_LENGTH_CODES)) as usize;

            if RESUMABLE && near && gb.is_eos(buf) {
                suspend!();
            }
            if cache_bits == 0 {
                crate::log::error("color cache not found");
                return Err(Error::InvalidData);
            }
            if slot >= 1 << cache_bits {
                crate::log::error("color cache index out-of-bounds");
                return Err(Error::InvalidData);
            }
            cached = cache_fill(cache, cache_bits, pixels, cached, pos);
            pixels[pos] = u32::from_ne_bytes(cache[slot].to_be_bytes());
            pos += 1;
            x += 1;
            if x == width as i32 {
                x = 0;
                y += 1;
            }
        }
    }
    /* Match libwebp: only complete-input overruns are errors here. */
    if !RESUMABLE && gb.is_eos(buf) {
        crate::log::error("image data runs past the end of the chunk");
        return Err(Error::InvalidData);
    }
    st.rows_done = y;
    Ok(Status::Done)
}

pub struct AlphaArgs<'a, 'e> {
    pub gb: &'a mut BitReader,
    pub buf: &'a [u8],
    pub pixels: &'a mut [u8],
    pub width: usize,
    pub groups: &'a [HTreeGroup],
    pub arena: &'a [u32],
    pub entropy: Option<Entropy<'e>>,
}

pub fn decode_alpha_pixels(args: AlphaArgs<'_, '_>) -> Result<()> {
    let AlphaArgs {
        gb,
        buf,
        pixels,
        width,
        groups,
        arena,
        entropy,
    } = args;

    let total = pixels.len();
    let multi_group = groups.len() > 1;
    let map = GroupMap::new(entropy, groups);
    let huff_mask = map.mask;

    let mut pos = 0usize;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut trees = resolve(&groups[0], arena);

    while pos < total {
        if gb.is_eos(buf) {
            return Err(Error::InvalidData);
        }
        if x & huff_mask == 0 {
            trees = resolve(&groups[map.at(x, y)], arena);
        }
        gb.fill(buf);

        let v = trees[HUFF_IDX_GREEN].read(gb);

        if v < NUM_LITERAL_CODES {
            pixels[pos] = v as u8;
            pos += 1;
            x += 1;
            if x == width as i32 {
                x = 0;
                y += 1;
            }
        } else if v < NUM_LITERAL_CODES + NUM_LENGTH_CODES {
            let prefix = v - NUM_LITERAL_CODES;
            let length = extend(gb, buf, prefix);
            let prefix = trees[HUFF_IDX_DIST].read(gb);

            gb.fill(buf);
            if prefix > 39 {
                crate::log::error_args(format_args!(
                    "distance prefix code too large: {prefix}"
                ));
                return Err(Error::InvalidData);
            }

            let coded = extend(gb, buf, prefix);
            let distance = if coded <= NUM_SHORT_DISTANCES {
                let [xi, yi] = LZ77_DISTANCE_OFFSETS[coded as usize - 1];

                (i32::from(xi) + i32::from(yi) * width as i32).max(1) as usize
            } else {
                (coded - NUM_SHORT_DISTANCES) as usize
            };
            let length = length as usize;

            if distance > pos || length > total - pos {
                return Err(Error::InvalidData);
            }

            copy_block(pixels, pos, distance, length);
            pos += length;
            x += length as i32;
            while x >= width as i32 {
                x -= width as i32;
                y += 1;
            }
            if multi_group && x & huff_mask != 0 {
                trees = resolve(&groups[map.at(x, y)], arena);
            }
        } else {
            crate::log::error("color cache not found");
            return Err(Error::InvalidData);
        }
    }
    if gb.is_eos(buf) {
        crate::log::error("alpha data runs past the end of the chunk");
        return Err(Error::InvalidData);
    }
    Ok(())
}

#[inline(always)]
fn extend(gb: &mut BitReader, buf: &[u8], prefix: u32) -> u32 {
    if prefix < 4 {
        return prefix + 1;
    }
    let extra_bits = (prefix - 2) >> 1;
    let offset = (2 + (prefix & 1)) << extra_bits;

    offset + gb.bits(buf, extra_bits) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_that_reaches_back_further_than_it_is_long_is_a_copy() {
        let mut px = [1u32, 2, 3, 4, 0, 0];

        copy_block(&mut px, 4, 4, 2);
        assert_eq!(px, [1, 2, 3, 4, 1, 2]);
    }

    #[test]
    fn a_one_pixel_run_repeats() {
        let mut px = [7u32, 0, 0, 0];

        copy_block(&mut px, 1, 1, 3);
        assert_eq!(px, [7, 7, 7, 7]);
    }

    #[test]
    fn an_overlapping_reference_repeats_its_own_output() {
        let mut px = [1u32, 2, 3, 0, 0, 0, 0, 0];

        copy_block(&mut px, 3, 3, 5);
        assert_eq!(px, [1, 2, 3, 1, 2, 3, 1, 2]);
    }
}
