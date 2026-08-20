#!/bin/bash -eu

REF="${1:-./build/wpd}"
TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"

if ! rustc +nightly --version >/dev/null 2>&1; then
    echo "rustsan.sh: needs the nightly toolchain:" >&2
    echo "  rustup toolchain install nightly --component rust-src" >&2
    exit 1
fi

export RUSTFLAGS="-Zsanitizer=address"
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
