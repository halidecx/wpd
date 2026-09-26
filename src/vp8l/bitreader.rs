pub const MAX_BITS: u32 = 24;
const LBITS: i32 = 64;
const WBITS: i32 = 32;

pub const TAIL_MARGIN: usize = 64;

/// Bytes `Fast` needs past its position at the start of a pixel: two refills
/// of a whole word each, and a whole word more to hand the stream back.
pub const FAST_MARGIN: usize = 24;

const BIT_MASK: [u32; MAX_BITS as usize + 1] = [
    0, 0x000001, 0x000003, 0x000007, 0x00000f, 0x00001f, 0x00003f, 0x00007f, 0x0000ff,
    0x0001ff, 0x0003ff, 0x0007ff, 0x000fff, 0x001fff, 0x003fff, 0x007fff, 0x00ffff,
    0x01ffff, 0x03ffff, 0x07ffff, 0x0fffff, 0x1fffff, 0x3fffff, 0x7fffff, 0xffffff,
];

#[derive(Clone, Copy, Default)]
pub struct BitReader {
    val: u64,
    pos: usize,
    bit_pos: i32,
    eos: bool,
}

impl BitReader {
    pub fn new(buf: &[u8]) -> Self {
        let prefetch = buf.len().min(8);
        let mut val = 0u64;

        for (i, &b) in buf[..prefetch].iter().enumerate() {
            val |= u64::from(b) << (8 * i);
        }
        Self {
            val,
            pos: prefetch,
            bit_pos: 0,
            eos: false,
        }
    }

    #[inline(always)]
    pub fn left(&self, buf: &[u8]) -> usize {
        buf.len() - self.pos
    }

    #[inline(always)]
    pub fn is_eos(&self, buf: &[u8]) -> bool {
        self.eos || (self.pos == buf.len() && self.bit_pos > LBITS)
    }

    #[inline(always)]
    fn set_eos(&mut self) {
        self.eos = true;
        self.bit_pos = 0;
    }

    #[inline(always)]
    fn shift_bytes(&mut self, buf: &[u8]) {
        while self.bit_pos >= 8 && self.pos < buf.len() {
            self.val >>= 8;
            self.val |= u64::from(buf[self.pos]) << (LBITS - 8);
            self.pos += 1;
            self.bit_pos -= 8;
        }
        if self.is_eos(buf) {
            self.set_eos();
        }
    }

    #[inline(always)]
    pub fn prefetch(&self) -> u32 {
        (self.val >> (self.bit_pos & (LBITS - 1))) as u32
    }

    #[inline(always)]
    pub fn advance(&mut self, n: i32) {
        self.bit_pos += n;
    }

    fn do_fill(&mut self, buf: &[u8]) {
        if self.pos + 8 < buf.len() {
            let word =
                u32::from_le_bytes(buf[self.pos..self.pos + 4].try_into().unwrap());

            self.val >>= WBITS;
            self.bit_pos -= WBITS;
            self.val |= u64::from(word) << (LBITS - WBITS);
            self.pos += 4;
            return;
        }
        self.shift_bytes(buf);
    }

    #[inline(always)]
    pub fn fill(&mut self, buf: &[u8]) {
        if self.bit_pos >= WBITS {
            self.do_fill(buf);
        }
    }

    #[inline(always)]
    pub fn bits(&mut self, buf: &[u8], n: u32) -> u32 {
        if !self.eos && n <= MAX_BITS {
            let v = self.prefetch() & BIT_MASK[n as usize];

            self.bit_pos += n as i32;
            self.shift_bytes(buf);
            return v;
        }
        self.set_eos();
        0
    }

    #[inline(always)]
    pub fn bit(&mut self, buf: &[u8]) -> u32 {
        self.bits(buf, 1)
    }

    /// Hands the stream to a `Fast` reader, if it is far enough from the end
    /// of `buf` for one pixel.
    #[inline(always)]
    pub fn fast(&self, buf: &[u8]) -> Option<Fast> {
        if self.eos || self.pos < 8 {
            return None;
        }
        // Positions stay in bytes: one in bits would overflow a 32-bit usize
        // past 512 MiB of stream.
        let bit_pos = self.bit_pos as usize;
        let mut fast = Fast {
            val: 0,
            avail: 0,
            pos: self.pos - 8 + (bit_pos >> 3),
        };

        if fast.pos + FAST_MARGIN > buf.len() {
            return None;
        }
        fast.refill(buf);
        fast.consume((bit_pos & 7) as u32);
        Some(fast)
    }

    /// Takes the stream back from a `Fast` reader, as if it had read the same
    /// bits itself.
    #[inline(always)]
    pub fn resume(&mut self, fast: &Fast, buf: &[u8]) {
        // The byte that holds the next unread bit, and how many of its bits
        // are read already.
        let avail = fast.avail as usize;
        let pos = fast.pos - avail.div_ceil(8);

        self.val = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
        self.pos = pos + 8;
        self.bit_pos = ((8 - avail % 8) % 8) as i32;
    }
}

/// The reader for the bulk of a chunk. It runs only while every load it makes
/// lies inside the buffer, so it never looks for the end of the stream, and a
/// refill tops it up to at least `FAST_BITS` without a branch.
#[derive(Clone, Copy)]
pub struct Fast {
    val: u64,
    avail: u32,
    pos: usize,
}

pub const FAST_BITS: u32 = 56;

impl Fast {
    #[inline(always)]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[inline(always)]
    pub fn refill(&mut self, buf: &[u8]) {
        let word = u64::from_le_bytes(buf[self.pos..self.pos + 8].try_into().unwrap());

        self.val |= word << (self.avail & 63);
        self.pos += 7 - ((self.avail >> 3) & 7) as usize;
        self.avail |= FAST_BITS;
    }

    #[inline(always)]
    pub fn peek(&self) -> u64 {
        self.val
    }

    #[inline(always)]
    pub fn consume(&mut self, n: u32) {
        self.val >>= n;
        self.avail -= n;
    }

    /// Consumes as many bits as the low byte of a table entry says, of at
    /// most 63. The shift looks at only the low six bits of its amount, so
    /// it takes the entry as it is, with no mask between the load of the
    /// entry and the bits that index the next one.
    #[inline(always)]
    pub fn consume_entry(&mut self, entry: u32) {
        self.val = self.val.wrapping_shr(entry);
        self.avail -= entry & 0xFF;
    }

    #[inline(always)]
    pub fn bits(&mut self, n: u32) -> u32 {
        let v = self.val as u32 & ((1u32 << n) - 1);

        self.consume(n);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_come_out_least_significant_first() {
        let buf = [0b1011_0010u8, 0x00, 0x00, 0x00];
        let mut br = BitReader::new(&buf);

        assert_eq!(br.bits(&buf, 2), 0b10);
        assert_eq!(br.bits(&buf, 3), 0b100);
        assert_eq!(br.bits(&buf, 3), 0b101);
    }

    #[test]
    fn reading_past_the_end_reports_eos_and_zeros() {
        let buf = [0xFFu8; 2];
        let mut br = BitReader::new(&buf);

        for _ in 0..16 {
            assert_eq!(br.bit(&buf), 1);
        }
        assert!(!br.is_eos(&buf));
        for _ in 0..64 {
            br.bit(&buf);
        }
        assert!(br.is_eos(&buf));
        assert_eq!(br.bits(&buf, 8), 0);
    }

    #[test]
    fn an_oversized_request_is_refused_rather_than_truncated() {
        let buf = [0xFFu8; 16];
        let mut br = BitReader::new(&buf);

        assert_eq!(br.bits(&buf, MAX_BITS + 1), 0);
        assert!(br.is_eos(&buf));
    }

    #[test]
    fn the_fast_reader_reads_and_hands_back_the_same_bits() {
        let buf: Vec<u8> = (0..256u32).map(|i| (i * 167 + 13) as u8).collect();

        for skip in 0..24 {
            let mut want = BitReader::new(&buf);
            let mut got = BitReader::new(&buf);

            want.bits(&buf, skip);
            got.bits(&buf, skip);

            let mut f = got.fast(&buf).unwrap();

            for i in 0..100u32 {
                let n = (i * 7 + skip) % 16;

                if i % 3 == 0 {
                    f.refill(&buf);
                }
                assert_eq!(f.bits(n), want.bits(&buf, n), "skip {skip}, read {i}");
                if f.pos() + FAST_MARGIN > buf.len() {
                    break;
                }
            }
            got.resume(&f, &buf);
            for n in [1, 9, 17, 24, 3] {
                assert_eq!(got.bits(&buf, n), want.bits(&buf, n), "skip {skip}");
            }
        }
    }

    #[test]
    fn the_fast_reader_is_refused_near_the_end() {
        let buf = [0xA5u8; FAST_MARGIN + 1];
        let mut br = BitReader::new(&buf);

        br.bits(&buf, 15);
        assert!(br.fast(&buf).is_some());
        br.bits(&buf, 1);
        assert!(br.fast(&buf).is_none());
    }
}
