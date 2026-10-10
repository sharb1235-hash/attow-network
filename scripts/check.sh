#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$repository_root"

run_cargo() {
    printf '> cargo +1.95.0'
    printf ' %s' "$@"
    printf '\n'
    cargo +1.95.0 "$@"
}

run_cargo run --package xtask --locked -- verify-repository
run_cargo fmt --all --check
run_cargo test --workspace --all-targets --locked
run_cargo clippy --workspace --all-targets --locked -- -D warnings
run_cargo doc --workspace --no-deps --locked
