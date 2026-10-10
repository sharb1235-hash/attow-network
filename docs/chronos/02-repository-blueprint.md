# ChronosOS production repository blueprint

**Status: target structure and build contract.** The current package contains a tested reference kernel, research and implementation prompts. The production workspace below is to be created by those prompts; its presence here does not mean the runtime, integrations or release artifacts already exist. Dependency selections were checked against live metadata on **9 October 2026, Pacific time**. Expanded production feature combinations remain subject to compilation and runtime validation.

## An execution layer above existing infrastructure

ChronosOS should make heterogeneous execution feel like one system while retaining each provider's actual guarantees. [Golem](https://learn.golem.cloud/v1.5/develop/forking) supplies durable agent forks; [DBOS](https://docs.dbos.dev/ai/debugging) supplies workflow replay/forking; [StateFork](https://arxiv.org/abs/2609.38648) supplies a control plane for branchable sessions. Browser controllers, database branches and VM snapshots supply other boundaries. The repository's contribution is a common **Worldline IR**, dependency and effect model, scheduling policy, commit domains and provider conformance—not an assertion that these foundations become obsolete.

Keep the portable logical worldline separate from its current worker. Immutable state roots and typed continuations can be shared or suspended; a browser, Wasm instance or database snapshot is a materialized resource with a declared fidelity profile. Deterministic replay consumes recorded observations. Only genuinely reproducible work enters a shared computation cache.

## Target tree

```text
chronosos/
  Cargo.toml                 # root facade + workspace; exact direct pins
  Cargo.lock                 # reviewed, committed transitive graph
  rust-toolchain.toml         # 1.95.0; rustfmt, clippy
  README.md
  LICENSE-MIT
  LICENSE-APACHE
  THIRD_PARTY_NOTICES.md
  CHANGELOG.md
  SECURITY.md
  CONTRIBUTING.md
  CODE_OF_CONDUCT.md
  GOVERNANCE.md
  SUPPORT.md
  .gitignore
  .cargo/config.toml         # xtask alias; no credentials or global flags
  deny.toml
  rustfmt.toml
  clippy.toml
  src/
    lib.rs                   # stable facade; no duplicate implementation
    core/mod.rs              # state/ledger APIs and error re-exports
    sandbox/mod.rs           # feature-gated executor interface
  crates/
    chronos-codec/src/       # canonical types/bytes, hashes, limits
    chronos-store/src/       # immutable CAS, indexes, GC, recovery
    chronos-state/src/       # worldline DAG, roots, dependencies, worker ports
    chronos-ledger/src/      # local authority, promotion, durable outbox
    chronos-sandbox/src/     # Wasmtime profile, hostcalls, continuations
    chronos-scheduler/src/   # admission, workers, cancellation, pure reuse
    chronos-adapters/src/    # provider contracts, browser/SQL, effect gateway
    chronos-protocol/src/    # versioned bounded frames; no sockets
    chronos-transport/src/   # Iroh endpoint, streams, peer quotas
    chronos-cli/src/         # composition, commands, diagnostics
  xtask/src/main.rs          # repository checks and artifact orchestration
  wit/chronos-v1/
    types.wit
    observation.wit
    effects.wit
    continuation.wit
    world.wit
  docs/
    architecture.md
    worldline-ir.md
    canonical-codec.md
    determinism-profile.md
    continuation-abi.md
    commit-domains.md
    effect-lifecycle.md
    provider-contract.md
    browser-fidelity.md
    protocol-v1.md
    persistence-recovery.md
    threat-model.md
    performance-method.md
    release-runbook.md
    adr/
    research/                # sourced decisions and dependency evidence
  model/
    ledger.rs                # explicitly wired finite model test
    promotion.tla
    promotion.cfg
    assumptions.md
  tests/
    replay.rs
    storage_crash.rs
    promotion.rs
    adapter_conformance.rs
    peer_protocol.rs
  fuzz/                      # separate workspace / nightly toolchain lane
    Cargo.toml
    Cargo.lock
    fuzz_targets/{codec,frame,worldline_import}.rs
    corpus/
    artifacts/               # ignored; minimize/review before retention
  benches/worldline.rs
  examples/
    worldline_checkout.rs    # production API example; created later
    offline_replay.rs
  demos/workflow-studio/
    README.md
    app/                     # owned local web service
    browser/                 # pinned sidecar + lockfile
    fixtures/
    expected/
  fixtures/
    manifest.toml
    licenses/
    upstream/{chinook,atuin,actual,sqlite-check}/
    original/{sql,browser,effects,protocol}/
  scripts/
    check.ps1
    check.sh
    verify-artifacts.ps1
    verify-artifacts.sh
  deploy/relay/              # self-hosted relay config, explicitly enabled
  .github/
    workflows/{ci,model,fuzz,browser,release}.yml
    ISSUE_TEMPLATE/
    PULL_REQUEST_TEMPLATE.md
    CODEOWNERS
    dependabot.yml
  reference-kernel/          # independent executable model; retained
```

Each crate also has its own manifest, focused tests and a short README describing its invariants. Stage 1 creates every manifest-referenced target, including the model, example and benchmark files. A directory containing tests is not sufficient unless Cargo discovers or explicitly registers them.

## Complete target root manifest

This manifest is the starting production contract. Child manifests inherit package metadata, lints and only the dependencies they use. Internal versions use exact equality to keep the initial coordinated release together. All packages remain unpublished until release review. The CLI initially uses standard-library argument parsing; adding a CLI framework requires a separately verified pin.

```toml
[package]
name = "chronosos"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Worldline execution facade for ChronosOS"

[workspace]
resolver = "3"
members = [
  ".", "xtask",
  "crates/chronos-codec", "crates/chronos-store",
  "crates/chronos-state", "crates/chronos-ledger",
  "crates/chronos-sandbox", "crates/chronos-scheduler",
  "crates/chronos-adapters", "crates/chronos-protocol",
  "crates/chronos-transport", "crates/chronos-cli"
]
default-members = ["."]
exclude = ["reference-kernel", "fuzz"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "MIT OR Apache-2.0"
publish = false

[workspace.dependencies]
chronos-codec = { path = "crates/chronos-codec", version = "=0.1.0", default-features = false }
chronos-store = { path = "crates/chronos-store", version = "=0.1.0", default-features = false }
chronos-state = { path = "crates/chronos-state", version = "=0.1.0", default-features = false }
chronos-ledger = { path = "crates/chronos-ledger", version = "=0.1.0", default-features = false }
chronos-sandbox = { path = "crates/chronos-sandbox", version = "=0.1.0", default-features = false }
chronos-scheduler = { path = "crates/chronos-scheduler", version = "=0.1.0", default-features = false }
chronos-adapters = { path = "crates/chronos-adapters", version = "=0.1.0", default-features = false }
chronos-protocol = { path = "crates/chronos-protocol", version = "=0.1.0", default-features = false }
chronos-transport = { path = "crates/chronos-transport", version = "=0.1.0", default-features = false }
wasmtime = { version = "=48.0.5", default-features = false, features = [
  "std", "runtime", "cranelift", "component-model", "wat", "anyhow", "pooling-allocator"
] }
iroh = { version = "=1.3.0", default-features = false, features = ["tls-ring"] }
tokio = { version = "=1.53.2", default-features = false, features = [
  "rt-multi-thread", "macros", "sync", "time", "io-util", "fs"
] }
rusqlite = { version = "=0.40.2", features = ["bundled", "backup"] }
blake3 = "=1.8.7"
minicbor = { version = "=2.3.0", default-features = false, features = ["alloc"] }
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
ed25519-dalek = "=3.0.0"
getrandom = "=0.4.3"
zeroize = "=1.9.1"
rpds = "=1.2.1"
thiserror = "=2.0.21"
anyhow = "=1.0.104"
tracing = "=0.1.44"
tracing-subscriber = "=0.3.23"
proptest = "=1.11.0"
loom = "=0.7.2"
shuttle = "=0.9.6"
turmoil = "=0.7.2"
stateright = "=0.31.0"
criterion = "=0.8.2"
tempfile = "=3.27.0"

[features]
default = ["local"]
local = ["dep:chronos-store", "dep:chronos-ledger"]
sandbox = ["dep:chronos-sandbox"]
scheduler = ["sandbox", "dep:chronos-scheduler"]
adapters = ["local", "dep:chronos-adapters"]
browser = ["adapters", "chronos-adapters/browser"]
peer = ["dep:chronos-protocol", "dep:chronos-transport"]

[dependencies]
chronos-codec.workspace = true
chronos-state.workspace = true
chronos-store = { workspace = true, optional = true }
chronos-ledger = { workspace = true, optional = true }
chronos-sandbox = { workspace = true, optional = true }
chronos-scheduler = { workspace = true, optional = true }
chronos-adapters = { workspace = true, optional = true }
chronos-protocol = { workspace = true, optional = true }
chronos-transport = { workspace = true, optional = true }

[dev-dependencies]
proptest.workspace = true
stateright.workspace = true
tempfile.workspace = true
criterion.workspace = true

[[test]]
name = "ledger_model"
path = "model/ledger.rs"
required-features = ["local"]

[[example]]
name = "worldline_checkout"
path = "examples/worldline_checkout.rs"
required-features = ["local"]

[[bench]]
name = "worldline"
path = "benches/worldline.rs"
harness = false
required-features = ["local"]

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
unwrap_used = "warn"

[lints]
workspace = true

[profile.release]
lto = "thin"
codegen-units = 1

[profile.bench]
debug = true
```

The version choices and optional-tool inventory are recorded in the dependency report. Relevant primary records include [Wasmtime 48.0.5](https://crates.io/api/v1/crates/wasmtime/48.0.5), [Iroh 1.3.0](https://crates.io/crates/iroh/1.3.0) and [rusqlite 0.40.2](https://crates.io/crates/rusqlite/0.40.2). Wasmtime 49.0.2 requires a newer Rust lane; do not silently upgrade it. The proposed Tokio feature expansion is intentional and needs validation with the final graph.

Every child sets `[lints] workspace = true` and `[features] default = []`; `chronos-adapters` additionally declares `browser = []` until its sidecar is implemented. No feature enables a network connection by itself. No WASI package is linked by default. Pin a later optional WASI adapter to the same Wasmtime release and review its capabilities independently. Keep `wat` for development fixtures initially; changing a release feature profile changes its recorded execution-profile identity.

The CLI child manifest explicitly names its target binary; a package named `chronos-cli` does not otherwise produce the documented `chronos` executable:

```toml
[[bin]]
name = "chronos"
path = "src/main.rs"
```

This remains a working executable name pending the naming review. Root integration tests and examples that use optional APIs declare `required-features`; the facade gates their imports consistently. Default tests must compile without selecting the browser or peer profiles.

## One-way dependency and API boundaries

The arrow below means “depends on.” Neither runtime crates nor `xtask` depend on the root facade. The CLI composes crates directly, avoiding a facade/CLI cycle.

| Package | Allowed internal dependencies | Owned contract |
|---|---|---|
| `chronos-codec` | none | IDs, canonical bytes, limits, observations, intent/receipt/continuation and worker/proposal envelopes; bounded decoders |
| `chronos-store` | codec | CAS `put/get/contains`, immutable-object verification, durable indexes, pinning and reachability GC |
| `chronos-state` | codec | `Worldline`, `StateRoot`, branch delta, tracked reads, diff, dependency graph and `execution::WorkerBackend`; storage access through traits |
| `chronos-ledger` | codec, store, state | `CommitDomain`, validation, atomic local promotion, intent slots, outbox transitions, recovery |
| `chronos-sandbox` | codec, state | `ExecutionProfile`, admitted programs, typed hostcalls, continuations and Wasm implementation of `WorkerBackend` |
| `chronos-scheduler` | codec, state | Injected worker registry, bounded materialization, cancellation, fairness, budgets and verified pure-result reuse |
| `chronos-adapters` | codec, store, state, ledger | Browser/SQL implementations of `WorkerBackend`, provider capabilities, effect dispatch/reconciliation; no policy ownership in browser code |
| `chronos-protocol` | codec | Stateless frame schemas, negotiation, peer-object requests and bounded import validation |
| `chronos-transport` | codec, protocol | Iroh connectivity, authenticated channel plumbing, backpressure and per-peer limits |
| `chronos-cli` | selected crates above | Lifecycle composition, configuration and user-visible outcomes; no hidden semantic shortcuts |
| root facade | selected crates above | Stable public imports from `src/core/` and `src/sandbox/` |
| `xtask` | none required | Repository automation; invokes binaries/tests rather than implementing correctness |

Application authorization belongs above transport authentication. Provider traits do not claim every backend supports `prepare`, `commit`, exact restore or reconciliation. Unsupported capabilities remain explicit. Transport receives validated application requests and returns bytes/events; it never directly mutates the authority's database.

Own the shared worker interface in `chronos-state::execution`, with request/reply/error envelopes in codec. Its object-safe asynchronous shape can use only the standard library:

```rust
pub type WorkerFuture<'a> = std::pin::Pin<Box<
    dyn std::future::Future<Output = Result<WorkerReply, WorkerError>>
        + Send + 'a
>>;
pub trait WorkerBackend: Send + Sync {
    fn execute<'a>(&'a self, request: WorkerRequest) -> WorkerFuture<'a>;
}
```

The scheduler receives an `Arc<dyn WorkerBackend>` registry through construction. Sandbox and adapters implement the trait; neither implementation depends on scheduler. Requests bind attempt, profile, immutable inputs, resource scope and limits. Replies contain computation results, recorded observations or proposed effects, never dispatch authority. Credentials and irreversible dispatch stay in the separate gateway. Runtime state does not depend on store, and scheduler does not depend on ledger; integration-only dependencies belong in tests.

`PeerNode::propose` accepts a canonical `ProposalEnvelope` and returns `PromotionReply`; protocol supplies their wire framing. An injected application handler, composed by the CLI, checks authorization and converts between those envelopes and ledger APIs. Transport depends only on codec/protocol, and cannot acquire authority by importing an object or receiving a proposal.

The WIT ABI carries typed state and continuation data at hostcall safe points. It grants no ambient clock, randomness, filesystem or sockets. Canonical encoding and deterministic replay rules live in codec/specs, not in JSON serialization defaults. `rpds` may serve an in-memory persistent map; its iteration order is not the durable Merkle encoding. Wasmtime compiled artifacts are executable code: never deserialize arbitrary peer-provided artifacts merely because their hashes match.

## Build stages and acceptance evidence

| Stage | Deliverable | Required gate |
|---|---|---|
| 1. Scaffold | Workspace, facade, manifests, docs, licenses and target files | Metadata resolution, formatting and minimal compile; target features remain marked planned |
| 2. Codec/CAS | Canonical objects, bounded decoding, durable immutable store | Golden vectors, malformed inputs, interrupted object write recovery |
| 3. State | Worldline DAG, isolated deltas, explicit reads and diffs | Missing reads, tombstones, ranges, deterministic roots, shared-base isolation |
| 4. Ledger/outbox | Local commit authority and pending/unknown/confirmed effects | Conflicts roll back atomically; crash/reopen; unknown slot remains occupied |
| 5. Sandbox | Pinned Wasmtime profile and continuation ABI | Unknown-import rejection, bounded memory/fuel, captured replay, incompatible restore rejection |
| 6. Scheduler | Logical branches, bounded resident workers, pure-work sharing | Budget/cancellation races, correct cache invalidation, per-branch accounting |
| 7. Adapters/gateway | Owned browser/app/SQL bundle and mock effect service | Fidelity checks, conditional promotion, lost acknowledgement, no egress bypass |
| 8. Protocol/transport | Versioned protocol and private Iroh exchange | Invalid/truncated frames, reconnect, duplicate delivery, quotas; no implicit public lookup |
| 9. CLI/integration | Source-installable CLI and workflow-studio demo | Fresh-checkout walkthrough, two-node replay, portable export and verification |
| 10. Release | Reproducible artifacts, documentation and distribution review | Supported-platform evidence, licenses/SBOM, signatures, upgrade and recovery tests |

The finite model specifies assumptions and explored bounds; it does not certify a browser engine or arbitrary service. The initial authority is transactional and local. Future quorum commit domains need their own membership, fencing, partition and recovery model before the feature is advertised.

## Fixtures and demonstration assets

Keep original adversarial cases beside a small licensed real-schema corpus. The fixture manifest records upstream commit, exact path, original URL, SHA-256, license/notice path, extraction steps and supported/unsupported expectations. Do not fetch mutable upstream files during normal tests or include production data.

Use pinned [Chinook SQL](https://github.com/lerocha/chinook-database/blob/7f67772503d71ba90f19283c38e93923addb43fa/ChinookDatabase/DataSources/Chinook_Sqlite.sql), MIT; [Atuin history migration](https://github.com/atuinsh/atuin/blob/95af87d4b9f76ff9ddd682320310d1e6b856b77b/crates/atuin-client/migrations/20210422143411_create_history.sql), MIT; and [Actual Budget payees migration](https://github.com/actualbudget/actual/blob/358823017c93dbe5bf4bd469e6b49eb2cac00cf8/packages/loot-core/migrations/1550601598648_payees.sql), MIT. These exercise constraints, expression indexes, defaults and migration handling. SQLite's pinned [check.test](https://github.com/sqlite/sqlite/blob/d76e14d85ee6688763c9e91ab95bfe33652b1684/test/check.test) has its own public-domain notice; that observation does not license every SQLite test product or harness. Preserve each source's applicable notice.

The studio demonstrates one workflow across browser, owned web app, SQL, files and a mock external endpoint. Show logical and resident counts separately, conflict rejection, replay after interruption, a receipt-backed effect, and an ambiguous effect that stays unknown. Vendor no credentials or authenticated storage state. Pin the browser sidecar's package manager lockfile and browser revision when selected; no unverified Playwright version is invented here.

## CI and release matrix

| Lane | Environment | Checks |
|---|---|---|
| Core | Linux x86-64, Windows MSVC x86-64, macOS ARM64; Rust 1.95.0 | Locked build/tests, codec vectors, local storage/recovery, reference model |
| Feature profiles | Same supported platforms | No-default facade, local, sandbox/scheduler, adapters, peer; browser compile separate from browser execution |
| Browser bundle | Pinned Linux runner/container and browser revision | Owned local services, OS-level restricted egress, independent DB/effect log assertions |
| Transport | Explicit loopback/private two-node configuration | Iroh `Minimal`, bounded streams, authorization rejection and reconnection |
| Models/concurrency | Linux | Finite state checks; isolated Loom/Shuttle harnesses with stated bounds |
| Fuzz | Pinned compatible nightly on Linux with sanitizer support | Actual codec/frame/import targets; regression corpus retained after review |
| Release | Native supported targets | Locked release build, artifact smoke test, SBOM/notices, signed checksums and install/uninstall verification |

Use `cargo test --workspace --all-targets --locked` for default tests and explicit feature lanes for opt-ins. Do not let `--all-features` silently launch live integrations. The model is runnable with `cargo test -p chronosos --test ledger_model --locked`. `.cargo/config.toml` defines `xtask = "run --package xtask --"`. `scripts/check.ps1` checks each exit code; neither script changes the user's global toolchain.

The standalone fuzz manifest declares its own `[workspace]`, path dependencies into production crates and `libfuzzer-sys = "=0.4.13"`; it registers all three binary targets with `test = false`, `doc = false`, `bench = false`. Install the **tool** `cargo-fuzz = 0.13.2` in its CI lane; `libfuzzer-sys` is a library dependency. Never describe ignored fuzz source files as running checks.

Optional tooling pins from the verified inventory are nextest **0.9.148**, deny **0.20.2**, semver-checks **0.51.0**, llvm-cov **0.9.1**, dist **0.32.0**, release-plz **0.3.170**, and cyclonedx **0.5.9**. Introduce them when their gate becomes relevant. Coverage requires the matching LLVM tools; fuzzing has a separate nightly lane. Pin GitHub Actions by reviewed commit SHA and use minimal permissions. Public-network tests and publication are explicit workflows with scoped credentials.

## Distribution, governance and release checks

Commit both license texts and track dependency obligations from the final lockfile, including optional features. Do not enable a broad license allowlist merely to make a checker green. Publish the full correctness-critical runtime, relay/peer protocol, portable format, verifier and self-hosting instructions. Optional paid broker operations, hosted workers and fleet support must not be required for correctness or export.

Resolve the existing [ChronosOS name use](https://github.com/spawn08/chronos) before reserving package names or publishing artifacts. Record the chosen name, package ownership and provenance in release documentation. An open-source license review is separate from freedom-to-operate analysis: maintain a dated invention/claim matrix and have qualified counsel review any intended patent-sensitive commercial claims. Research similarity or a clean dependency scan is not legal clearance.

The release checklist must distinguish implemented features, experimental providers and planned work; publish only measured performance. Include threat-model changes, supported upgrade paths, recovery/export instructions and a security contact that actually exists. The release workflow stays manual and unpublished until those fields, artifact ownership and signing credentials are deliberately configured.
