#!/bin/bash -eu

TRIALS="${1:-300}"
shift || true

if [ "$#" -gt 0 ]; then
    inputs=("$@")
else
    shopt -s nullglob
    inputs=(wpd-test-data/*.webp)
fi

if [ "${#inputs[@]}" -eq 0 ]; then
    echo "no input files" >&2
    exit 1
fi

run() {
    local build="$1" rac32="$2"
    local opts=(-Db_sanitize=address,undefined -Db_lundef=false
                -Dforce_rac32="$rac32" --buildtype=debugoptimized)

    if [ -d "$build" ]; then
        meson configure "$build" "${opts[@]}" >/dev/null
    else
        meson setup "$build" "${opts[@]}" >/dev/null
    fi
    meson compile -C "$build" test_fuzz

    ASAN_OPTIONS=detect_leaks=0 \
    UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1 \
        "./$build/test_fuzz" -n "$TRIALS" "${inputs[@]}"
    rm -rf "$build"
}

run build-fuzz false
run build-fuzz-rac32 true
