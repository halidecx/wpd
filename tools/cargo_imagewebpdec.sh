#!/bin/bash -eu

SOURCE_ROOT="${1:?source root}"
OUT_DIR="${2:?out dir}"
PROFILE="${3:?profile}"

args=(build --manifest-path "$SOURCE_ROOT/tools/imagewebpdec/Cargo.toml"
      --target-dir "$OUT_DIR/cargo-imagewebpdec")
case "$PROFILE" in
release) args+=(--release) ;;
debug)   ;;
*)       args+=(--profile "$PROFILE") ;;
esac

built="$OUT_DIR/cargo-imagewebpdec/$PROFILE"
if [ -n "${WPD_CARGO_TARGET:-}" ]; then
    args+=(--target "$WPD_CARGO_TARGET")
    built="$OUT_DIR/cargo-imagewebpdec/$WPD_CARGO_TARGET/$PROFILE"
fi

cargo "${args[@]}"

cp -f "$built/imagewebpdec" "$OUT_DIR/imagewebpdec"
