//! The lossy (VP8) frame decoder.
//!
//! A port of `src/vp8.c`, which is itself a WebP-only descendant of FFmpeg's
//! VP8 decoder: keyframes only, no inter prediction, no motion vectors.
//!
//! Two things are shaped differently from the C.
//!
//! The picture is one flat `Vec<u8>` holding all three planes, each addressed
//! by offset into its own slice of it. The decoder deliberately reads and
//! writes outside the visible frame — the left border column at `dst[-1]`, the
//! row above the first macroblock, the four samples above and to the right of a
//! subblock — and in the C those are negative indices off an interior pointer.
//! Against a flat allocation they are ordinary indices, which is what lets the
//! whole macroblock loop be safe code.
//!
//! The range coders hold offsets rather than pointers, so
//! `wpd_vp56_save_offsets` and `wpd_vp56_restore_offsets` have no counterpart
//! here: a streaming append that reallocates the chunk cannot invalidate them.
//! See [`rac`].

pub mod rac;
pub mod tables;

use crate::dsp::vp8::Vp8Dsp;
use crate::dsp::vp8pred::{self as pred, Vp8Pred};
use rac::RangeCoder;
use tables::*;

const NUM_DCT_TOKENS: usize = 12;
const MAX_PARTITIONS: usize = 8;

/// The padding `wpd_alloc_picture` puts around a plane: 32 rows above and
/// below, 64 bytes to the left of every row.
const PLANE_ROW_PAD: usize = 32;
const PLANE_COL_PAD: usize = 64;
const ALIGN: usize = 64;

pub use crate::error::{check_image_size, Error, Result, Status};

fn clip_uintp2(value: i32, bits: u32) -> i32 {
    value.clamp(0, (1 << bits) - 1)
}

fn rl16(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8
}

fn rl24(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16
}

/// The three planes of the picture being decoded, split out of its one
/// allocation for the length of a frame.
type Planes<'a> = [&'a mut [u8]; 3];

/// Moves eight bytes between the saved macroblock border and a plane, in
/// whichever direction `swap` asks for. `WPD_SWAP64`/`WPD_COPY64` in the C.
///
/// The border and the plane arrive already borrowed, because the caller has
/// split all three planes out of the one allocation and would otherwise pay
/// for that split once per call rather than once per macroblock.
#[inline(always)]
fn xchg8(
    border: &mut [[u8; 32]],
    tb: usize,
    to: usize,
    data: &mut [u8],
    po: usize,
    swap: bool,
) {
    let saved: [u8; 8] = border[tb][to..to + 8].try_into().unwrap();

    if swap {
        let old: [u8; 8] = data[po..po + 8].try_into().unwrap();

        border[tb][to..to + 8].copy_from_slice(&old);
    }
    data[po..po + 8].copy_from_slice(&saved);
}

/// Where one plane sits inside the picture's allocation, with the padding the
/// assembly and the border logic rely on.
///
/// `origin` is the offset of the visible top-left sample *within the plane's
/// own slice*; every access the decoder makes is expressed relative to it.
/// Keeping the planes at separate slices rather than at offsets into one is
/// what leaves every bounds check as tight as it was when each plane had its
/// own `Vec`: a kernel that walks off the end of the luma still trips an
/// assertion instead of scribbling on the chroma.
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub stride: usize,
    pub origin: usize,
    /// Offset of the plane inside [`Picture::data`], and how far it runs.
    base: usize,
    len: usize,
}

impl Plane {
    /// The stride a plane of `width` samples is allocated at.
    fn stride_for(width: usize) -> usize {
        let stride = (width + PLANE_COL_PAD + ALIGN - 1) & !(ALIGN - 1);

        // Strides that are a multiple of 1024 alias in L1/L2; pad them.
        if stride % 1024 == 0 {
            stride + ALIGN
        } else {
            stride
        }
    }

    /// The offset of the sample at `(x, y)` in the visible frame.
    #[inline(always)]
    fn at(&self, x: usize, y: usize) -> usize {
        self.origin + y * self.stride + x
    }
}

/// The three planes of a decoded frame, in one allocation.
///
/// The C asked the allocator three times per frame and gave all three back
/// when the frame size changed. Doing it once, and keeping the block when the
/// next frame fits in it, is what the sizes make natural — the three planes
/// are always allocated and freed together, and their sum sits either side of
/// glibc's mmap threshold, so an animation that reallocates per sub-frame pays
/// an mmap and a munmap for every frame. Each plane still ends up 64-byte
/// aligned, because every plane's extent is rounded up to that.
#[derive(Default)]
pub struct Picture {
    data: Vec<u8>,
    pub planes: [Plane; 3],
    /// Whether [`Self::planes`] describes the frame about to be decoded.
    ready: bool,
}

impl Picture {
    /// Lays the three planes out for a `width` by `height` frame and clears
    /// them, reusing the block when what is there is already big enough.
    fn alloc(&mut self, width: usize, height: usize) -> Result<()> {
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);
        let mut planes = [Plane::default(); 3];
        // Room to bring the first plane up to a 64-byte boundary.
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
            /* Growing already zeroes what it adds, and dropping what was there
            first means it zeroes the whole buffer exactly once. */
            self.data.clear();
            self.data
                .try_reserve_exact(total)
                .map_err(|_| Error::NoMemory)?;
            self.data.resize(total, 0);
        } else {
            self.data[..total].fill(0);
        }

        // Vec only promises byte alignment, and the C handed the assembly a
        // 64-byte aligned row start. Casting the pointer to an integer to find
        // the padding needed is not a dereference, so this stays safe.
        let pad = self.data.as_ptr() as usize % ALIGN;
        let pad = (ALIGN - pad) % ALIGN;

        for plane in &mut planes {
            plane.base = plane.base - ALIGN + pad;
        }
        self.planes = planes;
        self.ready = true;
        Ok(())
    }

    /// Drops the layout without giving the memory back, which is what a frame
    /// size change does: the next frame lays itself out over the same block.
    fn invalidate(&mut self) {
        self.planes = [Plane::default(); 3];
        self.ready = false;
    }

    fn allocated(&self) -> bool {
        self.ready
    }

    /// Plane `p`'s own slice, which its offsets are relative to.
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

#[derive(Clone, Copy, Default)]
struct Macroblock {
    skip: bool,
    mode: usize,
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
    token: [[[[u8; NUM_DCT_TOKENS - 1]; 3]; 16]; 4],
}

impl Default for Probs {
    fn default() -> Self {
        Self {
            segmentid: [255; 3],
            mbskip: 0,
            token: [[[[0; NUM_DCT_TOKENS - 1]; 3]; 16]; 4],
        }
    }
}

/// The 24 coefficient blocks of a macroblock: sixteen luma then four each of
/// the two chroma planes, in the one array the assembly expects.
#[repr(C, align(16))]
struct Blocks([[i16; 16]; 24]);

#[repr(C, align(16))]
struct BlockDc([i16; 16]);

/// The macroblock state a resumable decode has to be able to put back when a
/// partition turns out to have run past the end of what has arrived.
#[derive(Clone, Copy)]
struct ResumeState {
    c: RangeCoder,
    part: RangeCoder,
    intra4x4_top: [u8; 4],
    intra4x4_left: [u8; 4],
    top_nnz: [u8; 9],
    left_nnz: [u8; 9],
}

pub struct Decoder {
    dsp: Vp8Dsp,
    pred: Vp8Pred,

    pub picture: Picture,
    pub width: i32,
    pub height: i32,
    pub bypass_filtering: bool,

    mb_width: usize,
    mb_height: usize,

    deblock_filter: bool,
    mbskip_enabled: bool,
    segment: usize,
    chroma_pred_mode: usize,
    profile: u8,

    segmentation: Segmentation,
    filter: Filter,
    lf_delta: LfDelta,
    qmat: [QMat; 4],

    filter_strength: Vec<FilterStrength>,
    intra4x4_pred_mode_top: Vec<u8>,
    intra4x4_pred_mode_left: [u8; 4],
    intra4x4_pred_mode_mb: [u8; 16],
    top_nnz: Vec<[u8; 9]>,
    left_nnz: [u8; 9],
    top_border: Vec<[u8; 32]>,

    non_zero_count_cache: [[u8; 4]; 6],
    block: Blocks,
    block_dc: BlockDc,
    prob: Probs,

    c: RangeCoder,
    num_coeff_partitions: usize,
    coeff_partition: [RangeCoder; MAX_PARTITIONS],
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

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            dsp: Vp8Dsp::new(),
            pred: Vp8Pred::new(),
            picture: Picture::default(),
            width: 0,
            height: 0,
            bypass_filtering: false,
            mb_width: 0,
            mb_height: 0,
            deblock_filter: false,
            mbskip_enabled: false,
            segment: 0,
            chroma_pred_mode: 0,
            profile: 0,
            segmentation: Segmentation::default(),
            filter: Filter::default(),
            lf_delta: LfDelta::default(),
            qmat: [QMat::default(); 4],
            filter_strength: Vec::new(),
            intra4x4_pred_mode_top: Vec::new(),
            intra4x4_pred_mode_left: [0; 4],
            intra4x4_pred_mode_mb: [0; 16],
            top_nnz: Vec::new(),
            left_nnz: [0; 9],
            top_border: Vec::new(),
            non_zero_count_cache: [[0; 4]; 6],
            block: Blocks([[0; 16]; 24]),
            block_dc: BlockDc([0; 16]),
            prob: Probs::default(),
            c: RangeCoder::default(),
            num_coeff_partitions: 0,
            coeff_partition: [RangeCoder::default(); MAX_PARTITIONS],
            partition_start: [0; MAX_PARTITIONS],
            partition_size: [0; MAX_PARTITIONS],
            partition_ready: 0,
            partition_clamped: 0,
            mb_x: 0,
            mb_y: 0,
            mb_rows_done: 0,
            chunk_avail: 0,
            chunk_size: 0,
        }
    }

    fn linesize(&self) -> usize {
        self.picture.planes[0].stride
    }

    fn uvlinesize(&self) -> usize {
        self.picture.planes[1].stride
    }

    fn update_dimensions(&mut self, width: i32, height: i32) -> Result<()> {
        check_image_size(width, height)?;

        if width == self.width
            && height == self.height
            && self.picture.allocated()
            && !self.filter_strength.is_empty()
        {
            return Ok(());
        }

        self.width = width;
        self.height = height;
        self.mb_width = (width as usize).div_ceil(16);
        self.mb_height = (height as usize).div_ceil(16);

        self.picture.invalidate();
        self.filter_strength = vec![FilterStrength::default(); self.mb_width];
        self.intra4x4_pred_mode_top = vec![0; self.mb_width * 4];
        self.top_nnz = vec![[0; 9]; self.mb_width];
        self.top_border = vec![[0; 32]; self.mb_width + 1];
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

    /// Consumes all eight coded deltas, even though keyframes apply only two.
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

    /// Opens, or widens, a range coder over each coefficient partition that has
    /// enough of its bytes present to be started.
    fn open_partitions(&mut self, buf: &[u8]) {
        let init_bytes = if rac::RAC_64 { 0 } else { 3 };

        for i in 0..self.num_coeff_partitions {
            let start = self.partition_start[i];
            let size = self.partition_size[i];
            let have = self.chunk_avail.saturating_sub(start);
            let win = if have >= size {
                size
            } else if have >= init_bytes {
                have
            } else {
                continue;
            };

            if self.partition_ready & (1 << i) == 0 {
                self.coeff_partition[i] = RangeCoder::start(buf, start, win);
                self.partition_ready |= 1 << i;
            } else if self.coeff_partition[i].end() != start + win {
                self.coeff_partition[i].extend(start + win);
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
            let q = &mut self.qmat[i];

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

        if self.profile > 3 {
            crate::log::warning(&format!("Unknown profile {}", self.profile));
        }
        if header_size > total.saturating_sub(10) {
            crate::log::error("Header size larger than data provided");
            return Err(Error::InvalidData);
        }
        if avail.saturating_sub(10) < header_size {
            return Ok(Status::NeedMore);
        }
        if rl24(&buf[3..]) != 0x002a_019d {
            crate::log::error(&format!("Invalid start code 0x{:x}", rl24(&buf[3..])));
            return Err(Error::InvalidData);
        }

        let width = (rl16(&buf[6..]) & 0x3fff) as i32;
        let height = (rl16(&buf[8..]) & 0x3fff) as i32;
        let hscale = buf[7] >> 6;
        let vscale = buf[9] >> 6;

        if hscale != 0 || vscale != 0 {
            crate::log::warning("Upscaling is not supported");
        }

        for (plane, defaults) in self.prob.token.iter_mut().zip(&TOKEN_DEFAULT_PROBS) {
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

        self.filter.simple = self.c.get(buf) != 0;
        self.filter.level = self.c.get_uint(buf, 6) as u8;
        self.filter.sharpness = self.c.get_uint(buf, 3) as u8;

        self.lf_delta.enabled = self.c.get(buf) != 0;
        if self.lf_delta.enabled && self.c.get(buf) != 0 {
            self.update_lf_deltas(buf);
        }

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
                            self.prob.token[i][index as usize][k][l] = prob;
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
    fn decode_intra4x4_modes(&mut self, buf: &[u8], mb_x: usize) {
        for y in 0..4 {
            for x in 0..4 {
                let top = self.intra4x4_pred_mode_top[4 * mb_x + x] as usize;
                let left = self.intra4x4_pred_mode_left[y] as usize;
                let ctx = &PRED4X4_PROB_INTRA[top][left];
                let mode = self.c.get_tree(buf, &PRED4X4_TREE, ctx) as u8;

                self.intra4x4_pred_mode_mb[4 * y + x] = mode;
                self.intra4x4_pred_mode_left[y] = mode;
                self.intra4x4_pred_mode_top[4 * mb_x + x] = mode;
            }
        }
    }

    #[inline(always)]
    fn decode_mb_mode(&mut self, buf: &[u8], mb: &mut Macroblock, mb_x: usize) {
        if self.segmentation.update_map {
            let bit = self.c.get_prob(buf, self.prob.segmentid[0]) as usize;

            self.segment =
                self.c.get_prob(buf, self.prob.segmentid[1 + bit]) as usize + 2 * bit;
        } else {
            self.segment = 0;
        }

        mb.skip = self.mbskip_enabled && self.c.get_prob(buf, self.prob.mbskip) != 0;
        mb.mode = self
            .c
            .get_tree(buf, &PRED16X16_TREE_INTRA, &PRED16X16_PROB_INTRA);

        if mb.mode == MODE_I4 {
            self.decode_intra4x4_modes(buf, mb_x);
        } else {
            let mode = PRED4X4_MODE[mb.mode] as u8;

            self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4].fill(mode);
            self.intra4x4_pred_mode_left.fill(mode);
        }

        self.chroma_pred_mode =
            self.c.get_tree(buf, &PRED8X8C_TREE, &PRED8X8C_PROB_INTRA);
    }

    #[inline(always)]
    fn decode_mb_coeffs(
        &mut self,
        buf: &[u8],
        part: usize,
        mb: &mut Macroblock,
        mb_x: usize,
    ) {
        let mut nnz_total = 0;
        let mut luma_start = 0;
        let mut luma_ctx = 3;
        let mut block_dc = 0;
        let segment = self.segment;
        let mut t_nnz = self.top_nnz[mb_x];
        let mut l_nnz = self.left_nnz;

        if mb.mode != MODE_I4 {
            let nnz_pred = i32::from(t_nnz[8]) + i32::from(l_nnz[8]);
            let qmul = self.qmat[segment].luma_dc_qmul;
            let nnz = decode_block_coeffs(
                &mut self.coeff_partition[part],
                buf,
                &mut self.block_dc.0,
                &self.prob.token[1],
                0,
                nnz_pred,
                qmul,
            );

            t_nnz[8] = u8::from(nnz != 0);
            l_nnz[8] = u8::from(nnz != 0);
            if nnz != 0 {
                nnz_total += nnz;
                block_dc = 1;

                let luma: &mut [[i16; 16]; 16] =
                    (&mut self.block.0[..16]).try_into().unwrap();

                if nnz == 1 {
                    (self.dsp.luma_dc_wht_dc)(luma, &mut self.block_dc.0);
                } else {
                    (self.dsp.luma_dc_wht)(luma, &mut self.block_dc.0);
                }
            }
            luma_start = 1;
            luma_ctx = 0;
        }

        /* The indices address three things at once — the two contexts and
        the block at 4 * y + x — so an iterator over any one of them would put
        the other two back as indexing anyway. */
        #[allow(clippy::needless_range_loop)]
        for y in 0..4 {
            for x in 0..4 {
                let nnz_pred = i32::from(l_nnz[y]) + i32::from(t_nnz[x]);
                let qmul = self.qmat[segment].luma_qmul;
                let nnz = decode_block_coeffs(
                    &mut self.coeff_partition[part],
                    buf,
                    &mut self.block.0[4 * y + x],
                    &self.prob.token[luma_ctx],
                    luma_start,
                    nnz_pred,
                    qmul,
                );

                self.non_zero_count_cache[y][x] = (nnz + block_dc) as u8;
                t_nnz[x] = u8::from(nnz != 0);
                l_nnz[y] = u8::from(nnz != 0);
                nnz_total += nnz;
            }
        }

        for i in 4..6 {
            for y in 0..2 {
                for x in 0..2 {
                    let nnz_pred =
                        i32::from(l_nnz[i + 2 * y]) + i32::from(t_nnz[i + 2 * x]);
                    let qmul = self.qmat[segment].chroma_qmul;
                    let nnz = decode_block_coeffs(
                        &mut self.coeff_partition[part],
                        buf,
                        &mut self.block.0[4 * i + (y << 1) + x],
                        &self.prob.token[2],
                        0,
                        nnz_pred,
                        qmul,
                    );

                    self.non_zero_count_cache[i][(y << 1) + x] = nnz as u8;
                    t_nnz[i + 2 * x] = u8::from(nnz != 0);
                    l_nnz[i + 2 * y] = u8::from(nnz != 0);
                    nnz_total += nnz;
                }
            }
        }

        self.top_nnz[mb_x] = t_nnz;
        self.left_nnz = l_nnz;

        // An empty coefficient block skips both IDCT and the inner loop filter.
        if nnz_total == 0 {
            mb.skip = true;
        }
    }

    #[inline(always)]
    fn backup_mb_border(
        &mut self,
        planes: &Planes<'_>,
        mb_x: usize,
        off: [usize; 3],
        simple: bool,
    ) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();
        let border = &mut self.top_border[mb_x + 1];

        border[..16].copy_from_slice(&planes[0][off[0] + 15 * ls..][..16]);
        if !simple {
            for (i, p) in [1usize, 2].into_iter().enumerate() {
                let from = off[p] + 7 * uvls;

                border[16 + 8 * i..24 + 8 * i]
                    .copy_from_slice(&planes[p][from..from + 8]);
            }
        }
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn xchg_mb_border(
        &mut self,
        planes: &mut Planes<'_>,
        mb_x: usize,
        mb_y: usize,
        off: [usize; 3],
        simple: bool,
        swap: bool,
    ) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();
        let y = off[0] - ls;
        let cb = off[1] - uvls;
        let cr = off[2] - uvls;
        let this = mb_x + 1;
        let prev = mb_x;
        let last = mb_x == self.mb_width - 1;
        let border = &mut self.top_border;
        let [luma, cbp, crp] = planes;

        xchg8(border, prev, 8, luma, y - 8, swap);
        xchg8(border, this, 0, luma, y, swap);
        xchg8(border, this, 8, luma, y + 8, true);
        if !last {
            xchg8(border, this + 1, 0, luma, y + 16, true);
        }

        if !simple || mb_y == 0 {
            xchg8(border, prev, 16, cbp, cb - 8, swap);
            xchg8(border, prev, 24, crp, cr - 8, swap);
            xchg8(border, this, 16, cbp, cb, true);
            xchg8(border, this, 24, crp, cr, true);
        }
    }

    #[inline(always)]
    fn intra_predict(
        &mut self,
        planes: &mut Planes<'_>,
        mb: &Macroblock,
        off: [usize; 3],
        mb_x: usize,
        mb_y: usize,
    ) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();
        let simple = self.filter.simple;

        if self.deblock_filter || mb_y == 0 {
            self.xchg_mb_border(planes, mb_x, mb_y, off, simple, true);
        }

        if mb.mode < MODE_I4 {
            let mode = check_intra_pred8x8_mode(mb.mode, mb_x, mb_y);

            (self.pred.pred16x16[mode])(planes[0], off[0], ls);
        } else {
            let last = mb_x == self.mb_width - 1;

            if mb.skip {
                self.non_zero_count_cache[..4].fill([0; 4]);
            }

            let mut ptr = off[0];
            let luma = &mut *planes[0];
            // The four samples above and to the right of the macroblock, which
            // the last column of macroblocks has to fabricate.
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
                    let mode = self.intra4x4_pred_mode_mb[4 * y + x] as usize;

                    (self.pred.pred4x4[mode])(luma, ptr + 4 * x, ls, &topright);

                    let nnz = self.non_zero_count_cache[y][x];

                    if nnz != 0 {
                        let block = &mut self.block.0[4 * y + x];
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

        let mode = check_intra_pred8x8_mode(self.chroma_pred_mode, mb_x, mb_y);

        (self.pred.pred8x8[mode])(planes[1], off[1], uvls);
        (self.pred.pred8x8[mode])(planes[2], off[2], uvls);

        if self.deblock_filter || mb_y == 0 {
            self.xchg_mb_border(planes, mb_x, mb_y, off, simple, false);
        }
    }

    #[inline(always)]
    fn idct_mb(&mut self, planes: &mut Planes<'_>, mb: &Macroblock, off: [usize; 3]) {
        let ls = self.linesize();
        let uvls = self.uvlinesize();

        if mb.mode != MODE_I4 {
            let mut y_dst = off[0];
            let luma = &mut *planes[0];

            for y in 0..4 {
                let mut nnz4 = u32::from_le_bytes(self.non_zero_count_cache[y]);

                if nnz4 != 0 {
                    if nnz4 & !0x0101_0101 != 0 {
                        for x in 0..4 {
                            let n = nnz4 as u8;

                            if n == 1 {
                                (self.dsp.idct_dc_add)(
                                    luma,
                                    y_dst + 4 * x,
                                    ls,
                                    &mut self.block.0[4 * y + x],
                                );
                            } else if n > 1 {
                                (self.dsp.idct_add)(
                                    luma,
                                    y_dst + 4 * x,
                                    ls,
                                    &mut self.block.0[4 * y + x],
                                );
                            }
                            nnz4 >>= 8;
                            if nnz4 == 0 {
                                break;
                            }
                        }
                    } else {
                        let block: &mut [[i16; 16]; 4] =
                            (&mut self.block.0[4 * y..4 * y + 4]).try_into().unwrap();

                        (self.dsp.idct_dc_add4y)(luma, y_dst, ls, block);
                    }
                }
                y_dst += 4 * ls;
            }
        }

        for ch in 0..2 {
            let mut nnz4 = u32::from_le_bytes(self.non_zero_count_cache[4 + ch]);

            if nnz4 == 0 {
                continue;
            }
            let mut ch_dst = off[1 + ch];
            let chroma = &mut *planes[1 + ch];

            if nnz4 & !0x0101_0101 != 0 {
                'plane: for y in 0..2 {
                    for x in 0..2 {
                        let n = nnz4 as u8;
                        let block = &mut self.block.0[4 * (4 + ch) + (y << 1) + x];

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
                    (&mut self.block.0[base..base + 4]).try_into().unwrap();

                (self.dsp.idct_dc_add4uv)(chroma, ch_dst, uvls, block);
            }
        }
    }

    #[inline(always)]
    fn filter_level_for_mb(&self, mb: &Macroblock) -> FilterStrength {
        let mut filter_level = if self.segmentation.enabled {
            let level = i32::from(self.segmentation.filter_level[self.segment]);

            if self.segmentation.absolute_vals {
                level
            } else {
                level + i32::from(self.filter.level)
            }
        } else {
            i32::from(self.filter.level)
        };

        if self.lf_delta.enabled {
            filter_level += self.lf_delta.ref_intra;
            if mb.mode == MODE_I4 {
                filter_level += self.lf_delta.mode_i4;
            }
        }

        let filter_level = clip_uintp2(filter_level, 6);
        let mut interior_limit = filter_level;

        if self.filter.sharpness != 0 {
            interior_limit >>= (i32::from(self.filter.sharpness) + 3) >> 2;
            interior_limit = interior_limit.min(9 - i32::from(self.filter.sharpness));
        }
        interior_limit = interior_limit.max(1);

        FilterStrength {
            filter_level: filter_level as u8,
            inner_limit: interior_limit as u8,
            inner_filter: !mb.skip || mb.mode == MODE_I4,
        }
    }

    #[inline(always)]
    fn filter_mb(
        &mut self,
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

        let filter_level = i32::from(f.filter_level);

        if filter_level == 0 {
            return;
        }

        let inner_limit = i32::from(f.inner_limit);
        let bedge_lim = 2 * filter_level + inner_limit;
        let mbedge_lim = bedge_lim + 4;
        let hev = HEV_THRESH_LUT[filter_level as usize];
        let ls = self.linesize();
        let uvls = self.uvlinesize();
        let inner = f.inner_filter;
        let [y, u, v] = planes;

        if mb_x != 0 && inner {
            (self.dsp.h_loop_filter16y_mb)(
                y,
                off[0],
                ls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
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
            (self.dsp.h_loop_filter16y)(y, off[0], ls, mbedge_lim, inner_limit, hev);
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
            for k in 1..4 {
                (self.dsp.h_loop_filter16y_inner)(
                    y,
                    off[0] + 4 * k,
                    ls,
                    bedge_lim,
                    inner_limit,
                    hev,
                );
            }
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
            (self.dsp.v_loop_filter16y_mb)(
                y,
                off[0],
                ls,
                mbedge_lim,
                bedge_lim,
                inner_limit,
                hev,
            );
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
            (self.dsp.v_loop_filter16y)(y, off[0], ls, mbedge_lim, inner_limit, hev);
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
            for k in 1..4 {
                (self.dsp.v_loop_filter16y_inner)(
                    y,
                    off[0] + 4 * k * ls,
                    ls,
                    bedge_lim,
                    inner_limit,
                    hev,
                );
            }
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
    fn filter_mb_simple(
        &mut self,
        luma: &mut [u8],
        off: usize,
        f: FilterStrength,
        mb_x: usize,
        mb_y: usize,
    ) {
        let filter_level = i32::from(f.filter_level);

        if filter_level == 0 {
            return;
        }

        let inner_limit = i32::from(f.inner_limit);
        let bedge_lim = 2 * filter_level + inner_limit;
        let mbedge_lim = bedge_lim + 4;
        let ls = self.linesize();
        let inner = f.inner_filter;
        let y = luma;

        // The fused filter reads dst[-2..13], so it needs a macroblock to the
        // left.
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

        // The fused filter reads rows -2..13, so it needs a macroblock above.
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

    fn filter_mb_row(&mut self, planes: &mut Planes<'_>, mb_y: usize) {
        let mut off = [
            self.picture.planes[0].at(0, 16 * mb_y),
            self.picture.planes[1].at(0, 8 * mb_y),
            self.picture.planes[2].at(0, 8 * mb_y),
        ];

        for mb_x in 0..self.mb_width {
            self.backup_mb_border(planes, mb_x, off, false);
            self.filter_mb(planes, off, self.filter_strength[mb_x], mb_x, mb_y);
            off[0] += 16;
            off[1] += 8;
            off[2] += 8;
        }
    }

    fn filter_mb_row_simple(&mut self, luma: &mut [u8], mb_y: usize) {
        let ls = self.linesize();
        let mut off = self.picture.planes[0].at(0, 16 * mb_y);

        for mb_x in 0..self.mb_width {
            self.top_border[mb_x + 1][..16]
                .copy_from_slice(&luma[off + 15 * ls..][..16]);
            self.filter_mb_simple(luma, off, self.filter_strength[mb_x], mb_x, mb_y);
            off += 16;
        }
    }

    fn save_mb_state(&self, part: usize, mb_x: usize) -> ResumeState {
        ResumeState {
            c: self.c,
            part: self.coeff_partition[part],
            intra4x4_top: self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4]
                .try_into()
                .unwrap(),
            intra4x4_left: self.intra4x4_pred_mode_left,
            top_nnz: self.top_nnz[mb_x],
            left_nnz: self.left_nnz,
        }
    }

    fn restore_mb_state(&mut self, snap: &ResumeState, part: usize, mb_x: usize) {
        self.c = snap.c;
        self.coeff_partition[part] = snap.part;
        self.intra4x4_pred_mode_top[4 * mb_x..4 * mb_x + 4]
            .copy_from_slice(&snap.intra4x4_top);
        self.intra4x4_pred_mode_left = snap.intra4x4_left;
        self.top_nnz[mb_x] = snap.top_nnz;
        self.left_nnz = snap.left_nnz;
        self.block.0 = [[0; 16]; 24];
        self.block_dc.0 = [0; 16];
    }

    /// Starts a frame: parses the header and sets up the macroblock loop.
    pub fn frame_init(
        &mut self,
        chunk: &[u8],
        avail: usize,
        size: usize,
    ) -> Result<Status> {
        if size < 10 {
            return Err(Error::InvalidData);
        }
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

        self.deblock_filter = self.filter.level != 0 && !self.bypass_filtering;

        for row in self.top_nnz.iter_mut() {
            *row = [0; 9];
        }
        self.intra4x4_pred_mode_top.fill(pred::DC_PRED as u8);

        self.top_border[0] = [0; 32];
        self.top_border[0][15] = 127;
        self.top_border[0][23] = 127;
        for entry in self.top_border.iter_mut().skip(1) {
            entry.fill(127);
        }
        self.top_border[0][31] = 127;

        self.mb_x = 0;
        self.mb_y = 0;
        self.mb_rows_done = 0;
        self.open_partitions(chunk);
        Ok(Status::Done)
    }

    /// Takes note that more of the chunk has arrived.
    pub fn extend(&mut self, chunk: &[u8], avail: usize) {
        self.chunk_avail = avail;
        self.open_partitions(chunk);
    }

    /// Decodes as many macroblock rows as the chunk allows.
    pub fn decode_rows(&mut self, chunk: &[u8]) -> Result<Status> {
        self.decode_rows_tmpl(chunk, true)
    }

    /// Decodes a whole frame from a chunk that is known to be complete.
    pub fn decode_frame(&mut self, chunk: &[u8]) -> Result<()> {
        if self.frame_init(chunk, chunk.len(), chunk.len())? == Status::NeedMore {
            return Err(Error::InvalidData);
        }
        self.decode_rows_tmpl(chunk, false)?;
        Ok(())
    }

    /// Splits the picture into its three planes and runs the macroblock loop
    /// over them.
    ///
    /// The buffer is moved out of the decoder for the duration, which is what
    /// lets the split happen once per frame rather than once per access: the
    /// slices borrow a local, so every helper below still takes `&mut self`.
    /// The geometry stays behind, so `linesize` and friends keep working.
    fn decode_rows_tmpl(&mut self, chunk: &[u8], resumable: bool) -> Result<Status> {
        let mut data = std::mem::take(&mut self.picture.data);
        let ret = self.decode_rows_planes(&mut data, chunk, resumable);

        self.picture.data = data;
        ret
    }

    fn decode_rows_planes(
        &mut self,
        data: &mut [u8],
        chunk: &[u8],
        resumable: bool,
    ) -> Result<Status> {
        let g = self.picture.planes;
        let (head, third) = data.split_at_mut(g[2].base);
        let (first, second) = head.split_at_mut(g[1].base);
        let planes = &mut [
            &mut first[g[0].base..][..g[0].len],
            &mut second[..g[1].len],
            &mut third[..g[2].len],
        ];
        let start_row = if resumable { self.mb_y } else { 0 };

        for mb_y in start_row..self.mb_height {
            let part = mb_y & (self.num_coeff_partitions - 1);
            let mut mb_x0 = 0;
            let mut check = false;
            let mut off = [
                self.picture.planes[0].at(0, 16 * mb_y),
                self.picture.planes[1].at(0, 8 * mb_y),
                self.picture.planes[2].at(0, 8 * mb_y),
            ];

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
                self.left_nnz = [0; 9];
                self.intra4x4_pred_mode_left = [pred::DC_PRED as u8; 4];

                for (i, &at) in off.iter().enumerate() {
                    let rows = if i == 0 { 16 } else { 8 };
                    let stride = g[i].stride;

                    for y in 0..rows {
                        planes[i][at + y * stride - 1] = 129;
                    }
                }
                if mb_y == 1 {
                    self.top_border[0][15] = 129;
                    self.top_border[0][23] = 129;
                    self.top_border[0][31] = 129;
                }
            } else {
                off[0] += 16 * mb_x0;
                off[1] += 8 * mb_x0;
                off[2] += 8 * mb_x0;
            }

            for mb_x in mb_x0..self.mb_width {
                let mut mb = Macroblock::default();
                let snap = if resumable && check {
                    Some(self.save_mb_state(part, mb_x))
                } else {
                    None
                };

                self.decode_mb_mode(chunk, &mut mb, mb_x);

                if !mb.skip {
                    self.decode_mb_coeffs(chunk, part, &mut mb, mb_x);
                }

                if let Some(snap) = snap {
                    if self.coeff_partition[part].overran() {
                        self.restore_mb_state(&snap, part, mb_x);
                        self.mb_x = mb_x;
                        self.mb_y = mb_y;
                        return Ok(Status::NeedMore);
                    }
                }

                self.intra_predict(planes, &mb, off, mb_x, mb_y);

                if !mb.skip {
                    self.idct_mb(planes, &mb, off);
                } else {
                    self.left_nnz[..8].fill(0);
                    self.top_nnz[mb_x][..8].fill(0);

                    if mb.mode != MODE_I4 {
                        self.left_nnz[8] = 0;
                        self.top_nnz[mb_x][8] = 0;
                    }
                }

                if self.deblock_filter {
                    self.filter_strength[mb_x] = self.filter_level_for_mb(&mb);
                }

                off[0] += 16;
                off[1] += 8;
                off[2] += 8;
            }

            if self.deblock_filter {
                if self.filter.simple {
                    self.filter_mb_row_simple(planes[0], mb_y);
                } else {
                    self.filter_mb_row(planes, mb_y);
                }
            }

            if resumable {
                self.mb_x = 0;
                self.mb_rows_done = mb_y + 1;
            }
        }

        self.mb_y = self.mb_height;
        self.mb_rows_done = self.mb_height;
        Ok(Status::Done)
    }

    /// How many output rows are final, given how far the macroblock loop has
    /// got: the loop filter of the next row still reaches back into this one.
    pub fn rows_finalized(&self) -> i32 {
        const EXTRA: [i32; 3] = [0, 2, 8];

        if self.mb_rows_done >= self.mb_height {
            return self.height;
        }

        let kind = if !self.deblock_filter {
            0
        } else if self.filter.simple {
            1
        } else {
            2
        };
        let rows = 16 * self.mb_rows_done as i32 - EXTRA[kind];

        rows.clamp(0, self.height)
    }
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

/// Reads the coefficients of one block, returning the index one past the last
/// non-zero one.
///
/// The C reached this through a `goto` into the middle of a `do`-`while`, to
/// skip the end-of-block test on entry and after each run of zeros. The two
/// nested loops here have the same shape: the inner one is the zero run and
/// falls through to a coefficient, and the outer one tests for end of block
/// only after a coefficient has been read.
fn decode_coeffs_inner<'p>(
    c: &mut RangeCoder,
    buf: &[u8],
    block: &mut [i16; 16],
    probs: &'p [[[u8; NUM_DCT_TOKENS - 1]; 3]; 16],
    mut i: usize,
    mut token_prob: &'p [u8; NUM_DCT_TOKENS - 1],
    qmul: [i16; 2],
) -> i32 {
    loop {
        while !c.get_prob_branchy(buf, token_prob[1]) {
            i += 1;
            if i == 16 {
                return i as i32;
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
            return i as i32;
        }
        token_prob = &probs[i][next_ctx];
        if !c.get_prob_branchy(buf, token_prob[0]) {
            return i as i32;
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn decode_block_coeffs(
    c: &mut RangeCoder,
    buf: &[u8],
    block: &mut [i16; 16],
    probs: &[[[u8; NUM_DCT_TOKENS - 1]; 3]; 16],
    i: usize,
    zero_nhood: i32,
    qmul: [i16; 2],
) -> i32 {
    let token_prob = &probs[i][zero_nhood as usize];

    if !c.get_prob_branchy(buf, token_prob[0]) {
        return 0;
    }
    decode_coeffs_inner(c, buf, block, probs, i, token_prob, qmul)
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

    /// One allocation, three disjoint plane slices: what plane `p` hands out
    /// must not reach into plane `p + 1`.
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

    /// A frame that shrinks reuses the block, and must not be handed the
    /// previous frame's samples with it.
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
}
