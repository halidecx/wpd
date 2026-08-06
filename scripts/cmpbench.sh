#!/bin/bash -eu

OLD="${1:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
NEW="${2:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
REPEAT="${3:-50}"

for file in lossless.webp a_lossy.webp anim_rgb.webp anim_yuva.webp lossy.webp; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 12 --runs 64 --style basic \
        -n "old" "$OLD --repeat $REPEAT wpd-test-data/$file /dev/null" \
        -n "new" "$NEW --repeat $REPEAT wpd-test-data/$file /dev/null" \
        2>&1 | grep -E "Time|faster|±|Summary|old|new" | grep -v Benchmark
done
