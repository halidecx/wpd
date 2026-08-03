# wpd

WebP Decoder

## Build

```sh
meson setup build
meson compile -C build
```

The decoder executable is written to `build/wpd`. It accepts a VP8 IVF or lossy
WebP input and writes planar 4:2:0 YUV4MPEG output:

```sh
build/wpd input.ivf output.y4m
build/wpd input.webp output.y4m
```

An output filename ending in `.yuv` selects raw planar YUV output (no headers),
byte-identical to `dwebp input.webp -yuv -o output.yuv`:

```sh
build/wpd input.webp output.yuv
```

Using `/dev/null` selects decode-only output and avoids serializing the decoded
picture:

```sh
build/wpd input.ivf /dev/null
```

Architecture-specific assembly is enabled automatically. Use
`meson setup build -Denable_asm=false` for a portable C-only build.

## Test

```sh
meson test -C build
```

The checkasm dependency is detected through pkg-config or built as a Meson
subproject. The checkasm executable and test are enabled when assembly is
enabled.
