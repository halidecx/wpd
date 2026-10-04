# VP8 transform-width regressions

These are the VP8 payloads of the eleven damaged lossy files found by the
2026-10-03 differential fuzz run. The payload of
`ef1d46488254374abe677fc99c4a1b29bc6c8f21` is identical to
`05386ead209a72d4411d4d0eadd0f9ed6235730c`, so ten fixtures cover eleven cases.
Names are the original libFuzzer corpus identifiers. The tests wrap each payload
in a fresh, valid simple WebP container to isolate transform arithmetic.

The 160x160 files came from `segment01.webp` and `segment02.webp` in the public
[libwebp test data](https://chromium.googlesource.com/webm/libwebp-test-data/+/06ddd96/).
The three 640x480 files came from a synthetic image generated with ImageMagick
(`-seed 1 -size 640x480 plasma:fractal -attenuate 0.3 +noise Gaussian`) and
encoded by cwebp 1.6.0 at quality 75. None came from private images.

`tests/vp8_compat.rs` pins FNV-1a hashes of visible Y, U and V bytes decoded
with libwebp 1.6.0's portable C decoder. To reproduce an oracle, wrap a payload
in a RIFF `WEBP` / `VP8` chunk and run
`dwebp -noasm -yuv input.webp -o output.yuv`. The tests do not require a native
decoder or downloaded corpus.

[RFC 6386 sections 14.3 and 14.4](https://www.rfc-editor.org/rfc/rfc6386.html#section-14.3)
describe signed 16-bit intermediate buffers. Libwebp's C transforms keep the
first pass in 32-bit integers. Seven cases exceed the DCT first-pass width, one
exceeds the WHT first-pass width, and three exceed only the DCT second-pass
width. The latter three agree with default wpd and libwebp C; libwebp's native
x86 SSE2 path differs. Its DCT second-pass additions wrap to 16 bits before
shifting. Consequently a damaged file can have CPU-dependent pixels within
libwebp itself. The opt-in mode targets portable C arithmetic and does not claim
to match every libwebp CPU implementation.
