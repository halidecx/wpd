# wpd

A fast WebP decoder based on ffvp8 and FFmpeg's WebP decoder.

## Usage

Compile `wpd` binary to `build/`:

```sh
meson setup build
meson compile -C build
```

Test reference SSIM scores versus decodes:

```sh
./test.sh
```

Inspect the structure of a WebP file (still or animated):

```sh
webpmux -info [i.webp]
```

When testing, decode into the `testdata` dir. **Never** delete the WebP
reference files in there.
