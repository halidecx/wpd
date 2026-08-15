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
# usage: cargo_build.sh SOURCE_ROOT OUT_DIR NAME PROFILE SOVERSION [FEATURE...]

SOURCE_ROOT="${1:?source root}"
OUT_DIR="${2:?out dir}"
NAME="${3:?artifact name}"
PROFILE="${4:?profile}"
SOVERSION="${5:?soversion}"
shift 5

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

# Only the installed configuration needs a shared library.
[ "$NAME" = wpd ] || exit 0

# A Rust cdylib exports only the Rust symbols it was told to, which is nothing
# while the entry points are still C. So the shared library is linked from the
# archive instead, with the export list taken from the public header. That
# keeps the ABI exactly what include/wpd.h promises, and keeps working
# unchanged once the entry points become #[no_mangle] Rust.
symbols=$(sed -n 's/.*WPD_API.*[ *]\([a-z_][a-z_0-9]*\)(.*/\1/p' \
              "$SOURCE_ROOT/include/wpd.h" | sort -u)
if [ -z "$symbols" ]; then
    echo "cargo_build.sh: no WPD_API symbols found in include/wpd.h" >&2
    exit 1
fi

soname="libwpd.so.${SOVERSION%%.*}"
version_script="$OUT_DIR/cargo-$NAME/libwpd.ver"
{
    echo "WPD_${SOVERSION%%.*} {"
    echo "  global:"
    for s in $symbols; do echo "    $s;"; done
    echo "  local: *;"
    echo "};"
} > "$version_script"

undefined=()
for s in $symbols; do undefined+=("-Wl,-u,$s"); done

${CC:-cc} -shared -o "$OUT_DIR/libwpd.so.$SOVERSION" \
    -Wl,-soname,"$soname" \
    -Wl,--version-script="$version_script" \
    "${undefined[@]}" \
    "$OUT_DIR/libwpd.a" \
    -lm -lpthread -ldl
ln -sf "libwpd.so.$SOVERSION" "$OUT_DIR/$soname"
ln -sf "$soname" "$OUT_DIR/libwpd.so"
