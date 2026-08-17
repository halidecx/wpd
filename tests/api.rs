//! The safe API against the real corpus.
//!
//! `wpd-test-data` supplies the shapes the unit tests cannot reach —
//! animations, alpha, every packed output format, and a decode fed a byte at
//! a time.

use std::fs;
use std::path::PathBuf;

use wpd::api::{Animation, Decoder, Options};
use wpd::image::Format;

fn corpus() -> Vec<PathBuf> {
    /* miri interprets every instruction, so decoding the corpus in eight
    formats under it would take hours. The unit tests reach the same paths on
    a one-pixel file, which is what that run is for. */
    if cfg!(miri) {
        return Vec::new();
    }

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wpd-test-data")
        .canonicalize()
        .expect("wpd-test-data is missing");
    let entries = fs::read_dir(dir).expect("cannot read wpd-test-data");
    let mut files: Vec<PathBuf> = entries
        .map(|e| e.expect("cannot read a wpd-test-data entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "webp"))
        .collect();

    files.sort();
    assert!(!files.is_empty(), "wpd-test-data contains no WebP files");
    files
}

const FORMATS: [Format; 6] = [
    Format::Argb,
    Format::Rgba,
    Format::Bgra,
    Format::Rgb,
    Format::RgbaPre,
    Format::Rgb565,
];

/// Every file, every packed format, every frame: the rows come out the length
/// the geometry says and the frame count matches the header.
#[test]
fn the_corpus_decodes_in_every_packed_format() {
    for path in corpus() {
        let bytes = fs::read(&path).unwrap();

        for format in FORMATS {
            let mut d = Decoder::new();

            d.set_format(format).unwrap();
            d.open(&bytes)
                .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));

            let info = d.info().unwrap();
            let mut frames = 0;

            while let Some(picture) = d.next_frame().unwrap() {
                assert_eq!(picture.format(), format, "{}", path.display());
                assert_eq!(picture.planes(), 1);
                for row in picture.rows_of(0) {
                    assert_eq!(row.len(), picture.width() as usize * format.bpp());
                }
                frames += 1;
            }
            assert_eq!(frames, info.frame_count, "{}", path.display());
        }
    }
}

/// A planar decode hands out three or four planes, and the chroma ones are
/// half size in both directions.
#[test]
fn a_planar_decode_hands_out_subsampled_chroma() {
    for path in corpus() {
        let bytes = fs::read(&path).unwrap();
        let mut d = Decoder::new();

        d.set_format(Format::Yuva420p).unwrap();
        d.open(&bytes)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));
        while let Some(picture) = d.next_frame().unwrap() {
            assert_eq!(picture.planes(), 4);
            assert_eq!(picture.rows_of(0).count(), picture.height() as usize);
            assert_eq!(
                picture.rows_of(1).count(),
                wpd::image::ceil_rshift(picture.height(), 1) as usize
            );
            for row in picture.rows_of(1) {
                assert_eq!(
                    row.len(),
                    wpd::image::ceil_rshift(picture.width(), 1) as usize
                );
            }
        }
    }
}

/// A stream fed in small pieces reaches the same pixels as one whole open.
#[test]
fn a_stream_reaches_the_same_pixels_as_a_whole_file() {
    for path in corpus() {
        let bytes = fs::read(&path).unwrap();
        let mut whole = Decoder::new();

        whole.set_format(Format::Rgba).unwrap();
        whole
            .open(&bytes)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));

        let mut streamed = Decoder::new();

        streamed.set_format(Format::Rgba).unwrap();
        streamed.open_stream().unwrap();
        for chunk in bytes.chunks(97) {
            streamed.append(chunk).unwrap();
        }
        streamed.end_of_stream().unwrap();

        loop {
            let want: Option<Vec<Vec<u8>>> = whole
                .next_frame()
                .unwrap()
                .map(|p| p.rows_of(0).map(<[u8]>::to_vec).collect());
            let got: Option<Vec<Vec<u8>>> = streamed
                .next_frame()
                .unwrap()
                .map(|p| p.rows_of(0).map(<[u8]>::to_vec).collect());

            assert_eq!(want.is_some(), got.is_some(), "{}", path.display());
            match (want, got) {
                (Some(want), Some(got)) => assert_eq!(want, got, "{}", path.display()),
                _ => break,
            }
        }
    }
}

/// Sub-frame mode hands out each frame at its own position rather than the
/// composited canvas, so a sub-frame may be smaller than the canvas.
#[test]
fn sub_frame_mode_reports_a_position() {
    for path in corpus() {
        let bytes = fs::read(&path).unwrap();
        let mut d = Decoder::new();

        d.set_format(Format::Argb).unwrap();
        d.set_animation(Animation::Subframe).unwrap();
        d.open(&bytes)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));

        let info = d.info().unwrap();

        while let Some(picture) = d.next_frame().unwrap() {
            let (x, y) = picture.position();

            assert!(x >= 0 && y >= 0);
            assert!(x + picture.width() <= info.width);
            assert!(y + picture.height() <= info.height);
        }
    }
}

/// A flip is a reading order: the same rows come out in the opposite order.
#[test]
fn flipping_reverses_the_rows() {
    for path in corpus() {
        let bytes = fs::read(&path).unwrap();
        let mut plain = Decoder::new();

        plain.set_format(Format::Rgba).unwrap();
        plain
            .open(&bytes)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));

        let mut flipped = Decoder::new();

        flipped.set_format(Format::Rgba).unwrap();
        flipped
            .set_options(Options {
                flip: true,
                ..Options::default()
            })
            .unwrap();
        flipped.open(&bytes).unwrap();

        while let Some(want) = plain.next_frame().unwrap() {
            let want: Vec<Vec<u8>> = want.rows_of(0).map(<[u8]>::to_vec).collect();
            let got = flipped.next_frame().unwrap().expect("a matching frame");
            let got: Vec<Vec<u8>> = got.rows_of(0).map(<[u8]>::to_vec).collect();

            assert_eq!(want.len(), got.len(), "{}", path.display());
            for (a, b) in want.iter().zip(got.iter().rev()) {
                assert_eq!(a, b, "{}", path.display());
            }
        }
    }
}
