#!/bin/bash -eu
#
# Runs the whole test suite against a build that forces the 32-bit range
# coder. Every ordinary 64-bit build picks the 64-bit coder, so without this
# the 32-bit one is never compiled, let alone exercised.
#
# usage: rac32.sh [meson test args...]

BUILD=build-rac32
OPTS=(-Dforce_rac32=true -Dtestdata_tests=true)

if [ -d "$BUILD" ]; then
    meson configure "$BUILD" "${OPTS[@]}" >/dev/null
else
    meson setup "$BUILD" "${OPTS[@]}" >/dev/null
fi

meson test -C "$BUILD" "$@"
rm -rf "$BUILD"
