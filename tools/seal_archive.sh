#!/bin/bash -eu
# Merges a staged archive into one object holding nothing the public API cannot
# reach, and puts that back in an archive.
#
# usage: seal_archive.sh INPUT OUTPUT elf|macho SYMBOL...
#
# The symbols are the root set: the header's entry points, which is also what
# the export list names. Everything reachable from them stays and everything
# else goes.
#
# The two object formats get there by different routes, because their linkers
# disagree about what a partial link may do. GNU ld will dead-strip one, so the
# merge itself drops the unreachable sections. Apple's ld refuses -dead_strip
# together with -r outright, so there the merge keeps every section and demotes
# instead: an export list on a partial link makes every global that is not a
# root a private extern, which leaves the unreachable code with nothing outside
# the object naming it, and lets the -dead_strip on the eventual link to a
# binary be the thing that drops it. Same root set, same end state, one step
# later. Doing it with strip instead does not work -- a relocation entry still
# names those symbols, and nothing short of a real link resolves them away.

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
