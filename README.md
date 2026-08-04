# wpd

WebP decoder supporting lossy (VP8) and lossless (VP8L) images, alpha, and
animation. VP8 decoding is intra-only: there is no motion compensation,
reference frame management, or inter-frame prediction.

## Build

```sh
meson setup build
meson compile -C build
```

The decoder executable is written to `build/wpd`. It reads a WebP file and
writes raw frames:

```sh
build/wpd input.webp output.raw [pixel_format]
```

`pixel_format` is optional and may be `yuv420p`, `yuva420p`, or `argb`; when
omitted, the decoded frame format is used. Animated input writes its frames
sequentially to the output.

Using `/dev/null` selects decode-only output and avoids serializing the decoded
picture:

```sh
build/wpd input.webp /dev/null
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
