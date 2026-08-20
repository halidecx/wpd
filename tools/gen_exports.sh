#!/bin/bash -eu

HEADER="${1:?public header}"
FORMAT="${2:?elf, macho or names}"

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
    for s in $symbols; do echo "_$s"; done
    ;;
names)
    for s in $symbols; do echo "$s"; done
    ;;
*)
    echo "gen_exports.sh: unknown format $FORMAT" >&2
    exit 1
    ;;
esac
