#!/bin/bash -eu

OLD="${1:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
NEW="${2:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
REPEAT="${3:-48}"

for file in lossless.webp a_lossy.webp anim_rgb.webp anim_yuva.webp lossy.webp; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 2 --runs 16 \
        -n "old" "$OLD --repeat $REPEAT wpd-test-data/$file /dev/null" \
        -n "new" "$NEW --repeat $REPEAT wpd-test-data/$file /dev/null"
done
