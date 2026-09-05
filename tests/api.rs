use std::fs;
use std::path::PathBuf;

use wpd::api::{Animation, Decoder, Options};
use wpd::image::Format;

fn corpus() -> Vec<PathBuf> {
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

/// Every decode, at every thread count, must produce the same bytes. Counts
/// that are not powers of two matter once work is divided into bands and
/// batches rather than handed over whole.
#[test]
fn the_thread_count_does_not_change_a_single_byte() {
    const COUNTS: [i32; 6] = [1, 2, 3, 5, 8, 16];

    fn decode(bytes: &[u8], format: Format, n_threads: i32, subframe: bool) -> Vec<u8> {
        let mut d = Decoder::new();

        d.set_format(format).unwrap();
        d.set_options(Options {
            n_threads,
            ..Options::default()
        })
        .unwrap();
        if subframe {
            d.set_animation(Animation::Subframe).unwrap();
        }
        d.open(bytes).unwrap();

        let mut out = Vec::new();

        while let Some(picture) = d.next_frame().unwrap() {
            out.extend_from_slice(&picture.width().to_le_bytes());
            out.extend_from_slice(&picture.height().to_le_bytes());
            for plane in 0..picture.planes() {
                for row in picture.rows_of(plane) {
                    out.extend_from_slice(row);
                }
            }
        }
        out
    }

    for path in corpus() {
        let bytes = fs::read(&path).unwrap();

        for format in [Format::Rgba, Format::RgbaPre, Format::Rgb565, Format::Argb] {
            for subframe in [false, true] {
                let want = decode(&bytes, format, 1, subframe);

                assert!(!want.is_empty(), "{} decoded nothing", path.display());
                for n_threads in COUNTS {
                    assert_eq!(
                        decode(&bytes, format, n_threads, subframe),
                        want,
                        "{} in {format:?} at {n_threads} threads",
                        path.display()
                    );
                }
            }
        }
    }
}

/// Scaling splits the planes across threads and the rescaler carries state
/// down each one, so a scaled decode is swept separately.
#[test]
fn a_scaled_decode_is_the_same_at_any_thread_count() {
    fn decode(bytes: &[u8], scale: (i32, i32), n_threads: i32) -> Vec<u8> {
        let mut d = Decoder::new();

        d.set_format(Format::Rgba).unwrap();
        d.set_options(Options {
            n_threads,
            scale: Some(scale),
            ..Options::default()
        })
        .unwrap();
        d.open(bytes).unwrap();

        let mut out = Vec::new();

        while let Some(picture) = d.next_frame().unwrap() {
            for row in picture.rows_of(0) {
                out.extend_from_slice(row);
            }
        }
        out
    }

    for path in corpus() {
        let bytes = fs::read(&path).unwrap();

        for scale in [(64, 64), (0, 37), (320, 0)] {
            let want = decode(&bytes, scale, 1);

            for n_threads in [2, 3, 5, 8] {
                assert_eq!(
                    decode(&bytes, scale, n_threads),
                    want,
                    "{} scaled to {scale:?} at {n_threads} threads",
                    path.display()
                );
            }
        }
    }
}

/// Settings changed between frames apply to the frames that follow. A batch
/// decoded ahead bakes in the settings that were current when it ran, so it
/// has to be dropped when they change; otherwise the frames still in the
/// batch come out under the old ones and the decode stops agreeing with the
/// same call sequence on one thread.
#[test]
fn settings_changed_mid_animation_reach_the_next_frame() {
    type Switch = fn(&mut Decoder);

    fn decode(bytes: &[u8], n_threads: i32, switch: Switch) -> Vec<u8> {
        let mut d = Decoder::new();

        d.set_format(Format::Rgba).unwrap();
        d.set_options(Options {
            n_threads,
            ..Options::default()
        })
        .unwrap();
        d.open(bytes).unwrap();

        let mut out = Vec::new();
        let mut index = 0;

        loop {
            /* After the first frame, which is what fills the batch. */
            if index == 1 {
                switch(&mut d);
            }
            let Some(picture) = d.next_frame().unwrap() else {
                break;
            };

            for plane in 0..picture.planes() {
                for row in picture.rows_of(plane) {
                    out.extend_from_slice(row);
                }
            }
            index += 1;
        }
        out
    }

    let switches: [(&str, Switch); 2] = [
        ("format", |d| d.set_format(Format::RgbaPre).unwrap()),
        ("options", |d| {
            d.set_options(Options {
                no_fancy_upsampling: true,
                ..Options::default()
            })
            .unwrap();
        }),
    ];

    for path in corpus() {
        let bytes = fs::read(&path).unwrap();

        for (name, switch) in switches {
            let want = decode(&bytes, 1, switch);

            for n_threads in [2, 3, 5, 8, 16] {
                assert_eq!(
                    decode(&bytes, n_threads, switch),
                    want,
                    "{} switching {name} at {n_threads} threads",
                    path.display()
                );
            }
        }
    }
}
