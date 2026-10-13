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

Fast-fancy uses a tuned three-tap filter: `[-10, 56, 18] / 64` for the odd phase
and `[18, 56, -10] / 64` for the even phase. Both phases share the same three
source samples. The scalar interface pads the appropriate end with zero to reuse
the four-row cache; NEON skips that tap entirely. The coefficients sum to 64,
preserving constant chroma. Horizontal results lie in `[-2550, 18870]`, so NEON
computes eight signed samples per register without widening to 32 bits.

The coefficients were selected by searching around the previous Q6 Keys cubic
filter `[-7, 56, 17, -2]` on the 25-image gb82 photographic set at Q90, then
checking Q75 and Q95. This is an empirically tuned filter, not exact Keys cubic
or Lanczos interpolation. The simpler three-tap support improves the measured
quality while removing horizontal loads and vertical multiply-accumulates.

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

Measured on the same 25 photographic PNGs in the gb82 image set, at 576x576,
with cwebp `-m 6` and fssimu2 0.2.0. Each row compares identical encoded inputs.
Scores are arithmetic means; retained gain is
`(fast - fancy) / (extra - fancy)`.

| Chroma conversion and quality |   Fancy | Fast-fancy | Extra-fancy | Gain retained |
| ----------------------------- | ------: | ---------: | ----------: | ------------: |
| FFmpeg, Q75                   | 69.0974 |    69.2774 |     69.2466 |        120.7% |
| FFmpeg, Q90                   | 83.5503 |    84.0819 |     84.0618 |        103.9% |
| FFmpeg, Q95                   | 87.4293 |    88.1836 |     88.2022 |         97.6% |
| Sharp YUV, Q90                | 84.3623 |    83.9773 |     83.6808 |             — |

Fast-fancy exceeds Lanczos-5's average gain at Q75 and Q90 and retains 97.6% at
Q95. These fractions describe this dataset's metric differences, not all images
or a percentage of visual quality. Q75 and Q95 reuse the tuning images; they are
checks across encoding qualities, not independent image holdouts. Sharp YUV
still favors default fancy, which is why the filters remain opt-in.

On ten gb82 screen-content PNGs excluded from coefficient tuning, Q90 means are
84.3894 for fancy, 85.1980 for fast-fancy, and 85.3188 for extra-fancy: 87.0% of
the Lanczos-5 gain. The previous cubic filter scored 85.2301 here, so the
photographic improvement comes with a small screen-content regression.

At photographic Q90, FFmpeg's Lanczos-5 mean is 84.0614, compared with wpd's
84.0618. Fast-fancy scores 84.0819, versus 84.0321 for the previous cubic
filter.

Reproduce with FFmpeg, cwebp, and fssimu2 on `PATH`:

```sh
python3 scripts/chromaquality.py /path/to/gb82-image-set/png \
    --output wpd-test-data/chroma-fast-q90
python3 scripts/chromaquality.py /path/to/gb82-image-set/png \
    --quality 75 --output wpd-test-data/chroma-fast-q75
python3 scripts/chromaquality.py /path/to/gb82-image-set/png \
    --quality 95 --output wpd-test-data/chroma-fast-q95
python3 scripts/chromaquality.py /path/to/gb82-image-set/png \
    --sharp-yuv --output wpd-test-data/chroma-fast-sharp
python3 scripts/chromaquality.py /path/to/gb82-image-set/sc-png \
    --output wpd-test-data/chroma-fast-screen
```

Decoded PPMs, generated WebPs, and per-image JSON results stay in each output
directory. Use separate directories to retain each encoding comparison.

## Speed

Measured on Apple M5 Pro with a release build, assembly enabled, RGBA output, 64
decode repetitions per invocation, five warmups, and eighteen Hyperfine runs.
Times are milliseconds per complete file decode, including every frame of
animations, with output discarded. Builds, tests, and quality measurements were
finished before timing began.

| File                | Threads | Fancy (ms) | Fast-fancy (ms) | Extra-fancy (ms) | Fast over fancy |
| ------------------- | ------: | ---------: | --------------: | ---------------: | --------------: |
| lossy.webp          |       1 |      2.719 |           2.814 |            3.327 |           +3.5% |
| lossy.webp          |       3 |      1.613 |           1.661 |            1.844 |           +3.0% |
| simplelf-lossy.webp |       1 |      2.752 |           2.864 |            3.392 |           +4.1% |
| simplelf-lossy.webp |       3 |      1.979 |           2.022 |            2.190 |           +2.1% |
| a_lossy.webp        |       1 |      0.523 |           0.557 |            0.744 |           +6.5% |
| a_lossy.webp        |       3 |      0.398 |           0.414 |            0.471 |           +3.8% |
| anim_yuv.webp       |       1 |      2.362 |           2.404 |            2.619 |           +1.8% |
| anim_yuv.webp       |       3 |      0.897 |           0.913 |            1.004 |           +1.7% |
| anim_yuva.webp      |       1 |      5.673 |           5.842 |            6.534 |           +3.0% |
| anim_yuva.webp      |       3 |      2.241 |           2.305 |            2.575 |           +2.9% |

Across these five files, geometric mean fast-fancy time is 3.8% above fancy with
one thread and 2.7% above with three threads. It takes 15.2% less time than
Lanczos-5 with one thread and 9.9% less with three. It approaches default
fancy's speed but does not equal or beat it on these measurements.

A separate twelve-run comparison against the preserved cubic binary, with four
warmups and the same five files and repetition count, measures 1.5% less decode
time at one thread and 1.0% less at three threads (geometric means).

In checkasm, NEON horizontal interpolation takes 4.66 ns for fast-fancy versus
26.09 ns for Lanczos-5, producing 128 samples. The fused RGBA row takes 22.79 ns
versus 41.90 ns for 64 pixels. The previous cubic kernels took 6.46 ns and 25.91
ns respectively. These are kernel timings with data hot in cache, not complete
decode times. Besides skipping the zero tap, the new kernel initializes vertical
sums with widening multiplies and packs four-byte RGB layouts with interleaves
and paired stores.

Hoisting row pointers and using 16-bit operands for the full-precision RGB
matrix did not improve whole-decode timing and were dropped. Filters whose
coefficients could be reduced to a few shifts and adds lost too much metric
gain.

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
