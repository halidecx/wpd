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

# rustc links the tool with -nodefaultlibs, which suppresses the runtime the
# compiler driver would otherwise add for -fsanitize, so each runtime has to be
# named explicitly. These are GCC's names; a clang build wants
# -lclang_rt.<name>-<arch> instead and will fail to find these.
if [ -n "${WPD_SANITIZE:-}" ]; then
    runtimes=""
    for s in ${WPD_SANITIZE//,/ }; do
        case "$s" in
        address)   runtimes="$runtimes -Clink-arg=-lasan" ;;
        undefined) runtimes="$runtimes -Clink-arg=-lubsan" ;;
        thread)    runtimes="$runtimes -Clink-arg=-ltsan" ;;
        leak)      runtimes="$runtimes -Clink-arg=-llsan" ;;
        esac
    done
    export RUSTFLAGS="${RUSTFLAGS:-} -Clink-arg=-fsanitize=$WPD_SANITIZE$runtimes"
fi

args=(build --manifest-path "$SOURCE_ROOT/Cargo.toml" -p wpd-tool
      --target-dir "$OUT_DIR/cargo-tool" --no-default-features)
case "$PROFILE" in
release) args+=(--release) ;;
debug)   ;;
*)       args+=(--profile "$PROFILE") ;;
esac
[ $# -gt 0 ] && args+=(--features "$(IFS=,; echo "$*")")

cargo_args=()
built="$OUT_DIR/cargo-tool/$PROFILE"
if [ "${WPD_NIGHTLY_SIZE:-}" = 1 ]; then
    target="$(rustc -vV | awk '/^host: / { print $2 }')"
    export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options -Cpanic=immediate-abort -Zlocation-detail=none"
    cargo_args+=(-Zbuild-std=std,panic_abort -Zbuild-std-features=optimize_for_size)
    args+=(--target "$target")
    built="$OUT_DIR/cargo-tool/$target/$PROFILE"
fi

# ${var[@]+...} keeps macOS bash 3.2 from tripping set -u on an empty array.
cargo ${cargo_args[@]+"${cargo_args[@]}"} "${args[@]}"

cp -f "$built/wpd" "$OUT_DIR/wpd"

if [ "$PROFILE" = release ] || [ "$PROFILE" = minsize ]; then
    slim="$OUT_DIR/wpd.slim"
    if "${STRIP:-strip}" --strip-all -o "$slim" "$OUT_DIR/wpd" 2>/dev/null; then
        mv -f "$slim" "$OUT_DIR/wpd"
    else
        rm -f "$slim"
        echo "cargo_tool.sh: cannot strip wpd, shipping it whole" >&2
    fi
fi
