# wpd

A fast WebP decoder.

## Code

- **No comments**
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

Compile libwebp test harness:

```sh
meson setup build -Dlibwebpdecoder=/path/to/libwebpdecoder.a
meson compile -C build libwebpdec
```

Testdata:

```sh
meson configure build -Dtestdata_tests=true
meson test -C build --suite testdata
```

Test scripts:

```sh
./scripts/bench.sh      # performance, wpd vs libwebpdec
./scripts/test.sh       # correctness, vs FFmpeg
./scripts/cmpbench.sh   # performance, old vs new wpd
./scripts/md5check.sh   # correctness, old vs new wpd
./scripts/testdata.sh   # asm vs c E2E correctness
./scripts/stylecheck.sh # format codebase
```

Inspect the structure of a WebP file (still or animated):

```sh
webpmux -info [i.webp]
```
