# wpd

A safe, fast Rust & assembly WebP decoder with a C ABI.

| Image               | [image-webp](https://crates.io/crates/image-webp) (0.2.4) | libwebp (523e304) | wpd (latest)        |
| ------------------- | --------------------------------------------------------- | ----------------- | ------------------- |
| lossy.webp          | 401.7ms (1.00x)                                           | 156.1ms (2.57x)   | **137.8ms (2.92x)** |
| simplelf-lossy.webp | 310.6ms (1.00x)                                           | 152.2ms (2.04x)   | **130.6ms (2.38x)** |
| anim_yuv.webp       | 260.5ms (1.00x)                                           | 131.8ms (1.98x)   | **119.8ms (2.17x)** |
| lossless.webp       | 226.1ms (1.00x)                                           | 177.3ms (1.28x)   | **140.5ms (1.61x)** |
| anim_rgb.webp       | 97.0ms (1.00x)                                            | 78.6ms (1.23x)    | **42.4ms (2.29x)**  |
| a_lossy.webp        | 116.4ms (1.00x)                                           | 36.6ms (3.18x)    | **25.5ms (4.56x)**  |
| anim_yuva.webp      | 575.7ms (1.00x)                                           | 308.0ms (1.87x)   | **263.8ms (2.18x)** |

## Build

Dependencies:
- Rust
- `nasm` (on x86)

```sh
meson setup build
meson compile -C build
build/wpd [options] input.webp output.raw
```

Assembly is enabled by default; compile with `-Denable_asm=false` for memory
safety guarantees at the expense of speed.

For minimal binary & library sizes:

```sh
meson setup build-minsize --buildtype=minsize
meson compile -C build-minsize
```

`-Dnightly_size=true` additionally needs nightly Rust plus `rust-src`.
`libwpd.a` is still built, but alongside it `libwpd-sealed.a` is built as well.
The sealed static lib aborts instead of unwinding when encountering an internal
panic, and merges every object into one monolithic object for downstream
consumers.

## Library

`meson install -C build` installs the static/shared libraries, `wpd.h`, and
`wpd.pc`. Only `wpd_*` symbols declared in the header are exported.

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
fallback checks. For libwebp parity testing and benchmarking against alternative
WebP decoders, build the optional third-party test binaries:

```sh
meson compile -C build libwebpdec
meson compile -C build imagewebpdec
./scripts/bench.sh
```

All test scripts:

Test scripts:

```sh
./scripts/bench.sh      # performance, wpd vs libwebpdec vs image-webp
./scripts/cmpbench.sh   # performance, old vs new wpd
./scripts/md5check.sh   # correctness, old vs new wpd
./scripts/testdata.sh   # asm vs fallback E2E correctness
./scripts/fuzz.sh       # robustness, damaged input under sanitizers
cargo +nightly fuzz run container|vp8l|vp8|e2e  # coverage-guided; e2e is the driver
./scripts/animcheck.sh  # correctness, bit-exact argb, wpd vs libwebp
./scripts/rac32.sh      # correctness, forced 32-bit range coder
./scripts/sanitize.sh   # memory safety, C harnesses under sanitizers
./scripts/rustsan.sh    # memory safety, Rust w/ ASan (needs nightly)
./scripts/miri.sh       # UB in the safe core (needs nightly)
./scripts/stylecheck.sh # format & lint codebase
```

## Credits

This project would not be possible without:

- [libwebp](https://chromium.googlesource.com/webm/libwebp), the reference WebP
  implementation
- [dav1d](https://www.videolan.org/projects/dav1d.html), the fastest open-source
  AV1 decoder
- [rav1d](https://github.com/memorysafety/rav1d), a Rust rewrite of dav1d

> Disclaimer: LLMs were heavily utilized in the creation of this project. Usage
> included Claude Fable 5, Claude Opus 5, GPT-5.6 Sol/Terra, and DeepSeek V4
> Flash.
