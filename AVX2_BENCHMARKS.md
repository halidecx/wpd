# AVX2 decoder sweep measurements

The AVX2 implementation closes the architecture coverage gaps, but does **not**
meet the historical README ARM speedup ratios on six of its seven images. These
measurements record that shortfall as well as the kernel improvements.

## Setup

- Intel Core i7-13700K, Linux x86-64; benchmark commands pinned to CPU 0.
- Release Meson build, assembly enabled, `trim_dsp=false`.
- Baseline: branch HEAD `59a1c12`, built before these changes.
- libwebp: pinned Meson subproject `94d3c4a7b85aa34ca68963e3b3f00298f47c0bbb`.
- Both image decoders request RGBA, with wpd restricted to one thread.
- Standard image timings: 48 decodes per invocation, 3 warmups, 20 measured
  runs; tables show median wall-clock milliseconds for the entire invocation.
- checkasm: seed `123456789`, `--affinity=0 --bench --duration=12800`; tables
  show its adjusted median cycle estimates per kernel call. The baseline uses
  the same updated C harness as the new build.

`scripts/bench.sh` previously omitted `-f rgba` for wpd despite its comment. The
measurements here explicitly select RGBA for both decoders. Commit `f5c5b33`
identifies the historical README measurements as wpd `ac55d3e` on an Apple M5
Pro, on one thread, against libwebp `523e304`. That script selected
automatic/native output for wpd and RGBA for the other decoders. The historical
runs also had up to 7% variation while a browser played video. The ARM column
below preserves the originally requested numerical targets; it is not a
controlled comparison between CPUs or output formats. The README now reports a
fresh run of the corrected harness on x86: lossy stills use YUV/YUVA, while
lossless stills and composited animations use RGBA. image-webp is included only
in the RGBA rows.

## Standard images

| Image               | Before (ms) | AVX2 (ms) | libwebp (ms) | Before / AVX2 | libwebp / AVX2 | ARM target | Target met |
| ------------------- | ----------: | --------: | -----------: | ------------: | -------------: | ---------: | ---------- |
| lossy.webp          |      192.58 |    189.38 |       195.16 |        1.017x |         1.031x |     1.373x | no         |
| simplelf-lossy.webp |      178.91 |    179.66 |       188.91 |        0.996x |         1.052x |     1.337x | no         |
| anim_yuv.webp       |      168.63 |    169.00 |       190.98 |        0.998x |         1.130x |     1.258x | no         |
| lossless.webp       |      111.60 |    111.10 |       195.82 |        1.005x |         1.763x |     2.761x | no         |
| anim_rgb.webp       |       65.61 |     62.54 |       104.35 |        1.049x |         1.668x |     3.067x | no         |
| a_lossy.webp        |       61.24 |     60.37 |        71.78 |        1.014x |         1.189x |     2.005x | no         |
| anim_yuva.webp      |      372.80 |    374.45 |       494.97 |        0.996x |         1.322x |     1.294x | yes        |

The largest standard-image gain is `anim_rgb.webp`, about 4.9%. Changes below
one percent should be treated as neutral rather than as meaningful wins.
`lossy.webp` improves about 1.7%, while `lossless.webp` improves about 0.5%.

Profiling with `perf record -e cpu_core/cycles/u -F 999` attributes about 74% of
lossless decoding CPU cycles to `vp8l::entropy::decode_pixels`; the earlier
lossy profile attributes about 49% to coefficient entropy decoding. The new SIMD
paths cover a small fraction of these images. Matching the remaining ratios
requires work on those dominant paths and output handling in addition to
architecture parity.

## Kernels

Paired predictors are compared with the previous two single-row calls, and full
macroblock filters with the previous horizontal-plus-vertical calls. Their
generic full-operation fallbacks are much slower and would inflate the apparent
benefit. Other rows use the fastest dispatched baseline kernel. Values below
1.00x are regressions; no regression is omitted.

| Kernel / distribution           | Baseline ISA | Before (cycles) | AVX2 (cycles) | Before / AVX2 |
| ------------------------------- | ------------ | --------------: | ------------: | ------------: |
| `blend_row_argb`                | avx2         |            88.2 |         102.0 |         0.86x |
| `blend_row_argb_binary`         | avx2         |           440.5 |         114.2 |         3.86x |
| `blend_row_argb_clear`          | avx2         |            99.9 |          98.8 |         1.01x |
| `blend_row_argb_opaque`         | avx2         |            73.5 |          85.5 |         0.86x |
| `blend_row_argb_premult`        | avx2         |            94.4 |          87.5 |         1.08x |
| `blend_row_argb_premult_binary` | avx2         |            94.8 |          85.6 |         1.11x |
| `blend_row_argb_premult_clear`  | avx2         |            94.8 |          85.3 |         1.11x |
| `blend_row_argb_premult_opaque` | avx2         |            94.8 |          86.1 |         1.10x |
| `blend_row_argb_premult_random` | avx2         |            94.8 |         101.6 |         0.93x |
| `blend_row_argb_random`         | avx2         |           440.7 |         459.2 |         0.96x |
| `expand_alpha_nibbles`          | c            |          2009.2 |          43.6 |        46.12x |
| `gradient_unfilter`             | avx2         |          2458.4 |        2291.0 |         1.07x |
| `gradient_unfilter_flat`        | avx2         |           919.4 |         244.9 |         3.75x |
| `gradient_unfilter_random`      | avx2         |          2460.7 |        2292.4 |         1.07x |
| `gradient_unfilter_wrap`        | avx2         |          2369.4 |         244.9 |         9.67x |
| `map_color32`                   | avx2         |           343.0 |         321.1 |         1.07x |
| `map_color32_random`            | avx2         |           344.2 |         371.6 |         0.93x |
| `map_color32_runs`              | avx2         |           410.0 |         217.7 |         1.88x |
| `map_color32_short_runs`        | avx2         |           359.8 |         381.9 |         0.94x |
| `map_color32_small`             | avx2         |           345.3 |         320.2 |         1.08x |
| `pred_add_pair_11`              | avx2         |          1926.5 |        1629.6 |         1.18x |
| `pred_add_pair_12`              | sse2         |           976.2 |         841.2 |         1.16x |
| `pred_add_pair_13`              | sse4         |          2410.1 |        1693.7 |         1.42x |
| `pred_green_1`                  | c            |           167.5 |          52.0 |         3.22x |
| `pred_green_1_flat`             | c            |           663.1 |         208.5 |         3.18x |
| `pred_green_1_left`             | c            |           663.0 |         208.7 |         3.18x |
| `pred_green_1_random`           | c            |           663.2 |         208.6 |         3.18x |
| `pred_green_11`                 | c            |           535.0 |         486.2 |         1.10x |
| `pred_green_11_flat`            | c            |          2470.9 |         229.4 |        10.77x |
| `pred_green_11_left`            | c            |          2218.2 |         468.5 |         4.73x |
| `pred_green_11_random`          | c            |          2106.2 |        1925.2 |         1.09x |
| `premultiply_argb_row`          | avx2         |           111.9 |         123.4 |         0.91x |
| `premultiply_argb_row_binary`   | avx2         |           111.8 |          53.3 |         2.10x |
| `premultiply_argb_row_clear`    | avx2         |           111.8 |          52.0 |         2.15x |
| `premultiply_argb_row_opaque`   | avx2         |           111.8 |          52.3 |         2.14x |
| `premultiply_argb_row_random`   | avx2         |           111.8 |         121.8 |         0.92x |
| `premultiply_row_argb_binary`   | avx2         |           127.4 |          66.6 |         1.91x |
| `premultiply_row_argb_clear`    | avx2         |           128.0 |          54.9 |         2.33x |
| `premultiply_row_argb_opaque`   | avx2         |           128.2 |          49.7 |         2.58x |
| `premultiply_row_argb_random`   | avx2         |           128.8 |         129.2 |         1.00x |
| `premultiply_row_rgba_binary`   | avx2         |           130.7 |          61.1 |         2.14x |
| `premultiply_row_rgba_clear`    | avx2         |           131.3 |          63.9 |         2.06x |
| `premultiply_row_rgba_opaque`   | avx2         |           131.5 |          61.8 |         2.13x |
| `premultiply_row_rgba_random`   | avx2         |           130.3 |         127.6 |         1.02x |
| `vp8_loop_filter8uv_all`        | avx2         |           207.1 |         183.5 |         1.13x |
| `vp8_loop_filter16y_all`        | avx2         |           319.1 |         312.7 |         1.02x |

Predictor pair calls decode 256 pixels in each of two rows. The normal green
predictor calls decode 256 pixels; their named distribution cases decode 1024.
Palette mapping decodes 1024 pixels, nibble expansion 1024 alpha bytes, gradient
unfiltering 512 bytes, and blending/premultiplication 256 pixels. Full filters
handle one luma macroblock or both chroma blocks.

The data distributions expose tradeoffs: binary alpha, long palette runs and
flat predictors benefit most; some noisy and short-run cases retain small
overheads. Microbenchmarks also depend on alignment and surrounding workload;
the full image results above determine the practical gain.

The full checkasm runs measured all kernels (420 baseline and 434 new benchmark
versions), including unchanged SIMD. Raw results are in
`build/avx2-bench/checkasm-{before,after}-final.json`, and the image
measurements in `build/avx2-bench/rgba-end-to-end.json`.

## Whole corpus

All 51 WebP files were measured against the baseline and libwebp in RGBA. These
runs use 2 warmups and 10 measured invocations per command, with a per-file
repeat count chosen from a baseline pilot (minimum 48, maximum 8192). The larger
repeat counts keep process startup from dominating tiny fixtures. The table
shows median milliseconds per batch; different batch sizes are not comparable
across files. Ratios within one percent are effectively neutral.

| File                               | Repeats | Before (ms) | AVX2 (ms) | libwebp (ms) | Before / AVX2 | libwebp / AVX2 |
| ---------------------------------- | ------: | ----------: | --------: | -----------: | ------------: | -------------: |
| a_lossy.webp                       |      48 |       61.27 |     60.44 |        71.81 |        1.014x |         1.188x |
| a_lossy_cached.webp                |     933 |       41.33 |     41.30 |        90.37 |        1.001x |         2.188x |
| a_lossy_gradient.webp              |     193 |       46.72 |     46.36 |        55.02 |        1.008x |         1.187x |
| alpha_uncompressed_gradient.webp   |    1027 |       35.70 |     35.11 |        47.29 |        1.017x |         1.347x |
| alpha_uncompressed_horizontal.webp |    1159 |       37.67 |     37.06 |        50.58 |        1.017x |         1.365x |
| alpha_uncompressed_none.webp       |    1054 |       33.94 |     33.40 |        45.77 |        1.016x |         1.371x |
| alpha_uncompressed_vertical.webp   |    1143 |       37.01 |     36.43 |        49.82 |        1.016x |         1.368x |
| anim_rgb.webp                      |      48 |       65.71 |     62.76 |       104.82 |        1.047x |         1.670x |
| anim_yuv.webp                      |      48 |      168.63 |    168.92 |       191.13 |        0.998x |         1.132x |
| anim_yuva.webp                     |      48 |      372.69 |    374.09 |       494.34 |        0.996x |         1.321x |
| dispose_bg_blend.webp              |    1574 |       34.54 |     33.93 |        87.78 |        1.018x |         2.587x |
| dispose_bg_fullframe.webp          |    1014 |       41.67 |     41.96 |       106.01 |        0.993x |         2.526x |
| dispose_bg_noblend.webp            |    1617 |       35.53 |     34.84 |        90.21 |        1.020x |         2.590x |
| dispose_none_blend.webp            |    1613 |       35.79 |     35.30 |       100.55 |        1.014x |         2.849x |
| dispose_none_noblend.webp          |    1614 |       34.25 |     33.60 |        89.72 |        1.019x |         2.670x |
| durations.webp                     |    1100 |       31.46 |     31.05 |        90.56 |        1.013x |         2.917x |
| edge_frames.webp                   |    1648 |       35.51 |     35.23 |       119.27 |        1.008x |         3.385x |
| huffman_long_codes.webp            |    4698 |        7.67 |      7.61 |        22.06 |        1.008x |         2.899x |
| huffman_simple_duplicate.webp      |    6250 |        4.68 |      4.66 |        23.82 |        1.003x |         5.107x |
| huffman_simple_forms.webp          |    5736 |        4.74 |      4.73 |        23.21 |        1.002x |         4.909x |
| huffman_simple_single.webp         |    5741 |        4.33 |      4.30 |        21.88 |        1.007x |         5.095x |
| keyframe_midstream.webp            |     936 |       42.15 |     41.60 |       110.69 |        1.013x |         2.661x |
| kitchen_sink.webp                  |     695 |       43.76 |     43.03 |       131.26 |        1.017x |         3.051x |
| lossless.webp                      |      48 |      111.53 |    111.19 |       195.82 |        1.003x |         1.761x |
| lossy.webp                         |      48 |      192.56 |    189.33 |       195.30 |        1.017x |         1.032x |
| mixed_codecs.webp                  |     615 |       39.31 |     38.90 |        64.72 |        1.011x |         1.664x |
| odd_a_lossy.webp                   |    1076 |       40.05 |     39.63 |        61.79 |        1.011x |         1.559x |
| odd_canvas.webp                    |    1262 |       37.57 |     37.55 |       112.43 |        1.001x |         2.994x |
| odd_frames.webp                    |    1534 |       35.26 |     34.90 |       111.90 |        1.010x |         3.206x |
| odd_lossy.webp                     |    1421 |       36.70 |     36.49 |        39.08 |        1.006x |         1.071x |
| overlap_bottom.webp                |    1390 |       38.06 |     37.65 |       111.88 |        1.011x |         2.972x |
| overlap_contains.webp              |    1192 |       38.28 |     38.00 |       104.61 |        1.007x |         2.753x |
| overlap_corner.webp                |    1351 |       37.25 |     36.85 |       109.17 |        1.011x |         2.962x |
| overlap_disjoint.webp              |    1491 |       37.49 |     37.00 |       114.74 |        1.013x |         3.101x |
| overlap_exact.webp                 |    1392 |       37.61 |     37.12 |       108.62 |        1.013x |         2.927x |
| overlap_inside.webp                |    1417 |       35.40 |     35.02 |       108.50 |        1.011x |         3.099x |
| overlap_left.webp                  |    1379 |       37.97 |     37.49 |       109.97 |        1.013x |         2.933x |
| overlap_odd.webp                   |    1351 |       37.91 |     37.36 |       107.79 |        1.015x |         2.885x |
| overlap_right.webp                 |    1359 |       37.38 |     36.92 |       108.34 |        1.012x |         2.934x |
| overlap_single.webp                |    1531 |       35.98 |     35.43 |       103.05 |        1.016x |         2.909x |
| overlap_top.webp                   |    1360 |       37.15 |     36.79 |       108.64 |        1.010x |         2.953x |
| palette2bpp_rgb.webp               |     394 |       44.39 |     44.24 |        27.84 |        1.003x |         0.629x |
| palette4bpp_rgb.webp               |     575 |       44.91 |     44.84 |        46.53 |        1.001x |         1.038x |
| palette_rgb.webp                   |    1091 |       32.35 |     32.29 |        25.02 |        1.002x |         0.775x |
| predict_topright.webp              |     472 |       43.38 |     43.37 |        75.28 |        1.000x |         1.736x |
| simplelf-lossy.webp                |      48 |      178.99 |    179.67 |       188.81 |        0.996x |         1.051x |
| sweep/gradient.webp                |      48 |      169.20 |    168.36 |       183.56 |        1.005x |         1.090x |
| sweep/large-lossless.webp          |      48 |     1248.46 |   1237.85 |      1497.28 |        1.009x |         1.210x |
| sweep/repeats.webp                 |      48 |      139.43 |    133.20 |       168.00 |        1.047x |         1.261x |
| transforms_before_palette.webp     |    2134 |       28.08 |     28.15 |        57.80 |        0.998x |         2.053x |
| transparent_over.webp              |    1503 |       37.22 |     35.58 |       114.15 |        1.046x |         3.208x |

Raw results, including all samples, are in
`build/avx2-bench/corpus-end-to-end.json`.

## Validation

- 32 consecutive checkasm seeds: all 275 registered tests pass per seed,
  including zero lengths, vector boundaries, tails, ties, wraps, palette-path
  transitions and both in-place and separate-buffer calls.
- `meson test -C build`: all 235 tests pass.
- `scripts/testdata.sh`: all 231 tests pass at each CPU mask (`none`, `sse2`,
  `ssse3`, `sse41`, `avx2`).
- `scripts/animcheck.sh`: 48 files × 8 packed formats match libwebp exactly,
  including 27 animations.
- `scripts/threadcheck.sh ./build/wpd '1 3 8'`: 10,368 decodes agree.
- Guard-page row tests cover all four left/top edge combinations of the new
  full-macroblock filters, at both ends of the permitted memory window.
- NASM assembles all four modified sources as ELF64, Win64 and Mach-O64. These
  are assembly checks; runtime correctness and ABI checks ran on Linux.
- `scripts/stylecheck.sh` passes, including clippy with all features and without
  default features.

## Reproduction

```sh
meson setup build -Dtrim_dsp=false -Dtestdata_tests=true
meson compile -C build libwebpdec
meson compile -C build
./build/checkasm --repeat=32 123456789
./build/checkasm --affinity=0 --bench --duration=12800 --json 123456789
taskset -c 0 ./scripts/bench.sh
```
