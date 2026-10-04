# Changelog

This is a draft for the first tagged release. There are no historical release
dates to record. The proposed first tag is `v0.2.0`, matching the current Cargo
workspace version, after the maintainer merges the patch series and completes
the release checks. Nothing here announces a tag or crates.io publication.

## Unreleased — 0.2.0

### Existing decoder functionality

- Decode lossy VP8, lossless VP8L, alpha, and animated WebP in Rust, with
  optional handwritten assembly and worker threads.
- Provide Rust and C APIs, incremental decoding, composited animation or raw
  subframes, frame timing, metadata access, scaling, cropping, and frame size
  limits.
- Provide a CLI with packed RGB/RGBA and planar YUV output, raw, PAM, PPM,
  YUV4MPEG2 and MD5 muxers, replay, benchmark repeats, and input/output byte
  budgets.
- Build static and shared C libraries with Meson, including headers and
  pkg-config metadata. The project uses the BSD-2-Clause license.

### Build and CI

- Repair the end-to-end fuzz target's decoder options initializer so all four
  coverage-guided targets build again.
- Correct the optional Wuffs dependency name in the Meson build.
- Add one GitHub Actions workflow for tests, rustfmt/clippy, fuzz target builds,
  seeded smoke runs, and the existing correctness and sanitizer checks.
- Bound fuzz-harness pictures to one megapixel so mutated dimensions fit the
  smoke run's memory budget without changing decoder limits.

### CLI additions

- Add `--info=json` with dimensions, frame count, raw frame durations, loop
  count, alpha, ICC presence, and an ordered chunk list. Keep text `--info`.
- Add `--icc-out`, `--exif-out`, and `--xmp-out` for original metadata bytes.
- Add `--muxer frames` for numbered RGBA PAM files and a JSON timing manifest.
  Publish the final manifest after successful decoding and output flushing,
  refuse existing directories, and include the manifest in the output budget.
- Keep JSON separate from the recognized stdout paths and cover streaming,
  truncation, binary metadata, replay, scaling, and output errors in CLI tests.

### Validation scope

The local patch series has been tested with Rust 1.98 and 1.99. This does not
establish the declared Rust 1.82 minimum or claim crates.io packaging support.
