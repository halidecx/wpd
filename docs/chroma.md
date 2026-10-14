# Chroma upsampling

Use `--uvmode` to select chroma upsampling for unscaled lossy packed output:

- `0`: no interpolation (point-sampled chroma).
- `1`: libwebp-exact fancy upsampling (the default).
- `2`: separable tuned three-tap fast fancy interpolation, with most of the
  measured quality gain at a lower cost.
- `3`: separable ten-tap Lanczos-5 extra fancy interpolation.

The Rust and C options are `no_fancy_upsampling`, `fast_fancy_upsampling`, and
`extra_fancy_upsampling`. They are mutually exclusive. Mode 1 leaves all three
disabled. Older C option structs leave newer modes disabled, including when old
tail padding contains nonzero bytes.

```sh
build/wpd --uvmode 3 -f rgba input.webp lanczos.pam
build/wpd --uvmode 2 -f rgba input.webp fast.pam
```

## Filters

Lanczos-5 follows the filter evaluated in
[Halide's chroma-handling article](https://halide.cx/blog/chroma-handling/) and
[decbench](https://github.com/gianni-rosato/decbench):
`scale=flags=lanczos+accurate_rnd+full_chroma_int:param0=5,format=rgb24`. The
two quarter-sample phases use reversed, normalized Q14 coefficients. Horizontal
interpolation retains signed Q6 samples, including negative lobes. The filter
and its output are preserved alongside the new mode.

Fast-fancy uses a tuned three-tap filter: `[-10, 59, 15] / 64` for the odd phase
and `[15, 59, -10] / 64` for the even phase. Both phases share the same three
source samples. The scalar interface pads the appropriate end with zero to reuse
the four-row cache; NEON skips that tap entirely. The coefficients sum to 64,
preserving constant chroma. Horizontal results lie in `[-2550, 18870]` before
centering. Subtracting `128 * 64` there stores zero-centered Q6 in
`[-10742, 10678]`, still fitting eight signed samples per register. Because the
vertical coefficients sum to 64, this offset commutes exactly with vertical
filtering and rounding. It moves the subtraction to half as many rows and
removes four NEON vector subtractions per eight output pixels; RGB output
remains byte-identical.

The coefficients were selected using fssimu2, Butteraugli, and fcvvdp rather
than fssimu2 alone. A search around the earlier cubic and three-tap filters
included four-tap filters, separate horizontal/vertical coefficients, and
overshoot limits. The final three-tap filter balances the metrics without adding
taps. It is an empirically tuned filter, not exact Keys cubic or Lanczos
interpolation.

Both filters retain fractional chroma through the vertical pass and BT.601
limited-range RGB matrix, then round and clip once. This matters to quality: an
experiment rounding the matrix's terms separately lost a substantial part of the
gain on two nearly flat images. The fast NEON kernel instead folds the final
rounding bias into luma and narrows by 16 then 4 bits, giving exactly the same
bytes as the full-precision scalar matrix.

Each conversion band caches four horizontal rows per plane for fast-fancy, or
ten for extra-fancy. Scratch space grows with width. Each source row is filtered
once per band, and at most one new row enters the support per output row. Edges
replicate the nearest sample. Both modes use centered 4:2:0 siting, including at
odd dimensions; neither promises bit-exact FFmpeg output.

Both filters have handwritten AArch64 NEON and x86-64 AVX2 kernels for
horizontal interpolation and all five RGB layouts. Other architectures use
scalar implementations.

Alpha dispatch, premultiplication, and two-byte packed output retain their
existing behavior. Animation preparation captures the selected mode, so an
option change invalidates frames prepared under earlier settings. Native planar
and lossless frames bypass interpolation. Scaled stills retain the existing
chroma rescaler. Cropping replicates the cropped view's boundaries. Streaming
conservatively holds back four luma rows for fast-fancy or ten for Lanczos-5
until the two or five future chroma rows are stable.

## Quality

The full-metric evaluation uses upstream decbench's encoding and metric
functions: 25 gb82 photographs, cwebp method 6/Q90, FFmpeg or libwebp-default
input color conversion, fssimu2 0.2.0, Butteraugli 3-norm at 203 nits, and
fcvvdp 0.4.0's `fhd` display. PNG metric inputs are produced losslessly from
wpd's RGB output. Default fancy matches dwebp on every measured image and
metric.

Arithmetic mean scores for FFmpeg input conversion at Q90:

| Filter             | fssimu2 ↑ | Butteraugli ↓ | fcvvdp ↑ |
| ------------------ | --------: | ------------: | -------: |
| Ordinary fancy     |   83.5503 |      0.963581 | 9.831676 |
| Previous three-tap |   84.0819 |      0.927074 | 9.839952 |
| Balanced three-tap |   84.0596 |      0.923329 | 9.840240 |
| Lanczos-5          |   84.0618 |      0.921539 | 9.840444 |

The balanced filter trades 0.0223 fssimu2 points versus the previous three-tap
filter for better Butteraugli and fcvvdp scores. The gain retained relative to
Lanczos-5 is `(fast - fancy) / (extra - fancy)` for each metric:

| Input / quality / image set    | fssimu2 | Butteraugli | fcvvdp |
| ------------------------------ | ------: | ----------: | -----: |
| FFmpeg / Q75 / photos          |   93.8% |       96.8% | 101.8% |
| FFmpeg / Q90 / photos          |   99.6% |       95.7% |  97.7% |
| libwebp default / Q90 / photos |  101.6% |       96.7% |  98.9% |
| FFmpeg / Q95 / photos          |   98.5% |       96.6% |  96.8% |
| FFmpeg / Q90 / screen content  |   97.3% |       96.4% |  95.7% |

The screen-content set contains ten gb82 PNGs. The previous three-tap filter
retained 87.0%, 86.1%, and 75.6% there. All of these sets and encoding qualities
informed the final choice; none is an independent final holdout. The fractions
are differences in metric scores, not bitrate savings or percentages of visual
quality. Sharp YUV is excluded from filter selection: it targets ordinary fancy,
which remains the appropriate mode for those encodes.

Reproduce the full Q90 comparison with a decbench checkout and its dependencies
on `PATH` (`uv` supplies its Rich dependency):

```sh
uv run scripts/chromametrics.py /path/to/gb82-image-set/png \
    --decbench /path/to/decbench/dec.py \
    --output wpd-test-data/chromametrics-ffmpeg
uv run scripts/chromametrics.py /path/to/gb82-image-set/png \
    --decbench /path/to/decbench/dec.py --encoder libwebp_default \
    --output wpd-test-data/chromametrics-default
```

Use `--previous /path/to/previous-wpd` to include a preserved fast-fancy binary.
Per-image metrics and means are written to JSON. The adapter delegates encoding,
decoding orchestration, and all three metric parsers to upstream `dec.py`. For
the fssimu2-only quality sweep, use `scripts/chromaquality.py --quality 75` or
`--quality 95`, passing the reference directory and a separate output path.

## Speed

Measured on Apple M5 Pro with a release build, assembly enabled, RGBA output, 64
decode repetitions per invocation, five warmups, and 24 Hyperfine runs. Times
are milliseconds per complete file decode, including all animation frames, with
output discarded. No builds, tests, or quality scoring ran during timing. The
baseline is commit `38c34a6`, with the same filter coefficients before moving
chroma centering into the horizontal pass. All quality scores remain unchanged.

| File                | Threads | Fancy (ms) | Baseline fast (ms) | Centered fast (ms) | Lanczos-5 (ms) |
| ------------------- | ------: | ---------: | -----------------: | -----------------: | -------------: |
| lossy.webp          |       1 |      2.868 |              2.960 |              2.881 |          3.427 |
| lossy.webp          |       3 |      1.634 |              1.675 |              1.658 |          1.846 |
| simplelf-lossy.webp |       1 |      2.863 |              2.966 |              2.933 |          3.500 |
| simplelf-lossy.webp |       3 |      1.978 |              2.022 |              2.010 |          2.204 |
| a_lossy.webp        |       1 |      0.543 |              0.575 |              0.561 |          0.771 |
| a_lossy.webp        |       3 |      0.400 |              0.413 |              0.410 |          0.481 |
| anim_yuv.webp       |       1 |      2.453 |              2.465 |              2.449 |          2.685 |
| anim_yuv.webp       |       3 |      0.921 |              0.940 |              0.927 |          1.026 |
| anim_yuva.webp      |       1 |      5.795 |              5.968 |              5.921 |          6.793 |
| anim_yuva.webp      |       3 |      2.279 |              2.329 |              2.320 |          2.596 |

A second run alternated baseline and centered fast-fancy invocations, reversing
order on each pair: four warmup pairs, 24 measured pairs, and 128 decodes per
invocation. The geometric mean time reduction across the five files was **0.8%
at one thread and 0.6% at three threads**. Still-image reductions ranged from
0.0% to 1.9%; animation reductions ranged from -0.2% to 0.7%. This is a small
improvement, with several individual cases within timing noise. In the four-mode
Hyperfine run, centered fast-fancy took 1.7% more time than ordinary fancy at
one thread and 1.6% more at three threads, and 16.4%/10.9% less than Lanczos-5.

The hot-cache NEON RGBA kernel measures 19.71 ns per 64 pixels; horizontal
interpolation measures 5.69 ns per 128 samples. Lanczos-5 measures 42.09 ns and
26.23 ns respectively. The added horizontal centering trades a little horizontal
work for less work on every output row. The balanced kernel shifts byte luma to
Q6 in 16-bit lanes before its widening multiply, loads the RGB constants
together, and keeps the three row pointers per plane in registers.
Full-precision RGB sums and output rounding are retained.

An additional experiment narrowed the rounded vertical chroma to 16-bit lanes
and used a doubling multiply for the blue coefficient. It remained bit-exact but
did not improve decode timing, so it was dropped. Four-tap filters,
axis-specific coefficients, and overshoot limits did not offer a sufficiently
consistent quality advantage over the selected three-tap filter.

```sh
RUNS=18 WARMUP=5 ./scripts/chromabench.sh
THREADS=3 RUNS=18 WARMUP=5 BENCH_DIR=build/chromabench-t3 ./scripts/chromabench.sh
build/checkasm --test=yuvdsp --function='*chroma*' --bench
```

Native x86 speed has not been measured. Both filters pass checkasm under a
temporary harness selecting AVX2 directly in Rosetta, which executes the
instructions but does not advertise the required CPU features here. Library
feature detection is unchanged.

## Validation

All 237 Meson tests pass, including libwebp parity, the reference corpus, CLI,
and row-boundary tests. Rust workspace tests pass with and without assembly; the
x86-64 build also passes. Across 134 corpus, photographic, and screen-content
files and all five RGB layouts, centered fast-fancy output matches both the
preserved baseline and scalar fallback byte for byte. Extra-fancy output remains
unchanged. Rust tests cover scalar tails, signed values, constants, odd and tiny
dimensions, all layouts, two-byte output, alpha, disjoint conversion bands,
incremental row finality, and changes to options during threaded animation
decoding. Both NEON filters pass twenty checkasm seeds. The C tests cover
mutually exclusive flags and legacy option sizes. `stylecheck.sh` and the new
Python script's Ruff checks pass.
