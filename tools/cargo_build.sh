#!/bin/bash -eu
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

if [ "$PROFILE" = release ]; then
    slim="$OUT_DIR/lib$NAME.a.slim"
    if "${STRIP:-strip}" --strip-debug \
           --remove-section=.llvmbc --remove-section=.llvmcmd \
           -o "$slim" "$OUT_DIR/lib$NAME.a" 2>/dev/null; then
        mv -f "$slim" "$OUT_DIR/lib$NAME.a"
    else
        rm -f "$slim"
        echo "cargo_build.sh: cannot slim lib$NAME.a, shipping it whole" >&2
    fi
fi
