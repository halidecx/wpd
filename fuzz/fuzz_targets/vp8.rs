//! The lossy decoder, driven as a whole frame and as a partial one.
//!
//! `decode_rows` is reached through the resumable path, which is where the
//! decoder is asked to produce rows from a chunk that stops part way.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wpd::vp8::Decoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = Decoder::new();

    let _ = decoder.decode_frame(data);

    let mut decoder = Decoder::new();
    let split = data.len() / 2;

    if decoder
        .frame_init(&data[..split], split, data.len())
        .is_ok()
    {
        let _ = decoder.decode_rows(&data[..split]);
        decoder.extend(data, data.len());
        let _ = decoder.decode_rows(data);
    }
});
