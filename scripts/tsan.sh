#!/bin/bash -eu
# Races in the threaded paths. Nothing else in the tree exercises
# ThreadSanitizer, and the decoder only hands work off where the data is
# provably disjoint, so this is what checks that the proof is real.

TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"

if ! rustc +nightly --version >/dev/null 2>&1; then
    echo "tsan.sh: needs the nightly toolchain:" >&2
    echo "  rustup toolchain install nightly --component rust-src" >&2
    exit 1
fi

export RUSTFLAGS="-Zsanitizer=thread"
export TSAN_OPTIONS="halt_on_error=1:abort_on_error=1:second_deadlock_stack=1"

# --lib, not the whole suite: rustdoc does not take -Zsanitizer, so the
# doctests would fail to build rather than run, and the api sweep decodes the
# corpus at six thread counts, which instrumented takes the best part of an
# hour. The tool loop below covers the same paths end to end.
printf '\n=== unit tests ===\n'
cargo +nightly test -p wpd --lib -Zbuild-std --target "$TARGET" "$@"

bin="target/$TARGET/debug/wpd"

printf '\n=== the tool over the test data ===\n'
cargo +nightly build -q -p wpd-tool -Zbuild-std --target "$TARGET" "$@"

checked=0
shopt -s nullglob
for input in wpd-test-data/*.webp; do
    for fmt in rgba rgbA yuv420p yuva420p rgb565; do
        for mode in "" "--subframe" "--scale 320x240"; do
            set +e
            # shellcheck disable=SC2086
            "$bin" --threads 8 $mode -f "$fmt" "$input" /dev/null >/dev/null 2>&1
            status=$?
            set -e

            # A race aborts the process, which is a signal rather than an
            # ordinary refusal; nothing here may swallow that.
            if [ "$status" -ge 128 ]; then
                echo "tsan.sh: killed by signal $((status - 128)):" >&2
                # shellcheck disable=SC2086
                "$bin" --threads 8 $mode -f "$fmt" "$input" /dev/null >/dev/null
                exit 1
            fi
            if [ "$status" -ne 0 ]; then
                echo "tsan.sh: $input ($fmt${mode:+ $mode}) failed to decode" >&2
                exit 1
            fi
            checked=$((checked + 1))
        done
    done
done
echo "$checked threaded decodes clean"
