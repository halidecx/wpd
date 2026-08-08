#!/bin/bash -eu

WPD="${1:-"./build/wpd"}"
LWP="${2:-"./build/libwebpdec"}"
REPEAT="${3:-48}"

shopt -s nullglob
for file in wpd-test-data/*.webp; do
    printf '\n=== %s (x%s) ===\n' "$file" "$REPEAT"
    hyperfine -N --warmup 2 \
        -n "wpd" "$WPD --repeat $REPEAT $file /dev/null" \
        -n "lwp" "$LWP --repeat $REPEAT $file /dev/null"
done
