#!/bin/sh
set -eu
if [ "$#" -ne 3 ] || [ "$1" != '--dir' ]; then
    printf '%s\n' 'Usage: verify-artifacts.sh --dir DIRECTORY --integrity-only|--require-signatures' >&2
    exit 2
fi
case "$3" in
    --integrity-only|--require-signatures) mode=$3 ;;
    *) printf '%s\n' 'An explicit verification mode is required.' >&2; exit 2 ;;
esac
case "$2" in
    /*) candidate_dir=$2 ;;
    *) candidate_dir=$PWD/$2 ;;
esac
repository_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_dir"
exec cargo +1.95.0 run --locked -p xtask -- release-verify --dir "$candidate_dir" "$mode"
