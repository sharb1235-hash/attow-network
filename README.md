# Attow Network

Attow Network is a Rust project for exploring alternative executions, sharing immutable state and safe computations, and promoting selected effects through an accountable authority.

**Development status: local authority, bounded workers and safe computation reuse.** The workspace
provides bounded canonical objects, content identity, transactional SQLite object
storage, root pins, garbage collection, online backups, isolated branch overlays
and tracked dependencies, atomic promotion, a durable effect outbox, and offline
event replay, explicit WASM checkpoints and durable resumption. A bounded graph
scheduler adds tenant queues, admitted pure reuse, receipt invalidation and offline
execution-trace verification, alongside a development CLI and repository checks.
The execution system described below is the target
architecture. A compiling feature or placeholder target does not mean its runtime
exists. See the [implementation record](docs/progress.md) for completed stages and
actual validation results.

ChronosOS is the internal project name. Public packages use `attow-*`, the facade is `attow-network`, and the executable is `attow`. Internal `crates/chronos-*` paths preserve the implementation plan's layout. The original research package in `docs/chronos` retains its original naming as source material; it does not override the public name or report this workspace's implementation status.

```text
 Applications / agents / workflow systems
                   |
       alternative execution graph
                   v
 +-------------------------------------------+
 | Worldline kernel                          |
 | immutable state | continuations | history |
 | dependencies | bounded workers | reuse    |
 +----------------------+--------------------+
                        |
       +----------------+----------------+
       |                                 |
 Speculative workers             Authority per domain
 WASM / browser / SQL            validation + rights + outbox
       |                                 |
 observations and proposals      credentialed effect gateway
       |                                 |
       +---- comparison          provider receipt / unknown

 Peers exchange immutable objects and proposals.
 Each domain retains one accepted decision history.
```

## Implementation status

| Area | Current status |
| --- | --- |
| Workspace, crate boundaries, public naming | Foundation |
| Canonical objects and identifiers | Versioned bounded CBOR, domain-separated BLAKE3, malformed-input tests |
| CLI | Development help/status; unsupported operations fail explicitly |
| Repository policy checks | Foundation checks with negative fixtures |
| Immutable storage | SQLite transactions, verified objects, root pins, GC and online backup |
| Worldline state | Shared immutable roots, tracked reads/writes/scans, tombstones, diffs and durable admission |
| Serializable promotion, outbox and recovery | Atomic local authority, reserved rights, durable uncertainty, replay and abrupt crash tests |
| WASM execution | Admitted components, fuel/memory/output bounds, explicit checkpoints and new-process resume |
| Bounded scheduling and pure reuse | Tenant queues, resource caps, admitted pure sharing, receipt invalidation and offline traces |
| Provider/browser/SQL adapters and effect gateway | Planned: stage 7 |
| Authenticated peer exchange | Planned: stage 8 |
| Integrated CLI demonstration | Planned: stage 9 |
| Release artifacts and distribution evidence | Planned: stage 10 |

## Build from source

Install Rust with rustup; the repository selects Rust **1.95.0** side by side using `rust-toolchain.toml`. Run these commands from the repository root:

```sh
cargo run -p attow-cli -- --help
cargo run -p xtask -- verify-repository
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

The `local` feature is the default. `sandbox` enables the bounded WASM backend;
`scheduler` adds graph scheduling, validated result retention and offline replay.
`adapters`, `browser` and `peer` select boundaries for the following stages.
Features do not grant permission to connect to a network. All packages remain
`publish = false`.

The sandbox uses Wasmtime in process and links no WASI imports. Its tested contract
is explicit data checkpoints under a versioned profile; operating-system isolation
for hostile guests remains outside this support level. See the
[determinism profile](docs/determinism-profile.md).

The scheduler example runs two distinct branches through one shared WASM
evaluation, then verifies the recorded trace after dropping the worker. Replay
uses recorded outcomes and makes zero worker or provider calls; it does not
independently rerun the guest or establish external-provider truth.

```sh
cargo run --example offline_replay --features scheduler --locked
```

See [scheduler behavior](docs/scheduler.md) and the
[complete reuse contract](docs/reuse-contract.md) for admission and resource limits.

## Local checkout example

The owned fixture explores five alternatives for one purchase decision. Branches share a state root and record their dependencies, then propose offers. One local authority validates a selected proposal, reserves rights, and persists its intent and outbox entry in a single transaction. The example reports five predictions, one pending authorization, four conflicts, and zero confirmations or provider calls.

```sh
cargo run --example worldline_checkout --locked
```

The fixture gateway and reconciliation demonstration are planned for later stages.
An unknown outcome keeps the conflict slot occupied; choosing a different branch
cannot undo a transmitted request. Tests use owned fixtures and simulators.

## Design and contribution

Start with the [architecture](docs/architecture.md), [threat model](docs/threat-model.md), [contribution guide](CONTRIBUTING.md) and [architecture decisions](docs/adr/0001-worldlines.md). The design supports multiple backends with declared fidelity; it does not promise arbitrary browser process cloning, remote API rollback, universal atomic transactions or automatic authority failover.

Original project code is available under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE). See [third-party notices](THIRD_PARTY_NOTICES.md) for attribution policy and retained source material.
