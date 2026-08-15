#!/bin/bash -eu
# Formats the tree and runs the lints that are cheap enough to be a habit.
#
# clippy is here rather than in `meson test` because it needs a debug build of
# every target, which the test suite has no other reason to produce. A warning
# is an error: the list was empty when this gate went in, and the only way it
# stays useful is if it stays empty.

find . -path ./subprojects -prune -o -path ./target -prune -o -type f \
    \( -name "*.c" -o -name "*.h" -o -name "*.cc" -o -name "*.cxx" \
    -o -name "*.hpp" -o -name "*.hxx" \) -exec clang-format -i {} +

cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "done"
