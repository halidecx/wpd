# wpd

WebP Decoder

## Build

```sh
meson setup build
meson compile -C build
```

The decoder executable is written to `build/wpd`. It accepts a VP8 IVF input and
writes planar 4:2:0 YUV4MPEG output:

```sh
build/wpd input.ivf output.y4m
```

Architecture-specific assembly is enabled automatically. Use
`meson setup build -Denable_asm=disabled` for a portable C-only build.

## Test

```sh
meson test -C build
```

The checkasm dependency is detected through pkg-config or built as a Meson
subproject. It can be disabled with `-Denable_checkasm=disabled`.
