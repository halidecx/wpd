pub const MAX_PIXELS: u32 = 1 << 20;

pub fn fits(data: &[u8]) -> bool {
    // Keep malformed headers in coverage; only a successfully read size can
    // exceed the harness budget. Raw codecs have no decoder options limit.
    wpd::api::info(data).map_or(true, |info| {
        wpd::api::Options {
            frame_size_limit: MAX_PIXELS,
            ..Default::default()
        }
        .fits(info.width, info.height)
    })
}
