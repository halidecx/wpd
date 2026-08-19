#!/bin/bash -eu
# Runs the core crate's tests under miri, which checks for undefined behaviour
# the compiler is otherwise entitled to assume never happens.
#
# usage: miri.sh [cargo test args...]
#
# --no-default-features is not optional: with `asm` on, the crate calls
# hand-written assembly through `extern "C"`, which miri cannot execute. What
# this run proves is that the safe fallbacks — the ones the assembly is checked
# against by checkasm — are free of UB, and by extension that the slice
# arithmetic every kernel shares is sound.
#
# Isolation is disabled because the tests read the clock through the standard
# library's test harness.

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    echo "miri.sh: needs the nightly toolchain and the miri component:" >&2
    echo "  rustup toolchain install nightly --component miri" >&2
    exit 1
fi

export MIRIFLAGS="${MIRIFLAGS:--Zmiri-disable-isolation}"

cargo +nightly miri test -p wpd --no-default-features "$@"
