pub mod rac;
pub mod tables;

use crate::bits::{rl16, rl24};
use crate::dsp::vp8::Vp8Dsp;
use crate::dsp::vp8pred::{self as pred, Vp8Pred};
use rac::{RangeCoder, TokenCoder};
use tables::*;

const NUM_DCT_TOKENS: usize = 12;
const MAX_PARTITIONS: usize = 8;

const PLANE_ROW_PAD: usize = 32;
const PLANE_COL_PAD: usize = 64;
const ALIGN: usize = 64;

pub use crate::error::{check_image_size, Error, Result, Status};

fn clip_uintp2(value: i32, bits: u32) -> i32 {
    value.clamp(0, (1 << bits) - 1)
}

type Planes<'a> = [&'a mut [u8]; 3];

#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub stride: usize,
    pub origin: usize,
    base: usize,
    len: usize,
}

impl Plane {
    fn stride_for(width: usize) -> usize {
        let stride = (width + PLANE_COL_PAD + ALIGN - 1) & !(ALIGN - 1);

        if stride % 1024 == 0 {
            stride + ALIGN
        } else {
            stride
        }
    }

    #[inline(always)]
    fn at(&self, x: usize, y: usize) -> usize {
        self.origin + y * self.stride + x
    }
}

#[derive(Default)]
pub struct Picture {
    data: Vec<u8>,
    pub planes: [Plane; 3],
    ready: bool,
}

impl Picture {
    fn alloc(&mut self, width: usize, height: usize) -> Result<()> {
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);
        let mut planes = [Plane::default(); 3];
        let mut total = ALIGN;

        for (p, &(w, h)) in [(width, height), (cw, ch), (cw, ch)].iter().enumerate() {
            let stride = Plane::stride_for(w);
            let len = (h + 2 * PLANE_ROW_PAD)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(2 * ALIGN))
                .and_then(|n| n.checked_next_multiple_of(ALIGN))
                .ok_or(Error::TooLarge)?;

            planes[p] = Plane {
                stride,
                origin: PLANE_ROW_PAD * stride + PLANE_COL_PAD,
                base: total,
                len,
            };
            total = total.checked_add(len).ok_or(Error::TooLarge)?;
        }

        if self.data.len() < total {
            self.data.clear();
            self.data
                .try_reserve_exact(total)
                .map_err(|_| Error::NoMemory)?;
            self.data.resize(total, 0);
        } else {
            self.data[..total].fill(0);
        }

        let pad = self.data.as_ptr() as usize % ALIGN;
        let pad = (ALIGN - pad) % ALIGN;

        for plane in &mut planes {
            plane.base = plane.base - ALIGN + pad;
        }
        self.planes = planes;
        self.ready = true;
        Ok(())
    }

    fn invalidate(&mut self) {
        self.planes = [Plane::default(); 3];
        self.ready = false;
    }

    fn allocated(&self) -> bool {
        self.ready
    }

    #[inline(always)]
    pub fn plane(&self, p: usize) -> &[u8] {
        &self.data[self.planes[p].base..][..self.planes[p].len]
    }
}

#[derive(Clone, Copy, Default)]
struct FilterStrength {
    filter_level: u8,
    inner_limit: u8,
    inner_filter: bool,
}

impl FilterStrength {
    #[inline(always)]
    fn limits(self) -> Option<(i32, i32)> {
        let level = i32::from(self.filter_level);

        if level == 0 {
            return None;
        }
        let inner = i32::from(self.inner_limit);

        Some((inner, 2 * level + inner))
    }
}

/// A macroblock as parsing leaves it for reconstruction: its modes, which of
/// its blocks carry coefficients, and the coefficients. Reconstruction clears
/// every block it uses, so one parsed after it starts from zeroes again.
#[derive(Default)]
struct Macroblock {
    block: Blocks,
    non_zero_count_cache: [[u8; 4]; 6],
    intra4x4_pred_mode_mb: [u8; 16],
    skip: bool,
    mode: usize,
    segment: usize,
    chroma_pred_mode: usize,
}

#[derive(Clone, Copy, Default)]
struct Segmentation {
    enabled: bool,
    absolute_vals: bool,
    update_map: bool,
    base_quant: [i8; 4],
    filter_level: [i8; 4],
}

#[derive(Clone, Copy, Default)]
struct Filter {
    simple: bool,
    level: u8,
    sharpness: u8,
}

#[derive(Clone, Copy, Default)]
struct LfDelta {
    enabled: bool,
    ref_intra: i32,
    mode_i4: i32,
}

#[derive(Clone, Copy, Default)]
struct QMat {
    luma_qmul: [i16; 2],
    luma_dc_qmul: [i16; 2],
    chroma_qmul: [i16; 2],
}

struct Probs {
    segmentid: [u8; 3],
    mbskip: u8,
}

impl Default for Probs {
    fn default() -> Self {
        Self {
            segmentid: [255; 3],
            mbskip: 0,
        }
    }
}

#[derive(Default)]
#[repr(C, align(16))]
struct Blocks([[i16; 16]; 24]);

#[derive(Default)]
#[repr(C, align(16))]
struct BlockDc([i16; 16]);

#[derive(Clone, Copy)]
struct ResumeState {
    c: RangeCoder,
    part: RangeCoder,
    intra4x4_top: [u8; 4],
    intra4x4_left: [u8; 4],
    top_nnz: u16,
    left_nnz: u16,
}

/// A row of macroblocks parsed ahead of its reconstruction.
#[derive(Default)]
struct MbRow {
    mb_y: usize,
    /// Whether the first partition ran dry in this row, for the coefficients
    /// when they are parsed apart from the modes.
    modes_dry: bool,
    /// False for a row a partition ran dry in, which is reconstructed as the
    /// serial decode would but neither filtered nor followed.
    filter: bool,
    /// Whether the row came back to the parser with its reconstruction still
    /// to do, rather than never used or already done.
    pending: bool,
    mbs: Vec<Macroblock>,
}

/// Rows a parser may run ahead of reconstruction by. Eight did no better.
const RELAY_ROWS: usize = 4;

/// On two threads the coefficients get one to themselves, the modes and
/// reconstruction sharing the other, when they carry at least this many
/// tenths of a compressed byte a macroblock; below it reconstruction gets the
/// thread instead. Parsing them costs about 23ns a byte and 60ns a macroblock
/// and reconstruction 150-250ns a macroblock. Measured, coefficients alone
/// against reconstruction alone: 0.90x at 5.0 bytes (3072x3072), 0.99x at 6.0,
/// 1.03x at 6.9, 1.10x at 8.1 and 1.2-1.27x at 14-24 (Apple M-series, min of
/// 21).
const COEFF_HEAVY_TENTHS_PER_MB: u64 = 65;

/// Below this a frame is decoded on one thread: the relay's threads cost
/// about 20us to start, which a lossy 192x192 still does not make back (0.96x
/// the time on one thread) and a 224x224 one barely does (1.05x), while
/// 256x256 is 1.09-1.18x faster (Apple M-series, 3 threads, min of 21-31).
///
/// A frame one macroblock wide stays on one thread whatever its size, as each
/// row it hands on is a single macroblock: 16x4096 was 0.93x and 16x8192
/// 0.99x on three threads, and 16x4096 0.96x on two, while two macroblocks
/// already pay, 32x2048 1.06x and 32x4096 1.14x (min of 21-31).
const RELAY_PIXELS: usize = 256 * 256;

/// What reconstruction and the loop filter read and write, apart from the
/// parser's state, so the two can run on different threads.
#[derive(Default)]
struct Recon {
    dsp: Vp8Dsp,
    pred: Vp8Pred,
    planes: [Plane; 3],
    mb_width: usize,
    mb_height: usize,
    deblock_filter: bool,
    filter: Filter,
    filter_levels: [[FilterStrength; 2]; 4],
    /* Two rows, by mb_y parity: a row is filtered while the next one is
     * decoded. */
    filter_strength: Vec<FilterStrength>,
}

/// What parsing the coefficients reads and writes, apart from the modes, so
/// the two can run on different threads.
#[derive(Default)]
struct Coeffs {
    /* Its own table, for the WHT it runs on the DC coefficients. */
    dsp: Vp8Dsp,
    qmat: [QMat; 4],
    token: [[[[u8; NUM_DCT_TOKENS - 1]; 3]; 16]; 4],
    top_nnz: Vec<u16>,
    left_nnz: u16,
    block_dc: BlockDc,
    coeff_partition: [RangeCoder; MAX_PARTITIONS],
}

#[derive(Default)]
pub struct Decoder {
    coeffs: Coeffs,
    recon: Recon,

    pub picture: Picture,
    pub width: i32,
    pub height: i32,
    pub bypass_filtering: bool,
    /// Match the full-width intermediates of libwebp's portable C transforms.
    pub libwebp_compat: bool,
    strict_dsp: Option<(Vp8Dsp, Vp8Dsp)>,

    mb_width: usize,
    mb_height: usize,

    mbskip_enabled: bool,
    profile: u8,

    segmentation: Segmentation,
    lf_delta: LfDelta,

    intra4x4_pred_mode_top: Vec<u8>,
    intra4x4_pred_mode_left: [u8; 4],

    prob: Probs,
    rows: Vec<MbRow>,

    c: RangeCoder,
    num_coeff_partitions: usize,
    partition_start: [usize; MAX_PARTITIONS],
    partition_size: [usize; MAX_PARTITIONS],
    partition_ready: u8,
    partition_clamped: u8,

    mb_x: usize,
    mb_y: usize,
    mb_rows_done: usize,
    chunk_avail: usize,
    chunk_size: usize,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn update_dimensions(&mut self, width: i32, height: i32) -> Result<()> {
        check_image_size(width, height).map_err(|_| Error::InvalidData)?;

        if width == self.width
            && height == self.height
            && self.picture.allocated()
            && !self.recon.filter_strength.is_empty()
        {
            return Ok(());
        }

        let mb_width = (width as usize).div_ceil(16);
        let mb_height = (height as usize).div_ceil(16);
        let mut filter_strength = Vec::new();
        let mut intra4x4_pred_mode_top = Vec::new();
        let mut top_nnz = Vec::new();

        filter_strength
            .try_reserve_exact(2 * mb_width)
            .map_err(|_| Error::NoMemory)?;
        intra4x4_pred_mode_top
            .try_reserve_exact(mb_width * 4)
            .map_err(|_| Error::NoMemory)?;
        top_nnz
            .try_reserve_exact(mb_width)
            .map_err(|_| Error::NoMemory)?;

        filter_strength.resize(2 * mb_width, FilterStrength::default());
        intra4x4_pred_mode_top.resize(mb_width * 4, 0);
        top_nnz.resize(mb_width, 0);

        self.width = width;
        self.height = height;
        self.mb_width = mb_width;
        self.mb_height = mb_height;

        self.picture.invalidate();
        self.recon.mb_width = mb_width;
        self.recon.mb_height = mb_height;
        self.recon.filter_strength = filter_strength;
        self.intra4x4_pred_mode_top = intra4x4_pred_mode_top;
        self.coeffs.top_nnz = top_nnz;
        Ok(())
    }

    fn parse_segment_info(&mut self, buf: &[u8]) {
        self.segmentation.update_map = self.c.get(buf) != 0;

        if self.c.get(buf) != 0 {
            self.segmentation.absolute_vals = self.c.get(buf) != 0;

            for i in 0..4 {
                self.segmentation.base_quant[i] = self.c.get_sint(buf, 7) as i8;
            }
            for i in 0..4 {
                self.segmentation.filter_level[i] = self.c.get_sint(buf, 6) as i8;
            }
        }
        if self.segmentation.update_map {
            for i in 0..3 {
                self.prob.segmentid[i] = if self.c.get(buf) != 0 {
                    self.c.get_uint(buf, 8) as u8
                } else {
                    255
                };
            }
        }
    }

    fn update_lf_deltas(&mut self, buf: &[u8]) {
        for i in 0..8 {
            if self.c.get(buf) != 0 {
                let mut delta = self.c.get_uint(buf, 6);

                if self.c.get(buf) != 0 {
                    delta = -delta;
                }
                if i == 0 {
                    self.lf_delta.ref_intra = delta;
                } else if i == 4 {
                    self.lf_delta.mode_i4 = delta;
                }
            }
        }
    }

    fn setup_partitions(
        &mut self,
        buf: &[u8],
        table: usize,
        avail: usize,
        total: usize,
    ) -> Result<Status> {
        let n = 1usize << self.c.get_uint(buf, 2);

        self.num_coeff_partitions = n;

        let sizes_len = 3 * (n - 1);

        if total.saturating_sub(table) < sizes_len {
            return Err(Error::InvalidData);
        }
        if avail.saturating_sub(table) < sizes_len {
            return Ok(Status::NeedMore);
        }

        let mut off = table + sizes_len;

        for i in 0..n - 1 {
            let size = rl24(&buf[table + 3 * i..]) as usize;

            if total.saturating_sub(off) < size {
                return Err(Error::InvalidData);
            }
            self.partition_start[i] = off;
            self.partition_size[i] = size;
            off += size;
        }
        self.partition_start[n - 1] = off;
        self.partition_size[n - 1] = total - off;

        self.partition_ready = 0;
        self.partition_clamped = 0;
        Ok(Status::Done)
    }

    fn open_partitions(&mut self, buf: &[u8]) {
        let init_bytes: i64 = if rac::RAC_64 { 0 } else { 3 };

        for i in 0..self.num_coeff_partitions {
            let start = self.partition_start[i];
            let size = self.partition_size[i];
            /* Keep this signed so an unavailable partition stays unopened. */
            let have = self.chunk_avail as i64 - start as i64;
            let win = if have >= size as i64 {
                size
            } else if have >= init_bytes {
                have as usize
            } else {
                continue;
            };

            let coder = &mut self.coeffs.coeff_partition[i];

            if self.partition_ready & (1 << i) == 0 {
                *coder = RangeCoder::start(buf, start, win);
                self.partition_ready |= 1 << i;
            } else if coder.end() != start + win {
                coder.extend(start + win);
            }

            if win < size {
                self.partition_clamped |= 1 << i;
            } else {
                self.partition_clamped &= !(1 << i);
            }
        }
    }

    fn get_quants(&mut self, buf: &[u8]) {
        let yac_qi = self.c.get_uint(buf, 7);
        let ydc_delta = self.c.get_sint(buf, 4);
        let y2dc_delta = self.c.get_sint(buf, 4);
        let y2ac_delta = self.c.get_sint(buf, 4);
        let uvdc_delta = self.c.get_sint(buf, 4);
        let uvac_delta = self.c.get_sint(buf, 4);

        for i in 0..4 {
            let base_qi = if self.segmentation.enabled {
                let base = i32::from(self.segmentation.base_quant[i]);

                if self.segmentation.absolute_vals {
                    base
                } else {
                    base + yac_qi
                }
            } else {
                yac_qi
            };
            let dc =
                |d: i32| i16::from(DC_QLOOKUP[clip_uintp2(base_qi + d, 7) as usize]);
            let ac = |d: i32| AC_QLOOKUP[clip_uintp2(base_qi + d, 7) as usize] as i32;
            let q = &mut self.coeffs.qmat[i];

            q.luma_qmul[0] = dc(ydc_delta);
            q.luma_qmul[1] = ac(0) as i16;
            q.luma_dc_qmul[0] = 2 * dc(y2dc_delta);
            q.luma_dc_qmul[1] = ((ac(y2ac_delta) * 101581) >> 16) as i16;
            q.chroma_qmul[0] = dc(uvdc_delta);
            q.chroma_qmul[1] = ac(uvac_delta) as i16;

            q.luma_dc_qmul[1] = q.luma_dc_qmul[1].max(8);
            q.chroma_qmul[0] = q.chroma_qmul[0].min(132);
        }
    }

    fn get_filter_strengths(&mut self) {
        for segment in 0..4 {
            let base = if self.segmentation.enabled {
                let level = i32::from(self.segmentation.filter_level[segment]);

                if self.segmentation.absolute_vals {
                    level
                } else {
                    level + i32::from(self.recon.filter.level)
                }
            } else {
                i32::from(self.recon.filter.level)
            };

            for i4 in 0..2 {
                let mut filter_level = base;

                if self.lf_delta.enabled {
                    filter_level += self.lf_delta.ref_intra;
                    if i4 == 1 {
                        filter_level += self.lf_delta.mode_i4;
                    }
                }

                let filter_level = clip_uintp2(filter_level, 6);
                let mut interior_limit = filter_level;

                let sharpness = i32::from(self.recon.filter.sharpness);

                if sharpness != 0 {
                    interior_limit >>= (sharpness + 3) >> 2;
                    interior_limit = interior_limit.min(9 - sharpness);
                }
                interior_limit = interior_limit.max(1);

                self.recon.filter_levels[segment][i4] = FilterStrength {
                    filter_level: filter_level as u8,
                    inner_limit: interior_limit as u8,
                    inner_filter: i4 == 1,
                };
            }
        }
    }

    fn decode_frame_header(
        &mut self,
        buf: &[u8],
        avail: usize,
        total: usize,
    ) -> Result<Status> {
        if buf[0] & 1 != 0 {
            crate::log::error("Not a keyframe");
            return Err(Error::InvalidData);
        }
        self.profile = (buf[0] >> 1) & 7;

        let header_size = (rl24(buf) >> 5) as usize;

        /* Check the keyframe too, as libwebp's VP8GetHeaders does. */
        if self.profile > 3 {
            crate::log::error_args(format_args!("Unknown profile {}", self.profile));
            return Err(Error::InvalidData);
        }
        if buf[0] >> 4 & 1 == 0 {
            crate::log::error("Frame is not displayable");
            return Err(Error::Unsupported);
        }
        if header_size > total.saturating_sub(10) {
            crate::log::error("Header size larger than data provided");
            return Err(Error::InvalidData);
        }
        if avail.saturating_sub(10) < header_size {
            return Ok(Status::NeedMore);
        }
        if rl24(&buf[3..]) != 0x002a_019d {
            crate::log::error_args(format_args!(
                "Invalid start code 0x{:x}",
                rl24(&buf[3..])
            ));
            return Err(Error::InvalidData);
        }

        let width = (rl16(&buf[6..]) & 0x3fff) as i32;
        let height = (rl16(&buf[8..]) & 0x3fff) as i32;
        let hscale = buf[7] >> 6;
        let vscale = buf[9] >> 6;

        if hscale != 0 || vscale != 0 {
            crate::log::warning("Upscaling is not supported");
        }

        for (plane, defaults) in self.coeffs.token.iter_mut().zip(&TOKEN_DEFAULT_PROBS)
        {
            for (band, probs) in plane.iter_mut().zip(&COEFF_BAND) {
                *band = defaults[*probs as usize];
            }
        }
        self.segmentation = Segmentation::default();
        self.lf_delta = LfDelta::default();

        self.update_dimensions(width, height)?;
        self.c = RangeCoder::start(buf, 10, header_size);

        if self.c.get(buf) != 0 {
            crate::log::warning("Unspecified colorspace");
        }
        self.c.get(buf);

        self.segmentation.enabled = self.c.get(buf) != 0;
        if self.segmentation.enabled {
            self.parse_segment_info(buf);
        } else {
            self.segmentation.update_map = false;
        }

        self.recon.filter.simple = self.c.get(buf) != 0;
        self.recon.filter.level = self.c.get_uint(buf, 6) as u8;
        self.recon.filter.sharpness = self.c.get_uint(buf, 3) as u8;

        self.lf_delta.enabled = self.c.get(buf) != 0;
        if self.lf_delta.enabled && self.c.get(buf) != 0 {
            self.update_lf_deltas(buf);
        }
        self.get_filter_strengths();

        match self.setup_partitions(buf, 10 + header_size, avail, total) {
            Err(e) => {
                crate::log::error("Invalid partitions");
                return Err(e);
            }
            Ok(Status::NeedMore) => return Ok(Status::NeedMore),
            Ok(Status::Done) => {}
        }

        self.get_quants(buf);
        self.c.get(buf);

        for (i, plane) in TOKEN_UPDATE_PROBS.iter().enumerate() {
            for (j, band) in plane.iter().enumerate() {
                for (k, ctx) in band.iter().enumerate() {
                    for (l, &update) in ctx.iter().enumerate() {
                        if !self.c.get_prob_branchy(buf, update) {
                            continue;
                        }
                        let prob = self.c.get_uint(buf, 8) as u8;

                        for &index in &COEFF_BAND_INDEXES[j] {
                            if index < 0 {
                                break;
                            }
                            self.coeffs.token[i][index as usize][k][l] = prob;
                        }
                    }
                }
            }
        }

        self.mbskip_enabled = self.c.get(buf) != 0;
        if self.mbskip_enabled {
            self.prob.mbskip = self.c.get_uint(buf, 8) as u8;
        }
        Ok(Status::Done)
    }

    #[inline(always)]
    fn decode_intra4x4_modes(&mut self, buf: &[u8], mb: &mut Macroblock, mb_x: usize) {
        let top: &mut [u8; 4] = (&mut self.intra4x4_pred_mode_top[4 * mb_x..][..4])
            .try_into()
            .unwrap();
        let mut t = *top;

        for y in 0..4 {
            let mut mode = self.intra4x4_pred_mode_left[y];

            for x in 0..4 {
                let ctx = &PRED4X4_PROB_INTRA[usize::from(t[x])][usize::from(mode)];

                mode = read_pred4x4_mode(&mut self.c, buf, ctx);
                t[x] = mode;
            }
            mb.intra4x4_pred_mode_mb[4 * y..4 * y + 4].copy_from_slice(&t);
            self.intra4x4_pred_mode_left[y] = mode;
        }
        *top = t;
    }

    #[inline(always)]
    fn decode_mb_mode(&mut self, buf: &[u8], mb: &mut Macroblock, mb_x: usize) {
        let c = &mut self.c;

        mb.segment = if !self.segmentation.update_map {
            0
        } else if !c.get_prob_branchy(buf, self.prob.segmentid[0]) {
            c.get_prob(buf, self.prob.segmentid[1]) as usize
        } else {
            2 + c.get_prob(buf, self.prob.segmentid[2]) as usize
        };

        mb.skip = self.mbskip_enabled && c.get_prob(buf, self.prob.mbskip) != 0;
        mb.mode = if !c.get_prob_branchy(buf, PRED16X16_PROB_INTRA[0]) {
            MODE_I4
        } else if !c.get_prob_branchy(buf, PRED16X16_PROB_INTRA[1]) {
            if !c.get_prob_branchy(buf, PRED16X16_PROB_INTRA[2]) {
                pred::DC_PRED8X8
            } else {
                pred::VERT_PRED8X8
            }
        } else if !c.get_prob_branchy(buf, PRED16X16_PROB_INTRA[3]) {
            pred::HOR_PRED8X8
        } else {
            pred::PLANE_PRED8X8
        };

        if mb.mode == MODE_I4 {
            self.decode_intra4x4_modes(buf, mb, mb_x);
        } else {
            let mode = PRED4X4_MODE[mb.mode] as u8;

            self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4].fill(mode);
            self.intra4x4_pred_mode_left.fill(mode);
        }

        let c = &mut self.c;

        mb.chroma_pred_mode = if !c.get_prob_branchy(buf, PRED8X8C_PROB_INTRA[0]) {
            pred::DC_PRED8X8
        } else if !c.get_prob_branchy(buf, PRED8X8C_PROB_INTRA[1]) {
            pred::VERT_PRED8X8
        } else if !c.get_prob_branchy(buf, PRED8X8C_PROB_INTRA[2]) {
            pred::HOR_PRED8X8
        } else {
            pred::PLANE_PRED8X8
        };
    }

    /// Reads one macroblock's modes and coefficients into `mb`.
    #[inline(always)]
    fn parse_mb(&mut self, buf: &[u8], part: usize, mb: &mut Macroblock, mb_x: usize) {
        self.decode_mb_mode(buf, mb, mb_x);
        self.coeffs.parse_mb(buf, part, mb, mb_x);
    }
}

impl Coeffs {
    #[inline(never)]
    fn decode_mb_coeffs(
        &mut self,
        buf: &[u8],
        part: usize,
        mb: &mut Macroblock,
        mb_x: usize,
    ) {
        let q = self.qmat[mb.segment];
        let token = &self.token;
        let nzc = mb.non_zero_count_cache.as_flattened_mut();
        /* A local copy stays in registers across all 25 blocks; through
         * &mut it went back to memory after every bit. */
        let mut c = self.coeff_partition[part].tokens();
        let mut nz = u32::from(self.top_nnz[mb_x]) | u32::from(self.left_nnz) << 16;
        let mut any = 0;
        let mut dc_nz = 0;
        let (luma_probs, luma_start) = if mb.mode != MODE_I4 {
            let m = NNZ_MASK[24];
            let ctx = nnz_ctx(nz, m);
            let n = decode_block_coeffs(
                &mut c,
                buf,
                &mut self.block_dc.0,
                &token[1],
                0,
                ctx,
                token[1][0][ctx][0],
                q.luma_dc_qmul,
            );

            nz = if n != 0 { nz | m } else { nz & !m };
            dc_nz = n;
            (&token[0], 1)
        } else {
            (&token[3], 0)
        };
        let block_dc = u8::from(dc_nz != 0);
        /* All blocks of a plane start on one of three probabilities, picked
         * by context. Packed in a register, the right one is a shift away once
         * the previous block settles the context; indexing the table put an
         * address computation and a load between a mispredicted end of block
         * and the next decision. */
        let firsts = |probs: &[[[u8; NUM_DCT_TOKENS - 1]; 3]; 16], i: usize| {
            u32::from_le_bytes([probs[i][0][0], probs[i][1][0], probs[i][2][0], 0])
        };
        let luma_first = firsts(luma_probs, luma_start);
        let chroma_first = firsts(&token[2], 0);

        for (b, &m) in NNZ_MASK[..24].iter().enumerate() {
            let luma = b < 16;
            let ctx = nnz_ctx(nz, m);
            let first = (if luma { luma_first } else { chroma_first }) >> (8 * ctx);
            let n = decode_block_coeffs(
                &mut c,
                buf,
                &mut mb.block.0[b],
                if luma { luma_probs } else { &token[2] },
                if luma { luma_start } else { 0 },
                ctx,
                first as u8,
                if luma { q.luma_qmul } else { q.chroma_qmul },
            );

            nzc[b] = n as u8 + if luma { block_dc } else { 0 };
            nz = if n != 0 { nz | m } else { nz & !m };
            any |= n;
        }

        self.coeff_partition[part] = c.finish();
        self.top_nnz[mb_x] = nz as u16;
        self.left_nnz = (nz >> 16) as u16;

        if dc_nz != 0 {
            let luma: &mut [[i16; 16]; 16] =
                (&mut mb.block.0[..16]).try_into().unwrap();

            if dc_nz == 1 {
                (self.dsp.luma_dc_wht_dc)(luma, &mut self.block_dc.0);
            } else {
                (self.dsp.luma_dc_wht)(luma, &mut self.block_dc.0);
            }
        } else if any == 0 {
            mb.skip = true;
        }
    }

    /// Reads the coefficients of a macroblock whose modes are in `mb`.
    #[inline(always)]
    fn parse_mb(&mut self, buf: &[u8], part: usize, mb: &mut Macroblock, mb_x: usize) {
        if !mb.skip {
            self.decode_mb_coeffs(buf, part, mb, mb_x);
        }
        if mb.skip {
            let keep = if mb.mode != MODE_I4 { 0 } else { NNZ_Y2 };

            self.left_nnz &= keep;
            self.top_nnz[mb_x] &= keep;
        }
    }

    /// Reads the coefficients of a row whose modes are parsed. False once a
    /// partition has run dry in it, and the frame stops there.
    fn parse_row(&mut self, buf: &[u8], part: usize, row: &mut MbRow) -> bool {
        self.left_nnz = 0;
        for (mb_x, mb) in row.mbs.iter_mut().enumerate() {
            self.parse_mb(buf, part, mb, mb_x);
        }
        row.filter = !row.modes_dry && !self.coeff_partition[part].overran();
        row.filter
    }
}

impl Recon {
    fn linesize(&self) -> usize {
        self.planes[0].stride
    }

    fn uvlinesize(&self) -> usize {
        self.planes[1].stride
    }

    #[inline(always)]
    fn intra_predict(
        &mut self,
        planes: &mut Planes<'_>,
        mb: &mut Macroblock,
        off: [usize; 3],
        mb_x: usize,
        mb_y: usize,
    ) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();

        if mb.mode < MODE_I4 {
            let mode = check_intra_pred8x8_mode(mb.mode, mb_x, mb_y);

            (self.pred.pred16x16[mode])(planes[0], off[0], ls);
        } else {
            let last = mb_x == self.mb_width - 1;

            if mb.skip {
                mb.non_zero_count_cache[..4].fill([0; 4]);
            }

            let mut ptr = off[0];
            let luma = &mut *planes[0];
            let tr_right: [u8; 4] = if last {
                [luma[off[0] - ls + 15]; 4]
            } else {
                luma[off[0] - ls + 16..off[0] - ls + 20].try_into().unwrap()
            };

            for y in 0..4 {
                for x in 0..4 {
                    let topright: [u8; 4] = if x == 3 {
                        tr_right
                    } else {
                        let at = ptr + 4 + 4 * x - ls;

                        luma[at..at + 4].try_into().unwrap()
                    };
                    let mode = mb.intra4x4_pred_mode_mb[4 * y + x] as usize;

                    (self.pred.pred4x4[mode])(luma, ptr + 4 * x, ls, &topright);

                    let nnz = mb.non_zero_count_cache[y][x];

                    if nnz != 0 {
                        let block = &mut mb.block.0[4 * y + x];
                        let f = if nnz == 1 {
                            self.dsp.idct_dc_add
                        } else {
                            self.dsp.idct_add
                        };

                        f(luma, ptr + 4 * x, ls, block);
                    }
                }
                ptr += 4 * ls;
            }
        }

        let mode = check_intra_pred8x8_mode(mb.chroma_pred_mode, mb_x, mb_y);

        (self.pred.pred8x8[mode])(planes[1], off[1], uvls);
        (self.pred.pred8x8[mode])(planes[2], off[2], uvls);
    }

    #[inline(always)]
    fn idct_mb(&self, planes: &mut Planes<'_>, mb: &mut Macroblock, off: [usize; 3]) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();

        if mb.mode != MODE_I4 {
            let mut y_dst = off[0];
            let luma = &mut *planes[0];

            for y in 0..4 {
                let mut nnz4 = u32::from_le_bytes(mb.non_zero_count_cache[y]);

                if nnz4 != 0 {
                    if nnz4 & !0x0101_0101 != 0 {
                        for x in 0..4 {
                            let n = nnz4 as u8;

                            if n == 1 {
                                (self.dsp.idct_dc_add)(
                                    luma,
                                    y_dst + 4 * x,
                                    ls,
                                    &mut mb.block.0[4 * y + x],
                                );
                            } else if n > 1 {
                                (self.dsp.idct_add)(
                                    luma,
                                    y_dst + 4 * x,
                                    ls,
                                    &mut mb.block.0[4 * y + x],
                                );
                            }
                            nnz4 >>= 8;
                            if nnz4 == 0 {
                                break;
                            }
                        }
                    } else {
                        let block: &mut [[i16; 16]; 4] =
                            (&mut mb.block.0[4 * y..4 * y + 4]).try_into().unwrap();

                        (self.dsp.idct_dc_add4y)(luma, y_dst, ls, block);
                    }
                }
                y_dst += 4 * ls;
            }
        }

        for ch in 0..2 {
            let mut nnz4 = u32::from_le_bytes(mb.non_zero_count_cache[4 + ch]);

            if nnz4 == 0 {
                continue;
            }
            let mut ch_dst = off[1 + ch];
            let chroma = &mut *planes[1 + ch];

            if nnz4 & !0x0101_0101 != 0 {
                'plane: for y in 0..2 {
                    for x in 0..2 {
                        let n = nnz4 as u8;
                        let block = &mut mb.block.0[4 * (4 + ch) + (y << 1) + x];

                        if n == 1 {
                            (self.dsp.idct_dc_add)(chroma, ch_dst + 4 * x, uvls, block);
                        } else if n > 1 {
                            (self.dsp.idct_add)(chroma, ch_dst + 4 * x, uvls, block);
                        }
                        nnz4 >>= 8;
                        if nnz4 == 0 {
                            break 'plane;
                        }
                    }
                    ch_dst += 4 * uvls;
                }
            } else {
                let base = 4 * (4 + ch);
                let block: &mut [[i16; 16]; 4] =
                    (&mut mb.block.0[base..base + 4]).try_into().unwrap();

                (self.dsp.idct_dc_add4uv)(chroma, ch_dst, uvls, block);
            }
        }
    }

    #[inline(always)]
    fn filter_level_for_mb(&self, mb: &Macroblock) -> FilterStrength {
        let i4 = usize::from(mb.mode == MODE_I4);
        let mut f = self.filter_levels[mb.segment][i4];

        f.inner_filter = !mb.skip || i4 == 1;
        f
    }

    #[inline(always)]
    fn filter_mb<const LUMA: bool, const CHROMA: bool>(
        &self,
        planes: &mut Planes<'_>,
        off: [usize; 3],
        f: FilterStrength,
        mb_x: usize,
        mb_y: usize,
    ) {
        const HEV_THRESH_LUT: [i32; 64] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        ];

        let Some((inner_limit, bedge_lim)) = f.limits() else {
            return;
        };
        let mbedge_lim = bedge_lim + 4;
        let hev = HEV_THRESH_LUT[f.filter_level as usize];
        let ls = self.linesize();
        let uvls = self.uvlinesize();
        let inner = f.inner_filter;
        let [y, u, v] = planes;

        let edges = u32::from(mb_x != 0) | u32::from(mb_y != 0) << 1;

        if LUMA {
            match self.dsp.loop_filter16y {
                Some(all) if inner => all(
                    y,
                    off[0],
                    ls,
                    mbedge_lim,
                    bedge_lim,
                    inner_limit,
                    hev,
                    edges,
                ),
                _ => self.filter_mb_luma(
                    y,
                    off[0],
                    mbedge_lim,
                    bedge_lim,
                    inner_limit,
                    hev,
                    inner,
                    mb_x,
                    mb_y,
                ),
            }
        }

        if CHROMA {
            match self.dsp.loop_filter8uv {
                Some(all) if inner => all(
                    u,
                    off[1],
                    v,
                    off[2],
                    uvls,
                    mbedge_lim,
                    bedge_lim,
                    inner_limit,
                    hev,
                    edges,
                ),
                _ => self.filter_mb_chroma(
                    u,
                    v,
                    off,
                    mbedge_lim,
                    bedge_lim,
                    inner_limit,
                    hev,
                    inner,
                    mb_x,
                    mb_y,
                ),
            }
        }
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn filter_mb_chroma(
        &self,
        u: &mut [u8],
        v: &mut [u8],
        off: [usize; 3],
        mbedge_lim: i32,
        bedge_lim: i32,
        inner_limit: i32,
        hev: i32,
        inner: bool,
        mb_x: usize,
        mb_y: usize,
    ) {
        let uvls = self.uvlinesize();

        if mb_x != 0 && inner {
            (self.dsp.h_loop_filter8uv_mb)(
                u,
                off[1],
                v,
                off[2],
                uvls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
        } else if mb_x != 0 {
            (self.dsp.h_loop_filter8uv)(
                u,
                off[1],
                v,
                off[2],
                uvls,
                mbedge_lim,
                inner_limit,
                hev,
            );
        }

        if inner && mb_x == 0 {
            (self.dsp.h_loop_filter8uv_inner)(
                u,
                off[1] + 4,
                v,
                off[2] + 4,
                uvls,
                bedge_lim,
                inner_limit,
                hev,
            );
        }

        if mb_y != 0 && inner {
            (self.dsp.v_loop_filter8uv_mb)(
                u,
                off[1],
                v,
                off[2],
                uvls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
        } else if mb_y != 0 {
            (self.dsp.v_loop_filter8uv)(
                u,
                off[1],
                v,
                off[2],
                uvls,
                mbedge_lim,
                inner_limit,
                hev,
            );
        }

        if inner && mb_y == 0 {
            (self.dsp.v_loop_filter8uv_inner)(
                u,
                off[1] + 4 * uvls,
                v,
                off[2] + 4 * uvls,
                uvls,
                bedge_lim,
                inner_limit,
                hev,
            );
        }
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn filter_mb_luma(
        &self,
        y: &mut [u8],
        off: usize,
        mbedge_lim: i32,
        bedge_lim: i32,
        inner_limit: i32,
        hev: i32,
        inner: bool,
        mb_x: usize,
        mb_y: usize,
    ) {
        let ls = self.linesize();

        if mb_x != 0 && inner {
            (self.dsp.h_loop_filter16y_mb)(
                y,
                off,
                ls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
        } else if mb_x != 0 {
            (self.dsp.h_loop_filter16y)(y, off, ls, mbedge_lim, inner_limit, hev);
        }

        if inner && mb_x == 0 {
            for k in 1..4 {
                (self.dsp.h_loop_filter16y_inner)(
                    y,
                    off + 4 * k,
                    ls,
                    bedge_lim,
                    inner_limit,
                    hev,
                );
            }
        }

        if mb_y != 0 && inner {
            (self.dsp.v_loop_filter16y_mb)(
                y,
                off,
                ls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
        } else if mb_y != 0 {
            (self.dsp.v_loop_filter16y)(y, off, ls, mbedge_lim, inner_limit, hev);
        }

        if inner && mb_y == 0 {
            for k in 1..4 {
                (self.dsp.v_loop_filter16y_inner)(
                    y,
                    off + 4 * k * ls,
                    ls,
                    bedge_lim,
                    inner_limit,
                    hev,
                );
            }
        }
    }

    #[inline(always)]
    fn filter_mb_simple(
        &self,
        luma: &mut [u8],
        off: usize,
        f: FilterStrength,
        mb_x: usize,
        mb_y: usize,
    ) {
        let Some((_, bedge_lim)) = f.limits() else {
            return;
        };
        let mbedge_lim = bedge_lim + 4;
        let ls = self.linesize();
        let inner = f.inner_filter;
        let y = luma;

        if mb_x != 0 && inner {
            (self.dsp.h_loop_filter_simple_mb)(y, off, ls, mbedge_lim, bedge_lim);
        } else {
            if mb_x != 0 {
                (self.dsp.h_loop_filter_simple)(y, off, ls, mbedge_lim);
            }
            if inner {
                for k in 1..4 {
                    (self.dsp.h_loop_filter_simple)(y, off + 4 * k, ls, bedge_lim);
                }
            }
        }

        if mb_y != 0 && inner {
            (self.dsp.v_loop_filter_simple_mb)(y, off, ls, mbedge_lim, bedge_lim);
        } else {
            if mb_y != 0 {
                (self.dsp.v_loop_filter_simple)(y, off, ls, mbedge_lim);
            }
            if inner {
                for k in 1..4 {
                    (self.dsp.v_loop_filter_simple)(y, off + 4 * k * ls, ls, bedge_lim);
                }
            }
        }
    }

    fn filter_mb_at<const LUMA: bool, const CHROMA: bool>(
        &self,
        planes: &mut Planes<'_>,
        mb_x: usize,
        mb_y: usize,
    ) {
        let g = self.planes;
        let strength = self.filter_strength[(mb_y & 1) * self.mb_width + mb_x];
        let off = [
            g[0].at(16 * mb_x, 16 * mb_y),
            g[1].at(8 * mb_x, 8 * mb_y),
            g[2].at(8 * mb_x, 8 * mb_y),
        ];

        if self.filter.simple {
            if LUMA {
                self.filter_mb_simple(planes[0], off[0], strength, mb_x, mb_y);
            }
        } else {
            self.filter_mb::<LUMA, CHROMA>(planes, off, strength, mb_x, mb_y);
        }
    }

    fn row_offsets(&self, mb_y: usize) -> [usize; 3] {
        [
            self.planes[0].at(0, 16 * mb_y),
            self.planes[1].at(0, 8 * mb_y),
            self.planes[2].at(0, 8 * mb_y),
        ]
    }

    /// Sets the column left of a row, which intra prediction reads as 129.
    fn start_row(&self, planes: &mut Planes<'_>, off: [usize; 3]) {
        for (i, &at) in off.iter().enumerate() {
            let rows = if i == 0 { 16 } else { 8 };
            let stride = self.planes[i].stride;

            for y in 0..rows {
                planes[i][at + y * stride - 1] = 129;
            }
        }
    }

    fn reconstruct_row(&mut self, planes: &mut Planes<'_>, row: &mut MbRow) {
        let mb_y = row.mb_y;
        let mut off = self.row_offsets(mb_y);

        self.start_row(planes, off);
        for (mb_x, mb) in row.mbs.iter_mut().enumerate() {
            self.reconstruct_mb(planes, mb, off, mb_x, mb_y);
            off[0] += 16;
            off[1] += 8;
            off[2] += 8;
        }
        self.finish_row(planes, mb_y, row.filter);
    }

    #[inline(always)]
    fn reconstruct_mb(
        &mut self,
        planes: &mut Planes<'_>,
        mb: &mut Macroblock,
        off: [usize; 3],
        mb_x: usize,
        mb_y: usize,
    ) {
        /* Chroma prediction reads no top-right pixels, so the chroma of the
         * macroblock above and two to the left is final once the one before
         * this has been predicted. Its filter comes here, apart from the luma
         * one: back to back the two kernels fill most of the reorder buffer
         * and leave the parse behind them little room. */
        if self.deblock_filter && mb_y > 0 && mb_x > 1 {
            self.filter_mb_at::<false, true>(planes, mb_x - 2, mb_y - 1);
        }

        self.intra_predict(planes, mb, off, mb_x, mb_y);

        if !mb.skip {
            self.idct_mb(planes, mb, off);
        }

        if self.deblock_filter {
            let at = (mb_y & 1) * self.mb_width + mb_x;

            self.filter_strength[at] = self.filter_level_for_mb(mb);

            /* The row above is filtered a macroblock behind this one: its
             * bottom edge stays unfiltered until every macroblock that
             * predicts from it is done. */
            if mb_y > 0 && mb_x > 0 {
                self.filter_mb_at::<true, false>(planes, mb_x - 1, mb_y - 1);
            }
        }
    }

    /// Filters what reconstructing row `mb_y` has made final: the end of the
    /// row above, which reconstruct_mb() filters luma one and chroma two
    /// macroblocks behind itself, and the last row whole. `filter` is false
    /// for a row a partition ran dry in: the frame ends there, with that row
    /// unfiltered below filtered ones, as when each row was filtered as it
    /// was decoded.
    fn finish_row(&mut self, planes: &mut Planes<'_>, mb_y: usize, filter: bool) {
        if !self.deblock_filter {
            return;
        }
        if mb_y > 0 {
            let w = self.mb_width;

            self.filter_mb_at::<true, false>(planes, w - 1, mb_y - 1);
            if w > 1 {
                self.filter_mb_at::<false, true>(planes, w - 2, mb_y - 1);
            }
            self.filter_mb_at::<false, true>(planes, w - 1, mb_y - 1);
        }
        if filter && mb_y + 1 == self.mb_height {
            for mb_x in 0..self.mb_width {
                self.filter_mb_at::<true, true>(planes, mb_x, mb_y);
            }
        }
    }
}

impl Decoder {
    fn save_mb_state(&self, part: usize, mb_x: usize) -> ResumeState {
        ResumeState {
            c: self.c,
            part: self.coeffs.coeff_partition[part],
            intra4x4_top: self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4]
                .try_into()
                .unwrap(),
            intra4x4_left: self.intra4x4_pred_mode_left,
            top_nnz: self.coeffs.top_nnz[mb_x],
            left_nnz: self.coeffs.left_nnz,
        }
    }

    fn restore_mb_state(
        &mut self,
        snap: &ResumeState,
        part: usize,
        mb: &mut Macroblock,
        mb_x: usize,
    ) {
        self.c = snap.c;
        self.coeffs.coeff_partition[part] = snap.part;
        self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4]
            .copy_from_slice(&snap.intra4x4_top);
        self.intra4x4_pred_mode_left = snap.intra4x4_left;
        self.coeffs.top_nnz[mb_x] = snap.top_nnz;
        self.coeffs.left_nnz = snap.left_nnz;
        mb.block.0 = [[0; 16]; 24];
        self.coeffs.block_dc.0 = [0; 16];
    }

    pub fn frame_init(
        &mut self,
        chunk: &[u8],
        avail: usize,
        size: usize,
    ) -> Result<Status> {
        if size < 10 {
            return Err(Error::InvalidData);
        }
        let avail = avail.min(chunk.len()).min(size);

        if avail < 10 {
            return Ok(Status::NeedMore);
        }

        self.chunk_avail = avail;
        self.chunk_size = size;

        if self.decode_frame_header(chunk, avail, size)? == Status::NeedMore {
            return Ok(Status::NeedMore);
        }

        if !self.picture.allocated() {
            self.picture
                .alloc(self.width as usize, self.height as usize)
                .inspect_err(|_| crate::log::error("Frame allocation failed"))?;
        }

        if self.libwebp_compat {
            if self.strict_dsp.is_none() {
                // CPU detection or its global mask may have changed since
                // this decoder was created. Restore its own tables later.
                self.strict_dsp = Some((self.recon.dsp, self.coeffs.dsp));
                self.recon.dsp.libwebp_transforms();
                self.coeffs.dsp.libwebp_transforms();
            }
        } else if let Some((recon, coeffs)) = self.strict_dsp.take() {
            self.recon.dsp = recon;
            self.coeffs.dsp = coeffs;
        }

        self.recon.deblock_filter =
            self.recon.filter.level != 0 && !self.bypass_filtering;

        self.coeffs.top_nnz.fill(0);
        self.intra4x4_pred_mode_top.fill(pred::DC_PRED as u8);

        /* The first row predicts from 127 above it, the corner included. */
        for (p, g) in self.picture.planes.iter().enumerate() {
            let n = if p == 0 { 16 } else { 8 } * self.mb_width;
            let at = g.base + g.origin - g.stride - 1;

            self.picture.data[at..=at + n].fill(127);
        }

        self.mb_x = 0;
        self.mb_y = 0;
        self.mb_rows_done = 0;
        self.open_partitions(chunk);
        Ok(Status::Done)
    }

    pub fn extend(&mut self, chunk: &[u8], avail: usize) {
        self.chunk_avail = avail.min(chunk.len()).min(self.chunk_size);
        self.open_partitions(chunk);
    }

    pub fn decode_rows(&mut self, chunk: &[u8]) -> Result<Status> {
        self.decode_rows_tmpl(chunk, true, 1)
    }

    pub fn decode_frame(&mut self, chunk: &[u8]) -> Result<()> {
        if self.frame_init(chunk, chunk.len(), chunk.len())? == Status::NeedMore {
            return Err(Error::InvalidData);
        }
        self.decode_rows_whole(chunk, 1)
    }

    /// Reconstructs the whole frame, which frame_init() must have opened, on
    /// up to `threads` threads. The complete chunk must be available; partial
    /// input is rejected. Apart from decode_frame() this is for a caller that
    /// wants to put something else on another thread in between the two.
    pub fn decode_rows_whole(&mut self, chunk: &[u8], threads: usize) -> Result<()> {
        if self.chunk_avail != self.chunk_size || chunk.len() < self.chunk_size {
            return Err(Error::InvalidData);
        }
        self.decode_rows_tmpl(chunk, false, threads)?;
        Ok(())
    }

    fn decode_rows_tmpl(
        &mut self,
        chunk: &[u8],
        resumable: bool,
        threads: usize,
    ) -> Result<Status> {
        if !self.picture.allocated() || chunk.len() < self.chunk_avail {
            return Err(Error::InvalidData);
        }

        let mut data = std::mem::take(&mut self.picture.data);
        let ret = self.decode_rows_planes(&mut data, chunk, resumable, threads);

        self.picture.data = data;
        let status = ret?;

        if status == Status::Done
            && (self.c.overran()
                || self.coeffs.coeff_partition.iter().any(RangeCoder::overran))
        {
            return Err(Error::InvalidData);
        }
        Ok(status)
    }

    fn decode_rows_planes(
        &mut self,
        data: &mut [u8],
        chunk: &[u8],
        resumable: bool,
        threads: usize,
    ) -> Result<Status> {
        let g = self.picture.planes;
        let (head, third) = data.split_at_mut(g[2].base);
        let (first, second) = head.split_at_mut(g[1].base);
        let planes = &mut [
            &mut first[g[0].base..][..g[0].len],
            &mut second[..g[1].len],
            &mut third[..g[2].len],
        ];
        self.recon.planes = g;

        let pixels = self.width as usize * self.height as usize;
        let relay = !resumable
            && threads > 1
            && self.mb_width > 1
            && self.mb_height > 1
            && pixels >= RELAY_PIXELS;

        if relay {
            return self.decode_rows_relayed(planes, chunk, threads);
        }

        let start_row = if resumable { self.mb_y } else { 0 };
        let mut mb = Macroblock::default();

        for mb_y in start_row..self.mb_height {
            let part = mb_y & (self.num_coeff_partitions - 1);
            let mut mb_x0 = 0;
            let mut check = false;
            let mut off = self.recon.row_offsets(mb_y);

            if resumable {
                if self.partition_ready & (1 << part) == 0 {
                    self.mb_x = 0;
                    self.mb_y = mb_y;
                    return Ok(Status::NeedMore);
                }
                check = (self.partition_clamped >> part) & 1 != 0;
                mb_x0 = self.mb_x;
            }

            if !resumable || mb_x0 == 0 {
                self.start_row();
                self.recon.start_row(planes, off);
            } else {
                off[0] += 16 * mb_x0;
                off[1] += 8 * mb_x0;
                off[2] += 8 * mb_x0;
            }

            for mb_x in mb_x0..self.mb_width {
                let snap = if resumable && check {
                    Some(self.save_mb_state(part, mb_x))
                } else {
                    None
                };

                self.parse_mb(chunk, part, &mut mb, mb_x);

                if let Some(snap) = snap {
                    if self.coeffs.coeff_partition[part].overran() {
                        self.restore_mb_state(&snap, part, &mut mb, mb_x);
                        self.mb_x = mb_x;
                        self.mb_y = mb_y;
                        return Ok(Status::NeedMore);
                    }
                }

                self.recon.reconstruct_mb(planes, &mut mb, off, mb_x, mb_y);

                off[0] += 16;
                off[1] += 8;
                off[2] += 8;
            }

            let dry = !check && self.ran_dry(part);

            self.recon.finish_row(planes, mb_y, !dry);
            if dry {
                return Err(Error::InvalidData);
            }

            if resumable {
                /* A row's pixels are final once the row below it is decoded
                 * and it is filtered. */
                self.mb_x = 0;
                self.mb_rows_done = mb_y + usize::from(!self.recon.deblock_filter);
            }
        }

        self.mb_y = self.mb_height;
        self.mb_rows_done = self.mb_height;
        Ok(Status::Done)
    }

    /// Parses rows here while other threads reconstruct and filter the ones
    /// parsed before them. Entropy decoding is most of the work and runs
    /// through each partition in order, so a still gets its threads from
    /// splitting the work on a row into steps: the modes, the coefficients
    /// and reconstruction, each on a thread of its own when there are three.
    fn decode_rows_relayed(
        &mut self,
        planes: &mut Planes<'_>,
        chunk: &[u8],
        threads: usize,
    ) -> Result<Status> {
        let mut rows = std::mem::take(&mut self.rows);

        rows.resize_with(RELAY_ROWS, MbRow::default);
        for row in &mut rows {
            row.mbs
                .try_reserve(self.mb_width.saturating_sub(row.mbs.len()))
                .map_err(|_| Error::NoMemory)?;
            row.mbs.resize_with(self.mb_width, Macroblock::default);
            row.pending = false;
        }

        let mut recon = std::mem::take(&mut self.recon);
        let mut coeffs = std::mem::take(&mut self.coeffs);
        let parts = self.num_coeff_partitions;
        let mut parse =
            |row: &mut MbRow| coeffs.parse_row(chunk, row.mb_y & (parts - 1), row);
        let (ret, rows) = if threads >= 3 {
            let mut reconstruct = |row: &mut MbRow| {
                recon.reconstruct_row(planes, row);
                true
            };

            crate::task::relay(
                threads,
                rows,
                |relay| self.parse_modes(chunk, relay, |_| {}),
                &mut [&mut parse, &mut reconstruct],
            )
        } else if self.coeffs_outweigh_reconstruction() {
            crate::task::relay(
                threads,
                rows,
                |relay| {
                    self.parse_modes(chunk, relay, |row| {
                        recon.reconstruct_row(planes, row)
                    })
                },
                &mut [&mut parse],
            )
        } else {
            let mut reconstruct = |row: &mut MbRow| {
                recon.reconstruct_row(planes, row);
                true
            };

            self.coeffs = coeffs;
            let relayed = crate::task::relay(
                threads,
                rows,
                |relay| self.parse_rows(chunk, relay),
                &mut [&mut reconstruct],
            );

            coeffs = std::mem::take(&mut self.coeffs);
            relayed
        };

        self.coeffs = coeffs;
        self.recon = recon;
        self.rows = rows;
        ret?;

        /* The coefficients of the last row can run dry after the modes are
         * all passed on. */
        if self.c.overran()
            || self.coeffs.coeff_partition.iter().any(RangeCoder::overran)
        {
            return Err(Error::InvalidData);
        }
        self.mb_y = self.mb_height;
        self.mb_rows_done = self.mb_height;
        Ok(Status::Done)
    }

    /// Whether parsing the coefficients looks to take longer than
    /// reconstructing the frame, which decides what shares a thread with the
    /// modes when there are only two.
    fn coeffs_outweigh_reconstruction(&self) -> bool {
        let bytes = self.chunk_size.saturating_sub(self.partition_start[0]) as u64;
        let mbs = (self.mb_width * self.mb_height) as u64;

        10 * bytes >= COEFF_HEAVY_TENTHS_PER_MB * mbs
    }

    fn parse_rows(
        &mut self,
        chunk: &[u8],
        relay: &mut crate::task::Relay<'_, '_, MbRow>,
    ) -> Result<()> {
        for mb_y in 0..self.mb_height {
            let part = mb_y & (self.num_coeff_partitions - 1);
            let Some(mut row) = relay.take() else {
                return Err(Error::InvalidData);
            };

            self.start_row();
            for (mb_x, mb) in row.mbs.iter_mut().enumerate() {
                self.parse_mb(chunk, part, mb, mb_x);
            }

            let dry = self.ran_dry(part);

            row.mb_y = mb_y;
            row.filter = !dry;
            if !relay.pass(row) || dry {
                return Err(Error::InvalidData);
            }
        }
        Ok(())
    }

    /// Parses the modes of every row and passes it on. `finish` gets each row
    /// that comes back, in order, before its buffer is used again.
    fn parse_modes(
        &mut self,
        chunk: &[u8],
        relay: &mut crate::task::Relay<'_, '_, MbRow>,
        mut finish: impl FnMut(&mut MbRow),
    ) -> Result<()> {
        let mut ret = Ok(());
        let mut done = |row: &mut MbRow| {
            if row.pending {
                finish(row);
                row.pending = false;
            }
        };

        for mb_y in 0..self.mb_height {
            let Some(mut row) = relay.take() else {
                ret = Err(Error::InvalidData);
                break;
            };

            done(&mut row);
            self.intra4x4_pred_mode_left = [pred::DC_PRED as u8; 4];
            for (mb_x, mb) in row.mbs.iter_mut().enumerate() {
                self.decode_mb_mode(chunk, mb, mb_x);
            }

            let dry = self.c.overran();

            row.mb_y = mb_y;
            row.modes_dry = dry;
            row.pending = true;
            if !relay.pass(row) || dry {
                ret = Err(Error::InvalidData);
                break;
            }
        }

        relay.finish();
        while let Some(mut row) = relay.take() {
            done(&mut row);
        }
        ret
    }

    fn start_row(&mut self) {
        self.coeffs.left_nnz = 0;
        self.intra4x4_pred_mode_left = [pred::DC_PRED as u8; 4];
    }

    /* Overrun is sticky and fails the frame once it is done, so a whole
     * partition that has run dry fails it at the end of the row instead,
     * before a chunk of a few bytes pays for reconstructing a frame of up to
     * 16383x16383; libwebp stops at the first such macroblock. */
    fn ran_dry(&self, part: usize) -> bool {
        self.c.overran() || self.coeffs.coeff_partition[part].overran()
    }

    pub fn rows_finalized(&self) -> i32 {
        const EXTRA: [i32; 3] = [0, 2, 8];

        if self.mb_rows_done >= self.mb_height {
            return self.height;
        }

        let kind = if !self.recon.deblock_filter {
            0
        } else if self.recon.filter.simple {
            1
        } else {
            2
        };
        let rows = 16 * self.mb_rows_done as i32 - EXTRA[kind];

        rows.clamp(0, self.height)
    }
}

/* The subblock mode tree, walked with a branch per decision as libwebp's
 * ParseIntraMode does: each branch is predicted, so the next probability is
 * already loaded where a table walk would wait on the tree entry for it. */
#[inline(always)]
fn read_pred4x4_mode(c: &mut RangeCoder, buf: &[u8], p: &[u8; 9]) -> u8 {
    let mode = if !c.get_prob_branchy(buf, p[0]) {
        pred::DC_PRED
    } else if !c.get_prob_branchy(buf, p[1]) {
        pred::TM_VP8_PRED
    } else if !c.get_prob_branchy(buf, p[2]) {
        pred::VERT_PRED
    } else if !c.get_prob_branchy(buf, p[3]) {
        if !c.get_prob_branchy(buf, p[4]) {
            pred::HOR_PRED
        } else if !c.get_prob_branchy(buf, p[5]) {
            pred::DIAG_DOWN_RIGHT_PRED
        } else {
            pred::VERT_RIGHT_PRED
        }
    } else if !c.get_prob_branchy(buf, p[6]) {
        pred::DIAG_DOWN_LEFT_PRED
    } else if !c.get_prob_branchy(buf, p[7]) {
        pred::VERT_LEFT_PRED
    } else if !c.get_prob_branchy(buf, p[8]) {
        pred::HOR_DOWN_PRED
    } else {
        pred::HOR_UP_PRED
    };

    mode as u8
}

fn check_intra_pred8x8_mode(mode: usize, mb_x: usize, mb_y: usize) -> usize {
    if mode != pred::DC_PRED8X8 {
        return mode;
    }
    if mb_x == 0 {
        return if mb_y != 0 {
            pred::TOP_DC_PRED8X8
        } else {
            pred::DC_128_PRED8X8
        };
    }
    if mb_y != 0 {
        mode
    } else {
        pred::LEFT_DC_PRED8X8
    }
}

/* A block's above and left non-zero flags, one bit each in the low and high
 * halves of a macroblock's context mask: luma 0-3, U 4-5, V 6-7, Y2 8. */
const NNZ_Y2: u16 = 1 << 8;

const fn nnz_masks() -> [u32; 25] {
    let mut m = [0; 25];
    let mut b = 0;

    while b < 16 {
        m[b] = 1 << (b & 3) | 1 << (16 + (b >> 2));
        b += 1;
    }
    while b < 24 {
        let c = 4 + 2 * ((b - 16) >> 2);

        m[b] = 1 << (c + (b & 1)) | 1 << (16 + c + ((b >> 1) & 1));
        b += 1;
    }
    m[24] = 1 << 8 | 1 << 24;
    m
}

static NNZ_MASK: [u32; 25] = nnz_masks();

#[inline(always)]
fn nnz_ctx(nz: u32, m: u32) -> usize {
    usize::from(nz & m & 0xffff != 0) + usize::from(nz & m > 0xffff)
}

/* Returns one past the last coefficient decoded, 0 for an empty block.
 * first is probs[i][ctx][0], which the caller finds sooner. */
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn decode_block_coeffs(
    c: &mut TokenCoder,
    buf: &[u8],
    block: &mut [i16; 16],
    probs: &[[[u8; NUM_DCT_TOKENS - 1]; 3]; 16],
    mut i: usize,
    ctx: usize,
    first: u8,
    qmul: [i16; 2],
) -> usize {
    let mut token_prob = &probs[i][ctx];

    if !c.get_prob_branchy(buf, first) {
        return 0;
    }
    loop {
        while !c.get_prob_branchy(buf, token_prob[1]) {
            i += 1;
            if i == 16 {
                return i;
            }
            token_prob = &probs[i][0];
        }

        let coeff;
        let next_ctx;

        if !c.get_prob_branchy(buf, token_prob[2]) {
            coeff = 1;
            next_ctx = 1;
        } else {
            if !c.get_prob_branchy(buf, token_prob[3]) {
                let mut v = i32::from(c.get_prob_branchy(buf, token_prob[4]));

                if v != 0 {
                    v += c.get_prob(buf, token_prob[5]) as i32;
                }
                coeff = v + 2;
            } else if !c.get_prob_branchy(buf, token_prob[6]) {
                if !c.get_prob_branchy(buf, token_prob[7]) {
                    coeff = 5 + c.get_prob(buf, DCT_CAT1_PROB[0]) as i32;
                } else {
                    coeff = 7
                        + ((c.get_prob(buf, DCT_CAT2_PROB[0]) as i32) << 1)
                        + c.get_prob(buf, DCT_CAT2_PROB[1]) as i32;
                }
            } else {
                let a = c.get_prob(buf, token_prob[8]) as usize;
                let b = c.get_prob(buf, token_prob[9 + a]) as usize;
                let cat = (a << 1) + b;

                coeff = 3 + (8 << cat) + c.get_coeff(buf, DCT_CAT_PROB[cat]);
            }
            next_ctx = 2;
        }

        block[ZIGZAG_SCAN[i] as usize & 15] =
            (c.get_signed(buf, coeff) * i32::from(qmul[usize::from(i != 0)])) as i16;

        i += 1;
        if i >= 16 {
            return i;
        }
        token_prob = &probs[i][next_ctx];
        if !c.get_prob_branchy(buf, token_prob[0]) {
            return i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plane_starts_aligned_and_has_room_for_its_borders() {
        let mut pic = Picture::default();

        pic.alloc(17, 9).unwrap();

        for p in 0..3 {
            let g = pic.planes[p];
            let data = pic.plane(p);
            let rows = if p == 0 { 9 } else { 5 };

            assert_eq!(
                (data.as_ptr() as usize + g.origin - PLANE_COL_PAD) % ALIGN,
                0
            );
            assert!(g.origin >= PLANE_ROW_PAD * g.stride + PLANE_COL_PAD);
            assert!(data.len() >= g.at(0, rows - 1) + 32 * g.stride);
        }
    }

    #[test]
    fn the_planes_do_not_overlap() {
        let mut pic = Picture::default();

        pic.alloc(64, 64).unwrap();

        for p in 0..2 {
            let end = pic.planes[p].base + pic.planes[p].len;

            assert!(end <= pic.planes[p + 1].base);
        }
        assert!(pic.planes[2].base + pic.planes[2].len <= pic.data.len());
    }

    #[test]
    fn laying_the_planes_out_again_starts_from_zero() {
        let mut pic = Picture::default();

        pic.alloc(64, 64).unwrap();

        let was = pic.data.as_ptr() as usize;

        for p in 0..3 {
            let g = pic.planes[p];

            pic.data[g.base + g.at(0, 0)] = 0xff;
        }
        pic.invalidate();
        assert!(!pic.allocated());
        pic.alloc(32, 32).unwrap();
        assert_eq!(pic.data.as_ptr() as usize, was, "the block was replaced");
        for p in 0..3 {
            assert_eq!(pic.plane(p)[pic.planes[p].at(0, 0)], 0);
        }
    }

    #[test]
    fn a_stride_never_lands_on_a_cache_way_boundary() {
        for width in [960, 1984, 4032] {
            assert_ne!(Plane::stride_for(width) % 1024, 0);
        }
    }

    #[test]
    fn the_chroma_planes_round_an_odd_size_up() {
        let mut p = Picture::default();

        p.alloc(17, 9).unwrap();

        assert!(p.plane(1).len() >= p.planes[1].at(8, 4));
        assert_eq!(p.planes[1].stride, p.planes[2].stride);
    }

    #[test]
    fn a_dc_only_prediction_mode_falls_back_at_the_frame_edges() {
        assert_eq!(
            check_intra_pred8x8_mode(pred::DC_PRED8X8, 0, 0),
            pred::DC_128_PRED8X8
        );
        assert_eq!(
            check_intra_pred8x8_mode(pred::DC_PRED8X8, 0, 1),
            pred::TOP_DC_PRED8X8
        );
        assert_eq!(
            check_intra_pred8x8_mode(pred::DC_PRED8X8, 1, 0),
            pred::LEFT_DC_PRED8X8
        );
        assert_eq!(
            check_intra_pred8x8_mode(pred::DC_PRED8X8, 1, 1),
            pred::DC_PRED8X8
        );
        assert_eq!(
            check_intra_pred8x8_mode(pred::PLANE_PRED8X8, 0, 0),
            pred::PLANE_PRED8X8
        );
    }

    const SHORT: &[u8] = &[
        0xd0, 0x00, 0x00, 0x9d, 0x01, 0x2a, 0x10, 0x10, 0x04, 0x9d, 0x01, 0x2a, 0x00,
        0x01, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x00, 0x0a, 0x00, 0x00, 0x0a, 0x0a,
    ];

    // A cwebp-encoded 16x16 solid-color frame, with one coefficient partition.
    const SOLID: &[u8] = &[
        0x70, 0x01, 0x00, 0x9d, 0x01, 0x2a, 0x10, 0x00, 0x10, 0x00, 0x02, 0x00, 0x34,
        0x25, 0xa0, 0x02, 0x74, 0x01, 0x40, 0x00, 0x00, 0xfe, 0xef, 0x51, 0x2c, 0x61,
        0x34, 0x93, 0xa3, 0xff, 0xab, 0x43, 0xff, 0xf9, 0x68, 0x7f, 0xff, 0x2d, 0x0f,
        0xdb, 0xb0, 0x00,
    ];

    #[test]
    fn whole_rows_reject_an_unopened_coefficient_partition() {
        let mut dec = Decoder::new();
        let partial = &SOLID[..21];

        assert_eq!(
            dec.frame_init(partial, partial.len(), SOLID.len()),
            Ok(Status::Done)
        );
        assert_eq!(dec.decode_rows_whole(partial, 1), Err(Error::InvalidData));
        dec.extend(SOLID, SOLID.len());
        assert_eq!(dec.decode_rows_whole(SOLID, 1), Ok(()));
    }

    #[test]
    fn exhausted_partitions_are_invalid_on_both_decode_paths() {
        let truncated = &SOLID[..24];
        let mut dec = Decoder::new();

        assert_eq!(dec.decode_frame(truncated), Err(Error::InvalidData));
        let mut dec = Decoder::new();
        assert_eq!(
            dec.frame_init(truncated, truncated.len(), truncated.len()),
            Ok(Status::Done)
        );
        assert_eq!(dec.decode_rows(truncated), Err(Error::InvalidData));
    }

    #[test]
    fn a_dry_partition_stops_the_frame_at_the_row_it_ran_out_in() {
        let mut tall = SOLID[..24].to_vec();

        tall[8] = 0x40; /* 16x64: four macroblock rows from a partition that
                         * cannot fill the first */
        let mut dec = Decoder::new();

        assert_eq!(dec.decode_frame(&tall), Err(Error::InvalidData));

        let last_row = &dec.picture.plane(0)[dec.picture.planes[0].at(0, 63)..][..16];

        assert!(
            last_row.iter().all(|&b| b == 0),
            "the last macroblock row was reconstructed"
        );
    }

    /// SOLID at `w`x`h`, its first partition padded with `modes` zero bytes
    /// and its coefficient partition with `coeffs`, so each runs dry some way
    /// down the frame or not at all.
    fn padded(w: u16, h: u16, modes: usize, coeffs: usize) -> Vec<u8> {
        let first = 11 + modes;
        let tag = (first << 5) as u32 | 0x10;
        let mut big = tag.to_le_bytes()[..3].to_vec();

        big.extend_from_slice(&SOLID[3..21]);
        big.extend(std::iter::repeat_n(0, modes));
        big.extend_from_slice(&SOLID[21..]);
        big.extend(std::iter::repeat_n(0, coeffs));
        big[6..8].copy_from_slice(&w.to_le_bytes());
        big[8..10].copy_from_slice(&h.to_le_bytes());
        big
    }

    #[test]
    fn a_relayed_frame_stops_where_one_thread_stops() {
        /* At 256x256 the modes run dry in row 2 with 40 bytes and in the
         * last row with 125, the coefficients in row 5 with 600, row 14 with
         * 1750 and the last row with 1900, and 4000 of each is a whole frame.
         * Two threads split the work one way below 1664 coefficient bytes
         * and the other way above. */
        let cases = [
            (256, 256, 40, 4000),
            (256, 256, 40, 1000),
            (256, 256, 125, 4000),
            (256, 256, 4000, 600),
            (256, 256, 4000, 1750),
            (256, 256, 4000, 1900),
            (256, 256, 4000, 4000),
            (1024, 64, 40, 4000),
            (1024, 64, 4000, 600),
        ];

        for (w, h, modes, coeffs) in cases {
            let big = padded(w, h, modes, coeffs);
            let mut serial = Decoder::new();
            let want = serial.decode_frame(&big);

            for threads in [2, 3, 4] {
                let mut dec = Decoder::new();

                assert_eq!(
                    dec.frame_init(&big, big.len(), big.len()),
                    Ok(Status::Done)
                );
                assert_eq!(dec.decode_rows_whole(&big, threads), want);
                for p in 0..3 {
                    assert!(
                        dec.picture.plane(p) == serial.picture.plane(p),
                        "{w}x{h}, {modes} and {coeffs} bytes, {threads} threads, plane {p}"
                    );
                }
            }
        }
    }

    #[test]
    fn incomplete_partitions_resume_without_changing_the_picture() {
        let mut whole = Decoder::new();

        whole.decode_frame(SOLID).unwrap();
        for split in 21..SOLID.len() {
            let mut dec = Decoder::new();
            let partial = &SOLID[..split];

            assert_eq!(
                dec.frame_init(partial, split, SOLID.len()),
                Ok(Status::Done)
            );
            if dec.decode_rows(partial).unwrap() == Status::NeedMore {
                dec.extend(SOLID, SOLID.len());
                assert_eq!(dec.decode_rows(SOLID), Ok(Status::Done));
            }
            // Plane bases depend on each allocation's alignment padding.
            for p in 0..3 {
                assert_eq!(
                    dec.picture.plane(p),
                    whole.picture.plane(p),
                    "split {split}, plane {p}"
                );
            }
        }
    }

    #[test]
    fn rows_cannot_be_decoded_before_the_planes_exist() {
        let mut dec = Decoder::new();
        let split = SHORT.len() / 2;

        assert_eq!(
            dec.frame_init(&SHORT[..split], split, SHORT.len()),
            Ok(Status::NeedMore)
        );
        assert_eq!(dec.decode_rows(&SHORT[..split]), Err(Error::InvalidData));

        dec.extend(SHORT, SHORT.len());
        assert_eq!(dec.decode_rows(SHORT), Err(Error::InvalidData));
    }
}
