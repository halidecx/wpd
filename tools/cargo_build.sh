#!/bin/bash -eu
# Builds the library with cargo and stages the artifacts where Meson expects
# them. Meson owns the C test harnesses and the testdata suite; cargo owns the
# library and the assembly.
#
# NAME distinguishes configurations that must coexist in one build directory:
# checkasm needs a copy built without trim_dsp, so that the fallbacks it
# compares the assembly against still exist. Each gets its own target dir, so
# the two do not invalidate each other's incremental state.
#
# usage: cargo_build.sh SOURCE_ROOT OUT_DIR NAME PROFILE [FEATURE...]

SOURCE_ROOT="${1:?source root}"
OUT_DIR="${2:?out dir}"
NAME="${3:?artifact name}"
PROFILE="${4:?profile}"
shift 4

args=(build --manifest-path "$SOURCE_ROOT/Cargo.toml" -p wpd-capi
      --target-dir "$OUT_DIR/cargo-$NAME" --no-default-features)
case "$PROFILE" in
release) args+=(--release) ;;
debug)   ;;
*)       args+=(--profile "$PROFILE") ;;
esac
[ $# -gt 0 ] && args+=(--features "$(IFS=,; echo "$*")")

cargo "${args[@]}"

built="$OUT_DIR/cargo-$NAME/$PROFILE"
cp -f "$built/libwpd_capi.a" "$OUT_DIR/lib$NAME.a"
