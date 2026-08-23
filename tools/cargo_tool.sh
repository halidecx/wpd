#!/bin/bash -eu

SOURCE_ROOT="${1:?source root}"
OUT_DIR="${2:?out dir}"
PROFILE="${3:?profile}"
shift 3

sanitizer_runtime() {
    local short="$1" cc="${CC:-cc}" cand resolved

    case "$(uname -s)" in
    Darwin) set -- "libclang_rt.${short}_osx_dynamic.dylib" ;;
    *)      set -- "lib${short}.so" "lib${short}.a" \
                   "libclang_rt.${short}-$(uname -m).a" ;;
    esac

    for cand in "$@"; do
        resolved="$("$cc" -print-file-name="$cand" 2>/dev/null || true)"
        if [ -n "$resolved" ] && [ "$resolved" != "$cand" ] && [ -e "$resolved" ]; then
            printf '%s\n' "$resolved"
            return 0
        fi
    done
    return 1
}

sanitize_target=""
if [ -n "${WPD_SANITIZE:-}" ]; then
    runtimes=""
    rpaths=""
    for s in ${WPD_SANITIZE//,/ }; do
        case "$s" in
        address)   short=asan ;;
        undefined) short=ubsan ;;
        thread)    short=tsan ;;
        leak)      short=lsan ;;
        memory)    short=msan ;;
        *)         continue ;;
        esac
        if ! path="$(sanitizer_runtime "$short")"; then
            echo "cargo_tool.sh: no $short runtime for this toolchain, skipping" >&2
            continue
        fi
        runtimes="$runtimes -Clink-arg=$path"
        case "$path" in
        *.dylib)
            case " $rpaths " in
            *" -Clink-arg=-Wl,-rpath,${path%/*} "*) ;;
            *) rpaths="$rpaths -Clink-arg=-Wl,-rpath,${path%/*}" ;;
            esac
            ;;
        esac
    done
    export RUSTFLAGS="${RUSTFLAGS:-} -Clink-arg=-fsanitize=$WPD_SANITIZE$runtimes$rpaths"
    sanitize_target="$(rustc -vV | awk '/^host: / { print $2 }')"
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
target="${WPD_CARGO_TARGET:-$sanitize_target}"
if [ "${WPD_NIGHTLY_SIZE:-}" = 1 ]; then
    [ -n "$target" ] || target="$(rustc -vV | awk '/^host: / { print $2 }')"
    # Use panic=abort: immediate-abort traps can loop indefinitely on macOS.
    export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options -Cpanic=abort -Zlocation-detail=none"
    cargo_args+=(-Zbuild-std=std,panic_abort -Zbuild-std-features=optimize_for_size)
fi
if [ -n "$target" ]; then
    args+=(--target "$target")
    built="$OUT_DIR/cargo-tool/$target/$PROFILE"
fi

cargo ${cargo_args[@]+"${cargo_args[@]}"} "${args[@]}"

cp -f "$built/wpd" "$OUT_DIR/wpd"

if [ "$PROFILE" = release ] || [ "$PROFILE" = minsize ]; then
    . "$(dirname "$0")/strip_artifact.sh"
    strip_artifact binary "$OUT_DIR/wpd" ||
        echo "cargo_tool.sh: cannot strip wpd, shipping it whole" >&2
fi
