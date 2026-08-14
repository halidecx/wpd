#!/bin/bash -eu

find . -path ./subprojects -prune -o -path ./target -prune -o -type f \
    \( -name "*.c" -o -name "*.h" -o -name "*.cc" -o -name "*.cxx" \
    -o -name "*.hpp" -o -name "*.hxx" \) -exec clang-format -i {} +

cargo fmt --all

echo "done"
