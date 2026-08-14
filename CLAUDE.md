# wpd

A fast WebP decoder.

## Code

- **Do not add comments**, unless they are for a public header file
- No intrinsics; you may add them to test performance, but they should
  ultimately be re-written to handwritten assembly that matches or exceeds their
  performance
- Every assembly addition should have a checkasm test
- dav1d is the gold standard for decoder libraries; reference a dav1d checkout
  when you need to do research
- Run stylecheck after finishing changes
- After editing a Markdown file, run `deno fmt <file>`

## Usage

Compile `wpd` binary to `build/`:

```sh
meson setup build
meson compile -C build
```

Compile libwebp test harness. libwebp comes from a pinned Meson subproject by
default; `-Dlibwebp=system` or an explicit path overrides that:

```sh
meson compile -C build libwebpdec
meson setup build -Dlibwebpdecoder=/path/to/libwebpdecoder.a
```

Testdata:

```sh
meson configure build -Dtestdata_tests=true
meson test -C build --suite testdata
```

Test scripts:

```sh
./scripts/bench.sh      # performance, wpd vs libwebpdec
./scripts/cmpbench.sh   # performance, old vs new wpd
./scripts/md5check.sh   # correctness, old vs new wpd
./scripts/clicheck.sh   # correctness, everything else the tool prints
./scripts/testdata.sh   # asm vs c E2E correctness
./scripts/fuzz.sh       # robustness, damaged input under sanitizers
./scripts/animcheck.sh  # correctness, bit-exact argb, wpd vs libwebp
./scripts/rac32.sh      # correctness, forced 32-bit range coder
./scripts/sanitize.sh   # memory safety, suite under sanitizers, asm and c
./scripts/stylecheck.sh # format codebase
```

Inspect the structure of a WebP file (still or animated):

```sh
webpmux -info [i.webp]
```

When testing, decode into the `wpd-test-data` dir. **Never** delete the WebP
reference files in there.
