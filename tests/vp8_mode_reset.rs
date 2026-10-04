use wpd::vp8::Decoder;

fn visible_planes(decoder: &Decoder) -> Vec<Vec<u8>> {
    (0..3)
        .map(|p| {
            let plane = decoder.picture.planes[p];
            let bytes = decoder.picture.plane(p);
            let (width, height) = if p == 0 {
                (decoder.width as usize, decoder.height as usize)
            } else {
                (
                    (decoder.width as usize).div_ceil(2),
                    (decoder.height as usize).div_ceil(2),
                )
            };
            (0..height)
                .flat_map(|y| {
                    let at = plane.origin + y * plane.stride;
                    bytes[at..at + width].iter().copied()
                })
                .collect()
        })
        .collect()
}

// This separate test binary starts before any API decoder initializes CPU
// detection, making the raw scalar-to-assembly transition reproducible.
#[test]
fn compatibility_restores_the_original_raw_decoder_tables() {
    let vp8 =
        include_bytes!("data/vp8-compat/05386ead209a72d4411d4d0eadd0f9ed6235730c.vp8");
    let mut decoder = Decoder::new();
    decoder.decode_frame(vp8).unwrap();
    let strict = visible_planes(&decoder);

    // Another API decoder publishes detected CPU flags while this raw
    // decoder retains the tables it selected at construction.
    let _other = wpd::api::Decoder::new();
    for _ in 0..2 {
        decoder.libwebp_compat = true;
        decoder.decode_frame(vp8).unwrap();
        assert_ne!(strict, visible_planes(&decoder));

        decoder.libwebp_compat = false;
        decoder.decode_frame(vp8).unwrap();
        for (p, (expected, actual)) in
            strict.iter().zip(visible_planes(&decoder)).enumerate()
        {
            assert!(expected == &actual, "visible plane {p} changed after reset");
        }
    }
}
