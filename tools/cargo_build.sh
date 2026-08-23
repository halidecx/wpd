#!/bin/bash -eu

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

cargo_args=()
built="$OUT_DIR/cargo-$NAME/$PROFILE"
target="${WPD_CARGO_TARGET:-}"
if [ "${WPD_NIGHTLY_SIZE:-}" = 1 ]; then
    # -Zbuild-std needs an explicit target even when it is the host.
    [ -n "$target" ] || target="$(rustc -vV | awk '/^host: / { print $2 }')"
    export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options -Cpanic=abort -Zlocation-detail=none"
    cargo_args+=(-Zbuild-std=std,panic_abort -Zbuild-std-features=optimize_for_size)
fi
if [ -n "$target" ]; then
    args+=(--target "$target")
    built="$OUT_DIR/cargo-$NAME/$target/$PROFILE"
fi

cargo ${cargo_args[@]+"${cargo_args[@]}"} "${args[@]}"

cp -f "$built/libwpd_capi.a" "$OUT_DIR/lib$NAME.a"

if [ "$PROFILE" = release ] || [ "$PROFILE" = minsize ]; then
    . "$(dirname "$0")/strip_artifact.sh"
    strip_artifact archive "$OUT_DIR/lib$NAME.a" ||
        echo "cargo_build.sh: cannot slim lib$NAME.a, shipping it whole" >&2
fi
