#!/bin/bash -eu

WPD="${1:-"./build/wpd"}"
LWP="${2:-"./build/libwebpdec"}"
REPEAT="${3:-48}"
IWP="${4:-"./build/imagewebpdec"}"

testfiles=(
    lossy.webp
    simplelf-lossy.webp
    anim_yuv.webp
    lossless.webp
    anim_rgb.webp
    a_lossy.webp
    anim_yuva.webp
)

# force rgba since image-webp only outputs rgb[a]; wpd gets one thread, as
# neither of the others decodes on more
for f in "${testfiles[@]}"; do
    args=(-n "wpd" "$WPD --threads 1 --repeat $REPEAT wpd-test-data/$f /dev/null")
    [ -x "$LWP" ] && args+=(-n "lwp" "$LWP -f rgba --repeat $REPEAT wpd-test-data/$f /dev/null")
    [ -x "$IWP" ] && args+=(-n "iwp" "$IWP -f rgba --repeat $REPEAT wpd-test-data/$f /dev/null")

    printf '\n=== %s (x%s) ===\n' "$f" "$REPEAT"
    hyperfine -N --warmup 2 "${args[@]}"
done
