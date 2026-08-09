#!/bin/bash -eu
# Verify wpd against libwebp on animations: bit-exact ARGB compositing via
# libwebpdec, and container metadata via webpmux.
#
# libwebp's WebPAnimDecoder only emits RGBA, so only animations wpd composites
# in ARGB are directly comparable; the rest are metadata-checked only.

WPD="${1:-./build/wpd}"
LWP="${2:-./build/libwebpdec}"
CORPUS="${3:-wpd-test-data}"

for tool in "$WPD" "$LWP"; do
    if [[ ! -x $tool ]]; then
        echo "missing $tool; build it with 'meson compile -C build libwebpdec'" >&2
        exit 1
    fi
done
command -v webpmux >/dev/null || { echo "webpmux is required" >&2; exit 1; }

shopt -s nullglob
inputs=("$CORPUS"/*.webp)
if (( ${#inputs[@]} == 0 )); then
    echo "no WebP files found in $CORPUS" >&2
    exit 1
fi

failed=0
checked=0

fail() {
    printf 'FAIL %-22s %s\n' "$1" "$2"
    failed=1
}

for input in "${inputs[@]}"; do
    name=$(basename "$input" .webp)
    if ! wpd_info=$("$WPD" --info "$input" 2>/dev/null); then
        echo "$input: $WPD --info failed; is it too old to support it?" >&2
        exit 1
    fi

    get() { sed -n "s/^$1: //p" <<<"$wpd_info"; }

    [[ $(get animation) == 1 ]] || continue
    checked=$((checked + 1))
    problems=()

    if [[ $(awk '/^frame 0:/ { print $4; exit }' <<<"$wpd_info") == argb ]]; then
        "$WPD" -f argb "$input" "$CORPUS/$name.wpd.raw" 2>/dev/null
        "$LWP" -f argb "$input" "$CORPUS/$name.lwp.raw" 2>/dev/null
        if ! cmp -s "$CORPUS/$name.wpd.raw" "$CORPUS/$name.lwp.raw"; then
            problems+=("$(cmp -l "$CORPUS/$name.wpd.raw" \
                                 "$CORPUS/$name.lwp.raw" | wc -l) bytes differ")
        fi
        rm -f "$CORPUS/$name.wpd.raw" "$CORPUS/$name.lwp.raw"
    fi

    mux_info=$(webpmux -info "$input")

    want_canvas=$(awk '/^Canvas size:/ { print $3 "x" $5; exit }' <<<"$mux_info")
    want_frames=$(awk '/^Number of frames:/ { print $4; exit }' <<<"$mux_info")
    want_loops=$(awk '/Loop Count/ { print $NF; exit }' <<<"$mux_info")
    want_bg=$(awk '/Background color/ { print tolower($4); exit }' <<<"$mux_info")

    [[ $(get canvas) == "$want_canvas" ]] ||
        problems+=("canvas $(get canvas) != $want_canvas")
    [[ $(get frames) == "$want_frames" ]] ||
        problems+=("frames $(get frames) != $want_frames")
    [[ -z $want_loops || $(get loops) == "$want_loops" ]] ||
        problems+=("loops $(get loops) != $want_loops")
    [[ -z $want_bg || $(get background) == "$want_bg" ]] ||
        problems+=("background $(get background) != $want_bg")

    want_durations=$(awk '/^ *[0-9]+:/ { print $7 }' <<<"$mux_info" | tr '\n' ' ')
    got_durations=$(sed -n 's/.* duration \([0-9]*\) .*/\1/p' <<<"$wpd_info" |
                    tr '\n' ' ')
    [[ $want_durations == "$got_durations" ]] ||
        problems+=("durations [$got_durations] != [$want_durations]")

    if (( ${#problems[@]} == 0 )); then
        printf 'ok   %s\n' "$name"
    else
        for p in "${problems[@]}"; do fail "$name" "$p"; done
    fi
done

if (( checked == 0 )); then
    echo "no animations found in $CORPUS" >&2
    exit 1
fi
printf '%d animations checked\n' "$checked"
exit $failed
