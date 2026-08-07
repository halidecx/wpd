#!/bin/bash -eu

WPD="${1:-"./build/wpd"}"
LWP="${2:-"./build/libwebpdec"}"
REPEAT="${3:-48}"

for file in "lossy.webp" "a_lossy.webp" "anim_rgb.webp"; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 4 --runs 16 \
        -n "wpd" "$WPD --repeat $REPEAT wpd-test-data/$file /dev/null" \
        -n "lwp" "$LWP --repeat $REPEAT wpd-test-data/$file /dev/null"
done

for file in "lossless.webp" "anim_yuva.webp" "anim_yuv.webp"; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 2 --runs 8 \
        -n "wpd" "$WPD --repeat $REPEAT wpd-test-data/$file /dev/null" \
        -n "lwp" "$LWP --repeat $REPEAT wpd-test-data/$file /dev/null"
done
