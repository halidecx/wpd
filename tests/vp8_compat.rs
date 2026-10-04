use wpd::api::{Decoder, Options};
use wpd::image::Format;

const CASES: &[(&[u8], u64)] = &[
    (
        include_bytes!("data/vp8-compat/05386ead209a72d4411d4d0eadd0f9ed6235730c.vp8"),
        0xd5985d6a806cbc3e,
    ),
    (
        include_bytes!("data/vp8-compat/1106e5ea9977c4e83826977b05a664ba311fd396.vp8"),
        0x606a790c9aa52794,
    ),
    (
        include_bytes!("data/vp8-compat/2fa90d04ce17374a075194e72510319d4f12e5f1.vp8"),
        0xddf085515fdc19fc,
    ),
    (
        include_bytes!("data/vp8-compat/31ad5278df83871478b68166dc93f71a9d6ca96a.vp8"),
        0x662636aaaac3ffdb,
    ),
    (
        include_bytes!("data/vp8-compat/3d641847c558f1e8e204ae6b9ae7a39829df71e8.vp8"),
        0xaaf3d69cff46af8d,
    ),
    (
        include_bytes!("data/vp8-compat/41232e41e66c25526a68befb54c530f39385239e.vp8"),
        0xc9114c262ae54373,
    ),
    (
        include_bytes!("data/vp8-compat/45c6d1e69830bad6ca4e19014559805c7309a07a.vp8"),
        0x66bf1476e887b1f4,
    ),
    (
        include_bytes!("data/vp8-compat/d4e4675457e402210e2e72ea9e61f93c40a81a68.vp8"),
        0x3abd600787eaf9fc,
    ),
    // ef1d46488254374abe677fc99c4a1b29bc6c8f21 has the same VP8 chunk as 05386.
    (
        include_bytes!("data/vp8-compat/05386ead209a72d4411d4d0eadd0f9ed6235730c.vp8"),
        0xd5985d6a806cbc3e,
    ),
    (
        include_bytes!("data/vp8-compat/fd4b9d6c836f6b8c5d6e6705e4e98261dfabb0be.vp8"),
        0xa09d93c6d9f17892,
    ),
    (
        include_bytes!("data/vp8-compat/fddf820775b3266dc39aeadc299d34df5c895299.vp8"),
        0x7c9c719b14554032,
    ),
];

fn webp(vp8: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    let padded = vp8.len() + (vp8.len() & 1);
    bytes.extend_from_slice(&(padded as u32 + 12).to_le_bytes());
    bytes.extend_from_slice(b"WEBPVP8 ");
    bytes.extend_from_slice(&(vp8.len() as u32).to_le_bytes());
    bytes.extend_from_slice(vp8);
    bytes.resize(20 + padded, 0);
    bytes
}

// FNV-1a over visible Y, U and V rows, in dwebp -yuv order. These values
// were measured with libwebp 1.6.0's portable C decoder (dwebp -noasm).
fn pixels(decoder: &mut Decoder<'_>) -> u64 {
    let picture = decoder.next_frame().unwrap().unwrap();
    assert_eq!(picture.format(), Format::Yuv420p);
    let mut hash = 0xcbf29ce484222325u64;
    for plane in 0..3 {
        for row in picture.rows_of(plane) {
            for &value in row {
                hash = (hash ^ u64::from(value)).wrapping_mul(0x100000001b3);
            }
        }
    }
    hash
}

#[test]
fn compatibility_matches_libwebp_c_on_every_damaged_lossy_case() {
    for &(vp8, expected) in CASES {
        let bytes = webp(vp8);
        for n_threads in [1, 2, 4] {
            let mut decoder = Decoder::new();
            decoder.set_format(Format::Yuv420p).unwrap();
            decoder
                .set_options(Options {
                    libwebp_compat: true,
                    n_threads,
                    ..Options::default()
                })
                .unwrap();
            decoder.open(&bytes).unwrap();
            assert_eq!(pixels(&mut decoder), expected);
        }
    }
}

#[test]
fn incremental_compatibility_uses_the_same_transform_arithmetic() {
    for &(vp8, expected) in CASES {
        let bytes = webp(vp8);
        let mut decoder = Decoder::new();
        decoder.set_format(Format::Yuv420p).unwrap();
        decoder
            .set_options(Options {
                libwebp_compat: true,
                ..Options::default()
            })
            .unwrap();
        decoder.open_stream().unwrap();
        for part in bytes.chunks(97) {
            decoder.append(part).unwrap();
        }
        decoder.end_of_stream().unwrap();
        assert_eq!(pixels(&mut decoder), expected);
    }
}

#[test]
fn compatibility_preserves_the_valid_corpus_pixels() {
    if cfg!(miri) {
        return;
    }
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("wpd-test-data");
    for entry in std::fs::read_dir(corpus).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|ext| ext == "webp") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let mut hashes = Vec::new();
        for compat in [false, true] {
            let mut decoder = Decoder::new();
            decoder
                .set_options(Options {
                    libwebp_compat: compat,
                    n_threads: 4,
                    ..Options::default()
                })
                .unwrap();
            decoder.set_format(Format::Rgba).unwrap();
            decoder.open(&bytes).unwrap();
            let mut frames = Vec::new();
            while let Some(picture) = decoder.next_frame().unwrap() {
                let mut hash = 0xcbf29ce484222325u64;
                for row in picture.rows_of(0) {
                    for &value in row {
                        hash = (hash ^ u64::from(value)).wrapping_mul(0x100000001b3);
                    }
                }
                frames.push(hash);
            }
            hashes.push(frames);
        }
        assert_eq!(hashes[0], hashes[1], "{}", path.display());
    }
}
