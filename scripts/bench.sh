#!/bin/bash -eu

WPD="${1:-"./build/wpd"}"
LWP="${2:-"./build/libwebpdec"}"
REPEAT="${3:-48}"
IWP="${4:-"./build/imagewebpdec"}"
WUF="${5:-"./build/wuffsdec"}"
BENCH_DIR="${BENCH_DIR:-build/bench}"
# Threads a wpd decode may use, as its --threads takes them. libwebp has one
# optional worker, so any count above 1 turns that on. image-webp and Wuffs
# decode on the calling thread whatever this says.
THREADS="${THREADS:-1}"

mkdir -p "$BENCH_DIR"

lwp_threads=1
[ "$THREADS" != 1 ] && lwp_threads=2

testfiles=(
    lossy.webp
    simplelf-lossy.webp
    anim_yuv.webp
    lossless.webp
    anim_rgb.webp
    a_lossy.webp
    anim_yuva.webp
)

# Lossy stills are timed twice: once to planar YUV, skipping YUV-to-RGB
# conversion, and once to RGBA, the only output image-webp and Wuffs have.
# libwebp's animation API only exposes packed RGB output, so animations use
# RGBA, as do lossless stills. Wuffs cannot decode animations.
for f in "${testfiles[@]}"; do
    case "$f" in
        lossy.webp|simplelf-lossy.webp) formats=(yuv420p rgba) ;;
        a_lossy.webp) formats=(yuva420p rgba) ;;
        *) formats=(rgba) ;;
    esac
    for format in "${formats[@]}"; do
        args=(-n "wpd" "$WPD -f $format --threads $THREADS --repeat $REPEAT wpd-test-data/$f /dev/null")
        [ -x "$LWP" ] && args+=(-n "lwp" "$LWP -f $format -t $lwp_threads --repeat $REPEAT wpd-test-data/$f /dev/null")
        if [ "$format" = rgba ]; then
            [ -x "$IWP" ] && args+=(-n "iwp" "$IWP -f $format --repeat $REPEAT wpd-test-data/$f /dev/null")
            case "$f" in
                anim_*) ;;
                *) [ -x "$WUF" ] && args+=(-n "wuf" "$WUF -f $format --repeat $REPEAT wpd-test-data/$f /dev/null") ;;
            esac
        fi

        printf '\n=== %s (%s, x%s, %s threads) ===\n' "$f" "$format" "$REPEAT" "$THREADS"
        hyperfine -N --warmup 3 --runs 20 \
            --export-json "$BENCH_DIR/${f%.webp}-$format-t$THREADS.json" "${args[@]}"
    done
done
