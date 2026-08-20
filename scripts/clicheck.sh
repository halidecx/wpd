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

# BSD sed reads the argument after -i as the backup suffix and GNU sed does
# not, so in-place editing has to name one explicitly to mean the same thing
# under both. A C locale keeps BSD sed from refusing a line the tool wrote that
# is not valid UTF-8, which a damaged file's error text can be.
fold() {
    local bin="$1" tag="$2" file="$3"

    LC_ALL=C sed -i.bak -e "1s|^wpd by .*\$|wpd by BANNER|" \
                        -e "s|$bin|BIN|g" -e "s|$tmp/$tag|OUT|g" "$file"
    rm -f "$file.bak"
}

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
    "$bin" ${args[@]+"${args[@]}"} > "$tmp/$tag.stdout" 2> "$tmp/$tag.stderr"
    printf 'exit %d\n' "$?" > "$tmp/$tag.status"
    set -e

    # The banner carries the revision and usage echoes argv[0], so both name the
    # binary under test. Fold them, or every single case would differ.
    fold "$bin" "$tag" "$tmp/$tag.stderr"
    LC_ALL=C sed -i.bak -e "s|$tmp/$tag|OUT|g" "$tmp/$tag.stdout"
    rm -f "$tmp/$tag.stdout.bak"
}

# Runs one argument vector with a reader that goes away after a byte. A closed
# pipe has to kill the tool the way it killed the C, which is only true if the
# tool put SIGPIPE back: Rust's runtime ignores it before main, so without that
# the write turns into an error, or a panic out of println!, and the exit status
# stops being 141. Only the status and stderr are observable -- the pixels went
# to a reader that stopped caring.
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
        # Capped, and with the unprintables escaped: a muxer writing to stdout
        # puts a frame's worth of pixels in the diff, and the first few lines
        # say which case broke as well as the whole thing would.
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

# A rejection the muxer only reaches once it has a frame in hand, which is what
# separates it from the --fmt disagreements above: those are settled before the
# decode starts. --subframe hands y4m frames of differing size, and hands the
# raw muxer a format it cannot convert, so the message and the exit status here
# cover the sink's own error path rather than the argument parser's.
for f in "${files[@]}"; do
    check --muxer y4m --subframe "$f" @OUT@
    check --muxer y4m --subframe -f argb "$f" @OUT@
    check --subframe -f yuv420p "$f" @OUT@
    check --subframe -f yuva420p "$f" @OUT@
done

# --info writes through printf while the frames go through the sink, so the two
# share stdout when the output is "-". Anything that buffers the sink separately
# reorders the report against the pixels it describes, which is invisible unless
# both land in the same capture.
for f in "${files[@]}"; do
    check --info --muxer y4m "$f" -
    check --info --muxer ppm "$f" -
    check --info "$f" -
done

# A reader that stops after one byte. The output has to outrun the pipe buffer
# for the writer to reach a closed pipe at all, so these are the files with the
# most pixels rather than files[0]; a small one would exit 0 and prove nothing.
# The md5 case is the other side of it: its output fits, so nothing is ever
# written to a closed pipe and the tool still exits 0.
for f in wpd-test-data/anim_yuva.webp wpd-test-data/anim_rgb.webp \
         wpd-test-data/lossy.webp; do
    [ -e "$f" ] || continue
    check_pipe --muxer y4m -f yuv420p "$f" -
    check_pipe -f argb "$f" -
    check_pipe --info --muxer y4m -f yuv420p "$f" -
    check_pipe --muxer md5 "$f" -
done

# Writing to stdout, to a discarded sink, and reading the image from stdin.
check --muxer md5 "${files[0]}" /dev/null
check --info "${files[0]}" /dev/null

printf 'checked %d invocations, %d differ\n' "$checked" "$failed"
[ "$failed" -eq 0 ]
