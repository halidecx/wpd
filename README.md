# wpd

A fast Rust and hand-written-assembly WebP decoder with a C ABI.

| Image               | image-webp (0.2.4) | libwebp (523e304) | wpd (latest)        |
| ------------------- | ------------------ | ----------------- | ------------------- |
| lossy.webp          | 401.7ms (1.00x)    | 156.1ms (2.57x)   | **137.8ms (2.92x)** |
| simplelf-lossy.webp | 310.6ms (1.00x)    | 152.2ms (2.04x)   | **130.6ms (2.38x)** |
| anim_yuv.webp       | 260.5ms (1.00x)    | 131.8ms (1.98x)   | **119.8ms (2.17x)** |
| lossless.webp       | 226.1ms (1.00x)    | 177.3ms (1.28x)   | **140.5ms (1.61x)** |
| anim_rgb.webp       | 97.0ms (1.00x)     | 78.6ms (1.23x)    | **42.4ms (2.29x)**  |
| a_lossy.webp        | 116.4ms (1.00x)    | 36.6ms (3.18x)    | **25.5ms (4.56x)**  |
| anim_yuva.webp      | 575.7ms (1.00x)    | 308.0ms (1.87x)   | **263.8ms (2.18x)** |

## Build and CLI

Requires Rust and, on x86, `nasm`.

```sh
meson setup build
meson compile -C build
build/wpd [options] input.webp output.raw
```

`-f`/`--fmt` selects output: `auto` (default), `yuv420p`, `yuva420p`, packed
`argb`, `rgba`, `bgra`, `rgb`, `bgr`, premultiplied `Argb`, `rgbA`, `bgrA`, or
16-bit `rgb565`, `rgba4444`, `rgbA4444`. Animated frames are written in order.
`/dev/null` decodes without writing. `--muxer md5 input.webp -` hashes output;
`--verify MD5 input.webp` verifies it. `-r` repeats decoding for benchmarks.

Assembly is enabled by default; use `-Denable_asm=false` for safe scalar
fallbacks. `--cpumask` restricts dispatch (`none` or the platform SIMD level).
With release `trim_dsp`, lower fallbacks may be removed; disable it with
`-Dtrim_dsp=false` when testing them.

For smaller artifacts:

```sh
meson setup build-minsize --buildtype=minsize
meson compile -C build-minsize
```

`-Dnightly_size=true` additionally needs nightly Rust plus `rust-src`; it builds
`libwpd.a` and smaller, sealed `libwpd-sealed.a`. The latter roots all public
APIs and sacrifices downstream dead-code selection. It aborts rather than
unwinds on an internal Rust panic.

## Library

`meson install -C build` installs the static/shared libraries, `wpd.h`, and
`wpd.pc`. Only `wpd_*` symbols declared in the header are exported.

### C

```c
#include <wpd.h>

WPDImageInfo info = WPD_IMAGE_INFO_INIT;
WPDDecoder *decoder = wpd_decoder_create();
WPDFrame frame = WPD_FRAME_INIT;

if (wpd_get_info(data, size, &info) == WPD_OK &&
    wpd_decoder_open(decoder, data, size) == WPD_OK)
    while (wpd_decoder_next_frame(decoder, &frame) > 0)
        present(&frame);
wpd_decoder_free(decoder);
```

`wpd_get_info()` only reads headers. Frames normally borrow decoder-owned memory
until the next decode call. To decode directly into application memory, set a
`WPDOutputBuffer` with `wpd_decoder_set_output_buffer()`; packed output uses
plane 0, while planar output uses Y/U/V/(A), and negative strides flip a plane.
For a still or an animation's first frame, `wpd_decode()` returns an independent
frame which must be released with `wpd_frame_free()`.

Options control crop, scale, vertical flip, lossy filtering, and fancy chroma
upsampling. Crop precedes scale; a zero scale dimension is inferred. Cropping,
scaling, and packed conversion follow libwebp behavior (including its native
YUV/ARGB crop rules and alpha-aware scaling).

`wpd_decoder_open()` copies complete input; `wpd_decoder_open_borrowed()` avoids
that copy when input remains valid. For incremental input, use
`wpd_decoder_open_stream()`, `wpd_decoder_append()` or zero-copy
`wpd_decoder_update()`, then `wpd_decoder_end_of_stream()`. A zero return from
`wpd_decoder_next_frame()` before the end means more data is needed.
`wpd_decoder_partial_frame()` exposes completed rows for stills. Metadata flags
are in `info.metadata`; fetch bytes with `wpd_decoder_metadata()`. ICC, EXIF,
and XMP are exposed but not applied. Failures return `WPDStatus`; use
`wpd_decoder_error()` for diagnostics.

### Rust

```toml
[dependencies]
wpd = { git = "https://github.com/halidecx/wpd" }
```

```rust
use wpd::{api::Decoder, image::Format, options::Options};

let mut decoder = Decoder::new();
decoder.set_format(Format::Rgba)?;
decoder.set_options(Options { scale: Some((320, 0)), ..Default::default() })?;
decoder.open(&data)?;
while let Some(frame) = decoder.next_frame()? {
    for row in frame.rows_of(0) {
        present(row);
    }
}
```

Rust frames borrow the decoder; `rows_of()` yields output-order rows, so flips
need no special stride handling. `open_stream`, `append`, `end_of_stream`, and
`partial_frame` provide streaming; `update` and `UpdateBuffer` reuse a
cumulative allocation. Disable default features for safe scalar fallbacks and
`#![forbid(unsafe_code)]` support.

## Test

```sh
meson test -C build
meson configure build -Dtestdata_tests=true
meson test -C build --suite testdata
./scripts/stylecheck.sh
```

Test data is external; use `-Dtestdata_dir=/path/to/wpd-test-data` when needed.
`./scripts/testdata.sh` runs end-to-end assembly and fallback checks. For
libwebp parity and benchmarking, build the optional harness:

```sh
meson compile -C build libwebpdec
meson compile -C build imagewebpdec
./scripts/bench.sh
```

The default libwebp is the pinned Meson subproject; `-Dlibwebp=system` or
`-Dlibwebpdecoder=/path/to/libwebpdecoder.a` overrides it. `imagewebpdec` is the
same harness over the pure-Rust `image-webp` crate, which `bench.sh` includes
whenever it has been built. Other checks include `fuzz.sh`, `sanitize.sh`,
`rustsan.sh`, `miri.sh`, `rac32.sh`, and
`cargo +nightly fuzz run container|vp8l|vp8|e2e`. Decode test outputs into
`wpd-test-data`; never delete its reference WebP files.
