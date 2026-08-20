#!/bin/bash -eu

OLD="${1:?usage: clicheck.sh OLD_BIN NEW_BIN}"
NEW="${2:?usage: clicheck.sh OLD_BIN NEW_BIN}"

shopt -s nullglob

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

checked=0
failed=0

printf 'not a webp\n' > "$tmp/notwebp"

fold() {
    local bin="$1" tag="$2" file="$3"

    LC_ALL=C sed -i.bak -e "1s|^wpd by .*\$|wpd by BANNER|" \
                        -e "s|$bin|BIN|g" -e "s|$tmp/$tag|OUT|g" "$file"
    rm -f "$file.bak"
}

capture() {
    local bin="$1" tag="$2"
    shift 2
    local args=() a

    for a in "$@"; do
        args+=("${a//@OUT@/$tmp/$tag.file}")
    done

    rm -f "$tmp/$tag.file"*
    set +e
    "$bin" ${args[@]+"${args[@]}"} > "$tmp/$tag.stdout" 2> "$tmp/$tag.stderr"
    printf 'exit %d\n' "$?" > "$tmp/$tag.status"
    set -e

    fold "$bin" "$tag" "$tmp/$tag.stderr"
    LC_ALL=C sed -i.bak -e "s|$tmp/$tag|OUT|g" "$tmp/$tag.stdout"
    rm -f "$tmp/$tag.stdout.bak"
}

capture_pipe() {
    local bin="$1" tag="$2"
    shift 2

    rm -f "$tmp/$tag.file"*
    : > "$tmp/$tag.stdout"
    set +e
    "$bin" "$@" 2> "$tmp/$tag.stderr" | head -c 1 > /dev/null
    printf 'exit %d\n' "${PIPESTATUS[0]}" > "$tmp/$tag.status"
    set -e

    fold "$bin" "$tag" "$tmp/$tag.stderr"
}

compare() {
    local part diff_found="" produced

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
            [ -s "$tmp/diff.$part" ] || continue
            head -c 4096 "$tmp/diff.$part" | head -n 20 | cat -v |
                sed 's/^/  /' >&2
        done
    fi
    return 0
}

check() {
    checked=$((checked + 1))
    capture "$OLD" old "$@"
    capture "$NEW" new "$@"
    compare "$@"
}

check_pipe() {
    checked=$((checked + 1))
    capture_pipe "$OLD" old "$@"
    capture_pipe "$NEW" new "$@"
    compare "(closed pipe)" "$@"
}

shopt -s nullglob
files=(wpd-test-data/*.webp)
[ ${#files[@]} -gt 0 ] || { echo 'no test data' >&2; exit 1; }

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

for mask in none 0 1 0x10 010 4294967295; do
    check --cpumask "$mask" --info "${files[0]}"
done

for f in "${files[@]}"; do
    check --info "$f"
    check --info --subframe "$f"
    check --info --stream 997 "$f"
done

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

for f in "${files[@]}"; do
    check --muxer md5 --repeat 2 "$f" -
    check --muxer md5 --loops 3 "$f" -
    check --muxer md5 --stream 64 "$f" -
    check --muxer md5 --stream 1 --loops 2 "$f" -
    check --muxer md5 --subframe "$f" -
done

for f in "${files[@]}"; do
    check --muxer y4m --subframe "$f" @OUT@
    check --muxer y4m --subframe -f argb "$f" @OUT@
    check --subframe -f yuv420p "$f" @OUT@
    check --subframe -f yuva420p "$f" @OUT@
done

for f in "${files[@]}"; do
    check --info --muxer y4m "$f" -
    check --info --muxer ppm "$f" -
    check --info "$f" -
done

for f in wpd-test-data/anim_yuva.webp wpd-test-data/anim_rgb.webp \
         wpd-test-data/lossy.webp; do
    [ -e "$f" ] || continue
    check_pipe --muxer y4m -f yuv420p "$f" -
    check_pipe -f argb "$f" -
    check_pipe --info --muxer y4m -f yuv420p "$f" -
    check_pipe --muxer md5 "$f" -
done

check --muxer md5 "${files[0]}" /dev/null
check --info "${files[0]}" /dev/null

printf 'checked %d invocations, %d differ\n' "$checked" "$failed"
[ "$failed" -eq 0 ]
