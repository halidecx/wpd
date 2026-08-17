//! MD5, for the `md5` muxer and `--verify`.
//!
//! Not a security primitive here: it is a compact way to say "these decoded
//! bytes are the ones libwebp produced", which is what `scripts/md5check.sh`
//! and the testdata suite compare.

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14,
    20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11,
    16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

pub struct Md5 {
    state: [u32; 4],
    block: [u8; 64],
    len: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            block: [0; 64],
            len: 0,
        }
    }

    fn compress(&mut self) {
        let mut m = [0u32; 16];

        for (w, chunk) in m.iter_mut().zip(self.block.chunks_exact(4)) {
            *w = u32::from_le_bytes(chunk.try_into().unwrap());
        }

        let [mut a, mut b, mut c, mut d] = self.state;

        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;

            d = c;
            c = b;
            b = b.wrapping_add(
                f.wrapping_add(a)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = tmp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        let mut used = (self.len % 64) as usize;

        self.len = self.len.wrapping_add(data.len() as u64);
        if used != 0 {
            let take = data.len().min(64 - used);

            self.block[used..used + take].copy_from_slice(&data[..take]);
            data = &data[take..];
            used += take;
            if used < 64 {
                return;
            }
            self.compress();
        }
        while data.len() >= 64 {
            self.block.copy_from_slice(&data[..64]);
            self.compress();
            data = &data[64..];
        }
        self.block[..data.len()].copy_from_slice(data);
    }

    pub fn finish(mut self) -> [u8; 16] {
        let bits = self.len.wrapping_mul(8);
        let used = (self.len % 64) as usize;
        let pad = if used < 56 { 56 - used } else { 120 - used };

        self.update(&[0x80]);
        for _ in 1..pad {
            self.update(&[0]);
        }
        self.update(&bits.to_le_bytes());

        let mut digest = [0u8; 16];

        for (out, word) in digest.chunks_exact_mut(4).zip(self.state) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        digest
    }
}

pub fn hex(digest: &[u8; 16]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &[(&str, &str)] = &[
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ];

    fn digest_in_chunks(input: &str, chunk: usize) -> String {
        let mut md5 = Md5::new();

        for part in input.as_bytes().chunks(chunk.max(1)) {
            md5.update(part);
        }
        hex(&md5.finish())
    }

    #[test]
    fn the_reference_vectors_hash_correctly() {
        for (input, expected) in VECTORS {
            assert_eq!(&digest_in_chunks(input, input.len() + 1), expected);
        }
    }

    #[test]
    fn feeding_a_byte_at_a_time_gives_the_same_digest() {
        for (input, expected) in VECTORS {
            assert_eq!(&digest_in_chunks(input, 1), expected);
        }
    }

    #[test]
    fn a_block_aligned_input_pads_a_whole_extra_block() {
        let input = "x".repeat(64);

        assert_eq!(digest_in_chunks(&input, 64), digest_in_chunks(&input, 7));
    }
}
