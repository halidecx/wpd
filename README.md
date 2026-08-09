# wpd

WebP decoder supporting lossy (VP8) and lossless (VP8L) images, alpha, and
animation. VP8 decoding is intra-only: there is no motion compensation,
reference frame management, or inter-frame prediction.

## Build

```sh
meson setup build
meson compile -C build
```

The decoder executable is written to `build/wpd`. It reads a WebP file and
writes raw frames:

```sh
build/wpd [options] input.webp output.raw
```

Use `-f` or `--fmt` to select the output pixel format; the default `auto` uses
the decoded frame format. Use `-r` or `--repeat` to decode the input multiple
times in-process for benchmarking. Animated input writes its frames
sequentially to the output.

Besides `auto`, `yuv420p` and `yuva420p`, every packed format libwebp offers at
eight bits per channel is available, named after its byte order in memory:
`argb`, `rgba`, `bgra`, `rgb`, `bgr`, and the alpha-premultiplied `Argb`,
`rgbA` and `bgrA`. A lowercase letter marks the channels alpha has been
multiplied into.

The packed formats convert lossy frames with the same fixed-point BT.601
coefficients and fancy chroma upsampler libwebp uses, so the result is
bit-exact with the like-named libwebp colorspace. They are the only formats in
which an animation can be compared against libwebp, whose `WebPAnimDecoder`
composites in RGB only, and the only ones that give a lossy/lossless animation
a single output format. `rgb` and `bgr` drop transparency rather than
compositing it onto a background, matching libwebp; libwebp has no RGB
animation mode, so for animated input wpd composites in ARGB and drops alpha
last. Converting is not free, so `auto` remains the default.

Using `/dev/null` selects decode-only output and avoids serializing the decoded
picture:

```sh
build/wpd input.webp /dev/null
```

Use the MD5 muxer to hash the bytes that would be written as raw output, or
verify them directly without an output argument:

```sh
build/wpd --muxer md5 input.webp -
build/wpd --verify "$expected_md5" input.webp
```

Architecture-specific assembly is enabled automatically. Use
`meson setup build -Denable_asm=false` for a portable C-only build.

Use `--cpumask` to restrict the instruction sets the decoder dispatches to,
which is useful to compare an assembly path against the C fallback or a lower
SIMD level:

```sh
build/wpd --cpumask sse2 --muxer md5 input.webp -
build/wpd --cpumask none --muxer md5 input.webp -
```

Accepted names on x86 are `sse`, `sse2`, `ssse3`, `sse41`, `avx2` and `none`, on
32-bit Arm `armv6`, `neon` and `none`, and on AArch64 `neon` and `none`. Each
name enables that instruction set and everything below it; a decimal or
hexadecimal number sets the flag mask directly.

A mask cannot drop below the instruction set the binary was compiled for when
`trim_dsp` is enabled, since unreachable fallbacks are then optimized out.
`trim_dsp` defaults to `if-release`, so use `meson setup build -Dtrim_dsp=false`
to exercise every level; `wpd` warns when a mask cannot be honored for this
reason.

For benchmarks against libwebp, install its development libraries and build the
optional statically linked comparison binary explicitly:

```sh
meson compile -C build libwebpdec
./scripts/bench.sh
```

To use a particular static libwebp decoder instead of the system one, configure
the build with its path:

```sh
meson setup build -Dlibwebpdecoder=/path/to/libwebpdecoder.a
meson compile -C build libwebpdec
```

`libwebpdec` is not included in the default build.

## Test

```sh
meson test -C build
```

The checkasm dependency is detected through pkg-config or built as a Meson
subproject. The checkasm executable and test are enabled when assembly is
enabled.

Decode conformance tests against the `wpd-test-data` files are opt-in, since
that directory is not part of this repository:

```sh
meson configure build -Dtestdata_tests=true
meson test -C build --suite testdata
```

Each test decodes one file to one pixel format and compares the MD5 of the
decoded frames against the reference in `tests/meson.build`. Use
`-Dtestdata_dir=/path/to/wpd-test-data` if the data lives outside the source
root. Reference MD5s are regenerated with:

```sh
./build/wpd --muxer md5 -f FORMAT wpd-test-data/FILE.webp -
```

Detect the host architecture & run end-to-end tests with the corresponding
masks:

```sh
./scripts/testdata.sh
```
