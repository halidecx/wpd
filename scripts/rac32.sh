#!/bin/bash -eu

BUILD=build-rac32
OPTS=(-Dforce_rac32=true -Dtestdata_tests=true)

if [ -d "$BUILD" ]; then
    meson configure "$BUILD" "${OPTS[@]}" >/dev/null
else
    meson setup "$BUILD" "${OPTS[@]}" >/dev/null
fi

meson test -C "$BUILD" "$@"
rm -rf "$BUILD"
