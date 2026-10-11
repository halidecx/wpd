pub const MAX_PIXELS: u32 = 1 << 20;

pub fn fits(data: &[u8]) -> bool {
    // Raw codec fuzzers have no frame-size option. Read dimensions without
    // allocating pixels, and still exercise headers that fail to parse.
    wpd::api::info(data).map_or(true, |info| {
        wpd::api::Options {
            frame_size_limit: MAX_PIXELS,
            ..Default::default()
        }
        .fits(info.width, info.height)
    })
}
