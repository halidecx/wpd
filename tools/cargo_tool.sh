#!/bin/bash -eu
# Builds the command-line tool with cargo and stages the binary where Meson
# expects it. The tool links the same wpd-capi archive the library target
# builds, so it exercises the C ABI exactly as an outside consumer would.
#
# usage: cargo_tool.sh SOURCE_ROOT OUT_DIR PROFILE [FEATURE...]

SOURCE_ROOT="${1:?source root}"
OUT_DIR="${2:?out dir}"
PROFILE="${3:?profile}"
shift 3

args=(build --manifest-path "$SOURCE_ROOT/Cargo.toml" -p wpd-tool
      --target-dir "$OUT_DIR/cargo-tool" --no-default-features)
case "$PROFILE" in
release) args+=(--release) ;;
debug)   ;;
*)       args+=(--profile "$PROFILE") ;;
esac
[ $# -gt 0 ] && args+=(--features "$(IFS=,; echo "$*")")

cargo "${args[@]}"

cp -f "$OUT_DIR/cargo-tool/$PROFILE/wpd" "$OUT_DIR/wpd"
