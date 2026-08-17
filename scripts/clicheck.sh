#!/bin/bash -eu
# Differential check of everything the command-line tool prints: stdout, stderr,
# exit status and the bytes of any file it writes, for one binary against
# another.
#
# md5check.sh compares pixels, and pixels are only part of what the tool
# produces. The --info frame table, the ppm/pam/y4m container headers, the
# usage text and every argument error are invisible to a pixel comparison, so
# they get their own oracle here.
#
# usage: clicheck.sh OLD_BIN NEW_BIN

OLD="${1:?usage: clicheck.sh OLD_BIN NEW_BIN}"
NEW="${2:?usage: clicheck.sh OLD_BIN NEW_BIN}"

shopt -s nullglob

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

checked=0
failed=0

printf 'not a webp\n' > "$tmp/notwebp"

# Runs one argument vector and records everything observable about it. @OUT@ in
# an argument stands for a per-binary output path, so a case can ask the tool to
# write a file and still have the two runs land somewhere different. A suffix
# after it survives, which is how the extension-driven muxer choice gets tested.
capture() {
    local bin="$1" tag="$2"
    shift 2
    local args=() a

    for a in "$@"; do
        args+=("${a//@OUT@/$tmp/$tag.file}")
    done

    rm -f "$tmp/$tag.file"*
    set +e
    "$bin" "${args[@]}" > "$tmp/$tag.stdout" 2> "$tmp/$tag.stderr"
    printf 'exit %d\n' "$?" > "$tmp/$tag.status"
    set -e

    # The banner carries the revision and usage echoes argv[0], so both name the
    # binary under test. Fold them, or every single case would differ.
    sed -i -e "1s|^wpd by .*\$|wpd by BANNER|" \
           -e "s|$bin|BIN|g" -e "s|$tmp/$tag|OUT|g" "$tmp/$tag.stderr"
    sed -i -e "s|$tmp/$tag|OUT|g" "$tmp/$tag.stdout"
}

check() {
    local part diff_found="" produced

    checked=$((checked + 1))
    capture "$OLD" old "$@"
    capture "$NEW" new "$@"

    for part in status stdout stderr; do
        if ! diff -u "$tmp/old.$part" "$tmp/new.$part" > "$tmp/diff.$part"; then
            diff_found="$diff_found $part"
        fi
    done
    for produced in "$tmp"/old.file* "$tmp"/new.file*; do
        part="${produced##*/}"
        part="${part#???.file}"
        case " $diff_found " in
        *" output$part "*) continue ;;
        esac
        if ! cmp -s "$tmp/old.file$part" "$tmp/new.file$part"; then
            diff_found="$diff_found output$part"
        fi
    done

    if [ -n "$diff_found" ]; then
        failed=$((failed + 1))
        printf 'differs (%s): %s\n' "${diff_found# }" "$*" >&2
        for part in status stdout stderr; do
            [ -s "$tmp/diff.$part" ] && sed 's/^/  /' "$tmp/diff.$part" >&2
        done
    fi
    return 0
}

shopt -s nullglob
files=(wpd-test-data/*.webp)
[ ${#files[@]} -gt 0 ] || { echo 'no test data' >&2; exit 1; }

# Everything that never gets as far as opening an image: the usage text, and
# each way of being rejected by the argument parser.
check --help
check --help=x
check -h
check
check --info
check --info=x
check --subframe=x
check --fmt
check --fmt bogus
check -f bogus
check --fmt=rgba
check --muxer bogus
check --muxer
check --repeat
check --loops
check --stream
check --cpumask
check --verify
check -r
check "${files[0]}" -f
check --repeat 0
check --repeat -1
check --repeat abc
check --repeat 2147483648
check --loops 0
check --stream 0
check --verify deadbeef
check --verify 000102030405060708090a0b0c0d0e0f --muxer ppm "${files[0]}"
check --verify 000102030405060708090a0b0c0d0e0f "${files[0]}" out.raw
check --verify 000102030405060708090a0b0c0d0e0fz "${files[0]}"
check --unknown-option
check -x
check "${files[0]}"
check "${files[0]}" a b
check --info "$tmp/missing.webp"
check --info "$tmp/notwebp"
check --info /dev/null
check --info -- "${files[0]}"
check -hf argb
check --cpumask bogus
check --cpumask -1
check --inf "${files[0]}"
check --mux ppm "${files[0]}" @OUT@
check --verif 000102030405060708090a0b0c0d0e0f "${files[0]}"
check --rep 2 --info "${files[0]}"
check --loop 2 --info "${files[0]}"
check --cpum none --info "${files[0]}"
check --s "${files[0]}"

# The cpu mask parser takes names and strtoul's three bases.
for mask in none 0 1 0x10 010 4294967295; do
    check --cpumask "$mask" --info "${files[0]}"
done

# The frame table and the metadata report, for every file. This is the pass that
# sees struct field ordering: a swapped pair of ints in the frame-info binding
# leaves every decoded pixel identical and only shows up here.
for f in "${files[@]}"; do
    check --info "$f"
    check --info --subframe "$f"
    check --info --stream 997 "$f"
done

# Container headers and framing, which the md5 muxer never sees. The muxers each
# constrain the pixel format, so the cases that are meant to be rejected are
# listed alongside the ones that are meant to work.
for f in "${files[@]}"; do
    check --muxer ppm "$f" @OUT@
    check --muxer pam "$f" @OUT@
    check --muxer y4m "$f" @OUT@
    check --muxer y4m -f yuv420p "$f" @OUT@
    check --muxer y4m -f yuva420p "$f" @OUT@
    check --muxer y4m -f rgba "$f" @OUT@
    check --muxer ppm -f rgba "$f" @OUT@
    check --muxer pam -f rgb "$f" @OUT@
done

# The muxer also comes from the output extension, and every packed format has to
# survive a raw round trip.
for f in "${files[0]}" "${files[1]}"; do
    for ext in ppm pam y4m raw; do
        check "$f" "@OUT@.$ext"
    done
    for fmt in auto yuv420p yuva420p argb rgba bgra rgb bgr Argb rgbA bgrA \
               rgb565 rgba4444 rgbA4444 bgr565 bgra4444 bgrA4444; do
        check -f "$fmt" "$f" @OUT@
        check --muxer md5 -f "$fmt" "$f" -
    done
done

# Repeats, replays and streaming reach the same pixels by different paths, and
# each has its own report on stdout.
for f in "${files[@]}"; do
    check --muxer md5 --repeat 2 "$f" -
    check --muxer md5 --loops 3 "$f" -
    check --muxer md5 --stream 64 "$f" -
    check --muxer md5 --stream 1 --loops 2 "$f" -
    check --muxer md5 --subframe "$f" -
done

# Writing to stdout, to a discarded sink, and reading the image from stdin.
check --muxer md5 "${files[0]}" /dev/null
check --info "${files[0]}" /dev/null

printf 'checked %d invocations, %d differ\n' "$checked" "$failed"
[ "$failed" -eq 0 ]
