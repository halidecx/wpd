#!/bin/bash -eu
# Runs the decoder over the whole corpus with the Rust itself instrumented by
# AddressSanitizer.
#
# usage: rustsan.sh [reference-binary]
#
# scripts/sanitize.sh instruments the C test harnesses and catches what reaches
# the intercepted allocator; this instruments every load and store the decoder
# makes, which is the coverage that went away when the last C did. It needs
# nightly, because -Zsanitizer and the -Zbuild-std that gets an instrumented
# standard library are both unstable.
#
# Both feature configurations are run. With assembly the hot kernels are
# hand-written and ASan cannot see inside them, so the no-asm build is the one
# where a bad index in a fallback has nowhere to hide; the asm build is the one
# that exercises the real dispatch.

REF="${1:-./build/wpd}"
# Whatever this machine is. -Zsanitizer needs an explicit --target to keep the
# instrumentation off the build scripts, but which target that is is not a
# property of the decoder.
TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"

if ! rustc +nightly --version >/dev/null 2>&1; then
    echo "rustsan.sh: needs the nightly toolchain:" >&2
    echo "  rustup toolchain install nightly --component rust-src" >&2
    exit 1
fi

export RUSTFLAGS="-Zsanitizer=address"
# The decoder frees everything it allocates through Drop, so a leak is a real
# finding rather than noise, and detect_leaks stays on where there is a
# LeakSanitizer to do it. Darwin on arm64 has none, and asking for one there
# does not degrade to a warning -- ASan aborts on the unknown option, which
# would fail every decode below for a reason that has nothing to do with the
# decoder.
leaks=1
case "$(uname -s)/$(uname -m)" in
Darwin/arm64|Darwin/aarch64)
    leaks=0
    echo "rustsan.sh: no LeakSanitizer on $(uname -s)/$(uname -m); leaks unchecked" >&2
    ;;
esac
export ASAN_OPTIONS="halt_on_error=1:abort_on_error=1:detect_leaks=$leaks"

run() {
    local name="$1" bin="target/$TARGET/debug/wpd"
    shift

    printf '\n=== %s ===\n' "$name"
    cargo +nightly build -q -p wpd-tool -Zbuild-std --target "$TARGET" "$@"

    local checked=0

    shopt -s nullglob
    for input in wpd-test-data/*.webp; do
        for fmt in argb rgba bgra rgb bgr rgb565 rgba4444 yuv420p yuva420p; do
            local want report
            want=$("$REF" --muxer md5 -f "$fmt" "$input" - 2>/dev/null | tail -1) || continue
            [ -n "$want" ] || continue
            # Diagnostics are held rather than streamed: the tool prints a
            # banner on every run, and a sanitizer report has to stand out.
            if ! report=$("$bin" --verify "$want" -f "$fmt" "$input" 2>&1 >/dev/null)
            then
                printf '%s\n' "$report" >&2
                echo "mismatch or sanitizer report: $input ($fmt)" >&2
                exit 1
            fi
            checked=$((checked + 1))
        done
    done
    echo "$checked decodes clean"
}

run "asm"
run "no asm" --no-default-features

echo
echo "done"
