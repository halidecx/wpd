#!/bin/bash -eu

find . -type f \( -name "*.c" -o -name "*.h" -o -name "*.cc" -o -name "*.cxx" \
    -o -name "*.hpp" -o -name "*.hxx" \) -exec clang-format -i {} +

echo "done"
exit 0
