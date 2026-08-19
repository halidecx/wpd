#!/bin/bash -eu

INPUT="${1:?input archive}"
OUTPUT="${2:?output archive}"
FORMAT="${3:?format}"
shift 3

case "$FORMAT" in
elf)
    linker_args=(-r -Wl,--gc-sections)
    prefix=""
    ;;
macho)
    linker_args=(-r -Wl,-dead_strip)
    prefix="_"
    ;;
*)
    echo "seal_archive.sh: unsupported object format $FORMAT" >&2
    exit 1
    ;;
esac

for symbol in "$@"; do
    linker_args+=(-Wl,-u,"$prefix$symbol")
done

object="$OUTPUT.o"
slim="$OUTPUT.slim"
trap 'rm -f "$object" "$slim"' EXIT

"${CC:-cc}" "${linker_args[@]}" "$INPUT" -o "$object"
"${AR:-ar}" rcs "$OUTPUT" "$object"

if "${STRIP:-strip}" --strip-unneeded -o "$slim" "$OUTPUT" 2>/dev/null; then
    mv -f "$slim" "$OUTPUT"
fi
