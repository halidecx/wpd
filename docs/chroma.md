# Chroma upsampling

Default fancy upsampling remains libwebp-exact. Two additional modes are
available for unscaled lossy packed output:

- `--extra-fancy`: separable ten-tap Lanczos-5 interpolation.
- `--fast-fancy`: separable four-tap cubic interpolation, with most of the
  measured quality gain at a lower cost.

The Rust and C options are `extra_fancy_upsampling` and `fast_fancy_upsampling`.
These and `no_fancy_upsampling` are mutually exclusive. Older C option structs
leave newer modes disabled, including when old tail padding contains nonzero
bytes.

```sh
build/wpd --extra-fancy -f rgba input.webp lanczos.pam
build/wpd --fast-fancy -f rgba input.webp cubic.pam
```

## Filters

Lanczos-5 follows the filter evaluated in
[Halide's chroma-handling article](https://halide.cx/blog/chroma-handling/) and
[decbench](https://github.com/gianni-rosato/decbench):
`scale=flags=lanczos+accurate_rnd+full_chroma_int:param0=5,format=rgb24`. The
two quarter-sample phases use reversed, normalized Q14 coefficients. Horizontal
interpolation retains signed Q6 samples, including negative lobes. The filter
and its output are preserved alongside the new mode.

Fast-fancy uses Keys cubic interpolation with `a = -0.75`, sampled at the same
quarter-sample phase and rounded to Q6: `[-7, 56, 17, -2]`. The opposite phase
reverses these coefficients. Their sum of 64 preserves constant chroma.
Horizontal results lie in `[-2295, 18615]`, so NEON can produce eight signed
samples per register without widening its multiply-accumulates to 32 bits.

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
holds back four luma rows for cubic or ten for Lanczos-5 until the two or five
future chroma rows are stable.

## Quality

Measured on the same 25 photographic PNGs in the gb82 image set, at 576x576,
with cwebp `-m 6` and fssimu2 0.2.0. Each row compares identical encoded inputs.
Scores are arithmetic means; retained gain is
`(fast - fancy) / (extra - fancy)`.

| Chroma conversion and quality |   Fancy | Fast-fancy | Extra-fancy | Gain retained |
| ----------------------------- | ------: | ---------: | ----------: | ------------: |
| FFmpeg, Q75                   | 69.0974 |    69.2368 |     69.2466 |         93.4% |
| FFmpeg, Q90                   | 83.5503 |    84.0321 |     84.0618 |         94.2% |
| FFmpeg, Q95                   | 87.4293 |    88.1338 |     88.2022 |         91.2% |
| Sharp YUV, Q90                | 84.3623 |    83.8409 |     83.6808 |             — |

At Q90, fast-fancy improves 23 of 25 images and preserves 94.2% of the average
Lanczos-5 gain. The same coefficients retain over 91% at Q75 and Q95. These
fractions describe this dataset's metric differences, not all images or a
percentage of visual quality. Sharp YUV still favors default fancy, which is why
the filters remain opt-in.

At Q90, FFmpeg's Lanczos-5 mean is 84.0614, compared with wpd's 84.0618.
Fast-fancy's choice trades about 0.030 fssimu2 points for a shorter filter.

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
| lossy.webp          |       1 |      2.697 |           2.860 |            3.299 |           +6.0% |
| lossy.webp          |       3 |      1.627 |           1.688 |            1.846 |           +3.8% |
| simplelf-lossy.webp |       1 |      2.745 |           2.968 |            3.365 |           +8.1% |
| simplelf-lossy.webp |       3 |      1.999 |           2.052 |            2.208 |           +2.6% |
| a_lossy.webp        |       1 |      0.530 |           0.586 |            0.746 |          +10.7% |
| a_lossy.webp        |       3 |      0.411 |           0.419 |            0.469 |           +1.8% |
| anim_yuv.webp       |       1 |      2.350 |           2.430 |            2.622 |           +3.4% |
| anim_yuv.webp       |       3 |      0.910 |           0.958 |            1.005 |           +5.2% |
| anim_yuva.webp      |       1 |      5.724 |           5.895 |            6.711 |           +3.0% |
| anim_yuva.webp      |       3 |      2.279 |           2.344 |            2.597 |           +2.9% |

Across these five files, geometric mean fast-fancy time is 6.2% above fancy with
one thread and 3.3% above with three threads. It takes 13.3% less time than
Lanczos-5 with one thread and 8.2% less with three. It approaches default
fancy's speed but does not equal it on these measurements.

In checkasm, NEON horizontal interpolation takes 6.46 ns for cubic versus 25.79
ns for Lanczos-5, producing 128 samples. The fused RGBA row takes 25.91 ns
versus 41.55 ns for 64 pixels. These are kernel timings with data hot in cache,
not complete decode times. The exact rounding/packing change reduced a separate
12-run single-threaded `lossy.webp` measurement from 188.1 to 182.4 ms per 64
decodes; it does not change the filter's output.

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
