# Extra-fancy chroma upsampling

`--extra-fancy` selects Lanczos-5 interpolation when converting lossy 4:2:0
planes to packed RGB. The Rust and C option is `extra_fancy_upsampling`. Default
fancy upsampling remains libwebp-exact. The two options `no_fancy_upsampling`
and `extra_fancy_upsampling` cannot both be enabled.

This follows the filter used by
[Halide's chroma-handling article](https://halide.cx/blog/chroma-handling/) and
[decbench](https://github.com/gianni-rosato/decbench):
`scale=flags=lanczos+accurate_rnd+full_chroma_int:param0=5,format=rgb24`. The
parameter is a radius of five chroma samples, giving ten taps per axis. wpd uses
centered 4:2:0 siting, including at odd dimensions, rather than resizing an odd
chroma plane to a slightly different scale factor. Its output is not intended to
be bit-exact with FFmpeg.

## Implementation

The two quarter-sample phases use reversed, normalized Q14 coefficients.
Horizontal interpolation retains signed Q6 samples. A ring caches up to ten
horizontal rows of each chroma plane per conversion band, so each source row is
filtered once per band and scratch space grows with width rather than image
area. Once the first window is filled, each output row checks only the last
source row because at most one new row enters its support. Ring lookups use a
constant modulus, including for tiny images. Edges replicate the nearest source
sample.

The vertical filter and BT.601 limited-range color matrix share one kernel.
Fractional chroma and negative filter lobes survive until the final rounded RGB
clipping; a version that rounded chroma to bytes before RGB scored about 0.19
fssimu2 points lower in this test. Handwritten AArch64 NEON and x86-64 AVX2
kernels have scalar tails and checkasm coverage for both phases, all five packed
layouts, signed inputs, unaligned addresses, and output guards. No new x86 SSE
paths or intrinsics are used.

Alpha dispatch and premultiplication retain their existing behavior. Two-byte
packed output uses one ARGB scratch row and keeps the horizontal cache across
the whole band. Animation preparation captures the sampling option so changing
it invalidates frames decoded ahead under earlier settings.

Native planar and lossless frames bypass interpolation. Stills scaled to packed
output retain the existing chroma rescaler. Cropping uses the existing cropped
plane view and replicates that view's boundaries. Streaming holds back ten luma
rows until the five future chroma rows needed by the filter are stable.

## Quality

Measured on the 25 photographic PNGs in the gb82 image set, at 576x576, with
cwebp `-m 6 -q 90` and fssimu2 0.2.0. These are arithmetic means on identical
encoded inputs for each decoder comparison.

| Encoding chroma conversion | wpd fancy | wpd extra-fancy | FFmpeg filtered |
| -------------------------- | --------: | --------------: | --------------: |
| FFmpeg yuv420p             |   83.5503 |         84.0618 |         84.0614 |
| cwebp Sharp YUV            |   84.3623 |         83.6808 |         83.6792 |

With FFmpeg-converted input, extra-fancy improves 23 of 25 images and gains
0.5116 points on average. Its mean absolute score difference from FFmpeg's
filtered output is 0.0047 points. Sharp YUV favors the existing fancy
interpolator, confirming that this option should remain an explicit choice.
These results measure reconstruction quality; they are not encoder efficiency
percentages or a guarantee for every image.

Reproduce with FFmpeg, cwebp, and fssimu2 on `PATH`:

```sh
python3 scripts/chromaquality.py /path/to/gb82-image-set/png
python3 scripts/chromaquality.py /path/to/gb82-image-set/png \
    --sharp-yuv --output wpd-test-data/chromaquality-sharp
```

Decoded PPMs, generated WebPs, and per-image JSON results stay under the
selected output directory. Use separate directories to retain both encoding
comparisons.

## Speed

The initial SIMD implementation was measured on Apple M5 Pro with a release
build, assembly enabled, RGBA output, 64 decode repetitions per invocation,
three warmups, and twelve Hyperfine runs. The table reports milliseconds per
complete file decode, including all frames for animations, by dividing each
invocation's elapsed time by 64. Output is discarded. Benchmarks run without
other build or test jobs.

| File                | Threads | Fancy (ms) | Extra-fancy (ms) | Extra time |
| ------------------- | ------: | ---------: | ---------------: | ---------: |
| lossy.webp          |       1 |      2.747 |            3.323 |     +21.0% |
| lossy.webp          |       3 |      1.627 |            1.850 |     +13.7% |
| simplelf-lossy.webp |       1 |      2.772 |            3.367 |     +21.5% |
| simplelf-lossy.webp |       3 |      1.975 |            2.193 |     +11.0% |
| a_lossy.webp        |       1 |      0.523 |            0.743 |     +42.1% |
| a_lossy.webp        |       3 |      0.405 |            0.485 |     +20.0% |
| anim_yuv.webp       |       1 |      2.371 |            2.630 |     +10.9% |
| anim_yuv.webp       |       3 |      0.916 |            0.997 |      +8.8% |
| anim_yuva.webp      |       1 |      5.654 |            6.511 |     +15.2% |
| anim_yuva.webp      |       3 |      2.243 |            2.597 |     +15.8% |

The isolated NEON horizontal kernel measures 30.3 ns versus 66.4 ns for the
scalar implementation. The fused RGBA row measures 42.5 ns versus 210.7 ns,
about 5x faster. Those calls produce 128 horizontal samples and 64 packed
pixels, respectively, with data hot in cache; they do not represent whole
decodes.

Reproduce the end-to-end comparison and save Hyperfine JSON:

```sh
./scripts/chromabench.sh
THREADS=3 BENCH_DIR=build/chromabench-t3 ./scripts/chromabench.sh
build/checkasm --test=yuvdsp --function='chroma*' --bench
```

The AVX2 kernels build and passed the six new checkasm cases over twenty random
seeds through a temporary harness that directly selects them under Rosetta.
Rosetta executes these instructions but does not advertise AVX2/BMI CPU features
here, so normal x86 dispatch falls back. Library feature detection was left
unchanged. Native x86 speed has not been measured.

## Optimization follow-up

The row cache now skips nine redundant cache checks after its first window, and
the SIMD wrapper skips constructing twenty scalar-tail slices when the row is
fully covered by assembly. These changes preserve the filter and output.

A follow-up compared the original extra-fancy implementation with these changes
on the same M5 Pro: single-threaded `lossy.webp`, RGBA, 128 repetitions, five
warmups, and 24 Hyperfine runs. The original averaged 3.475 ms per decode
(standard deviation 0.022 ms); the current implementation averaged 3.523 ms
(standard deviation 0.097 ms). This does not establish a decode-speed gain.

Several NEON scheduling experiments were also measured, including reusing
overlapping horizontal loads, keeping vertical row pointers in registers, and
splitting ten-tap accumulation into independent sums. They did not produce a
reliable improvement and were discarded. The initial SIMD speedups above remain
the measured benefit; native AVX2 timing is still needed.

## Validation

All 237 Meson tests pass, including libwebp parity, the reference corpus, CLI,
and row-boundary tests. Rust workspace tests pass with and without assembly.
NEON and AVX2 checkasm each pass twenty seeds. Tests cover kernel coefficients,
fractional chroma, odd and tiny dimensions, all packed layouts, two-byte output,
alpha, scalar tails, disjoint conversion bands, incremental row finality, and
option changes during threaded animation decoding. Older C option structs leave
extra-fancy disabled. `scripts/stylecheck.sh` passes.
