#!/bin/bash -eu

for input in testdata/*.webp; do
    echo -ne "$input\t"
    case "$input" in
        *yuva*)              pixel_format=yuva420p ;;
        *yuv*|*lossy.webp)    pixel_format=yuv420p ;;
        *rgb*|*lossless.webp) pixel_format=argb ;;
        *)
            echo "$input: cannot determine pixel format from filename" >&2
            exit 1
            ;;
    esac

    video_size=$(webpmux -info "$input" | awk '/^Canvas size:/ { print $3 "x" $5; exit }')
    if [[ -z "$video_size" ]]; then
        echo "$input: cannot determine canvas size" >&2
        exit 1
    fi

    output=${input%.webp}.yuv
    ffmpeg_vt -loglevel warning -hide_banner -y -threads 1 \
        -i "$input" \
        -fps_mode passthrough -f rawvideo \
        -pix_fmt "$pixel_format" "$output"
    ffmpeg_vt -hide_banner -threads 1 \
        -i "$input" \
        -threads 1 -f rawvideo \
        -pixel_format "$pixel_format" -video_size "$video_size" -i "$output" \
        -lavfi "[0:v]settb=AVTB,setpts=N[webp];[1:v]settb=AVTB,setpts=N[raw];[webp][raw]ssim=shortest=1" \
        -f null - 2>&1 | grep "SSIM"
done
