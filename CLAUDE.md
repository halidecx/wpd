# wpd

A fast WebP decoder based on ffvp8 and FFmpeg's WebP decoder.

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
