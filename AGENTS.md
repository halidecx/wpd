# wpd

A fast WebP decoder based on ffvp8 and FFmpeg's WebP decoder.

## Code

- Comments should be added only when necessary, maximum 2 lines
- No intrinsics; you may add them to test performance, but they should
  ultimately be re-written to handwritten assembly that matches or exceeds their
  performance
- Every assembly addition should have a checkasm test
- dav1d is the gold standard for decoder libraries; reference a dav1d checkout
  when you need to do research
- Run stylecheck after finishing changes

## Usage

Compile `wpd` binary to `build/`:

```sh
meson setup build
meson compile -C build
```

Test scripts:

```sh
./scripts/bench.sh      # performance, wpd vs libwebpdec
./scripts/test.sh       # correctness, vs FFmpeg
./scripts/cmpbench.sh   # performance, old vs new wpd
./scripts/md5check.sh   # correctness, old vs new wpd
./scripts/stylecheck.sh # format codebase
```

Inspect the structure of a WebP file (still or animated):

```sh
webpmux -info [i.webp]
```

When testing, decode into the `wpd-test-data` dir. **Never** delete the WebP
reference files in there.
