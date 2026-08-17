# wpd

WebP decoder supporting lossy (VP8) and lossless (VP8L) images, alpha, and
animation. VP8 decoding is intra-only: there is no motion compensation,
reference frame management, or inter-frame prediction.

## Build

```sh
meson setup build
meson compile -C build
```

Meson drives the build and owns the tools and test harnesses; the library itself
is built by cargo, which also assembles the hand-written assembly. A Rust
toolchain and `nasm` (on x86) are therefore build requirements. The decoder is
written in Rust and hand-written assembly, ported from the C the project started
as — see [LOG.md](LOG.md) — and it keeps the same public C ABI, declared by the
single header `include/wpd.h`.

The decoder executable is written to `build/wpd`. It reads a WebP file and
writes decoded frames:

```sh
build/wpd [options] input.webp output.raw
```

Use `-f` or `--fmt` to select the output pixel format; the default `auto` uses
the decoded frame format. Use `-r` or `--repeat` to decode the input multiple
times in-process for benchmarking. Animated input writes its frames sequentially
to the output.

Besides `auto`, `yuv420p` and `yuva420p`, packed output is available in: `argb`,
`rgba`, `bgra`, `rgb`, `bgr`, and the alpha-premultiplied `Argb`, `rgbA` and
`bgrA`. The 16-bit packed formats are `rgb565`, `rgba4444` and premultiplied
`rgbA4444`. A lowercase letter marks the channels alpha has been multiplied
into.

The packed formats convert lossy frames with the same fixed-point BT.601
coefficients and fancy chroma upsampler libwebp uses, so the result is bit-exact
with the like-named libwebp colorspace. They are the only formats in which an
animation can be compared against libwebp, whose `WebPAnimDecoder` composites in
RGB only, and the only ones that give a lossy/lossless animation a single output
format. `rgb` and `bgr` drop transparency rather than compositing it onto a
background, matching libwebp; libwebp has no RGB animation mode, so for animated
input wpd composites in ARGB and drops alpha last. Converting is not free, so
`auto` remains the default.

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
`meson setup build -Denable_asm=false` for a portable build with the safe Rust
fallbacks alone.

## Library

`meson install -C build` installs the shared and static libraries, the single
public header `wpd.h`, and a `wpd.pc` for pkg-config. Only the `wpd_*` entry
points declared in `wpd.h` are exported.

```c
#include <wpd.h>

WPDImageInfo info = WPD_IMAGE_INFO_INIT;
if (wpd_get_info(data, size, &info) != WPD_OK)
    return;  /* not a WebP, or the headers are incomplete */

WPDDecoder *decoder = wpd_decoder_create();
WPDOutputBuffer out = WPD_OUTPUT_BUFFER_INIT;
WPDFrame frame = WPD_FRAME_INIT;

out.plane[0].data = pixels;
out.plane[0].size = pixels_size;
out.plane[0].stride = 4 * info.width;

wpd_decoder_set_output_format(decoder, WPD_PIX_FMT_RGBA);
wpd_decoder_set_output_buffer(decoder, &out);  /* optional */
if (wpd_decoder_open(decoder, data, size) == WPD_OK)
    while (wpd_decoder_next_frame(decoder, &frame) > 0)
        present(&frame);
wpd_decoder_free(decoder);
```

`wpd_get_info()` reads only the headers; it does not decode, allocate, or retain
`data`. Without `wpd_decoder_set_output_buffer()` frames borrow decoder memory
that the next call invalidates; with it they are written straight into
caller-owned memory. Packed formats use `plane[0]`; planar output uses separate
Y, U, V and optional A planes. A negative stride reverses a plane vertically.

For a still image or the first frame of an animation, the one-shot API owns the
finished pixels independently of a decoder:

```c
WPDFrame frame = WPD_FRAME_INIT;
if (wpd_decode(data, size, WPD_PIX_FMT_RGBA, NULL, &frame) == WPD_OK)
    present(&frame);
wpd_frame_free(&frame);
```

`WPDDecoderOptions` controls cropping, scaling, vertical flipping, lossy in-loop
filtering and fancy chroma upsampling. Cropping precedes scaling. Setting one
scaled dimension to zero infers it from the other, rounded up. A lossy frame is
cropped in its native YUV, so its crop origin is rounded down to even
coordinates; a lossless frame is cropped in ARGB and takes the origin exactly,
as it does in libwebp.

Scaling is the same area rescaler libwebp uses, applied where libwebp applies
it: over ARGB for a lossless frame and over the Y, U and V planes for a lossy
one, in both cases with the colour weighted by alpha across the rescaler so
transparent edges do not bleed. Asking a lossy frame to scale brings chroma up
through the rescaler rather than the fancy upsampler, and a steep enough
downscale — under three quarters in both directions — drops the in-loop filter,
so scaled output is not the unscaled output resampled. Both are libwebp's
behaviour, and scaled output is bit-exact with it.

Decoded output is checked against libwebp byte for byte:

```sh
meson test -C build --suite parity
```

decodes every still in the test data in every output format, plain, cropped and
scaled, and fails on any difference. The libwebp it measures against is a pinned
Meson subproject rather than whatever is installed, since a released libwebp
predates decoder changes wpd already follows; `-Dlibwebp=system` uses the
installed one and skips the comparisons an older one would fail.

`./scripts/fuzz.sh` covers the other half: it truncates and corrupts the test
data and pushes the result through every entry point, whole-file and streamed,
borrowed and copied, under AddressSanitizer and UndefinedBehaviorSanitizer. It
looks for crashes rather than pixels.

The `huffman_*` stills cover what neither reaches: bitstreams the format permits
but no encoder produces, so no ordinary corpus file carries them and random
mutation will not stumble into them either. A simple Huffman code naming the
same symbol twice is the case they were written for — it has to collapse to a
single-symbol code consuming no bits, and counting it as two silently
desynchronises everything after it, without reading out of range or crashing.
`wpd-test-data/scripts/mk_huffman_codes.py` writes them out bit by bit, along
with the code shapes either side of that one and codes long enough to need
secondary tables. Their expected pixels come from the specification rather than
from a decode, and `huffman_simple_duplicate` and `huffman_simple_single` differ
in bytes while having to decode identically.

`./scripts/sanitize.sh` runs the ordinary suite under the same two sanitizers,
once with the assembly enabled and once without. The decoder is Rust, so what
ASan instruments there is the C test harnesses; the decoder benefits through the
intercepted allocator, which still catches a heap overrun that crosses a
redzone, and not through instrumented loads and stores.

`./scripts/rustsan.sh` closes that gap. It needs a nightly toolchain, because
both `-Zsanitizer=address` and the `-Zbuild-std` that gets an instrumented
standard library are unstable, and it decodes the whole corpus in every output
format with every load and store the decoder makes checked, in both feature
configurations. The no-asm run is the one where a bad index in a fallback has
nowhere to hide; the asm run exercises the real dispatch.

`./scripts/miri.sh` runs the core crate's tests under miri, which reports
undefined behaviour the compiler is otherwise entitled to assume away. It builds
with `--no-default-features`, since miri cannot execute the hand-written
assembly.

`cargo +nightly fuzz run container|vp8l|vp8|e2e` drives the safe core under
coverage-guided fuzzing. That is a different question from what
`scripts/fuzz.sh` asks: this one is looking for a panic on damaged input, which
is a denial of service the C did not have, rather than for a memory error.

The first three enter below the driver, so each one reaches its own decoder
without the validation a real file passes through first; `e2e` drives a whole
file through the safe API, which is what a caller can actually provoke. Seed it
with the corpus — `cargo +nightly fuzz run e2e fuzz/corpus/e2e wpd-test-data` —
because a file that reaches the pixels is not something a mutation finds on its
own.

The boolean coder has a 64-bit implementation and a 32-bit one, and every 64-bit
build picks the former, so `./scripts/rac32.sh` runs the whole suite again
against a `-Dforce_rac32=true` build to keep the latter honest.

`wpd_decoder_open()` copies input. `wpd_decoder_open_borrowed()` avoids that
copy when the complete input remains alive and unchanged for the decoder's
lifetime. Both want the whole file: one that stops inside a chunk is rejected
with `WPD_ERR_TRUNCATED`, and one carrying no image at all with
`WPD_ERR_BITSTREAM`, so a file still arriving belongs on the streaming API
below. For a cumulative streaming buffer, `wpd_decoder_update()` provides the
complete prefix at each call without copying; it is the zero-copy alternative to
`wpd_decoder_append()`.

In addition to normal RIFF WebP files, header inspection and decoding accept
bare VP8 and VP8L payloads and the internal `ALPH`-plus-`VP8` chunk sequence.
Nothing wraps those, so their length is only known once the stream ends: a
streamed bare payload yields no frame and no partial rows until
`wpd_decoder_end_of_stream()`.

A file that is still arriving can be decoded as it comes in. Frames become
available a whole frame at a time, so an animation starts playing while the rest
downloads:

```c
wpd_decoder_open_stream(decoder);
while ((n = read(fd, buf, sizeof(buf))) > 0) {
    if (wpd_decoder_append(decoder, buf, n) < 0)
        break;
    while (wpd_decoder_next_frame(decoder, &frame) > 0)
        present(&frame);
}
if (wpd_decoder_end_of_stream(decoder) == WPD_OK)
    while (wpd_decoder_next_frame(decoder, &frame) > 0)
        present(&frame);
```

While a stream is open, `wpd_decoder_next_frame()` returning 0 means "not yet"
rather than end of stream; `wpd_decoder_end_of_stream()` is what makes 0 final,
and reports `WPD_ERR_TRUNCATED` if the file stopped inside a chunk.

A still image is only handed over once it is complete, but it decodes rows as
the bytes arrive, so a large photo can be shown filling in rather than all at
once:

```c
if (wpd_decoder_partial_frame(decoder, &frame, &rows) == WPD_OK && rows > 0)
    present_rows(&frame, rows);
```

Rows below `rows` are final and will not change; the rest of the frame is
whatever the decoder happens to have written. `rows` reaches the image height
when the frame is done, and stays 0 for animations, which decode a whole frame
at a time. A lossy still gives rows away a macroblock row at a time, a lossless
one in blocks of sixteen. Nothing is consumed, so the finished frame still
arrives from `wpd_decoder_next_frame()`. Asking a lossless still for rows costs
one more image buffer, because its backward references go on reading the
untransformed pixels; a stream nobody asks costs nothing extra.

Input the decoder can no longer look at is dropped as it goes, so a long
animation does not accumulate the whole file in memory. A still image is the
exception: its image chunk has to be kept whole until the frame is done.
`build/wpd --stream N` drives this path from the command line, appending N bytes
at a time, and prints the row progression with `--info`.

`info.metadata` reports the ICC profile and EXIF and XMP metadata the file says
it carries, and `wpd_decoder_metadata()` hands over the bytes:

```c
const uint8_t *icc;
size_t icc_size;
if (wpd_decoder_metadata(decoder, WPD_METADATA_ICCP, &icc, &icc_size) == WPD_OK
    && icc_size)
    colour_manage(&frame, icc, icc_size);
```

The decoder never acts on any of it: an EXIF orientation does not rotate the
frames and an ICC profile does not change how they are converted, matching
libwebp. EXIF and XMP follow the image data, so in a stream they arrive after
the frames do; the flags in `info.metadata` are set as soon as the header is in.
`--info` lists whatever is present.

Every entry point that can fail returns a `WPDStatus`, negative on error;
`wpd_decoder_status()` retrieves the last one and `wpd_decoder_error()`
describes it for a log. Diagnostics are silent by default. Passing a callback to
`wpd_set_log_callback()` routes them somewhere else; passing NULL silences them
again.

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

For benchmarks against libwebp, build the optional comparison binary explicitly:

```sh
meson compile -C build libwebpdec
./scripts/bench.sh
```

It links the same libwebp the parity suite does. `-Dlibwebp=` chooses where that
comes from: `subproject` for the pinned wrap, `system` for the installed
libwebp, `disabled` for neither, and the default `auto` for the wrap, falling
back to the system libwebp when the wrap cannot be fetched. A particular static
decoder takes precedence over all of them:

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
