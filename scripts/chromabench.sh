#!/bin/bash -eu

WPD="${1:-./build/wpd}"
REPEAT="${2:-64}"
THREADS="${THREADS:-1}"
RUNS="${RUNS:-12}"
BENCH_DIR="${BENCH_DIR:-build/chromabench}"

mkdir -p "$BENCH_DIR"

for f in lossy simplelf-lossy a_lossy anim_yuv anim_yuva; do
    hyperfine -N --warmup 3 --runs "$RUNS" \
        --export-json "$BENCH_DIR/$f-t$THREADS.json" \
        -n fancy "$WPD -f rgba --threads $THREADS --repeat $REPEAT wpd-test-data/$f.webp /dev/null" \
        -n extra-fancy "$WPD --extra-fancy -f rgba --threads $THREADS --repeat $REPEAT wpd-test-data/$f.webp /dev/null"
done
