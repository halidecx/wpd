# Chroma upsampling

## Select a mode

Use `--uvmode` to select the chroma upsampling mode. This option applies to
unscaled lossy images with packed pixel output. Mode 1 is the default mode.

| Mode | Filter         | Function                                                  |
| ---: | -------------- | --------------------------------------------------------- |
|    0 | Point sampling | Copy the nearest chroma sample. Do not use interpolation. |
|    1 | Libwebp fancy  | Use the same chroma upsampling as libwebp.                |
|    2 | Fast fancy     | Use a three-tap filter.                                   |
|    3 | Extra fancy    | Use a ten-tap Lanczos-5 filter.                           |

Use mode 1 when the output must be the same as libwebp output. Use mode 2 for
better measured image quality with a small increase in decode time. Use mode 3
for the highest effort. Mode 0 can decrease decode time, but it can cause color
stepping at image edges.

```sh
build/wpd --uvmode 0 -f rgba input.webp point.pam
build/wpd --uvmode 1 -f rgba input.webp fancy.pam
build/wpd --uvmode 2 -f rgba input.webp fast.pam
build/wpd --uvmode 3 -f rgba input.webp lanczos.pam
```

The Rust and C interfaces use the `uvmode` field. Set this field to a value from
0 to 3. The values are the same as the command-line values. Other values cause
an argument error.

Rust `Options::default()` and C `WPD_DECODER_OPTIONS_INIT` set `uvmode` to 1. C
callers must use the new header and compile again after this interface change.

## Output conditions

The selected mode also applies to lossy animation frames. The decoder does not
use chroma interpolation for lossless frames or native planar output. Scaled
images use the existing chroma rescaler.

The modes do not change alpha processing, alpha premultiplication, or two-byte
pixel formats. An option change causes the decoder to prepare animation frames
again. The decoder uses the new mode for these frames.

Modes 2 and 3 use centered 4:2:0 chroma positions. They also support odd image
dimensions. Their output can differ from FFmpeg output. At an image edge, the
filter uses copies of the nearest sample. For a cropped image, the filter uses
the edges of the cropped area.

## Image quality

A quality metric is a numerical measure of image quality. The tests use three
metrics: fssimu2, Butteraugli, and fcvvdp. A higher fssimu2 or fcvvdp score
shows better image quality. A lower Butteraugli score shows better image
quality.

The test set contains 25 gb82 photographs. The encoder is cwebp, with method 6
and quality 90. FFmpeg converts the input colors before the encoder processes
each image. The tests use the metric functions from
[decbench](https://github.com/gianni-rosato/decbench). The test configuration is
as follows:

| Metric      | Configuration                        |
| ----------- | ------------------------------------ |
| fssimu2     | Version 0.2.0                        |
| Butteraugli | 3-norm, intensity target of 203 nits |
| fcvvdp      | Version 0.4.0, `fhd` display model   |

PNG files contain the decoder's RGB output without additional loss. Mode 1 gives
the same output as dwebp for each measured image and metric. The current mode 1,
2, and 3 outputs are the same as the outputs from the earlier quality tests. The
mode 0 tests use the same encoded images.

All changes in the table below use mode 1 as the reference. Each change is the
mode's arithmetic mean score minus the mode 1 arithmetic mean score. A positive
change is better for fssimu2 and fcvvdp. A negative change is better for
Butteraugli.

| Metric      | Mode 1 mean score | Mode 0 change | Mode 2 change | Mode 3 change |
| ----------- | ----------------: | ------------: | ------------: | ------------: |
| fssimu2     |         83.550274 |     +0.000368 |     +0.509303 |     +0.511557 |
| Butteraugli |          0.963581 |     -0.006302 |     -0.040252 |     -0.042042 |
| fcvvdp      |          9.831676 |     +0.004608 |     +0.008564 |     +0.008768 |

The mode 0 fssimu2 change is very small. Mode 0 gives a lower mean Butteraugli
score and a higher mean fcvvdp score than mode 1.

The changes are metric score differences. They are not bitrate savings or
percentages of visual quality. The image sets used for filter selection are not
independent test sets. These results do not show the quality change for all
images. SharpYUV inputs were not part of filter selection or this comparison.

## Decode time

The speed tests use an Apple M5 Pro, a release build, NEON assembly, and RGBA
output. The benchmark executable is from commit `688bd67`. The test set contains
three still images and two animations. Each animation measurement includes all
frames. The decoder sends the output to `/dev/null`.

Each command uses five warmup runs and 24 measured runs. Each run decodes the
complete file 64 times. No build, other test, or quality measurement runs at the
same time.

All ratios use mode 1 as the reference. A ratio below 1.000 means less decode
time than mode 1. A ratio above 1.000 means more decode time than mode 1. The
summary uses the geometric mean of the five file ratios.

| Mode | One thread | Three threads |
| ---: | ---------: | ------------: |
|    0 |      0.965 |         0.982 |
|    1 |      1.000 |         1.000 |
|    2 |      1.023 |         1.025 |
|    3 |      1.221 |         1.138 |

The table below gives the ratio for each file.

| File                  | Threads | Mode 0 / mode 1 | Mode 2 / mode 1 | Mode 3 / mode 1 |
| --------------------- | ------: | --------------: | --------------: | --------------: |
| `lossy.webp`          |       1 |           0.967 |           1.014 |           1.226 |
| `lossy.webp`          |       3 |           0.982 |           1.019 |           1.138 |
| `simplelf-lossy.webp` |       1 |           0.948 |           1.009 |           1.216 |
| `simplelf-lossy.webp` |       3 |           1.003 |           1.027 |           1.119 |
| `a_lossy.webp`        |       1 |           0.938 |           1.062 |           1.416 |
| `a_lossy.webp`        |       3 |           0.992 |           1.080 |           1.217 |
| `anim_yuv.webp`       |       1 |           0.986 |           1.013 |           1.118 |
| `anim_yuv.webp`       |       3 |           0.974 |           0.984 |           1.085 |
| `anim_yuva.webp`      |       1 |           0.986 |           1.017 |           1.151 |
| `anim_yuva.webp`      |       3 |           0.958 |           1.014 |           1.136 |

Small differences can be the result of measurement variation. The ratios apply
to this processor and test set. Image dimensions, image content, pixel format,
and thread count can change the ratios. The tests did not measure speed on a
native x86 processor.

## Filter operation

A filter tap is a source sample with a coefficient. Mode 2 uses three taps for
each output phase. The odd phase uses `[-10, 59, 15] / 64`. The even phase uses
`[15, 59, -10] / 64`. The coefficient sum is 1, so the filter does not change
constant chroma.

Mode 3 uses the Lanczos-5 filter from
[Halide's article about chroma upsampling](https://halide.cx/blog/chroma-handling/).
The FFmpeg filter configuration is as follows:

```text
scale=flags=lanczos+accurate_rnd+full_chroma_int:param0=5,format=rgb24
```

Modes 2 and 3 use separate horizontal and vertical filters. Both modes keep
fractional chroma through the vertical filter and the BT.601 RGB matrix. The RGB
matrix uses the limited range for its input. The decoder rounds the RGB results
and limits them to the output range.

Q6 is a fixed-point format with six fractional bits. Mode 2 subtracts `128 * 64`
from the horizontal Q6 results. The resulting range is `[-10742, 10678]`. This
operation removes the need for the same subtraction on each output row. The
output bytes do not change.

Each conversion band stores horizontal results for four rows per chroma plane in
mode 2. Mode 3 stores results for ten rows per chroma plane. The required memory
increases with image width. The decoder processes each source row once per band.

Modes 2 and 3 have handwritten NEON and AVX2 assembly for horizontal
interpolation and all five RGB layouts. When assembly is not available, these
modes use scalar code. During stream decoding, mode 2 waits for two future
chroma rows. Mode 3 waits for five future chroma rows. These conditions can
delay output by four or ten luma rows, respectively.

## Repeat the tests

Install decbench and its required programs. Add the required programs to `PATH`.
Use the commands below to repeat the quality tests for modes 1, 2, and 3. The
`uv` program supplies the Rich package.

```sh
uv run scripts/chromametrics.py /path/to/gb82-image-set/png \
    --decbench /path/to/decbench/dec.py \
    --output wpd-test-data/chromametrics-ffmpeg
```

Use `--encoder libwebp_default` to use libwebp input color conversion. The
script writes individual image scores and mean scores to JSON files. The script
does not include mode 0. For mode 0, decode the same inputs with `--uvmode 0`
and use the same decbench metric functions.

Use the commands below to repeat the speed tests for all four modes. The
commands use mode 1 as the reference.

```sh
mkdir -p build/chromabench
for threads in 1 3; do
    for file in lossy simplelf-lossy a_lossy anim_yuv anim_yuva; do
        hyperfine -N --warmup 5 --runs 24 \
            --export-json "build/chromabench/$file-t$threads.json" \
            -n mode1 "build/wpd --uvmode 1 -f rgba --threads $threads --repeat 64 wpd-test-data/$file.webp /dev/null" \
            -n mode0 "build/wpd --uvmode 0 -f rgba --threads $threads --repeat 64 wpd-test-data/$file.webp /dev/null" \
            -n mode2 "build/wpd --uvmode 2 -f rgba --threads $threads --repeat 64 wpd-test-data/$file.webp /dev/null" \
            -n mode3 "build/wpd --uvmode 3 -f rgba --threads $threads --repeat 64 wpd-test-data/$file.webp /dev/null"
    done
done
```

Divide each mode's mean time by the mode 1 mean time for the same file and
thread count. Use the geometric mean of these ratios for the summary.

## Verification

The implementation passed all 237 Meson tests. Rust tests passed with and
without assembly. The x86-64 build also passed.

Mode 2 output was the same as the saved implementation and scalar code for 134
files and all five RGB layouts. The mode 3 output did not change. Both filters
passed 20 checkasm seeds on NEON and AVX2. The AVX2 tests used direct kernel
selection under Rosetta. The library's CPU feature detection did not change.

The tests include output guards, unaligned buffers, both phases, and extreme
chroma values. Other tests include small dimensions, scalar tails, alpha,
animation, stream decoding, and option changes. The C tests include older option
structures. Stylecheck and Ruff checks passed for the implementation.
