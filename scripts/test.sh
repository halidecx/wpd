#!/bin/bash -eu

shopt -s nullglob
inputs=(wpd-test-data/*.webp)
if (( ${#inputs[@]} == 0 )); then
    echo "no WebP files found in wpd-test-data" >&2
    exit 1
fi

for input in "${inputs[@]}"; do
    echo -ne "$input\t"
    pixel_format=$(./build/wpd --info "$input" 2>/dev/null |
                   awk '/^frame 0:/ { print $4; exit }')
    if [[ -z "$pixel_format" ]]; then
        echo "$input: cannot determine pixel format" >&2
        exit 1
    fi

    video_size=$(webpmux -info "$input" | awk '/^Canvas size:/ { print $3 "x" $5; exit }')
    if [[ -z "$video_size" ]]; then
        echo "$input: cannot determine canvas size" >&2
        exit 1
    fi

    output=${input%.webp}.yuv
    ./build/wpd -f "$pixel_format" "$input" "$output"
    ffmpeg_vt -hide_banner -threads 1 \
        -i "$input" \
        -threads 1 -f rawvideo \
        -pixel_format "$pixel_format" -video_size "$video_size" -i "$output" \
        -lavfi "[0:v]settb=AVTB,setpts=N[webp];[1:v]settb=AVTB,setpts=N[raw];[webp][raw]ssim=shortest=1" \
        -f null - 2>&1 | grep "SSIM"
done

outputs=(wpd-test-data/*.yuv)
for output in "${outputs[@]}"; do
    rm "$output"
done
