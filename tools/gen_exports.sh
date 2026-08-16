#!/bin/bash -eu
# Prints the linker export list for the shared library, taken from the public
# header. A Rust cdylib exports only the Rust symbols it was told to, which is
# nothing while the entry points are still C, so the shared library is linked
# out of the staged archive instead and told explicitly what to export. That
# keeps the ABI exactly what include/wpd.h promises, and keeps working
# unchanged once the entry points become #[no_mangle] Rust.
#
# usage: gen_exports.sh HEADER elf|macho

HEADER="${1:?public header}"
FORMAT="${2:?elf or macho}"

# Anchored on the declaration, so that the #define lines that give WPD_API its
# __declspec and __attribute__ bodies do not read as entry points.
symbols=$(sed -n 's/^WPD_API .*[ *]\([a-z_][a-z_0-9]*\)(.*/\1/p' "$HEADER" | sort -u)
if [ -z "$symbols" ]; then
    echo "gen_exports.sh: no WPD_API symbols found in $HEADER" >&2
    exit 1
fi

case "$FORMAT" in
elf)
    major=$(sed -n 's/^#define WPD_VERSION_MAJOR \([0-9]*\).*/\1/p' "$HEADER")
    echo "WPD_${major:?no WPD_VERSION_MAJOR in $HEADER} {"
    echo "  global:"
    for s in $symbols; do echo "    $s;"; done
    echo "  local: *;"
    echo "};"
    ;;
macho)
    # Mach-O prefixes C symbols with an underscore.
    for s in $symbols; do echo "_$s"; done
    ;;
*)
    echo "gen_exports.sh: unknown format $FORMAT" >&2
    exit 1
    ;;
esac
