#!/bin/bash
# Strips one build artifact in place, for the three scripts that ship one.
#
# usage: . strip_artifact.sh   then   strip_artifact binary|archive FILE
#
# GNU strip and the one Apple ships share no spelling for this: --strip-all and
# --strip-unneeded do not exist on the BSD tool, whose nearest equivalents are
# -S -x for a linked binary and -S alone for an archive, whose symbols still
# have to resolve afterwards. Which one is in front of us is not a question
# `uname` answers either, since $STRIP is whatever the build was pointed at and
# a cross build may well be holding a GNU one on a Mac. So each form is tried
# in turn and the first that the tool accepts wins.
#
# Returns non-zero having changed nothing when none of them do, which is the
# caller's cue to ship the artifact whole rather than to fail the build: a
# binary with its symbols still in it works exactly as well.

strip_artifact() {
    local kind="$1" file="$2"
    local slim="$file.slim" strip="${STRIP:-strip}" form
    local forms=()

    case "$kind" in
    binary)
        forms=("--strip-all" "-S -x")
        ;;
    archive)
        # The two LLVM sections are where a fat-LTO archive keeps its bitcode,
        # which nothing downstream reads and which is most of its size. They
        # are ELF section names; the Mach-O build has no equivalent to name, so
        # the second and third forms drop the request rather than the tool.
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
        # Deliberately unquoted: each form is an argument list, not one word.
        # shellcheck disable=SC2086
        if "$strip" $form -o "$slim" "$file" 2>/dev/null; then
            mv -f "$slim" "$file"
            return 0
        fi
        rm -f "$slim"
    done
    return 1
}
