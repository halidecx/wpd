#!/bin/bash -eu

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    echo "miri.sh: needs the nightly toolchain and the miri component:" >&2
    echo "  rustup toolchain install nightly --component miri" >&2
    exit 1
fi

export MIRIFLAGS="${MIRIFLAGS:--Zmiri-disable-isolation}"

cargo +nightly miri test -p wpd --no-default-features "$@"
