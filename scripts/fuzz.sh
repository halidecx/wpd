#!/bin/bash -eu
#
# Decodes damaged and truncated copies of the test data through every entry
# point wpd has, under AddressSanitizer and UndefinedBehaviorSanitizer. It
# checks for crashes and undefined behaviour, not for particular pixels; the
# testdata and parity suites cover output.
#
# Both range coders get a run: the 32-bit one is dead code in an ordinary
# 64-bit build, so without a second build it is never sanitized at all.
#
# usage: fuzz.sh [trials-per-file] [files...]   (default 300, wpd-test-data/*)

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

    # A directory left behind by an interrupted run may have been configured
    # for the other range coder, so reconfigure rather than trust it.
    if [ -d "$build" ]; then
        meson configure "$build" "${opts[@]}" >/dev/null
    else
        meson setup "$build" "${opts[@]}" >/dev/null
    fi
    meson compile -C "$build" test_fuzz

    # The harness leaks nothing of its own, but a decode that dies part way
    # through legitimately leaves the decoder holding memory, so only look for
    # memory errors here.
    ASAN_OPTIONS=detect_leaks=0 \
    UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1 \
        "./$build/test_fuzz" -n "$TRIALS" "${inputs[@]}"
    rm -rf "$build"
}

run build-fuzz false
run build-fuzz-rac32 true
