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

## CLI metadata

```sh
build/wpd --info=json input.webp
build/wpd --icc-out profile.icc --exif-out exif.bin --xmp-out xmp.bin input.webp
```

`--info` retains its text output. `--info=json` writes one JSON object to stdout
after the complete image has decoded successfully:

```json
{
  "width": 2,
  "height": 1,
  "frame_count": 1,
  "loop_count": 0,
  "has_alpha": false,
  "has_icc": false,
  "durations_ms": [0],
  "chunks": [
    { "fourcc": "VP8L", "offset": 12, "size": 17, "complete": true }
  ]
}
```

Dimensions describe the original canvas, including with `--scale`. Durations are
the original milliseconds, without a minimum playback delay; a still has one
duration of 0. An animation loop count of 0 means infinite repetition. The
ordered chunk list describes top-level RIFF chunks; offsets point to their
FourCC, sizes exclude the header and padding, and `complete` includes padding.
Raw VP8/VP8L input has an empty chunk list. Arbitrary FourCC bytes outside
printable ASCII are escaped as `\u00XX`.

Metadata outputs contain the original chunk payload, without its header or
padding. Missing metadata produces an empty file. Extraction alone needs no
pixel output. It does not interpret EXIF orientation or apply colour profiles.
These options also work with `--stream`, `--loops`, and `--repeat`; JSON and
metadata are written once. JSON can accompany a pixel output file, but the pixel
output cannot also use stdout. Metadata paths must name files.

The existing input and frame size limits apply. `--max-output` limits decoded
pixel output; metadata is bounded by `--max-input`. Exit codes are 0 for
success, 1 for input, decoding, limits or output errors, and 2 for invalid
arguments. Failed decodes write no JSON or metadata, and leave existing metadata
output files untouched.

## CLI frame sequences

```sh
build/wpd --muxer frames input.webp output-frames
```

The `frames` muxer creates a new directory, writes `frame-000000.pam`,
`frame-000001.pam`, and so on, and publishes `manifest.json` after successful
decoding and output flushing. Every PAM file has straight RGBA pixels and its
own header. An existing output directory is refused.

The manifest has `canvas_width`, `canvas_height`, `loop_count`, `composited`,
`frame_count`, and an ordered `frames` array. Each frame has `file`, `width`,
`height`, `duration_ms`, `timestamp_ms`, `x`, and `y`. Durations are the
original milliseconds, including 0; timestamps are the start of each frame in
the first pass, starting at 0. A still has one frame with duration and
timestamp 0.

Frames are composited canvases by default. `--subframe` keeps raw frame sizes
and canvas offsets, and sets `composited` to false. `--scale` changes the frame
dimensions while the manifest's canvas dimensions describe the original file.
`--stream` is supported; only the first pass of `--loops` or `--repeat` is
written. The muxer requires RGBA output and a directory path, and `--max-output`
covers all PAM bytes and the complete manifest together.

A failed decode or write leaves partial output with `manifest.json.part`,
without a final `manifest.json`. Consumers should require the final manifest
before using a sequence.

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

GitHub Actions runs assembly and fallback tests, checkasm, format/lint checks,
the correctness scripts, C and Rust sanitizers, and a container Miri smoke
check. The corpus revision is pinned in `.github/workflows/ci.yml`. CI compares
the assembly and fallback tools with `md5check.sh` and `clicheck.sh`; comparing
an older release still needs an explicit baseline binary. Timing scripts
(`bench.sh` and `cmpbench.sh`) remain manual because shared CI runners do not
provide stable performance measurements.

`./scripts/fuzz-smoke.sh [seconds-per-target] [corpus-directory]` builds every
fuzz target and runs each for 15 seconds by default. It requires nightly Rust,
Python 3 and cargo-fuzz 0.13.2. It derives container and raw VP8/VP8L seeds from
the test corpus without changing its WebP files, and leaves generated seeds and
failure artifacts under `fuzz/`. Longer fuzzing and the full safe-core Miri
suite (`./scripts/miri.sh`) are useful local checks before releases. The
decoding harnesses bound pictures to 1,048,576 pixels so mutated dimensions fit
the smoke run's memory budget; larger pictures remain in ordinary corpus tests.
This is a harness limit, not a decoder limit.

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
