# wpd

A safe, fast Rust and assembly WebP decoder with a C ABI.

| Image               | [image-webp](https://crates.io/crates/image-webp) (0.2.4) | libwebp (523e304) | wpd (latest)        |
| ------------------- | --------------------------------------------------------- | ----------------- | ------------------- |
| lossy.webp          | 414.2ms (1.00x)                                           | 163.2ms (2.54x)   | **118.9ms (3.48x)** |
| simplelf-lossy.webp | 321.1ms (1.00x)                                           | 159.2ms (2.02x)   | **119.1ms (2.70x)** |
| anim_yuv.webp       | 272.0ms (1.00x)                                           | 136.7ms (1.99x)   | **108.7ms (2.50x)** |
| lossless.webp       | 232.0ms (1.00x)                                           | 182.8ms (1.27x)   | **66.2ms (3.50x)**  |
| anim_rgb.webp       | 100.5ms (1.00x)                                           | 87.1ms (1.15x)    | **28.4ms (3.54x)**  |
| a_lossy.webp        | 121.6ms (1.00x)                                           | 39.1ms (3.11x)    | **19.5ms (6.24x)**  |
| anim_yuva.webp      | 592.7ms (1.00x)                                           | 317.7ms (1.87x)   | **245.5ms (2.41x)** |

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
meson compile -C build libwebpdec
meson compile -C build imagewebpdec
./scripts/bench.sh
```

The default libwebp is the pinned Meson subproject. `-Dlibwebp=system` or
`-Dlibwebpdecoder=/path/to/libwebpdecoder.a` overrides it. `imagewebpdec` is the
same harness over the pure-Rust `image-webp` crate, which `bench.sh` includes
whenever it has been built.

## Credits

This project would not be possible without:

- [libwebp](https://chromium.googlesource.com/webm/libwebp), the reference WebP
  implementation
- [dav1d](https://www.videolan.org/projects/dav1d.html), the fastest open-source
  AV1 decoder
- [rav1d](https://github.com/memorysafety/rav1d), a Rust rewrite of dav1d
