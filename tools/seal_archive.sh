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
    linker_args=(-r)
    prefix="_"
    ;;
*)
    echo "seal_archive.sh: unsupported object format $FORMAT" >&2
    exit 1
    ;;
esac

object="$OUTPUT.o"
roots="$OUTPUT.roots"
trap 'rm -f "$object" "$roots" "$OUTPUT.slim"' EXIT

: > "$roots"
for symbol in "$@"; do
    linker_args+=(-Wl,-u,"$prefix$symbol")
    printf '%s%s\n' "$prefix" "$symbol" >> "$roots"
done

if [ "$FORMAT" = macho ]; then
    linker_args+=(-Wl,-exported_symbols_list,"$roots")
fi

"${CC:-cc}" "${linker_args[@]}" "$INPUT" -o "$object"
"${AR:-ar}" rcs "$OUTPUT" "$object"

. "$(dirname "$0")/strip_artifact.sh"
strip_artifact archive "$OUTPUT" || true
