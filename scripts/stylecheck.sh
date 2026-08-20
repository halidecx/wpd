#!/bin/bash -eu

find . -path ./subprojects -prune -o -path ./target -prune -o -type f \
    \( -name "*.c" -o -name "*.h" -o -name "*.cc" -o -name "*.cxx" \
    -o -name "*.hpp" -o -name "*.hxx" \) -exec clang-format -i {} +

cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings

IWP=tools/imagewebpdec/Cargo.toml
cargo fmt --manifest-path "$IWP" --all
cargo clippy --manifest-path "$IWP" --all-targets -- -D warnings

echo "done"
