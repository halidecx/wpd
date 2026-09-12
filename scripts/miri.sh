#!/bin/bash -eu

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    echo "miri.sh: needs the nightly toolchain and the miri component:" >&2
    echo "  rustup toolchain install nightly --component miri" >&2
    exit 1
fi

export MIRIFLAGS="${MIRIFLAGS:--Zmiri-disable-isolation}"

cargo +nightly miri test -p wpd --no-default-features "$@"
cargo +nightly miri test -p wpd-capi --no-default-features \
    legacy_frame_storage_is_only_accessed_through_its_extent
