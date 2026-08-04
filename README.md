# wpd

WebP Decoder

Decodes the lossy (VP8) bitstream of a WebP file. Because a still image is a
single VP8 keyframe, the decoder is intra-only: there is no motion compensation,
no reference frame management and no inter-frame prediction. Lossless (VP8L)
WebP and alpha are not supported.

## Build

```sh
meson setup build
meson compile -C build
```

The decoder executable is written to `build/wpd`. It reads a lossy WebP file and
writes raw planar 4:2:0 YUV, byte-identical to
`dwebp input.webp -yuv -o output.yuv`:

```sh
build/wpd input.webp output.yuv
```

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
