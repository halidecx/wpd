#!/bin/sh
set -eu

source_root=$1
build_dir=$2
pkg_config=$3
shift 3

stage=$(mktemp -d "${TMPDIR:-/tmp}/wpd-pkgconfig.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/include" "$stage/lib/pkgconfig"
cp "$source_root/include/wpd.h" "$stage/include/"
cp "$build_dir/libwpd.a" "$stage/lib/"
cp "$build_dir/meson-private/wpd.pc" "$stage/lib/pkgconfig/"

# Only the archive is staged, so -lwpd cannot select the shared library.
export PKG_CONFIG_PATH=
export PKG_CONFIG_LIBDIR="$stage/lib/pkgconfig"
flags=$("$pkg_config" --define-variable=prefix="$stage" \
    --define-variable=libdir="$stage/lib" \
    --define-variable=includedir="$stage/include" --static --cflags --libs wpd)

# pkg-config emits compiler arguments separated by spaces.
# shellcheck disable=SC2086
"$@" -DWPD_STATIC "$source_root/tests/pkgconfig-static.c" $flags \
    -o "$stage/test-static"
"$stage/test-static"
