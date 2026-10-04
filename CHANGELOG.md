# Changelog

## Unreleased — 0.2.0

### Build and CI

- Repair the end-to-end fuzz target's decoder options initializer so all four
  coverage-guided targets build again.
- Correct the optional Wuffs dependency name in the Meson build.
- Add one GitHub Actions workflow for tests, rustfmt/clippy, fuzz target builds,
  seeded smoke runs, and the existing correctness and sanitizer checks.
- Bound fuzz-harness pictures to one megapixel so mutated dimensions fit the
  smoke run's memory budget without changing decoder limits.

### libwebp compatibility

- Reject corrupted first-chunk FourCCs instead of silently treating an extended
  container as a simple image.
- Add opt-in libwebp still compatibility through Rust, C and `--libwebp-compat`,
  retaining strict decoding by default. Match duplicate VP8X chunks, shortened
  image chunks, damaged trailing chunks and simple-image final padding.
- Retain the complete compatible still payload until streaming EOF and preserve
  animation container validation.
- Match portable libwebp VP8 transforms on non-conforming lossy streams, with
  damaged-stream regressions. Save and restore each decoder's strict DSP tables
  when toggling the mode; native libwebp CPU output can still differ.
- Accept a final alpha entropy symbol crossing EOF only when compatibility mode
  has already produced the complete alpha plane.
- Add a directory comparison script for libwebp 1.6.0 `dwebp` and `anim_dump`,
  distinguishing visible differences from transparent RGB.
- Fuzz strict and compatible decoding with public and damaged regression seeds.
