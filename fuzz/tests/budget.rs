#[path = "../budget.rs"]
mod budget;

#[test]
fn raw_dimensions_are_bounded_without_decoding_pixels() {
    for (width, height, fits) in [
        (1024, 1024, true),
        (1025, 1024, false),
        (16384, 14336, false),
    ] {
        let bits = (width - 1) | (height - 1) << 14;
        let mut lossless = vec![0x2f];

        lossless.extend_from_slice(&u32::to_le_bytes(bits));
        assert_eq!(budget::fits(&lossless), fits);
        if width < 16384 {
            let mut lossy = vec![0x10, 0, 0, 0x9d, 1, 0x2a];

            lossy.extend_from_slice(&(width as u16).to_le_bytes());
            lossy.extend_from_slice(&(height as u16).to_le_bytes());
            assert_eq!(budget::fits(&lossy), fits);
        }
    }
}

#[test]
fn malformed_headers_remain_in_the_fuzzing_domain() {
    for data in [
        &b""[..],
        &b"RIFF"[..],
        &[0x2f, 0, 0][..],
        &b"not a header"[..],
    ] {
        assert!(budget::fits(data));
    }
}
