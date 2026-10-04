use wpd::api::{Decoder, Options};
use wpd::image::Format;

type AlphaCase = (&'static [(usize, u8, u8)], u64);

fn pixel_hash(decoder: &mut Decoder<'_>) -> u64 {
    let picture = decoder.next_frame().unwrap().unwrap();
    picture
        .rows_of(0)
        .flatten()
        .fold(0xcbf29ce484222325u64, |hash, &value| {
            (hash ^ u64::from(value)).wrapping_mul(0x100000001b3)
        })
}

#[test]
fn compatibility_matches_libwebp_at_the_end_of_a_paletted_alpha_plane() {
    if cfg!(miri) {
        return;
    }
    // Public project testdata, mutated only in its compressed alpha stream.
    // Native libwebp 1.6.0 accepts the final symbol at EOF. The unchanged
    // colour stream and container isolate alpha decoding from other policies.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("wpd-test-data/odd_a_lossy.webp");
    let seed = std::fs::read(path).unwrap();
    let cases: &[AlphaCase] = &[
        (&[(231, 202, 200), (351, 128, 125)], 0x87ed788ad3cb8e12),
        (
            &[(384, 53, 49), (385, 57, 56), (669, 191, 36)],
            0x01cdcded58bfb418,
        ),
    ];
    for &(patch, expected) in cases {
        let mut bytes = seed.clone();
        for &(offset, old, new) in patch {
            assert_eq!(bytes[offset], old);
            bytes[offset] = new;
        }
        let mut strict = Decoder::new();
        strict.open(&bytes).unwrap();
        assert!(strict.next_frame().is_err());
        for n_threads in [1, 4] {
            let mut decoder = Decoder::new();
            decoder
                .set_options(Options {
                    libwebp_compat: true,
                    n_threads,
                    ..Options::default()
                })
                .unwrap();
            decoder.set_format(Format::Rgba).unwrap();
            decoder.open(&bytes).unwrap();
            assert_eq!(pixel_hash(&mut decoder), expected);
        }
        let mut decoder = Decoder::new();
        decoder
            .set_options(Options {
                libwebp_compat: true,
                ..Options::default()
            })
            .unwrap();
        decoder.set_format(Format::Rgba).unwrap();
        decoder.open_stream().unwrap();
        for part in bytes.chunks(97) {
            decoder.append(part).unwrap();
        }
        decoder.end_of_stream().unwrap();
        assert_eq!(pixel_hash(&mut decoder), expected);
    }
}
