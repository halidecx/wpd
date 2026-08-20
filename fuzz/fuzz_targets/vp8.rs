
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
