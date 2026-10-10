# Attow Network codec and protocol fuzzing

This is a separate, excluded Cargo workspace with its own committed lockfile. The `codec` target exercises both canonical `Value` and `Object` decoding, including restrictive and zero limits. Every accepted input must equal its re-encoding and preserve its value after another decode. The harness caps input at 64 KiB; CI also bounds time, individual-input timeout and memory.

Use the pinned nightly on Linux x86_64 with a C++ compiler and sanitizer support:

```sh
rustup toolchain install nightly-2026-10-08 --profile minimal
cargo +nightly-2026-10-08 install cargo-fuzz --version =0.13.2 --locked
cargo +nightly-2026-10-08 fetch --manifest-path fuzz/Cargo.toml --locked
CARGO_NET_OFFLINE=true cargo +nightly-2026-10-08 fuzz run codec --sanitizer address -- -max_total_time=60 -max_len=65536 -timeout=5 -rss_limit_mb=1024
git diff --exit-code -- fuzz/Cargo.lock
```

Run from the repository root. This selects a side-by-side nightly without changing the global default. cargo-fuzz 0.13.2 does not expose a `--locked` build argument; prefetch with `--locked`, run offline, and reject lockfile changes afterward. The manual workflow performs those steps and fails on any error. It is opt-in and makes no external provider calls.

`corpus/codec` contains original tiny seed inputs, including intentionally invalid encodings. Future discoveries go through minimization and review before being retained; generated crash artifacts and coverage are ignored.

Stage 8 adds `frame`, which exercises fixed-header admission and canonical message
decoding under default and restrictive limits. Inputs are capped at 128 KiB;
accepted bytes must exactly re-encode. `worldline_import` feeds bounded recorded
objects into the real state root and frozen-branch import validators. It has no
worker, authority or network port and cannot execute downloaded code. Its input
is canonical `[domain, root-or-null, [[kind, schema, bytes], ...]]`; null selects
the first object by ID. The harness caps input at 256 KiB, 32 objects and 64 KiB
per object. Original seeds reside in the corresponding corpus directories.

Use the same pinned Linux toolchain and locked/offline preparation above, then:

```sh
CARGO_NET_OFFLINE=true cargo +nightly-2026-10-08 fuzz run frame --sanitizer address -- -max_total_time=60 -max_len=131072 -timeout=5 -rss_limit_mb=1024
CARGO_NET_OFFLINE=true cargo +nightly-2026-10-08 fuzz run worldline_import --sanitizer address -- -max_total_time=60 -max_len=262144 -timeout=5 -rss_limit_mb=1024
```

A normal `cargo check --manifest-path fuzz/Cargo.toml --locked` verifies compilation only. It does not execute libFuzzer or provide sanitizer evidence. The workflow is configuration until actually run on Linux; no Windows sanitizer result is implied. See [tool provenance](../docs/research/stage2-sources.md).
