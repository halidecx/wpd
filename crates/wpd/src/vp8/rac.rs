//! The VP5/VP6/VP8 binary range decoder.
//!
//! The C kept three raw pointers into the chunk and had to save and restore
//! them as byte offsets every time a streaming append moved the buffer. Here
//! the coder holds those offsets to begin with and takes the chunk as a slice
//! per call, so `wpd_vp56_save_offsets` and its partner have no counterpart:
//! a coder that outlives the buffer it was reading is not expressible.
//!
//! Two forms exist, as in the C. The 64-bit one consumes seven bytes per
//! refill and is what any 64-bit target uses; the 32-bit one refills two bytes
//! at a time and is what `force_rac32` and 32-bit targets get. They are
//! bit-exact with each other, which `scripts/rac32.sh` checks end to end.

pub const RAC_64: bool = cfg!(all(
    target_pointer_width = "64",
    not(feature = "force_rac32")
));

#[cfg(all(target_pointer_width = "64", not(feature = "force_rac32")))]
mod imp {
    #[derive(Clone, Copy, Default)]
    pub struct RangeCoder {
        value: u64,
        range: u32,
        bits: i32,
        pos: usize,
        buf_max: usize,
        end: usize,
        eof: bool,
    }

    impl RangeCoder {
        /// Nothing to prime: the 64-bit form fills its window on first use.
        pub fn prime(&mut self, _buf: &[u8]) {}

        /// Starts decoding at `start`, over the `size` bytes that follow it.
        pub fn new(start: usize, size: usize) -> Self {
            Self {
                value: 0,
                range: 255 - 1,
                bits: -8,
                pos: start,
                end: start + size,
                buf_max: if size >= 8 { start + size - 7 } else { start },
                eof: false,
            }
        }

        /// Grows the window this coder may read, after a streaming append made
        /// more of the same chunk available.
        pub fn extend(&mut self, end: usize) {
            self.end = end;
            self.buf_max = if end - self.pos >= 8 {
                end - 7
            } else {
                self.pos
            };
        }

        pub fn end(&self) -> usize {
            self.end
        }

        pub fn overran(&self) -> bool {
            self.eof
        }

        #[cold]
        fn load_final_bytes(&mut self, buf: &[u8]) {
            if self.pos < self.end {
                self.value = (self.value << 8) | u64::from(buf[self.pos]);
                self.pos += 1;
                self.bits += 8;
            } else if !self.eof {
                self.value <<= 8;
                self.bits += 8;
                self.eof = true;
            } else {
                self.bits = 0;
            }
        }

        #[inline(always)]
        fn refill(&mut self, buf: &[u8]) {
            if self.pos < self.buf_max {
                let word =
                    u64::from_be_bytes(buf[self.pos..self.pos + 8].try_into().unwrap());

                self.value = (self.value << 56) | (word >> 8);
                self.pos += 7;
                self.bits += 56;
            } else {
                self.load_final_bytes(buf);
            }
        }

        #[inline(always)]
        pub fn get_prob(&mut self, buf: &[u8], prob: u8) -> u32 {
            // Loaded before the rare refill call so it stays in a register.
            let mut range = self.range;

            if self.bits < 0 {
                self.refill(buf);
            }

            let pos = self.bits;
            let split = (range * u32::from(prob)) >> 8;
            let value = (self.value >> pos) as u32;
            let bit = u32::from(value > split);

            if bit != 0 {
                range -= split;
                self.value -= u64::from(split + 1) << pos;
            } else {
                range = split + 1;
            }

            let shift = 7 ^ (31 ^ range.leading_zeros() as i32);

            range <<= shift;
            self.bits -= shift;
            self.range = range - 1;
            bit
        }

        /// [`Self::get_prob`] where the caller branches on the result, so the
        /// two outcomes stay separate rather than being folded arithmetically.
        #[inline(always)]
        pub fn get_prob_branchy(&mut self, buf: &[u8], prob: u8) -> bool {
            let range = self.range;

            if self.bits < 0 {
                self.refill(buf);
            }

            let pos = self.bits;
            let split = (range * u32::from(prob)) >> 8;
            let value = (self.value >> pos) as u32;

            if value > split {
                let shift = 7 ^ (31 ^ (range - split).leading_zeros() as i32);

                self.value -= u64::from(split + 1) << pos;
                self.range = ((range - split) << shift) - 1;
                self.bits = pos - shift;
                true
            } else {
                let shift = 7 ^ (31 ^ (split + 1).leading_zeros() as i32);

                self.range = ((split + 1) << shift) - 1;
                self.bits = pos - shift;
                false
            }
        }

        /// Reads one equiprobable bit and applies it as the sign of `v`.
        #[inline(always)]
        pub fn get_signed(&mut self, buf: &[u8], v: i32) -> i32 {
            if self.bits < 0 {
                self.refill(buf);
            }

            let pos = self.bits;
            let split = self.range >> 1;
            let value = (self.value >> pos) as u32;
            let mask = ((split.wrapping_sub(value)) as i32) >> 31;

            self.bits -= 1;
            self.range = (self.range.wrapping_add(mask as u32)) | 1;
            self.value -= u64::from((split + 1) & mask as u32) << pos;
            (v ^ mask) - mask
        }
    }
}

#[cfg(not(all(target_pointer_width = "64", not(feature = "force_rac32"))))]
mod imp {
    static NORM_SHIFT: [u8; 256] = [
        8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
        3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[derive(Clone, Copy, Default)]
    pub struct RangeCoder {
        high: u32,
        bits: i32,
        pos: usize,
        end: usize,
        code_word: u32,
        eof: bool,
    }

    impl RangeCoder {
        pub fn new(start: usize, size: usize) -> Self {
            Self {
                high: 255,
                bits: -16,
                pos: start,
                end: start + size,
                code_word: 0,
                eof: size < 3,
            }
        }

        /// Reads the three bytes the constructor cannot, since it has no
        /// buffer. Kept separate so `new` stays a plain value constructor.
        pub fn prime(&mut self, buf: &[u8]) {
            let mut word = 0u32;

            for _ in 0..3 {
                let byte = if self.pos < self.end {
                    let b = buf[self.pos];
                    self.pos += 1;
                    b
                } else {
                    0
                };

                word = word << 8 | u32::from(byte);
            }
            self.code_word = word;
        }

        pub fn extend(&mut self, end: usize) {
            self.end = end;
        }

        pub fn end(&self) -> usize {
            self.end
        }

        pub fn overran(&self) -> bool {
            self.eof
        }

        #[inline(always)]
        fn renorm(&mut self, buf: &[u8]) -> u32 {
            let shift = i32::from(NORM_SHIFT[self.high as usize]);
            let mut bits = self.bits;
            let mut code_word = self.code_word;

            self.high <<= shift;
            code_word <<= shift;
            bits += shift;

            if bits >= 0 {
                if self.end - self.pos >= 2 {
                    let hi = u32::from(buf[self.pos]);
                    let lo = u32::from(buf[self.pos + 1]);

                    self.pos += 2;
                    code_word |= (hi << 8 | lo) << bits;
                    bits -= 16;
                } else if self.pos < self.end {
                    code_word |= u32::from(buf[self.pos]) << (bits + 8);
                    self.pos += 1;
                    bits -= 16;
                    self.eof = true;
                } else {
                    self.eof = true;
                }
            }
            self.bits = bits;
            code_word
        }

        #[inline(always)]
        pub fn get_prob(&mut self, buf: &[u8], prob: u8) -> u32 {
            let code_word = self.renorm(buf);
            let low = 1 + (((self.high - 1) * u32::from(prob)) >> 8);
            let low_shift = low << 16;
            let bit = u32::from(code_word >= low_shift);

            self.high = if bit != 0 { self.high - low } else { low };
            self.code_word = if bit != 0 {
                code_word - low_shift
            } else {
                code_word
            };
            bit
        }

        #[inline(always)]
        pub fn get_prob_branchy(&mut self, buf: &[u8], prob: u8) -> bool {
            let code_word = self.renorm(buf);
            let low = 1 + (((self.high - 1) * u32::from(prob)) >> 8);
            let low_shift = low << 16;

            if code_word >= low_shift {
                self.high -= low;
                self.code_word = code_word - low_shift;
                true
            } else {
                self.high = low;
                self.code_word = code_word;
                false
            }
        }

        #[inline(always)]
        pub fn get_signed(&mut self, buf: &[u8], v: i32) -> i32 {
            if self.get_prob(buf, 128) != 0 {
                -v
            } else {
                v
            }
        }
    }
}

pub use imp::RangeCoder;

impl RangeCoder {
    /// Starts a coder over `buf[start..start + size]`, doing whatever priming
    /// the underlying form needs.
    pub fn start(buf: &[u8], start: usize, size: usize) -> Self {
        let mut c = Self::new(start, size);

        c.prime(buf);
        c
    }

    #[inline(always)]
    pub fn get(&mut self, buf: &[u8]) -> u32 {
        self.get_prob(buf, 128)
    }

    pub fn get_uint(&mut self, buf: &[u8], bits: u32) -> i32 {
        let mut value = 0;

        for _ in 0..bits {
            value = (value << 1) | self.get(buf) as i32;
        }
        value
    }

    pub fn get_sint(&mut self, buf: &[u8], bits: u32) -> i32 {
        if self.get(buf) == 0 {
            return 0;
        }
        let v = self.get_uint(buf, bits);

        if self.get(buf) != 0 {
            -v
        } else {
            v
        }
    }

    /// Walks a binary tree whose nodes are `[left, right]` pairs, negative
    /// entries being leaves.
    #[inline(always)]
    pub fn get_tree(&mut self, buf: &[u8], tree: &[[i8; 2]], probs: &[u8]) -> usize {
        let mut i = 0i32;

        loop {
            let node = tree[i as usize];
            let bit = self.get_prob(buf, probs[i as usize]);

            i = i32::from(node[bit as usize]);
            if i <= 0 {
                return (-i) as usize;
            }
        }
    }

    /// Reads a coefficient's extra bits: one per non-zero probability.
    #[inline(always)]
    pub fn get_coeff(&mut self, buf: &[u8], probs: &[u8]) -> i32 {
        let mut v = 0;

        for &p in probs {
            if p == 0 {
                break;
            }
            v = (v << 1) + self.get_prob(buf, p) as i32;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_buffer_reports_overrun_rather_than_reading() {
        let buf = [];
        let mut c = RangeCoder::start(&buf, 0, 0);

        for _ in 0..64 {
            c.get(&buf);
        }
        assert!(c.overran());
    }

    #[test]
    fn a_uniform_stream_decodes_its_own_bits() {
        // 0x00 repeated keeps every equiprobable bit at zero; 0xff at one.
        let zeros = [0u8; 32];
        let ones = [0xffu8; 32];
        let mut a = RangeCoder::start(&zeros, 0, zeros.len());
        let mut b = RangeCoder::start(&ones, 0, ones.len());

        assert_eq!(a.get_uint(&zeros, 16), 0);
        assert_eq!(b.get_uint(&ones, 16), 0xffff);
    }

    #[test]
    fn a_tree_walk_ends_on_a_leaf() {
        let buf = [0u8; 16];
        let tree = [[-1i8, 1], [-2, -3]];
        let probs = [128u8, 128];
        let mut c = RangeCoder::start(&buf, 0, buf.len());

        assert_eq!(c.get_tree(&buf, &tree, &probs), 1);
    }
}
