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
