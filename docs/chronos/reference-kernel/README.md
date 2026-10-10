# ChronosOS reference kernel

An executable Rust reference model for isolated logical branches, dependency validation, atomic local promotion and pending effect intents. **It performs no external booking or other network action.** ChronosOS is a working title with an existing adjacent name collision; see the [research appendix](../06-prior-art-and-research.md).

The package is `chronosos-reference` version 0.1.0, imported as `chronosos_reference`, with `publish = false`. It is a semantics demonstration, not the proposed full runtime or a production sandbox.

## Run

Use Rust 1.95 or later, Cargo and a compatible native C toolchain for bundled SQLite. From this directory:

```sh
cargo fmt --all --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo run --locked --example parallel_checkout
```

All four commands passed on Windows with Rust/Cargo 1.95.0. The test run passed **13 integration tests**, with zero failures. The example reported:

```text
branches=5 promoted=1 conflicts=4 pending_effects=1 external_calls=0
Local intent chosen. No external seat has been booked.
```

The example uses five threads sharing one authority through a mutex. A separate integration test uses **two independently opened SQLite connections** and ten shared effect slots. It verifies one winner per slot, explicit conflict results, absence of loser writes, durable effect counts and a valid commit chain after reopening. These checks are functional validation, not throughput measurements. The [validation record](../validation/reference-results.json) lists the actual checks and scope.

## Implemented semantics

- `Snapshot::fork` shares an immutable managed-state snapshot and gives each branch private writes. This does not clone a live process, browser or remote service.
- Reads and writes record expected key versions. Tombstones retain versions for missing-value/ABA detection. Prefix scans conservatively depend on the whole promotion epoch.
- `Authority::promote` uses an immediate SQLite transaction to validate dependencies, apply writes, append a deterministic commit, and reserve a unique logical effect slot together. Conflicts roll back the transaction.
- Each database has a persistent 32-byte authority identity. A branch from an unrelated database is rejected even when versions coincide; independently opened connections to the same database and reopening preserve identity. Older reference metadata gains this identity transactionally.
- `mark_dispatching`, `recover_ambiguous` and `confirm` provide durable `PENDING`, `DISPATCHING`, `UNKNOWN` and `CONFIRMED` bookkeeping. An unknown operation retains its slot.
- `verify_chain` checks commit hashes, sequence, parent links and the recorded head within one read transaction, so another connection's commit cannot split its view.

The 13 tests cover branch isolation/disjoint writes, write skew, missing reads/tombstones, scan phantoms, five-thread selection, conflict rollback, ambiguous outcomes across restart, reproducible commit hashes, cross-authority rejection, reopening, metadata migration, independent-connection contention and concurrent chain verification.

## Boundaries

The model assumes trusted callers, trusted local storage and modeled writes through `Authority`. Authority identity is provenance metadata, not authentication, a signature or a distributed membership protocol. Copying the database also copies its identity; creating an independent writable authority from a copied database requires a future explicit reidentity/import protocol. No live multi-master file-copy workflow is supported.

There is no dispatcher, provider adapter, sandbox, credential broker, peer transport, consensus group, scheduler, CLI or full continuation/replay engine. `confirm` stores caller-supplied bytes without authenticating a provider receipt. A local pending intent does not establish an external effect, and no exactly-once provider guarantee is demonstrated.

Effect-state transitions and receipts are stored in the `effects` table, **not appended to the commit history**. The commit chain therefore cannot reconstruct the full effect lifecycle. `verify_chain` does not reconstruct and compare the materialized `kv`/`effects` tables, validate every encoded payload's semantics, prove provider truth or detect a privileged attacker rewriting the entire database. Deterministic encoding and equal-hash tests are narrower than full replay verification.

Promotion has **no request-ID/result deduplication API**. Resubmitting an already committed effect proposal conflicts instead of returning its original promotion result. The model supports one staged intent per branch, retains occupied slots indefinitely, and lacks definitive rejection, slot-generation reopening, cancellation and compensation workflows. The complete effect protocol belongs in the full runtime.

Tests close and reopen databases and exercise concurrent connections; they do not establish power-loss durability, arbitrary crash-schedule correctness, Byzantine safety or distributed availability. SQLite durability still depends on its storage configuration and the host's guarantees. No latency, capacity, memory-use or production-readiness claim is made.

## License

Copyright (c) 2026 ChronosOS contributors.

This reference package is available under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option, matching `MIT OR Apache-2.0` in `Cargo.toml`. Dependencies retain their own licenses and notices. The broader proposed runtime's release and licensing decisions remain separate. A software license does not establish trademark or third-party patent clearance.
