#!/bin/bash -eu

WPD="${1:-"./build/wpd"}"
LWP="${2:-"./build/libwebpdec"}"
REPEAT="${3:-48}"
IWP="${4:-"./build/imagewebpdec"}"
BENCH_DIR="${BENCH_DIR:-build/bench}"

mkdir -p "$BENCH_DIR"

testfiles=(
    lossy.webp
    simplelf-lossy.webp
    anim_yuv.webp
    lossless.webp
    anim_rgb.webp
    a_lossy.webp
    anim_yuva.webp
)

# Avoid YUV-to-RGB conversion for lossy stills. libwebp's animation API only
# exposes packed RGB output, so animations use RGBA, as do lossless stills.
for f in "${testfiles[@]}"; do
    case "$f" in
        lossy.webp|simplelf-lossy.webp) format=yuv420p ;;
        a_lossy.webp) format=yuva420p ;;
        *) format=rgba ;;
    esac
    args=(-n "wpd" "$WPD -f $format --threads 1 --repeat $REPEAT wpd-test-data/$f /dev/null")
    [ -x "$LWP" ] && args+=(-n "lwp" "$LWP -f $format --repeat $REPEAT wpd-test-data/$f /dev/null")
    # image-webp has no planar YUV output.
    if [ "$format" = rgba ] && [ -x "$IWP" ]; then
        args+=(-n "iwp" "$IWP -f $format --repeat $REPEAT wpd-test-data/$f /dev/null")
    fi

    printf '\n=== %s (%s, x%s) ===\n' "$f" "$format" "$REPEAT"
    hyperfine -N --warmup 3 --runs 20 \
        --export-json "$BENCH_DIR/${f%.webp}-$format.json" "${args[@]}"
done
