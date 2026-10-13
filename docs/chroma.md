# Chroma upsampling

Default fancy upsampling remains libwebp-exact. Two additional modes are
available for unscaled lossy packed output:

- `--extra-fancy`: separable ten-tap Lanczos-5 interpolation.
- `--fast-fancy`: separable tuned three-tap interpolation, with most of the
  measured quality gain at a lower cost.

The Rust and C options are `extra_fancy_upsampling` and `fast_fancy_upsampling`.
These and `no_fancy_upsampling` are mutually exclusive. Older C option structs
leave newer modes disabled, including when old tail padding contains nonzero
bytes.

```sh
build/wpd --extra-fancy -f rgba input.webp lanczos.pam
build/wpd --fast-fancy -f rgba input.webp fast.pam
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
preserving constant chroma. Horizontal results lie in `[-2550, 18870]`, so NEON
computes eight signed samples per register without widening to 32 bits.

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

Fast-fancy has handwritten AArch64 NEON kernels for horizontal interpolation and
all five RGB layouts. Other architectures use its scalar implementation; no new
AVX2 path is added. Extra-fancy retains its NEON and AVX2 kernels.

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
decode repetitions per invocation, five warmups, and eighteen Hyperfine runs.
Times are milliseconds per complete file decode, including all animation frames,
with output discarded. No builds, tests, or quality scoring ran during timing.
The previous fast binary is commit `87bcdd8` (`[-10, 56, 18] / 64`).

| File                | Threads | Fancy (ms) | Previous fast (ms) | Balanced fast (ms) | Lanczos-5 (ms) |
| ------------------- | ------: | ---------: | -----------------: | -----------------: | -------------: |
| lossy.webp          |       1 |      2.688 |              2.781 |              2.772 |          3.294 |
| lossy.webp          |       3 |      1.630 |              1.668 |              1.667 |          1.843 |
| simplelf-lossy.webp |       1 |      2.754 |              2.841 |              2.828 |          3.374 |
| simplelf-lossy.webp |       3 |      1.979 |              2.018 |              2.007 |          2.197 |
| a_lossy.webp        |       1 |      0.520 |              0.550 |              0.547 |          0.735 |
| a_lossy.webp        |       3 |      0.398 |              0.409 |              0.407 |          0.475 |
| anim_yuv.webp       |       1 |      2.371 |              2.399 |              2.408 |          2.607 |
| anim_yuv.webp       |       3 |      0.897 |              0.929 |              0.926 |          0.996 |
| anim_yuva.webp      |       1 |      5.647 |              5.946 |              5.900 |          6.622 |
| anim_yuva.webp      |       3 |      2.237 |              2.311 |              2.292 |          2.569 |

The balanced filter's geometric mean time is 3.4% above fancy at one thread and
2.3% above at three threads. It takes 15.4% and 10.1% less time than Lanczos-5.
Compared directly with the previous three-tap binary, it takes 0.4% less time at
one thread and 0.5% less at three threads; those small differences are within
the run-to-run variation. Whole-decode speed is essentially unchanged, and it
still does not beat ordinary fancy.

The NEON RGBA kernel measures 21.34 ns per 64 pixels, versus the previous 22.79
ns; horizontal interpolation measures 4.49 ns per 128 samples. Lanczos-5
measures 41.65 ns and 25.87 ns respectively. These are hot-cache kernel timings.
The balanced kernel shifts byte luma to Q6 in 16-bit lanes before its widening
multiply, loads the RGB constants together, and keeps the three row pointers per
plane in registers. Full-precision RGB sums and output rounding are retained.

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

Native x86 speed has not been measured. The existing Lanczos-5 AVX2 kernels
previously passed checkasm under a temporary harness selecting them directly in
Rosetta, which executes the instructions but does not advertise the required CPU
features here. Library feature detection is unchanged.

## Validation

All 237 Meson tests pass, including libwebp parity, the reference corpus, CLI,
and row-boundary tests. Rust workspace tests pass with and without assembly; the
x86-64 build also passes a compile check. Across all 49 top-level corpus files,
extra-fancy output matches the preserved binary, and fast-fancy NEON output
matches its scalar fallback. Rust tests cover scalar tails, signed values,
constants, odd and tiny dimensions, all layouts, two-byte output, alpha,
disjoint conversion bands, incremental row finality, and changes to options
during threaded animation decoding. Both NEON filters pass twenty checkasm
seeds. The C tests cover mutually exclusive flags and legacy option sizes.
`stylecheck.sh` and the new Python script's Ruff checks pass.
