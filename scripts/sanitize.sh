#!/bin/bash -eu

COMMON=(-Db_sanitize=address,undefined -Db_lundef=false
        --buildtype=debugoptimized -Dtestdata_tests=true)

export ASAN_OPTIONS=halt_on_error=1:abort_on_error=1:print_summary=1:verify_asan_link_order=0

configure() {
    local build="$1"
    shift

    if [ -d "$build" ]; then
        meson configure "$build" "${COMMON[@]}" "$@" >/dev/null
    else
        meson setup "$build" "${COMMON[@]}" "$@" >/dev/null
    fi
}

configure build-sanitize-asm -Denable_asm=true -Dtrim_dsp=false
configure build-sanitize-noasm -Denable_asm=false

for build in build-sanitize-asm build-sanitize-noasm; do
    printf '\n=== %s ===\n' "$build"
    meson test -C "$build" "$@"
    rm -rf "$build"
done
