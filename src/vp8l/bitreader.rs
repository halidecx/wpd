pub const MAX_BITS: u32 = 24;
const LBITS: i32 = 64;
const WBITS: i32 = 32;

pub const TAIL_MARGIN: usize = 64;

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
}
