//! A whole file through the safe API, which is the only target that reaches
//! the driver.
//!
//! The other three enter below it: `container` walks the RIFF without decoding
//! a pixel, and `vp8`/`vp8l` are handed a chunk the driver would have validated
//! first. What a caller can actually provoke is the composition of the two, so
//! this one drives what ships — both entry points, in every output format,
//! taking the first byte as the format so a mutation can move between them.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wpd::api::Decoder;
use wpd::image::Format;

const FORMATS: [Format; 16] = [
    Format::Yuv420p,
    Format::Yuva420p,
    Format::Argb,
    Format::Rgba,
    Format::Bgra,
    Format::Rgb,
    Format::Bgr,
    Format::ArgbPre,
    Format::RgbaPre,
    Format::BgraPre,
    Format::Rgb565,
    Format::Rgba4444,
    Format::Rgba4444Pre,
    Format::Bgr565,
    Format::Bgra4444,
    Format::Bgra4444Pre,
];

/// Enough frames to pass the end of any corpus animation, so the exhausted
/// path is reached rather than only the frames a file has.
const FRAMES: usize = 16;

fuzz_target!(|data: &[u8]| {
    let Some(&first) = data.first() else {
        return;
    };
    let format = FORMATS[first as usize % FORMATS.len()];

    let mut whole = Decoder::new();
    let _ = whole.set_format(format);

    if whole.open(data).is_ok() {
        let _ = whole.info();
        for i in 0..4 {
            let _ = whole.frame_info(i);
        }
        for _ in 0..FRAMES {
            if whole.next_frame().is_err() {
                break;
            }
        }
    }

    /* The same file arriving in pieces, decoded as far as it will go after
    each one, which is what puts a frame boundary inside a chunk. */
    let mut stream = Decoder::new();
    let _ = stream.set_format(format);

    if stream.open_stream().is_err() {
        return;
    }

    let step = (data.len() / 4).max(1);
    let mut offset = 0;

    while offset < data.len() {
        let end = (offset + step).min(data.len());
        if stream.append(&data[offset..end]).is_err() {
            return;
        }
        offset = end;

        for _ in 0..2 {
            if stream.next_frame().is_err() {
                break;
            }
        }
    }

    let _ = stream.end_of_stream();

    for _ in 0..FRAMES {
        if stream.next_frame().is_err() {
            break;
        }
    }
});
