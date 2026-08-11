#!/bin/bash -eu
# usage: sanitize.sh [meson test args...]

BASE=(-Db_lundef=false --buildtype=debugoptimized -Dtestdata_tests=true)
COMMON=(-Db_sanitize=address,undefined "${BASE[@]}")
# ThreadSanitizer cannot see into hand-written assembly, and does not compose
# with the other two, so the race build stands on its own.
THREAD=(-Db_sanitize=thread -Denable_asm=false "${BASE[@]}")

configure() {
    local build="$1"
    shift

    if [ -d "$build" ]; then
        meson configure "$build" "$@" >/dev/null
    else
        meson setup "$build" "$@" >/dev/null
    fi
}

configure build-sanitize-asm "${COMMON[@]}" -Denable_asm=true -Dtrim_dsp=false
configure build-sanitize-noasm "${COMMON[@]}" -Denable_asm=false
configure build-sanitize-thread "${THREAD[@]}"

for build in build-sanitize-asm build-sanitize-noasm build-sanitize-thread; do
    printf '\n=== %s ===\n' "$build"
    meson test -C "$build" "$@"
    rm -rf "$build"
done
