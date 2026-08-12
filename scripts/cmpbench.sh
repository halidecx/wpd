#!/bin/bash -eu

OLD="${1:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
NEW="${2:?usage: cmpbench.sh OLD_BIN NEW_BIN [repeat]}"
REPEAT="${3:-48}"

shopt -s nullglob
for file in wpd-test-data/*.webp; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 2 --runs 16 \
        -n "old" "$OLD --repeat $REPEAT $file /dev/null" \
        -n "new" "$NEW --repeat $REPEAT $file /dev/null"
done
