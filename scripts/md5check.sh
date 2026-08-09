#!/bin/bash -eu

OLD="${1:?usage: md5check.sh OLD_BIN NEW_BIN}"
NEW="${2:?usage: md5check.sh OLD_BIN NEW_BIN}"

shopt -s nullglob
for input in wpd-test-data/*.webp; do
    case "$("$NEW" --info "$input" 2>/dev/null |
            awk '/^frame 0:/ { print $4; exit }')" in
        argb)      formats="argb" ;;
        yuva420p)  formats="yuva420p yuv420p" ;;
        yuv420p)   formats="yuv420p" ;;
        *)
            echo "$input: cannot determine pixel format" >&2
            exit 1
            ;;
    esac
    for fmt in $formats; do
        expected=$("$OLD" --muxer md5 -f "$fmt" "$input" -)
        if "$NEW" --verify "$expected" -f "$fmt" "$input"; then
            printf '%s\t%s\t%s\n' "$input" "$fmt" "$expected"
        else
            actual=$("$NEW" --muxer md5 -f "$fmt" "$input" -)
            printf 'mismatch: %s (%s)\nold: %s\nnew: %s\n' \
                "$input" "$fmt" "$expected" "$actual" >&2
            exit 1
        fi
    done
done
