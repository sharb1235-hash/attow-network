# ChronosOS: dependency and runtime validation

Packaged reproduction: the source-only probe is in `validation/runtime-probe/`. From the ChronosOS package root, run `./validation/runtime-probe/verify-existing.ps1`. References below to `work/chronos-dep-probe` identify the original location where the checks were performed; the packaged Cargo.lock and source files are copies of that tested probe.

Registry retrieval: 2026-10-10 UTC / 2026-10-09 America/Los_Angeles. Exact metadata and archive checksums are in `chronos-dependencies.json`; the complete probe dependency graph is in `chronos-dep-probe/Cargo.lock`. Registry metadata is evidence of publication, not a claim that every optional package was built.

## Decision

Use **Rust 1.95.0 + Wasmtime 48.0.5** as the locally validated compatibility lane. The machine has Rust 1.95.0 and Cargo 1.95.0 under the installed `stable-x86_64-pc-windows-msvc` alias. Latest published Wasmtime **49.0.2 requires Rust 1.96.0**, while 48.0.5 declares 1.95.0. Upstream's stable Rust channel manifest advertises 1.99.0, dated 2026-10-01. Upgrading the toolchain and Wasmtime together belongs in a separate compatibility change; do not silently update a developer's global toolchain. [Wasmtime 48.0.5 metadata](https://crates.io/api/v1/crates/wasmtime/48.0.5), [49.0.2 metadata](https://crates.io/api/v1/crates/wasmtime/49.0.2), [Rust stable manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml).

The fork primitive should clone a **logical immutable state root and explicit continuation data**, not promise a live Wasmtime Store clone. Wasmtime's `memory_init_cow` shares an eligible **initial module memory image**; it does not capture live stacks, mutated globals, host resources or sockets. Its 48.0.5 source explicitly states that Windows does not support this optimization and always copies initialization bytes. Linux supports it; other Unix systems require the appropriate precompiled file mapping. [Versioned Config source](https://docs.rs/wasmtime/48.0.5/src/wasmtime/config.rs.html).

The current upstream policy also designates every multiple-of-12 release as LTS with 24 months of support. Version 48 is therefore an LTS line; 49 is a normal release with a shorter support window. Continue reviewing security patches within the chosen line and retain a controlled upgrade gate for changed execution semantics. [Wasmtime release policy](https://docs.wasmtime.dev/stability-release.html).

Use thousands of cheap branch descriptors, with a bounded number of instantiated workers. Browser sessions, live APIs and production databases sit behind explicit adapters. A browser context, database backup or HTTP cassette is a specific adapter capability, not a generic OS checkpoint.

## Exact package candidates

MSRV below is the crate's registry `rust_version`, not a measured minimum for the entire dependency graph. A dash means it is undeclared. Each exact-version link is a primary registry record. All version selection was done against live metadata; the separately stated build result covers only the probe's selected dependencies.

| Purpose | Exact version | SPDX license expression | Declared MSRV | Selection |
|---|---|---|---|---|
| WASM execution | [wasmtime 48.0.5](https://crates.io/crates/wasmtime/48.0.5) | Apache-2.0 WITH LLVM-exception | 1.95.0 | Selected compatibility lane |
| Newer WASM execution | [wasmtime 49.0.2](https://crates.io/crates/wasmtime/49.0.2) | Apache-2.0 WITH LLVM-exception | 1.96.0 | Upgrade candidate only |
| Async orchestration | [tokio 1.53.2](https://crates.io/crates/tokio/1.53.2) | MIT | 1.71 | Selected; host orchestration only |
| QUIC peer transport | [iroh 1.3.0](https://crates.io/crates/iroh/1.3.0) | MIT OR Apache-2.0 | 1.91 | Selected |
| Peer-address ticket | [iroh-tickets 1.0.0](https://crates.io/crates/iroh-tickets/1.0.0) | MIT OR Apache-2.0 | 1.91 | Optional adapter |
| Self-hosted relay | [iroh-relay 1.3.0](https://crates.io/crates/iroh-relay/1.3.0) | MIT OR Apache-2.0 | 1.91 | Deployment package, not consensus |
| Raw QUIC alternative | [quinn 0.11.12](https://crates.io/crates/quinn/0.11.12) | MIT OR Apache-2.0 | 1.85 | Alternative to Iroh |
| SQLite binding | [rusqlite 0.40.2](https://crates.io/crates/rusqlite/0.40.2) | MIT | — | Selected, bundled + backup |
| Bundled SQLite FFI | [libsqlite3-sys 0.38.2](https://crates.io/crates/libsqlite3-sys/0.38.2) | MIT | — | Lockfile pin; bundles SQLite 3.53.2 |
| Content addressing | [blake3 1.8.7](https://crates.io/crates/blake3/1.8.7) | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | — | Selected |
| Explicit binary codec | [minicbor 2.3.0](https://crates.io/crates/minicbor/2.3.0) | BlueOak-1.0.0 | — | Optional reviewed CBOR profile |
| Serialization traits | [serde 1.0.229](https://crates.io/crates/serde/1.0.229) | MIT OR Apache-2.0 | 1.56 | Diagnostics and adapter DTOs |
| Diagnostic JSON | [serde_json 1.0.151](https://crates.io/crates/serde_json/1.0.151) | MIT OR Apache-2.0 | 1.71 | Never implicit hash canonicalization |
| Signature | [ed25519-dalek 3.0.0](https://crates.io/crates/ed25519-dalek/3.0.0) | BSD-3-Clause | 1.85 | Selected strict verification |
| OS randomness | [getrandom 0.4.3](https://crates.io/crates/getrandom/0.4.3) | MIT OR Apache-2.0 | 1.85 | Identity creation outside replay |
| Secret clearing | [zeroize 1.9.1](https://crates.io/crates/zeroize/1.9.1) | Apache-2.0 OR MIT | 1.85 | Keys where required |
| Persistent maps | [rpds 1.2.1](https://crates.io/crates/rpds/1.2.1) | MIT | 1.85.0 | Optional in-memory root implementation |
| Persistent collections alternative | [imbl 7.0.2](https://crates.io/crates/imbl/7.0.2) | MPL-2.0+ | 1.85 | Alternative; license review |
| Older persistent collections | [im 15.1.0](https://crates.io/crates/im/15.1.0) | MPL-2.0+ | 1.46.0 | Not preferred for a fresh dependency |
| Embedded KV alternative | [redb 4.3.0](https://crates.io/crates/redb/4.3.0) | MIT OR Apache-2.0 | 1.90 | Avoid a second storage engine in v0.1 |
| Errors | [thiserror 2.0.21](https://crates.io/crates/thiserror/2.0.21) | MIT OR Apache-2.0 | 1.77 | Typed library errors |
| Executable errors | [anyhow 1.0.104](https://crates.io/crates/anyhow/1.0.104) | MIT OR Apache-2.0 | 1.68 | CLI/probe only |
| Tracing | [tracing 0.1.44](https://crates.io/crates/tracing/0.1.44) | MIT | 1.65 | Host observability |
| Trace output | [tracing-subscriber 0.3.23](https://crates.io/crates/tracing-subscriber/0.3.23) | MIT | 1.65 | Host observability |

Metadata licenses do not describe every transitive dependency or automatically establish distribution compliance. Inspect the final lockfile with the release policy; this is especially relevant to optional MPL packages and BlueOak-1.0.0.

The independent probe uses this exact manifest subset:

```toml
[package]
edition = "2024"
rust-version = "1.95"

[dependencies]
wasmtime = { version = "=48.0.5", default-features = false, features = [
  "std", "runtime", "cranelift", "component-model", "wat", "anyhow",
  "pooling-allocator"
] }
iroh = { version = "=1.3.0", default-features = false, features = ["tls-ring"] }
tokio = { version = "=1.53.2", features = ["rt-multi-thread", "macros", "time"] }
rusqlite = { version = "=0.40.2", features = ["bundled", "backup"] }
blake3 = "=1.8.7"
minicbor = { version = "=2.3.0", features = ["alloc"] }
ed25519-dalek = "=3.0.0"
rpds = "=1.2.1"
anyhow = "=1.0.104"
tempfile = "=3.27.0"
```

Production can remove `wat` when it only accepts binary WASM. `pooling-allocator` permits allocator tuning but is not required for correctness. We deliberately omit `wasmtime-wasi`: there is no automatic filesystem, network, clock or randomness grant in the execution core. If an adapter later requires WASI, select `wasmtime-wasi = "=48.0.5"` alongside the same runtime release, review its capabilities and validate that additional graph separately. The latest 49.0.2 WASI crate also requires Rust 1.96.0. [wasmtime-wasi versions](https://crates.io/api/v1/crates/wasmtime-wasi).

## Runtime contract and supported calls

The probe exercises `Config::consume_fuel`, canonical NaNs, disabled SIMD/relaxed SIMD, component-model support, Store limits, a core typed call, a component typed call, and an infinite-loop out-of-fuel trap. Wasmtime 48's component `TypedFunc::call` handles post-return automatically; the old explicit `post_return` method is a deprecated no-op. [TypedFunc 48.0.5 source](https://docs.rs/wasmtime/48.0.5/src/wasmtime/runtime/component/func/typed.rs.html). Example shape:

```rust
let mut config = wasmtime::Config::new();
config.consume_fuel(true)
    .cranelift_nan_canonicalization(true)
    .wasm_simd(false)
    .wasm_relaxed_simd(false)
    .wasm_component_model(true);
let engine = wasmtime::Engine::new(&config)?;
let mut store = wasmtime::Store::new(&engine, HostState {
    limits: wasmtime::StoreLimitsBuilder::new()
        .memory_size(64 * 1024 * 1024)
        .memories(2).table_elements(4096).tables(2).instances(8).build(),
});
store.limiter(|state| &mut state.limits);
store.set_fuel(1_000_000)?;
```

`HostState` is application-owned and includes a `StoreLimits` field. `memory_size` applies separately to **each** guest memory, not aggregate process RSS; instance/table/memory counts also need caps. Host allocations, compiler memory, host recursion and imported-function work need separate budgets. [Versioned Store limits](https://docs.rs/wasmtime/48.0.5/wasmtime/struct.StoreLimitsBuilder.html), [Store API](https://docs.rs/wasmtime/48.0.5/wasmtime/struct.Store.html).

The selected features omit WASM threads, GC and async components; their feature-gated setters are not all callable in this build. Do not copy `wasm_threads(false)` into the above manifest: that setter itself requires the `threads` feature. To support different component toolchains later, explicitly expand the feature profile and rerun validation rather than relying on all defaults.

Fuel is a reproducible instruction budget within a pinned execution profile, not a wall-time guarantee or a cross-release constant. Epoch interruption is suitable for host responsiveness but timer-driven interruption is not a deterministic ledger outcome. Define cancellation, process termination and host OOM as incomplete infrastructure attempts; only a completed, validated evaluation may enter the accepted execution ledger. [Deterministic execution guide](https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html).

The runtime probe also demonstrated that reusing the Store after a core out-of-fuel trap makes a subsequent component call fail with `CannotEnterComponent`. Its synchronous component implementation checks whether the Store has trapped. Discard a trapped Store and construct a fresh attempt from the authoritative input state; resetting fuel alone is not a rollback. Share compiled code across attempts, not arbitrary damaged execution state. [Versioned synchronous component implementation](https://docs.rs/wasmtime/48.0.5/src/wasmtime/runtime/component/concurrent_disabled.rs.html).

A deterministic profile needs more than config flags:

1. Accept a fixed module/component binary hash, runtime/config profile hash, explicit state root and recorded input values.
2. Link only a small typed effect interface. No clocks, random sources, filesystem/environment enumeration or sockets except as supplied ledger inputs. Reject unknown imports; do not use automatic WASI linking.
3. Admit fixed-size memories/tables or a separately proved deterministic allocation policy. Resource-limit setters alone do not prevent host pressure from causing allocation failure before the configured ceiling. Validate the supported binary subset before accepting a program.
4. Treat every effect return as bounded data. Keep JSON/SQL collation, timezone, locale, floating-point formatting and map iteration out of authoritative state transitions unless explicitly specified.
5. Checkpoint only at an application boundary with no in-flight imports. Persist the typed continuation/state schema. Reinstantiate the same code and restore through that schema. An arbitrary component's private globals, resource table and stack are not implicitly captured.

Items 1–5 are proposed ChronosOS admission rules, not a claim that Wasmtime supplies an end-to-end deterministic application framework.

`Module::serialize` / `Component::serialize` serialize compiled **code**. Deserialization accepts an engine-specific executable artifact and is unsafe with untrusted bytes. Distribute validated WASM bytes to peers; compile locally, or require a tightly controlled artifact provenance path. Never treat a remote peer's content hash as sufficient authorization to call unsafe compiled-artifact deserialization. [Versioned Module API](https://docs.rs/wasmtime/48.0.5/wasmtime/struct.Module.html), [Component API](https://docs.rs/wasmtime/48.0.5/wasmtime/component/struct.Component.html).

## Transport and ledger persistence

Iroh 1.3.0's reviewed source uses **noq 1.3.0** internally. Quinn 0.11.12 is a separate low-level alternative; its connection types are not a drop-in Iroh transport type. Iroh carries streams, peer identity and path establishment. It does not supply application authorization, ledger agreement, business effect exactly-once semantics, or branch merging. [Versioned Iroh manifest](https://docs.rs/crate/iroh/1.3.0/source/Cargo.toml).

For a no-public-services test, use `Endpoint::builder(presets::Minimal)`, `clear_ip_transports()`, then `bind_addr("127.0.0.1:0")`. Extract the bound loopback socket from `server.bound_sockets()` and construct the destination with `EndpointAddr::new(server.id()).with_ip_addr(socket)`. The probe establishes authenticated direct QUIC, calls `open_bi`/`accept_bi`, sends data, finishes the sender and reads with a length bound. The initiator must write before waiting for a response; merely opening a stream is insufficient to make it visible to the remote peer. [Endpoint 1.3.0 source](https://docs.rs/iroh/1.3.0/src/iroh/endpoint.rs.html).

`presets::N0` enables Number 0 DNS/address publication and default relays. `N0DisableRelay` still uses public address lookup. A private deployment must configure its lookup and relay deliberately; changing only relay mode does not eliminate discovery traffic. `Minimal` supplies the selected crypto provider without those services. [Presets 1.3.0 source](https://docs.rs/iroh/1.3.0/src/iroh/endpoint/presets.rs.html).

Apply protocol frame limits, per-peer quotas, immutable-object validation and application-level acknowledgments. Bound the number of active streams independently from the number of branches. Disable 0-RTT for state-changing application operations unless a replay-safe operation identity and acceptance rule are already in place. Relay availability affects connectivity, not truth of a branch execution.

SQLite is the local ledger/index owner. WAL permits concurrent readers and one writer; it requires cooperating processes on the same host. It is not a replication format to exchange between devices. Long read transactions can delay checkpoints. Never copy only the main `.sqlite` file while a WAL contains committed updates. [SQLite WAL](https://www.sqlite.org/wal.html).

The probe uses `pragma_update(None, "journal_mode", "WAL")`, `synchronous=FULL`, a transaction inserting an event, and `rusqlite::backup::Backup::new` plus `run_to_completion`. The backup API is a supported snapshot/copy operation; it is not an O(1) persistent database fork. [rusqlite backup API](https://docs.rs/rusqlite/0.40.2/rusqlite/backup/struct.Backup.html), [SQLite backup contract](https://www.sqlite.org/backup.html).

Use one writer actor and short transactions for ledger metadata. Sharing immutable data pages or content-addressed records across branch roots is a separate storage design. Do not instantiate a physical SQLite database for each cheap speculative branch. A micro-sandbox that must run arbitrary SQL needs a bounded physical or logical snapshot adapter and a defined SQL-feature subset; a ledger database alone does not provide that capability.

`rpds::HashTrieMap` structurally shares nodes on persistent updates; the default pointer kind is not the thread-sharing choice. Use `HashTrieMapSync::new_sync()` for roots that cross worker threads. Neither its default hasher nor iteration order is a canonical Merkle layout. Specify durable node bytes independently before using such a collection as a content-addressed representation. [rpds 1.2.1 API](https://docs.rs/rpds/1.2.1/rpds/map/hash_trie_map/struct.HashTrieMap.html).

CBOR is also not automatically canonical because a crate supports it. Use explicit ordered arrays and fixed variant tags, bounded lengths and integer ranges; reject indefinite lengths, unexpected fields and alternate representations. Normalize/re-encode and require byte equality on acceptance when using a deterministic CBOR profile. The probe validates an explicit encoder path, not a full hostile-input decoder. BLAKE3 hashes bytes; it does not choose or enforce the schema. Signature checks in the probe use `verify_strict` individually. [minicbor 2.3.0](https://docs.rs/minicbor/2.3.0/minicbor/), [ed25519-dalek 3.0.0](https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html).

## Test and release tool candidates

| Role | Exact pin | License | Declared MSRV | Scope |
|---|---|---|---|---|
| Property tests | [proptest 1.11.0](https://crates.io/crates/proptest/1.11.0) | MIT OR Apache-2.0 | 1.85 | Stateful branch/merge/codec properties |
| Concurrency model checking | [loom 0.7.2](https://crates.io/crates/loom/0.7.2) | MIT | 1.65 | Small synchronization primitives |
| Schedule exploration | [shuttle 0.9.6](https://crates.io/crates/shuttle/0.9.6) | Apache-2.0 | — | Isolated deterministic concurrency harness |
| Async network simulation | [turmoil 0.7.2](https://crates.io/crates/turmoil/0.7.2) | MIT | — | Abstract transport tests; not a claim to simulate Iroh OS sockets |
| Protocol model checking | [stateright 0.31.0](https://crates.io/crates/stateright/0.31.0) | MIT | — | Small ledger/state-machine models |
| Benchmarks | [criterion 0.8.2](https://crates.io/crates/criterion/0.8.2) | Apache-2.0 OR MIT | 1.86 | Explicit hardware/profile reports |
| Temporary files | [tempfile 3.27.0](https://crates.io/crates/tempfile/3.27.0) | MIT OR Apache-2.0 | 1.63 | Probe and storage tests |
| Fuzz driver | [cargo-fuzz 0.13.2](https://crates.io/crates/cargo-fuzz/0.13.2) | MIT OR Apache-2.0 | — | Separate supported sanitizer/nightly CI lane |
| Fuzz runtime | [libfuzzer-sys 0.4.13](https://crates.io/crates/libfuzzer-sys/0.4.13) | (MIT OR Apache-2.0) AND NCSA | — | Codec and frame parsers |
| Test runner | [cargo-nextest 0.9.148](https://crates.io/crates/cargo-nextest/0.9.148) | Apache-2.0 OR MIT | 1.91 | Optional CI tooling |
| Policy/advisories | [cargo-deny 0.20.2](https://crates.io/crates/cargo-deny/0.20.2) | MIT OR Apache-2.0 | 1.88 | Lockfile and distribution policy |
| API compatibility | [cargo-semver-checks 0.51.0](https://crates.io/crates/cargo-semver-checks/0.51.0) | Apache-2.0 OR MIT | 1.93 | Public SDK releases |
| Coverage | [cargo-llvm-cov 0.9.1](https://crates.io/crates/cargo-llvm-cov/0.9.1) | Apache-2.0 OR MIT | 1.87 | CI with matching LLVM tools |
| Installers/artifacts | [cargo-dist 0.32.0](https://crates.io/crates/cargo-dist/0.32.0) | MIT OR Apache-2.0 | 1.74 | Generated config must be reviewed |
| Version automation | [release-plz 0.3.170](https://crates.io/crates/release-plz/0.3.170) | MIT OR Apache-2.0 | — | Release PR generation, optional |
| SBOM | [cargo-cyclonedx 0.5.9](https://crates.io/crates/cargo-cyclonedx/0.5.9) | Apache-2.0 | 1.85 | Final resolved graph |

This table is a verified publication inventory, not evidence that all tools were installed or executed. Do not make the initial build depend on every tool. Root-state, crash-recovery and replay invariants deserve tests first; merge heuristics, VM reuse and performance optimizations can have separate gates. A passing finite model does not prove the operating system, a browser engine or a third-party API deterministic.

## Windows setup without changing the default toolchain

The native MSVC target requires Visual C++ build tools and a Windows SDK. Bundled SQLite and ring include native compilation, so Rust alone is insufficient on a fresh machine. The current machine's existing compiler environment is used by the probe. [Rust MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

For a fresh environment, these are proposed setup commands, **not actions taken in this research**:

```powershell
rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy --no-self-update
if ($LASTEXITCODE -ne 0) { throw 'Toolchain installation failed' }
cargo +1.95.0-x86_64-pc-windows-msvc build --locked
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
cargo +1.95.0-x86_64-pc-windows-msvc test --locked
if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
```

This installs an explicit side-by-side toolchain and does not use `rustup default` or `rustup update`. For an existing machine with the verified 1.95.0 installed under a different alias, the probe script checks the compiler version and uses the existing compiler without installing anything. Commit the application's `Cargo.lock`; exact direct version pins alone do not freeze transitives. Release builds additionally record target, rustc verbose version, Cargo lock hash and runtime feature/config hash.

## Validation status

**Passed on native x86_64 Windows/MSVC with the already-installed Rust 1.95.0.** `cargo run --locked --manifest-path work/chronos-dep-probe/Cargo.toml` returned exit code 0. The probe is intentionally separate from the root reference kernel. Its complete dependency graph is locked (440 registry packages were resolved), and no global toolchain was installed or updated.

Observed output:

```text
wasmtime core: fixed-memory call=42; fuel limit traps
wasmtime component: typed call=42; automatic post-return
data: SQLite 3.53.2 WAL commit + online backup; explicit CBOR+BLAKE3+strict Ed25519; persistent map fork
iroh: Minimal preset direct loopback bidirectional QUIC echo; endpoints closed
```

Initial compile/runtime attempts exposed and corrected actual API drift: the singular Iroh `with_ip_addr` method, SQLite signed-integer decoding, automatic component post-return, and the fresh-Store boundary after a trap. This is API compatibility and a narrow runtime smoke test. It does not establish production readiness, hostile-input decoder correctness, cross-architecture replay equivalence or thousand-sandbox performance; no optional release tools were installed or executed.
