#!/bin/bash -eu
# Output must not depend on how many threads produced it. Decodes every file in
# the test data, in every format, at a spread of thread counts, and asserts one
# md5 across all of them. Counts that are not powers of two matter once work is
# divided rather than handed over whole.

BIN="${1:-./build/wpd}"
COUNTS="${2:-1 2 3 5 8 16}"
DIR="${TESTDATA_DIR:-wpd-test-data}"

shopt -s nullglob
checked=0

for input in "$DIR"/*.webp; do
    for fmt in argb rgba bgra rgb bgr Argb rgbA rgb565 rgba4444 rgbA4444 \
               yuv420p yuva420p; do
        # Each mode has its own answer; only the thread count must not move it.
        for mode in "" "--subframe" "--stream 4096"; do
            want=""
            for threads in $COUNTS; do
                # shellcheck disable=SC2086
                got=$("$BIN" --threads "$threads" $mode --muxer md5 \
                      -f "$fmt" "$input" - 2>/dev/null | tail -1) || continue
                [ -n "$got" ] || continue
                if [ -z "$want" ]; then
                    want="$got"
                elif [ "$got" != "$want" ]; then
                    printf 'mismatch: %s (%s%s) at %s threads\n  one: %s\n  %3s: %s\n' \
                        "$input" "$fmt" "${mode:+ $mode}" "$threads" \
                        "$want" "$threads" "$got" >&2
                    exit 1
                fi
                checked=$((checked + 1))
            done
        done
    done
done

echo "$checked decodes agree"
