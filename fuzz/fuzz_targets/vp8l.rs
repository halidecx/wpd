#![no_main]

use libfuzzer_sys::fuzz_target;
use wpd::vp8l::{AlphaDst, Decoder, Target};

#[path = "../budget.rs"]
mod budget;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let (head, payload) = data.split_at(2);
    if !budget::fits(payload) {
        return;
    }
    let mut decoder = Decoder::new();
    let alpha_chunk = head[0] & 1 != 0;

    /* A second thread runs the inverse transforms beside the entropy decoder
     * of an image at least 192x192. */
    decoder.threads = 1 + usize::from(head[1] & 1);

    decoder.set_canvas(i32::from(head[0]), i32::from(head[1]));
    if alpha_chunk {
        let width = usize::from(head[0]);
        let mut plane = vec![0; width * usize::from(head[1])];
        let dst = AlphaDst {
            data: &mut plane,
            stride: width,
            unfilter: None,
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
