//! The lossless decoder, driven as both a whole chunk and a resumable stream.
//!
//! The canvas is set from the input rather than left at zero, because an ALPH
//! chunk carries no dimensions of its own and the disagreement between what
//! the container promised and what the frame header says is a path of its own.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wpd::vp8l::{AlphaDst, Decoder, Target};

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let (head, payload) = data.split_at(2);
    let mut decoder = Decoder::new();
    let alpha_chunk = head[0] & 1 != 0;

    decoder.set_canvas(i32::from(head[0]), i32::from(head[1]));
    if alpha_chunk {
        let width = usize::from(head[0]);
        let mut plane = vec![0; width * usize::from(head[1])];
        let dst = AlphaDst {
            data: &mut plane,
            stride: width,
        };

        let _ = decoder.decode_frame(Target::Alpha, payload, true, Some(dst));
    } else {
        let _ = decoder.decode_frame(Target::Argb, payload, false, None);
    }

    decoder.reset();
    decoder.set_canvas(i32::from(head[0]), i32::from(head[1]));

    let split = payload.len() / 2;

    if decoder
        .still_step(&payload[..split], payload.len(), false)
        .is_ok()
    {
        let _ = decoder.still_peek();
        let _ = decoder.still_step(payload, payload.len(), true);
    }
});
