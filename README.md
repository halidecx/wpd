# wpd

A safe, fast Rust and assembly WebP decoder with a C ABI.

| Image               | Output   | image-webp (ms) | libwebp (ms) | wpd (ms) | libwebp / wpd |
| ------------------- | -------- | --------------: | -----------: | -------: | ------------: |
| lossy.webp          | YUV420P  |               — |       180.40 |   173.56 |         1.04x |
| simplelf-lossy.webp | YUV420P  |               — |       174.43 |   162.07 |         1.08x |
| anim_yuv.webp       | RGBA     |          359.85 |       190.90 |   169.18 |         1.13x |
| lossless.webp       | RGBA     |          231.50 |       195.75 |   111.27 |         1.76x |
| anim_rgb.webp       | RGBA     |          206.09 |       104.47 |    62.56 |         1.67x |
| a_lossy.webp        | YUVA420P |               — |        50.70 |    36.38 |         1.39x |
| anim_yuva.webp      | RGBA     |          776.54 |       494.37 |   374.46 |         1.32x |

## Build

Dependencies:

- Rust
- `nasm` (on x86)

```sh
meson setup build
meson compile -C build
build/wpd [options] input.webp output.raw
```

The build enables assembly by default. Compile with `-Denable_asm=false` for
memory safety guarantees at the cost of speed.

For minimal binary and library sizes:

```sh
meson setup build-minsize --buildtype=minsize
meson compile -C build-minsize
```

`-Dnightly_size=true` also needs nightly Rust and `rust-src`. The build then
produces `libwpd-sealed.a` alongside `libwpd.a`. The sealed static lib aborts
instead of unwinding on an internal panic. The build merges every object into
one monolithic object for downstream consumers.

## Library

`meson install -C build` installs the static/shared libraries, `wpd.h`, and
`wpd.pc`. The library exports only the `wpd_*` symbols declared in the header.

### C

See [`wpd.h`](include/wpd.h) for C API usage.

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

- `rows_of()` yields output-order rows, so flips don't need special stride
  handling
- `open_stream`, `append`, `end_of_stream`, and `partial_frame` provide
  streaming
- `update` and `UpdateBuffer` reuse an allocation

## Test

```sh
meson test -C build
meson configure build -Dtestdata_tests=true
meson test -C build --suite testdata
./scripts/stylecheck.sh
```

Test data is maintained at
[wpd-test-data](https://github.com/halidecx/wpd-test-data), which you can clone
into the `wpd/` root. `./scripts/testdata.sh` runs end-to-end assembly and
fallback checks.

For libwebp parity testing and benchmarking against alternative WebP decoders,
build the optional third-party test binaries:

```sh
meson configure build -Dbuildtype=release -Dtrim_dsp=false
meson compile -C build
meson compile -C build libwebpdec imagewebpdec wuffsdec
./scripts/bench.sh
THREADS=0 ./scripts/bench.sh
```

On Linux, use `taskset -c 0 ./scripts/bench.sh` to reproduce the CPU affinity
above. The harness exports raw Hyperfine timings to `build/bench/`; set
`BENCH_DIR` to select another directory. Output is discarded, but requested
format conversion still runs on every decode.

The default libwebp is the pinned Meson subproject. `-Dlibwebp=system` or
`-Dlibwebpdecoder=/path/to/libwebpdecoder.a` overrides it. `imagewebpdec` is the
same harness over the pure-Rust `image-webp` crate, and `wuffsdec` the same over
[Wuffs](https://github.com/google/wuffs) from a pinned Meson subproject;
`bench.sh` includes each whenever it has been built. Neither has planar YUV
output, so `bench.sh` also times lossy stills to RGBA, and Wuffs cannot decode
animations.

`bench.sh` times wpd on one thread. `THREADS` passes another count to wpd's
`--threads`, 0 meaning every processor, and any count above 1 turns on libwebp's
worker thread; image-webp and Wuffs are single-threaded.

## Credits

This project would not be possible without:

- [libwebp](https://chromium.googlesource.com/webm/libwebp), the reference WebP
  implementation
- [dav1d](https://www.videolan.org/projects/dav1d.html), the fastest open-source
  AV1 decoder
- [rav1d](https://github.com/memorysafety/rav1d), a Rust rewrite of dav1d
