#!/bin/bash -eu

case "$(uname -m)" in
    x86_64|amd64|i?86) masks=(none sse2 ssse3 sse41 avx2) ;;
    aarch64|arm64)     masks=(none neon) ;;
    arm*)              masks=(none armv6 neon) ;;
    *)
        echo "unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

meson configure build -Dtrim_dsp=false -Dtestdata_tests=true

for mask in "${masks[@]}"; do
    meson test -C build --suite testdata --test-args "--cpumask $mask"
done
