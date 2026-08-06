#!/bin/bash -eu

RUNS="${1:-"256"}"
WARMUP_RUNS="${2:-"24"}"

for file in "testdata/lossy.webp" "testdata/a_lossy.webp" "testdata/lossless.webp"; do
    hyperfine -N --warmup "${WARMUP_RUNS}" --runs "${RUNS}" "./build/wpd $file /dev/null" "dwebp $file -yuv -o /dev/null"
done
