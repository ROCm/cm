#!/bin/bash

# A hacky script to scrape some of the "known values" from llvm/CMakeLists.txt
#
# Ideally some stable interface to LLVM's CMake will replace this eventually.

set -u

if (( $# != 1 )); then
    printf >&2 'usage: scrape.sh PATH-TO-LLVM-PROJECT\n'
    exit 1
fi

PATH_TO_LLVM_PROJECT="$1"
readonly PATH_TO_LLVM_PROJECT
shift

ROOT="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
readonly ROOT

GAWK_SRC=$(cat <<EOF
/set\(LLVM_ALL_PROJECTS/ {
    sub(/^/, "[", \$2)
    sub(/)$/, "]", \$2)
    gsub(/;/, "\", \"", \$2)
    printf "%s\n", \$2 >"$ROOT/llvm_all_projects.in"
}

/set\(LLVM_ALL_TARGETS/ {
    TARGETS = 1
    printf "[" > "$ROOT/llvm_all_targets.in"
    next
}
TARGETS == 1 && /)/ {
    TARGETS = 0
    printf "]\n" >> "$ROOT/llvm_all_targets.in"
    next
}
TARGETS == 1 {
    printf "\"%s\", ", \$1 >> "$ROOT/llvm_all_targets.in"
}
EOF
)
readonly GAWK_SRC

gawk "$GAWK_SRC" "$PATH_TO_LLVM_PROJECT/llvm/CMakeLists.txt"

sed 's/^\[/["Native", /' <"$ROOT/llvm_all_targets.in" >"$ROOT/llvm_all_targets_alt.in"
