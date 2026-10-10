# Stage 2 source and tool provenance

Reviewed 10 October 2026 UTC (9 October 2026 Pacific). Stage 2 builds on established content-addressed storage, deterministic encoding and transaction techniques. The schema, fixture values and harness here are original project work under MIT OR Apache-2.0; reading a standard does not imply copying its reference code or fixtures.

## Encoding and hashing

- [RFC 8949, section 4.2](https://www.rfc-editor.org/rfc/rfc8949.html#section-4.2) defines deterministic CBOR considerations. Attow's versioned application profile separately fixes schemas, field counts, supported value types, ordering and resource limits. Refer to the project codec specification for its exact ordering; a generic CBOR decoder alone is insufficient to establish this contract.
- The [BLAKE3 specification](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf) is prior art for the hash primitive. The project uses a separate, documented kind/schema/length identity envelope; a content hash is neither encryption nor authority.
- [BLAKE3 1.8.7 registry metadata](https://crates.io/api/v1/crates/blake3/1.8.7) and [minicbor 2.3.0 registry metadata](https://crates.io/api/v1/crates/minicbor/2.3.0) identify the exact selected direct dependencies. Their inclusion and transitives are determined by the committed lockfiles.

## SQLite persistence

- [SQLite isolation](https://sqlite.org/isolation.html) explains the single-writer transaction boundary. Root pins and referenced objects must participate in the same protected transaction when required for atomicity.
- [SQLite write-ahead logging](https://sqlite.org/wal.html) explains the WAL and associated files. Copying only a live database's main file does not provide the online-backup contract.
- [SQLite's online backup API](https://sqlite.org/backup.html) is the basis for consistent backup while a database is in use. Backup correctness does not grant permission to activate another authority writer.
- [SQLite PRAGMA documentation](https://sqlite.org/pragma.html) describes the foreign-key, synchronous and journal settings that the store configures. Garbage collection is logical reclamation, not a secure-deletion guarantee.

## Fuzz lane pins

| Item | Exact selection and primary evidence |
| --- | --- |
| cargo-fuzz | `0.13.2`; [crates.io metadata](https://crates.io/api/v1/crates/cargo-fuzz/0.13.2), published 2026-06-09; archive SHA-256 `5acfd01930e49823e58c30dd8012d3338a620377d7c7d4cc140ca4b2169400e2` |
| cargo-fuzz source | [Official tag reference](https://api.github.com/repos/rust-fuzz/cargo-fuzz/git/ref/tags/0.13.2) resolved to commit `984c861c8dfea28055254c5f1d2659ab2cd63f76` |
| libfuzzer-sys | `0.4.13`; [crates.io metadata](https://crates.io/api/v1/crates/libfuzzer-sys/0.4.13), published 2026-06-04; archive SHA-256 `a9fd2f41a1cba099f79a0b6b6c35656cf7c03351a7bae8ff0f28f25270f929d2`; metadata license `(MIT OR Apache-2.0) AND NCSA` |
| Nightly | `nightly-2026-10-08`; [official distribution manifest](https://static.rust-lang.org/dist/2026-10-08/channel-rust-nightly.toml) records `1.101.0-nightly (1d81eb4ad 2026-10-07)` and `available = true` for `x86_64-unknown-linux-gnu` |
| Nightly Linux archive | Distribution-manifest `xz_hash`: `217d2f384c0c4b76b0d9c8e903cb95a6e4bd567610c0da249045cef395ebb295`; rustup verifies its download through the official distribution metadata |
| Checkout action | Full commit already verified in [CI pin provenance](ci-pins.json) |

The [Rust Fuzz Book setup instructions](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html) document nightly, C++ and sanitizer requirements. The pinned [cargo-fuzz build options](https://github.com/rust-fuzz/cargo-fuzz/blob/984c861c8dfea28055254c5f1d2659ab2cd63f76/src/options.rs) expose an explicit sanitizer selection but no `--locked` option. The workflow therefore fetches the committed dependency graph with Cargo `--locked`, builds/runs offline and checks that `fuzz/Cargo.lock` is unchanged.

Primary metadata verifies availability of these versions, not execution of the Linux workflow or sanitizer compatibility under every host configuration. The nightly is newer than the project's Rust 1.95 minimum; actual compilation/run evidence belongs in the [progress record](../progress.md). The current environment is Windows x86_64. No Linux or Windows sanitizer result is asserted by these configuration files.
