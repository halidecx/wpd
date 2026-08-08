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

Use `-f` or `--fmt` to select `auto`, `yuv420p`, `yuva420p`, or `argb` output;
the default `auto` uses the decoded frame format. Use `-r` or `--repeat` to
decode the input multiple times in-process for benchmarking. Animated input
writes its frames sequentially to the output.

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
