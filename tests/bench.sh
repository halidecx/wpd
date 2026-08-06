#!/bin/bash -eu

RUNS="${1:-"256"}"
WARMUP_RUNS="${2:-"24"}"

for file in "wpd-test-data/lossy.webp" "wpd-test-data/a_lossy.webp" "wpd-test-data/lossless.webp"; do
    hyperfine -N --warmup "${WARMUP_RUNS}" --runs "${RUNS}" "./build/wpd $file /dev/null" "dwebp $file -yuv -o /dev/null"
done
