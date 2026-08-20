#!/bin/bash

strip_artifact() {
    local kind="$1" file="$2"
    local slim="$file.slim" strip="${STRIP:-strip}" form
    local forms=()

    case "$kind" in
    binary)
        forms=("--strip-all" "-S -x")
        ;;
    archive)
        forms=("--strip-unneeded --remove-section=.llvmbc --remove-section=.llvmcmd"
               "--strip-unneeded"
               "-S")
        ;;
    *)
        echo "strip_artifact: unknown artifact kind $kind" >&2
        return 1
        ;;
    esac

    for form in "${forms[@]}"; do
        # Deliberately unquoted: each form is an argument list.
        if "$strip" $form -o "$slim" "$file" 2>/dev/null; then
            mv -f "$slim" "$file"
            return 0
        fi
        rm -f "$slim"
    done
    return 1
}
