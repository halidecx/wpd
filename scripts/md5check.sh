#!/bin/bash -eu

BIN="${1:-./build/wpd}"
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

shopt -s nullglob
for input in wpd-test-data/*.webp; do
    case "$input" in
        *anim_rgb*|*lossless*) formats="argb" ;;
        *anim_yuva*|*a_lossy*) formats="yuva420p yuv420p" ;;
        *)                     formats="yuv420p" ;;
    esac
    for fmt in $formats; do
        "$BIN" -f "$fmt" "$input" "$OUT/out.raw"
        printf '%s\t%s\t%s\n' "$input" "$fmt" "$(md5 -q "$OUT/out.raw")"
    done
done
