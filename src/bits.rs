pub fn rl16(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8
}

pub fn rl24(b: &[u8]) -> u32 {
    rl16(b) | u32::from(b[2]) << 16
}

pub fn rl32(b: &[u8]) -> u32 {
    rl24(b) | u32::from(b[3]) << 24
}

pub fn quad(b: &[u8], at: usize) -> [u8; 4] {
    let mut out = [0; 4];

    for (i, slot) in out.iter_mut().enumerate() {
        *slot = b.get(at + i).copied().unwrap_or(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_little_endian() {
        let b = [0x78, 0x56, 0x34, 0x12];

        assert_eq!(rl16(&b), 0x5678);
        assert_eq!(rl24(&b), 0x345678);
        assert_eq!(rl32(&b), 0x12345678);
    }

    #[test]
    fn a_quad_past_the_end_reads_as_zero() {
        let b = [1, 2];

        assert_eq!(quad(&b, 0), [1, 2, 0, 0]);
        assert_eq!(quad(&b, 1), [2, 0, 0, 0]);
        assert_eq!(quad(&b, 2), [0; 4]);
        assert_eq!(quad(&b, 99), [0; 4]);
    }
}
