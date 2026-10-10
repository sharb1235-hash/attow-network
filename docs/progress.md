# Attow Network implementation progress

Source: the user supplied ten ordered implementation prompts under docs/chronos/prompts.
Public name: Attow Network. Public Cargo packages use attow-* and executable attow.
ChronosOS and chronos-* directory/dependency aliases are internal historical names.
The original source package is preserved byte-for-byte under docs/chronos; its embedded
instructions are reference material, subject to the user's naming and task instructions.
All 42 manifest-listed input files passed SHA-256 verification before copying.

| Stage | Deliverable | Status |
|---|---|---|
| 1 | Foundation, architecture, reproducible checks | Complete |
| 2 | Canonical codec and transactional content store | Complete |
| 3 | State and worldline forks | Complete |
| 4 | Ledger and atomic promotion | Complete |
| 5 | WASM continuations | Complete |
| 6 | Scheduler and safe computation reuse | Complete |
| 7 | Adapters and effect gateway | Complete |
| 8 | Peer federation | Pending |
| 9 | CLI and owned demonstration | Pending |
| 10 | Release candidate and signing | Pending |

## Execution evidence

### Stage 1

Implemented the 12-package Rust workspace, facade and feature boundaries, `attow`
development CLI, repository checker, registered future targets, architecture/ADRs,
community documents, license texts and native-platform CI configuration. Exact
Rust 1.95.0 was installed side by side; the global default remains unchanged.

Executed separately on Windows x86_64 MSVC, all with exit code 0:

- `cargo generate-lockfile`
- `cargo run -p xtask --locked -- verify-repository`
- `cargo fmt --all --check` (after fixing initial formatting differences)
- `cargo test --workspace --all-targets --locked` — 15 tests passed
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo doc --workspace --no-deps --locked`
- `cargo check -p attow-network --no-default-features --locked`

The checker was first run against its negative fixtures before implementation:
9 expected failures; its final suite contains 13 passing tests. Two CLI tests
verify honest development help and failure of unsupported commands. No runtime
tests are claimed for the intentionally empty future-stage modules. CI action
SHAs were verified against official upstream metadata. Remote CI, other platform
builds, publication and signing have not run.

### Stage 2

Implemented `crates/chronos-codec` and `crates/chronos-store`, canonical byte/hash
fixtures, independent facade adversarial tests, codec/persistence documentation,
dependency provenance, and a separately locked fuzz workspace with an opt-in
Linux sanitizer workflow. Root and fuzz lockfiles are present. Fixture bytes are
protected from checkout newline conversion.

The codec uses a fixed versioned CBOR envelope, shortest integers, deterministic
text-map ordering, explicit sorted references, bounded parsing and BLAKE3 hashes
binding kind/schema/length. SQLite uses WAL/FULL/foreign keys, immediate write
transactions, bounded verified reads, pins, fail-closed GC, and online backups.
The transaction API preserves a composing module's typed errors through rollback.

Executed separately on Windows x86_64 MSVC, all exit code 0:

- `cargo fmt --all --check`
- `cargo test --workspace --all-targets --locked` — 52 tests passed
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo doc --workspace --no-deps --locked`
- `cargo test -p attow-codec -p attow-store --locked` — 13 codec and 17 store tests
- `cargo run -p xtask --locked -- verify-repository`
- `cargo +1.95.0 check --manifest-path fuzz/Cargo.toml --locked`

Tests were run before implementation: codec 9 expected failures; store 11 expected
failures; facade tests initially failed on absent APIs/encoding. Review discovered
nested declared-array allocation amplification and a custom-limit bypass for
opaque store schemas; both were repaired and gained regression tests. Tests cover
pin/GC ordering across connections and committed-WAL backup consistency. Fuzz
compilation is not a sanitizer result; the opt-in Linux fuzz run and remote CI
have not executed. No external packfiles, encryption or secret erasure is claimed.
Next: Stage 3 immutable worldline state and tracked branch dependencies.

### Stage 3

Implemented immutable roots and isolated overlays in `crates/chronos-state`, canonical
worldline descriptors (including observation roots), bounded transactional admission,
worker ports, typed step envelopes in the codec, and the facade SQLite persistence
adapter. Added state/admission/independent property tests, an original SQL fixture,
ADR 0006, worldline documentation and a real logical-fork/durable-admission benchmark.

Executed separately on Windows x86_64 MSVC, all exit code 0:

- `cargo fmt --all --check`
- `cargo test --workspace --all-targets --locked` — 84 tests passed
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo doc --workspace --no-deps --locked`
- `cargo run -p xtask --locked -- verify-repository`
- `cargo test -p attow-state --locked` — 12 tests passed
- `cargo test --test state_properties --locked` — 13 tests, including 128 generated traces
- `cargo +1.95.0 bench --bench worldline --locked -- --entries 1000 --forks 1000 --samples 3`

Four initial state behavior tests failed before implementation. Review added aggregate
allocation preflight and descriptor-inclusive graph depth checks. Tests reject dishonest
readers, malformed dependency/reference sets, missing reads, ABA and scan phantoms.
Benchmark raw samples, machine metadata and limits are in `docs/performance-method.md`;
worker materialization was not measured. `freeze` is explicitly in-memory; `admit` inserts
and pins the reachable graph in one transaction. Branches confer no spendable authority.
Next: Stage 4 serializable promotion, durable outbox, replay and abrupt crash tests.

### Stage 4

Implemented `crates/chronos-ledger` schema, pure reducer, atomic promotion,
rights/policy/generation checks, semantic slots, durable outbox, trusted adapter
verification port, exact ticket/evidence binding, uncertainty recovery and replay.
Added store read snapshots/read-only opening, 12 independent promotion tests,
7 replay tests, 10 crash tests, finite model, capacity regressions and the owned
checkout example. Updated commit-domain, effect, recovery and primary-source docs.

Executed separately on Windows x86_64 MSVC, all exit code 0 after repairs:

- `cargo fmt --all --check`
- `cargo test --workspace --all-targets --locked` — 134 tests passed
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo doc --workspace --no-deps --locked`
- `cargo run -p xtask --locked -- verify-repository`
- `cargo test --test promotion --locked` — 12 passed
- `cargo test --test replay --locked` — 7 passed
- `cargo +1.95.0 test --test ledger_model --locked -- --nocapture` — 5 passed;
  3,184 states and 10,380 transitions, including four unsafe-rule mutants detected
- `cargo +1.95.0 test --test storage_crash --locked -- --nocapture` — 10 passed;
  eight actual child-process kill boundaries before/after promotion, claim,
  outcome and recovery commits; child destructors did not execute
- `cargo run --example worldline_checkout --locked` — 5 predictions, 1 pending,
  4 conflicts, 0 confirmed, 0 provider calls
- `cargo +1.95.0 test -p attow-ledger --test admission_limits --locked -- --nocapture`
  — 3 passed after reproducing both capacity failures

Initial promotion/replay checks failed on absent ledger APIs; five initial model
checks failed before its transition implementation. Store snapshot APIs and five
lifecycle capacity checks also had recorded failing tests before implementation.
Review repaired operation identity rebinding, unrelated staged CAS bytes blocking
reopen, and event/snapshot capacity that could strand authorized effects. Replay
now streams admitted event objects from one snapshot into a fresh SQLite database
and compares all ten materialized tables plus exact event rows. It has no provider
interface. Active intents reserve log slots, snapshot bytes and codec items needed
for claim, recovery and terminal evidence; the public capacity regression confirms
an operation can finish and reopen at the 4,096-event limit.

Limits: the authority is deliberately bounded (no log archival/rotation); local
path binding blocks moved copies but is not cross-host fencing. Read-only inspection
of copies is available. Full malicious history replacement requires an independent
trusted checkpoint. No real provider, remote authority, physical power-loss test or
non-Windows execution is claimed. Next: Stage 5 constrained WASM continuations.

### Stage 5

Implemented the Wasmtime component executor, structural admission, canonical
execution profiles, bounded scalar host interface, explicit continuations and
admission-checked WorkerBackend. Added the atomic SQLite sandbox repository,
public attow-v1 WIT, ten original component fixtures, native CI runtime lanes,
determinism/continuation/source documentation and dependency/license records.
Exposed the existing bounded state graph verifier for compositional storage.

Executed separately on Windows x86_64 MSVC, all exit code 0 after repairs:

- `cargo test -p attow-sandbox --locked` — 7 unit and 12 real-component tests
- `cargo test --workspace --all-targets --features sandbox --locked` — 160 passed
- `cargo test --workspace --all-targets --locked` — 153 passed
- `cargo test --test sandbox_storage --features sandbox --locked` — 2 passed
- `cargo test --test sandbox_backend --features sandbox --locked` — 3 passed
- `cargo +1.95.0 test --features sandbox --test sandbox_resume --locked -- --nocapture`
  — 2 passed, including an actual child process reconstructing code and state
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo clippy --workspace --all-targets --features sandbox --locked -- -D warnings`
- `cargo doc --workspace --no-deps --locked`
- `cargo run -p xtask --locked -- verify-repository`
- `cargo check -p attow-network --no-default-features --locked`
- `cargo check -p attow-network --no-default-features --features sandbox --locked`

The initial profile/admission tests failed five checks, component tests failed
eleven checks against the executor stub, and SQLite checkpoint tests failed on
the missing adapter. Repairs included Wasmtime 48's concrete error type, an
explicit feature whitelist instead of an MVP mask that implicitly enabled GC
types, and consistent rejection of unsupported multi-memory component shapes.
No test was removed or rejection invariant weakened. All eighteen registered
fixture hashes match their manifest entries. Package tests compile the actual WIT.

Tests exercise arithmetic, replay hash equality, sibling isolation, absence of
ambient imports, malformed components, hidden mutable globals, actual fuel traps,
memory/output bounds, discarded trapped instances, persistence failure and new
process resumption. Exact admission and budget changes fail before cache
eligibility or execution. Pure workers cannot emit managed deltas or requests.
Objects, evaluation records, standard outcomes and pins commit together; failed
checkpoints leave no partial objects or pins. Every evaluation binds its profile.

Limits: this is an in-process Wasmtime backend with up to sixteen active workers,
bounded source compilation/cache and explicit byte streams. It has no WASI,
credentials or external dispatch. OS isolation for hostile guests, process RSS
quotas and compilation deadlines are not implemented. Runtime/profile upgrades
require distinct identities. Cross-platform CI is configured but only Windows
was executed here. Next: Stage 6 bounded scheduling and safe computation reuse.

### Stage 6

Implemented the bounded scheduler in `crates/chronos-scheduler/src/` (`graph`,
`cache`, `invalidate`, `engine`, `queue`, `budget`, `metrics`, `replay` and public
exports), with tenant fairness, global/resource-class worker caps, exact admitted
pure reuse, shared in-flight work, consumer cancellation and conservative receipt
replacement. The scheduler depends on the shared worker port rather than concrete
adapters. Graph dependencies order execution; requests bind explicit immutable
inputs and retain separate node, branch and attempt identities.

Added the facade's transactional `src/scheduler` SQLite result repository,
`tests/reuse_properties.rs`, `tests/scheduler_storage.rs`, the actual WASM fixture
support in `tests/support/scheduled_wasm.rs`, `examples/offline_replay.rs` and
`benches/scheduler.rs`. Updated manifests/lockfile, facade exports, native CI lanes,
third-party notices, README, scheduler/reuse/performance documentation and Stage 6
dependency and primary-source research records. No adapter or peer implementation
is claimed by the registered feature boundaries.

Executed separately on Windows x86_64 MSVC, all listed checks exit code 0 after
repairs:

- `cargo +1.95.0 test -p attow-scheduler --locked` — 16 unit tests passed
- `cargo +1.95.0 test --workspace --all-targets --locked -j 2`
  — 169 tests passed across 32 reported suites; log `target/stage6-default-tests.log`
- `cargo +1.95.0 test --workspace --all-targets --features scheduler --locked -j 2`
  — 197 tests passed across 38 reported suites; log `target/stage6-feature-tests-final.log`
- `cargo +1.95.0 test --features scheduler --test reuse_properties --locked`
  — 16 passed, including 32 generated DAG comparisons with reuse enabled/disabled
- `cargo +1.95.0 test --features scheduler --bench scheduler --locked -- --test`
  — four-node, one-sample smoke path for the mock CPU and actual WASM/SQLite backends
- `cargo +1.95.0 run --example offline_replay --features scheduler --locked`
  — 2 branches, 1 actual WASM evaluation, 0 replay worker calls, 0 provider calls
- `cargo +1.95.0 fmt --all --check`
- `cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings`
- `cargo +1.95.0 clippy --workspace --all-targets --features scheduler --locked -- -D warnings`
- `cargo +1.95.0 clippy --features scheduler --test reuse_properties --bench scheduler --locked -- -D warnings`
- `cargo +1.95.0 doc --workspace --no-deps --locked`
- `cargo +1.95.0 run -p xtask --locked -- verify-repository`
- `cargo +1.95.0 check -p attow-network --no-default-features --features scheduler --locked`

The first graph/key/invalidation tests recorded six failures against explicit
stubs; the two initial independent scheduler tests then failed with `InvalidLimits`
before the engine existed. Offline replay and SQLite retention checks likewise
failed on the initial stubs. Later behavioral regressions were reproduced before
repair: spare vector capacity bypassing descriptor accounting; unfair reinsertion
of ready work; prior receipt revisions losing their object pins while their trace
events remained; pure observations changing when reuse was disabled; oversized
successful repository retention responses; mismatched scheduler/replay cumulative
invalidation limits; and a replay accepting pending nodes with failed or cancelled
dependencies because they also had suspended ancestry. Tests were preserved and
rejection invariants were not weakened.

A transient feature-build `E0463` missing-rlib artifact failure cleared on retry
with `-j 2`, without source changes or artifact cleanup. It was a build failure,
not a passing test run; the final recorded workspace run completed successfully.

The independent checks cover every reuse-key field, backend admission before
cache access, denied pure reuse, non-pure classes, trapped attempts, actual
simultaneous worker caps, cross-run shared-work cancellation, dropped `drive`
futures, queue capacity, fairness, parked dependencies, subtree replacement and
explicit pin cleanup. SQLite tests verify atomic full-graph retention, rejected
partial checkpoints, actual WASM sharing followed by worker-free replay, ledger
dispatch remaining unaffected by scheduler cancellation, and historical trace
objects surviving garbage collection until `forget`.

Replay validates graph transitions, attempt/profile bindings, recorded object
hashes and references, full-key result consistency and receipt invalidation. It
has no worker or dispatch interface and consumes recorded outcomes, rather than
independently executing guest code or attesting provider truth. The example drops
the executor before replay. Trace data stays in memory until exported by the
application; SQLite durably retains request/outcome graphs and their pins, not the
entire scheduler trace.

Limits: scheduling is process-local, with bounded graphs, queues, resident
workers, history, cache entries and conservative retained-byte accounting.
Observation replacement requires a quiescent run and charges the same cumulative
1,000,000 node-plus-edge work budget used by replay before changing accepted
history. Cancelling a consumer does not interrupt arbitrary synchronous backends
or revoke ledger authority; permits remain occupied until the real worker exits.
Historical result pins remain charged until `forget`; dropping a handle is not
durable cleanup. There is no distributed scheduler, dependency projection,
e-graph optimizer, automatic durable trace export or hostile-worker OS isolation.
Native CI lanes are configured; only Windows was executed here.

The final release-profile benchmark passed with exit code 0:

```sh
cargo +1.95.0 bench -j 1 --bench scheduler --features scheduler --locked -- --nodes 8 --samples 3
```

The saved JSONL has 24 actual samples and eight recomputed percentile summaries across mock CPU and real WASM/SQLite workloads. Common-prefix graphs used five evaluations versus eight with reuse disabled; replay used zero. Peak residency stayed at two. The three-sample real WASM common-prefix median was 43.6497 ms versus 37.4871 ms without sharing, so this run demonstrates evaluation reuse, not a latency speedup. Raw data, hashes, exclusions and all percentiles are in docs/performance-method.md and docs/research/stage6-scheduler-2026-10-10.jsonl. Debug smoke data is not used as release evidence.

Next: Stage 7 owned browser/SQL adapters and the effect gateway simulator.

### Stage 7 — owned adapters and an enforceable effect boundary

Implementation is in `crates/chronos-adapters/src/{contract,storage,sql,browser,
gateway,evidence,provider_simulator}.rs`, with focused adapter tests and the facade
conformance tests. Ledger additions expose an immutable proposal, revalidate an
exact live dispatch ticket/body, and return verified durable receipt roots. The
optional browser bridge and original assets are under `demos/workflow-studio`.
Provider, browser-fidelity and adapter-security documents describe the supported
contract; original fixtures and upstream notices have provenance records. Public
names remain Attow Network and `attow-*`.

The common worker admission binds tenant, security scope, code, runtime profile,
policy, fidelity, effect class, resource class, exact budget and unsupported sample
identity. Unknown capabilities default unsupported. Adapter CAS writes retain
verified object graphs and pins atomically. Observations include capture time and
provenance. SQL snapshot workers admit only immutable, explicitly ordered reads;
browser observations remain fresh, and checkout previews carry `Model` fidelity.

SQLite snapshots use the online backup API, capture committed WAL state, and check
owner and version before restore. The intentionally small query language rejects
functions, writes, views, virtual/generated columns, floats, missing identifiers,
ambiguous ordering and unsupported syntax. Limits include a 512 KiB snapshot,
1,024 result rows, sixteen columns, a 256 KiB result and SQLite instruction fuel.
Dependencies conservatively include the entire immutable snapshot.

The browser runs Node 24.21.0, npm 11.19.0, Playwright 1.64.0 and actual Chromium
156.0.8078.4 (revision 1248) from workspace-local installations. The launcher checks
fixture source bytes, and the sidecar checks runtime versions before admission.
Supported cookies, localStorage, sessionStorage and the fixed IndexedDB schema
restore into fresh contexts and new processes. Snapshots are tenant/scope/profile
bound. Framing uses a bounded length, explicit version, monotonic request ID,
allowlisted methods and a deadline for the entire exchange. Four resident bridges
are permitted per host process; concurrent restore/observe cannot interleave.

The simulator owns its own SQLite database and loopback TCP server. Inventory,
holds, idempotent operation records, receipts, clock and failure injection survive
restart independently of the ledger/client. A private gateway authorization
borrows the active authority and exact ticket; copied tickets do not grant provider
access. Default-deny provider preflight checks the immutable body and reserved
cost before claim/send. The fixture charges one reserved unit per purchased item.
Requests and receipts bind domain, epoch, intent, attempt, provider, contract,
operation and payload hash. Keyed BLAKE3 authenticates fixture messages under a
local shared key; this is not a public attestation system.

Provider commit-before-disconnect becomes `Unknown`, preserves reserved rights
and the conflict slot, and reconciles the same operation after both sides restart.
A hidden or missing lookup remains `Unknown`. Expired idempotency retention blocks
unsafe resend, and independent providers expose partial outcomes. Frames cap at
64 KiB and share absolute two-second read/write deadlines. Server model, holds and
operation histories are bounded; safety records are never silently evicted.

The facade's `rerun_confirmed_receipt` reads the authority's exact confirmed
receipt, replaces the selected scheduler observation and reruns every affected
node. It returns a private, one-use `ConfirmedResponse` only for completed work
bound to that receipt. Its promotion method rejects a changed run, receipt,
authority or branch/code/profile/scope/policy binding before invoking ordinary
ledger validation. Four integration tests exercise an actual simulator receipt,
SQLite retention and scheduler reruns, using an explicitly labelled original
fixture interpreter for the dependent computation. This is an explicit composing
application API; it does not disable the trusted host's raw authority API or make
the scheduler and ledger one cross-component transaction.

Original failing tests were run against adapter/browser/provider stubs. Subsequent
regressions were reproduced before repair: quoted missing SQLite identifiers being
treated as string literals, slow partial frame reads/writes extending a nominal
timeout, purchase quantity exceeding reserved cost, and browser observation
envelopes exceeding their output budget. The real browser run also caught a
JavaScript restore variable-shadowing error and a service-worker probe that
mistook a resolved blocked registration for an active worker. The repaired probe
checks actual registrations/controller state and received requests. Tests were
kept intact; no canned browser or provider success substitutes for execution.
SQL review additionally reproduced pre-cache admission of malformed snapshot
envelopes and an output budget that omitted canonical object overhead. Shared
snapshot validation and full-object bounds repair those cases. The response guard
tests first failed against explicit stubs before the guarded implementation landed.

Setup commands completed successfully on native Windows x86_64 MSVC:

- `.tools/node-v24.21.0-win-x64/node.exe --version` — `v24.21.0`
- `.tools/node-v24.21.0-win-x64/node.exe .tools/node-v24.21.0-win-x64/node_modules/npm/bin/npm-cli.js --version` — `11.19.0`
- `npm.cmd ci --prefix demos/workflow-studio/browser` — two packages installed,
  lock integrity checked; npm reported zero vulnerabilities at that time
- `npm.cmd --prefix demos/workflow-studio/browser exec -- playwright install chromium`
  — completed using the absolute workspace-local `PLAYWRIGHT_BROWSERS_PATH`

The npm commands used the exact local Node directory prepended only to that command's
environment. The global Node/Rust defaults were not changed. Node archive SHA-256,
npm package integrity, browser revision and actual upstream license notices are in
`docs/research/stage7-browser-provenance.json` and `docs/notices/stage7`.

Limitations: these are owned fixtures and trusted native host components. Browser
live credentials, arbitrary navigation, hidden JavaScript stacks and hostile-site
operation are rejected. Page routes/CSP and the tested loopback network probes are
not OS egress enforcement; Chromium native RSS/CPU are not constrained by the
logical bridge budget. Snapshots are owner-scoped plaintext CAS objects, not
encrypted storage or remote-account clones. SQL query fuel does not cover backup,
materialization or integrity checking, and native SQLite memory is not an OS quota.
The host must protect local databases, serialize application workflow mutations,
and explicitly release retained pins. There is no universal provider transaction,
live payment integration, automatic compensation or distributed failover. Only
native Windows execution is evidenced here; configured CI is not a remote pass.

Final Stage 7 gates executed separately, all with exit code 0:

- `cargo +1.95.0 fmt --all --check`
- `cargo +1.95.0 test --workspace --all-targets --locked -j 2`
  — 194 tests passed across 37 suites; `target/stage7-default-tests.log`
- `cargo +1.95.0 test --workspace --all-targets --features adapters --locked -j 2`
  — 197 tests passed across 38 suites; `target/stage7-adapter-tests.log`
- `cargo +1.95.0 test --test adapter_conformance --features browser --locked -j 2`
  — four passed, including the real browser worker, exact admission and Model
  observation, plus real provider failure-window scenarios
- `cargo +1.95.0 test -p attow-adapters --all-targets --features browser --locked -j 2`
  — 32 passed across five suites, including nine real Chromium conformance cases,
  eight SQL cases, four gateway cases, two simulator persistence cases and nine
  unit checks; `target/stage7-browser-tests.log`
- `cargo +1.95.0 test --test adapter_receipt --features browser,scheduler --locked -j 2`
  — four passed: real receipt replacement/rerun/promotion, incomplete work and
  model-bound promotion rejection, stale guard rejection and exact branch binding
- `cargo +1.95.0 clippy --workspace --all-targets --locked -j 2 -- -D warnings`
- `cargo +1.95.0 clippy --workspace --all-targets --features browser,scheduler --locked -j 2 -- -D warnings`
- `cargo +1.95.0 doc --workspace --no-deps --locked -j 2`
- `cargo +1.95.0 doc --workspace --no-deps --features browser,scheduler --locked -j 2`
- `cargo +1.95.0 run -p xtask --locked -- verify-repository`

Suite totals describe each gate and overlap; they are not distinct-test totals
summed across all commands. Existing heavy ledger capacity and abrupt-crash
coverage remains enabled. The repository checker initially followed the downloaded
Node runtime's documentation and failed on its unbundled changelog links. A failing
regression led to exact exclusions for the two generated runtime roots only;
authored browser files, similarly named directories and retained notices remain
checked. All fifteen checker tests pass. Hash-preserving Git attributes cover the
owned browser fixtures and retained notices, avoiding checkout newline changes.
The final staged whitespace check identified original CRLF/trailing whitespace in
upstream license notices. Those exact copies retain their original bytes and have
narrow Git whitespace exemptions; authored source and documentation stay checked.
All 36 recorded fixture/notice hashes (21 fixtures and 15 upstream notices) were
also checked against the exact staged Git blob bytes before the local commit.

Rust direct versions are unchanged. The lockfile adds three process-support
transitives; SQLite enables hooks/limits and only the browser adapter enables
Tokio process support. Registry checksums and license metadata are recorded in
`docs/research/stage7-dependencies.json`; the exact bundled SQLite notice has its
own source/archive/extraction record. No third-party implementation was copied
into the fixtures. All runtime downloads remain ignored workspace-local inputs.

Next: Stage 8 authenticated peer exchange and explicit external fencing. No Stage
8 implementation or public repository publication is part of this checkpoint.
