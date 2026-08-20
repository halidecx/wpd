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

# The path to one sanitizer's runtime, or nothing if this toolchain has no such
# library. The compiler knows where its own runtimes live and nothing else
# reliably does, so every candidate name goes through -print-file-name, which
# echoes the name back unchanged when it cannot resolve it.
#
# GCC calls them libasan and friends. clang calls them libclang_rt.<name>, with
# a platform suffix that differs between Darwin and everywhere else, and keeps
# them in its resource directory rather than anywhere the linker looks by
# default -- which is why the answer here is an absolute path rather than a -l.
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

# rustc links the tool with -nodefaultlibs, which suppresses the runtime the
# compiler driver would otherwise add for -fsanitize, so each runtime has to be
# named explicitly. This is true on Darwin as much as on Linux; only the names
# and the search path differ.
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
        # Darwin has no standalone leak sanitizer: it is part of asan there,
        # and asking for one by name finds nothing. Say so rather than fail
        # the link with a name the linker cannot place.
        if ! path="$(sanitizer_runtime "$short")"; then
            echo "cargo_tool.sh: no $short runtime for this toolchain, skipping" >&2
            continue
        fi
        runtimes="$runtimes -Clink-arg=$path"
        # A dylib whose install name is @rpath/... needs one, and the
        # runtimes all share a directory, so the same one would be added
        # once per sanitizer and the linker would say so.
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
    # RUSTFLAGS reaches build scripts and proc macros too unless a --target is
    # named, and a build script linked against the sanitizer runtime is a host
    # program nobody asked to instrument. Naming the host triple explicitly is
    # what splits the two.
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
target="$sanitize_target"
if [ "${WPD_NIGHTLY_SIZE:-}" = 1 ]; then
    target="$(rustc -vV | awk '/^host: / { print $2 }')"
    # -Cpanic=abort and not immediate-abort, which is smaller and does not
    # reliably kill anything. immediate-abort lowers a panic to a bare trap
    # instruction rather than a call to abort(), and a trap is not a signal:
    # on macOS it raises a Mach exception, which on macOS 26 is caught and
    # resumed without advancing past it, so the process spins on the trap
    # forever at no diagnostic and cannot be killed by anything it does to
    # itself. A hung decoder is worse than a dead one -- it is a denial of
    # service where the C had a crash -- and README promises this build aborts.
    # libc's abort() raises SIGABRT, which is a signal everywhere.
    #
    # It costs the panic formatting machinery back: 11% of the tool and 13% of
    # the sealed archive. -Zlocation-detail=none still drops the file and line.
    export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options -Cpanic=abort -Zlocation-detail=none"
    cargo_args+=(-Zbuild-std=std,panic_abort -Zbuild-std-features=optimize_for_size)
fi
if [ -n "$target" ]; then
    args+=(--target "$target")
    built="$OUT_DIR/cargo-tool/$target/$PROFILE"
fi

# ${var[@]+...} keeps macOS bash 3.2 from tripping set -u on an empty array.
cargo ${cargo_args[@]+"${cargo_args[@]}"} "${args[@]}"

cp -f "$built/wpd" "$OUT_DIR/wpd"

if [ "$PROFILE" = release ] || [ "$PROFILE" = minsize ]; then
    . "$(dirname "$0")/strip_artifact.sh"
    strip_artifact binary "$OUT_DIR/wpd" ||
        echo "cargo_tool.sh: cannot strip wpd, shipping it whole" >&2
fi
