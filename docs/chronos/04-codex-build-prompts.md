# Ten self-contained Codex build prompts

These prompts build the target repository, one stage at a time. The included reference kernel is already runnable; the full target workspace is not yet implemented. Each individual prompt repeats its operating contract and exact checks, so it can be submitted independently after its prerequisites. Stage 10 requires real signing identity and platform evidence before it can truthfully finish as release-ready.

# Stage 1 — Repository, architecture and reproducible green baseline

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context and objective: Start from an empty repository or the initialized research package. Create a buildable workspace whose purpose is obvious immediately. This stage implements foundation and repository checks only. Do not claim the later runtime exists.

Files: root Cargo.toml/Cargo.lock/rust-toolchain.toml; src/lib.rs, src/core/mod.rs, src/sandbox/mod.rs; crates/{chronos-codec,chronos-store,chronos-state,chronos-ledger,chronos-sandbox,chronos-scheduler,chronos-adapters,chronos-protocol,chronos-transport,chronos-cli}/Cargo.toml and src/lib.rs or main.rs; xtask; AGENTS.md; README.md; docs/{architecture,progress,threat-model,release-runbook}.md; docs/adr/0001-worldlines.md through 0005-open-core.md; licenses, CONTRIBUTING, SECURITY, GOVERNANCE, SUPPORT, CODE_OF_CONDUCT, CHANGELOG, THIRD_PARTY_NOTICES; .github CI/issue/PR templates; scripts/check.ps1 and check.sh; .cargo/config.toml; deny.toml; .gitignore. Do not invent maintainer names, reporting email addresses or badge URLs. Create referenced example/model/benchmark targets if listed in manifests.

Dependencies: stage 1 can use std only; declare future exact workspace candidates but do not activate heavy unused dependencies: blake3=1.8.7, minicbor=2.3.0, rusqlite=0.40.2, rpds=1.2.1, wasmtime=48.0.5, iroh=1.3.0, tokio=1.53.2, thiserror=2.0.21, serde=1.0.229, serde_json=1.0.151, ed25519-dalek=3.0.0, getrandom=0.4.3, zeroize=1.9.1, tempfile=3.27.0, proptest=1.11.0. The blueprint may declare additional reviewed tools; inherit only used ones. Workspace resolver=3, version=0.1.0, publish=false. Pin Rust side by side; do not change the global default.

APIs: create an initial codec `pub struct ObjectId(pub [u8;32]);` and facade re-export; define CLI main returning std Result; define xtask `fn verify_repository(root: &Path) -> Result<(), Box<dyn Error>>`. Avoid dummy public APIs that pretend to execute work. Unimplemented CLI commands must fail with an explicit unsupported message.

Create root features exactly: default=["local"]; local=["dep:chronos-store","dep:chronos-ledger"]; sandbox=["dep:chronos-sandbox"]; scheduler=["sandbox","dep:chronos-scheduler"]; adapters=["local","dep:chronos-adapters"]; browser=["adapters","chronos-adapters/browser"]; peer=["dep:chronos-protocol","dep:chronos-transport"]. Codec/state are unconditional facade dependencies, other named crates optional. Child default features are empty; adapters declares browser. Feature-gate facade re-exports. Register ledger_model and worldline_checkout with required-features=["local"]; later sandbox/scheduler/browser/peer integration targets must use matching required-features so default gates compile and execute only their declared subset. A feature is not permission to connect to the internet.

Tests first: repository checker must detect a missing license, floating direct dependency, missing manifest target, accidentally enabled publish and broken internal docs link. Fixtures for the checker are temporary miniature repositories, not mutation of the working repository. Verify CLI help truthfully reports development status.

Numbered implementation steps:
1. Inspect files and toolchain. Create AGENTS.md with invariants, test commands, change discipline, authority/effect rules, dependency policy and no public publishing. Establish the ten-stage progress checklist.
2. Describe Worldline IR, explicit continuation boundary, effect classes, local serializable authority, UNKNOWN recovery, per-domain peer model and OSS/paid boundary. ADRs must explain alternatives and why arbitrary browser state cloning/universal external atomicity are excluded from current guarantees.
3. Create the workspace and root facade. Each child inherits metadata and only needed dependencies; one-way dependencies go from codec to store/state, ledger, sandbox/scheduler/adapters, protocol/transport, CLI composition. Avoid cycles and duplicate implementations. The CLI package must declare `[[bin]] name="chronos", path="src/main.rs"`; package and executable names differ intentionally.
4. Write standard license texts and community files with honest placeholders only where owner decisions are genuinely required. Security reporting directs to a maintainer-configured private channel, not a fabricated address.
5. Write README with one-sentence purpose, ASCII architecture, implementation-status table, source cargo commands and clearly labelled planned checkout example. No live badges without CI.
6. Build checks and CI for Linux x86_64, Windows x86_64 and macOS arm64, using native runners only when available. Resolve official action releases to full commit SHAs through verified upstream metadata and record them in docs/research/ci-pins.json; do not guess SHAs. Pin toolchain exactly; workflow permissions default read-only. CI performs fmt, tests, clippy and docs, with feature lanes added as implemented.
7. Generate Cargo.lock, run the checker and all gates, then record actual local platform and unavailable remote CI execution honestly.

Non-goals: WASM engine, networking, actual adapters, sales copy claiming a shipped OS, deployment or remote repository creation. Architecture breadth must remain, with unsupported runtime behavior clearly marked.

Definition of done: all workspace targets compile, checker negative fixtures pass, documentation accurately separates implemented/planned, and these commands pass:
```sh
cargo generate-lockfile
cargo run -p xtask -- verify-repository
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 2 — Canonical objects and transactional content store

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: A worldline references immutable objects. If encoding or persistence is ambiguous, branch identity, signatures and recovery cannot be trusted. Implement codec and CAS beneath all later modules; existing stage-1 scaffold is the prerequisite.

Files: crates/chronos-codec/src/{lib,ids,encode,decode,limits}.rs; crates/chronos-store/src/{lib,sqlite,gc,migrations}.rs; crate tests; fixtures/original/protocol; docs/{canonical-codec,persistence-recovery}.md; fuzz/Cargo.toml and fuzz_targets/codec.rs in an excluded separate workspace. Register the fuzz binary explicitly.

Exact dependencies: blake3=1.8.7; minicbor=2.3.0 with alloc and no defaults; rusqlite=0.40.2 with bundled,backup; thiserror=2.0.21; tempfile=3.27.0 and proptest=1.11.0 for tests; fuzz-only libfuzzer-sys=0.4.13. Commit normal and fuzz lockfiles. No new database engine or compression dependency.

API contract:
```rust
pub struct ObjectId(pub [u8; 32]);
pub struct Limits { pub object_bytes: usize, pub depth: usize, pub items: usize }
pub trait Canonical: Sized {
    fn encode(&self) -> Result<Vec<u8>, CodecError>;
    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError>;
}
pub fn object_id(kind: u16, schema: u16, bytes: &[u8]) -> ObjectId;
pub struct Store { /* connection owner; private */ }
impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self, StoreError>;
    pub fn put(&mut self, kind: u16, schema: u16, bytes: &[u8]) -> Result<ObjectId, StoreError>;
    pub fn get(&self, id: ObjectId) -> Result<Vec<u8>, StoreError>;
    pub fn pin(&mut self, owner: &str, id: ObjectId) -> Result<(), StoreError>;
}
```
Add transaction-scoped internal operations so later ledger writes can share one connection/transaction rather than opening an independent CAS commit.

Tests first: golden canonical bytes/hash vectors; insertion-order independence where maps are semantically unordered; non-shortest integers, indefinite lengths, duplicate fields, trailing bytes and oversized nesting rejected; truncated object errors; corruption detected on get; failed transaction leaves no dangling root; concurrent pin/GC safety; reopen/migration idempotence; backup reflects committed data. Property `decode(encode(x))==x`, plus accepted bytes equal re-encoding.

Steps:
1. Define versioned CBOR array layouts with fixed tags/field counts, bounded integers, explicit byte strings, sorted keys where applicable and no floating-point authoritative keys. Domain-separate hash by kind/schema and payload length.
2. Decode incrementally with checked conversions and limits before allocation. Minicbor support alone does not imply deterministic CBOR. Make error variants stable enough for callers.
3. Use SQLite objects table `(id BLOB PRIMARY KEY, kind, schema, bytes)` and root-pin table; enable foreign keys, WAL, FULL synchronous and bounded busy handling. Hash-check imported content. Verify hash/kind/schema match on collision reuse.
4. Keep blobs inside SQLite initially for atomic references. Implement mark/sweep against a transactionally protected root set and object-reference decoder; unknown schemas are not traversed blindly. Retain objects pinned by active branches/ledger/exports.
5. Add safe online backup; never copy only a live main SQLite file. Test transaction failures before and after object insertion.
6. Register fuzz targets and an opt-in bounded CI lane; unavailable sanitizer tools are reported, not treated as passing fuzz evidence. Document that GC is not secure deletion.
7. Run gates and publish only measured fixture sizes/times if a measurement actually ran.

Licence/prior-art guardrail: content addressing, Merkle structures and deterministic encoding are established techniques. Cite BLAKE3, CBOR and SQLite documentation; original schema code is dual-licensed. Do not claim invention of CAS.

Non-goals: external packfiles, network transfer, compression, encryption scheme invention or native executable artifact deserialization.

Definition of done: byte-level compatibility fixtures checked into the repo; corruption and malformed input fail closed; root/object persistence is transactionally consistent; all gates plus:
```sh
cargo test -p chronos-codec -p chronos-store --locked
cargo run -p xtask -- verify-repository
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 3 — Persistent state, worldline identity and dependency tracking

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: codec/CAS now exist. Implement cheap logical branches without creating one process/browser/database per branch. Fork state while preserving separate execution identity and authority scope.

Files: crates/chronos-state/src/{lib,root,branch,dependency,diff,worldline}.rs; tests/state_properties.rs; fixtures/original/sql; docs/worldline-ir.md and branch semantics ADR; root src/core/mod.rs re-exports. Add a benchmark only with a truthful harness and no invented results.

Pins: rpds=1.2.1 for `HashTrieMapSync` where appropriate; existing codec; thiserror=2.0.21, proptest=1.11.0, tempfile=3.27.0. Store is a dev-dependency for integration tests only; production state defines storage interfaces without importing the store implementation. Persistent map iteration is not durable encoding. Canonicalize durable nodes separately. No new networking or browser dependency.

Type/function contract:
```rust
pub struct DomainId(pub [u8;32]);
pub struct BranchId(pub [u8;32]);
pub struct Version(pub u64);
pub struct StateRoot { /* immutable */ }
pub struct Branch { /* domain, base, reads, overlay, continuation */ }
pub enum Dependency { Key { key: Vec<u8>, version: Version }, Namespace { epoch: u64 } }
pub enum Mutation { Put(Vec<u8>), Delete }
pub fn fork(domain: DomainId, root: &StateRoot, nonce: [u8;32]) -> Result<Branch, StateError>;
impl Branch {
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError>;
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StateError>;
    pub fn delete(&mut self, key: &[u8]) -> Result<(), StateError>;
    pub fn scan_prefix(&mut self, prefix: &[u8]) -> Result<Vec<(Vec<u8>,Vec<u8>)>, StateError>;
    pub fn freeze(self) -> Result<FrozenBranch, StateError>;
}
```
Define FrozenBranch with immutable base root/sequence, canonical overlay, read dependencies, continuation/policy/binding roots, code/profile/schema/security-scope/ancestry references and branch/domain identity. Expose no authority-spending method from the branch API. Define `chronos-state::execution::WorkerBackend: Send + Sync` as a standard-library-only, object-safe interface with `fn resource_class(&self) -> ResourceClass` and `fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError>`. Add materialize/suspend/release operations using opaque backend handles as needed; a handle is not a portable checkpoint. Serializable StepRequest/StepOutcome envelopes live in codec. Sandbox and adapters implement this trait; scheduler receives an injected registry. Blocking operations run on bounded worker threads, never a Tokio reactor thread.

Tests first: sibling/parent isolation; get observes own writes; absent read tracked; blind write records base version; absent-present-delete ABA detectable; scan includes overlay/deletes and marks namespace dependency; state roots stable across insertion order; branch IDs differ for distinct nonces even if state equal; domain mismatch rejected; unsupported schema rejected; active roots survive GC. Generated operation traces compare with a simple BTreeMap model.

Steps:
1. Define immutable cell values with monotonically assigned authoritative versions and tombstones. Zero means never-seen absence only; a deletion retains a version.
2. Fork by sharing roots and allocating a small descriptor/overlay. First-use root loading may cost O(state size) until a durable tree implementation exists; document exact complexity.
3. Track every semantic read, including nonexistence and predicate queries. Use whole-namespace epoch for scans initially. A write observes the expected base version even without an earlier explicit get.
4. Freeze into canonical data; verify all referenced objects exist and are pinned. Encode branch identity separately from state equality. Cap keys, values, dependency counts and graph depth.
5. Provide structural diff and explicit conflict reports. A join returns compatible proposals or conflicts; it never authorizes external effects or claims that deterministic merging implies serializability.
6. Persist/reopen roots with same domain identity. Import into a different domain requires an explicit data-only conversion, not preserving source authority.
7. Add logical-fork benchmark reporting separately from durable admission and materialization, then run all gates. Do not insert a “submillisecond” assertion into CI.

Licence/prior-art: cite persistent data structures, Concurrent Revisions and branchable state research. Structural sharing is prior art. Copying an rpds collection is not a novel algorithm or complete distributed snapshot.

Non-goals: arbitrary native heap/stack clone, physical SQL COW, copied spendable rights, general CRDT merge and automatic live browser state restoration.

Definition of done: branch model tests pass, all read dependencies appear in frozen output, no shared mutable branch state, and:
```sh
cargo test -p chronos-state --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 4 — Serializable promotion, atomic ledger and durable effect outbox

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: immutable branches exist. Implement the single authoritative local state transition that decides which speculative effects may become real. This is the correctness center of the MVP. Five agents proposing one seat must yield one unresolved authorized operation, not five attempted purchases.

Files: crates/chronos-ledger/src/{lib,schema,promotion,reducer,effects,recovery,verify}.rs; migrations; tests/{promotion,storage_crash,replay}.rs; model/ledger.rs explicitly registered as a Cargo test; model/assumptions.md; docs/{commit-domains,effect-lifecycle,persistence-recovery}.md; examples/worldline_checkout.rs.

Pins: existing codec/store/state; rusqlite=0.40.2 bundled,backup; blake3=1.8.7; thiserror=2.0.21; stateright=0.31.0, proptest=1.11.0, tempfile=3.27.0 for verification. No homemade consensus dependency.

API:
```rust
pub struct PromotionRequest { pub id: ObjectId, pub payload: ObjectId, pub branch: FrozenBranch, pub decision: ObjectId, pub generation: u64 }
pub enum EffectState { Authorized, Dispatching, Unknown, Confirmed, DefinitivelyRejected, CancelledBeforeSend }
pub enum PromotionOutcome { LocalCommitted { seq:u64 }, AcceptedPending { seq:u64, intent:ObjectId }, Conflict(ConflictReport) }
impl Authority {
    pub fn open(path:&Path) -> Result<Self, LedgerError>;
    pub fn promote(&mut self, req:PromotionRequest) -> Result<PromotionOutcome, LedgerError>;
    pub fn claim_effect(&mut self, id:ObjectId) -> Result<DispatchTicket, LedgerError>;
    pub fn record_outcome(&mut self, ticket:DispatchTicket, evidence:VerifiedEvidence) -> Result<(), LedgerError>;
    pub fn rebuild_and_verify(&self) -> Result<VerificationReport, LedgerError>;
}
```
VerifiedEvidence is produced by an adapter contract, not arbitrary agent text. The ledger still validates intent identity/payload and legal transitions. Keep the pure reducer free of network and wall-clock calls.

Tests first: two independent DB connections race same decision/slot; exactly one AcceptedPending, loser writes absent; write-skew rejection; missing-key ABA; scan phantom; different-authority branch rejection; duplicate promotion same ID/same payload returns byte-equivalent stored result; same ID/different payload rejects; unknown holds slot; cancellation-before-send races are synchronized; replay makes zero provider calls; tampering with materialized state is caught by replay comparison. Child-process failpoints before/after each transaction boundary must test real abrupt process termination, not just graceful close/reopen.

Steps:
1. Create persistent random authority identity and schema migration transaction. Define decision/generation, rights/budgets, promotion dedup, immutable intents, outbox, outcome evidence, events and state tables with constraints.
2. Under BEGIN IMMEDIATE validate domain, policy, request dedup, all key/write versions, scan epoch, business invariants and exclusive logical slots. The active semantic slot is keyed independently of decision generation; a new generation cannot bypass Unknown/Dispatching or an unresolved confirmed booking replacement. Either reject or reserve rights, apply pending managed transition, append canonical event, insert outbox and save response atomically.
3. Commit before any dispatch. Never mark provider-confirmed truth from speculative state. Use stable semantic operation IDs across retries; branch ID must not be the idempotency scope.
4. Persist Authorized->Dispatching before the dispatcher can send. On uncertain restart mark potentially sent effects Unknown. Keep unresolved rights and slots reserved. Persist every outcome transition as a ledger event as well as updating indexes.
5. Implement pure event replay into a fresh database and compare state/effect roots, sequence and trusted head. Hash-chain checks alone are insufficient. Use consistent read transactions for verification under concurrent writers.
6. Model two branches, one slot, crashes, duplicate replies and ambiguous provider success. Assert durable authority before send, single unresolved operation, no dispatch during replay and conservation of rights. Report explored bounds/counts as model evidence only.
7. Build checkout example that emits predicted/pending/confirmed separately using a local provider simulator in later stages; at this stage zero external calls. Document startup/recovery and schema upgrade rollback strategy.

Licence/prior-art: OCC, transactional outbox, idempotency and consensus are established. Cite SQLite isolation and existing transaction/replay work; no universal exactly-once or atomic third-party API claim.

Non-goals: distributed leader election, automatic unknown-as-failed conversion, arbitrary compensation as time reversal, provider purchase execution.

Definition of done: race, model, duplicate, crash and replay tests pass; no split durability boundary; exact commands:
```sh
cargo test --test ledger_model --locked
cargo test --test storage_crash --locked
cargo run --example worldline_checkout --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 5 — Constrained WASM execution and portable continuations

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: the kernel can persist state and promote proposals. Add a bounded compute backend with explicit step checkpoints. A checkpoint is data in the Worldline IR; it is not a promise to clone arbitrary live Wasmtime Stores.

Files: crates/chronos-sandbox/src/{lib,profile,admission,worker,host,continuation}.rs; wit/chronos-v1/{types,observation,effects,continuation,world}.wit; fixtures/original/wasm; sandbox tests; docs/{determinism-profile,continuation-abi}.md. Record profile hash in every evaluation.

Pins/features: wasmtime={version="=48.0.5",default-features=false,features=["std","runtime","cranelift","component-model","wat","anyhow","pooling-allocator"]}; existing codec/state; anyhow=1.0.104, thiserror=2.0.21, tempfile=3.27.0. Do not add wasmtime-wasi automatically. This compatibility lane was locally probed on Rust1.95.0. Wasmtime49.0.2 requires a newer compiler. Version48 TypedFunc::call performs post-return automatically; do not call the deprecated no-op post_return. A trapped Store may be unusable for subsequent component entry: discard it.

API contract:
```rust
pub struct ExecutionProfile { pub fuel:u64, pub memory_bytes:usize, pub output_bytes:usize, pub profile_hash:ObjectId }
pub struct StepInput { pub code:ObjectId, pub state:ObjectId, pub continuation:ObjectId, pub observation:Option<ObjectId> }
pub enum StepExit { Done { result:ObjectId, delta:ObjectId }, Suspended { continuation:ObjectId, request:ObjectId, delta:ObjectId } }
pub struct Executor { /* shared Engine/code cache; no shared guest state */ }
impl Executor {
    pub fn admit(&self, wasm:&[u8]) -> Result<AdmittedCode, SandboxError>;
    pub fn run_step(&self, code:&AdmittedCode, input:StepInput, profile:&ExecutionProfile) -> Result<StepExit, SandboxError>;
}
```
Define the WIT record/variant equivalents and a small bounded typed import/export boundary. If code validation needs wasmparser, first resolve the version already used by Wasmtime48 from Cargo.lock, pin that exact version, check licence and document it; do not guess a parser API or leave it floating.

Tests first: arithmetic component; explicit suspend/resume across a new process; sibling continuation isolation; unknown import denied; network/clock/random environment access absent; infinite loop fuel trap; oversized memory/output rejection; malformed WASM; hidden mutable-global reliance rejected by admitted contract or caught by fresh-step replay; recorded input replay hash equality; trapped worker discarded before next attempt. Add Windows and Linux fixtures without assuming memory_init_cow behavior matches.

Steps:
1. Define supported WASM/component subset and continuation schema. No in-flight import or unexported mutable program state may be needed after a checkpoint. Resume reconstructs code/state from explicit data.
2. Configure consume_fuel, canonical NaNs, SIMD/relaxed-SIMD exclusion and selected component features. Do not call wasm_threads(false) when the compile feature that exposes it is absent. Validate admitted binaries accordingly.
3. Fix/preallocate guest memory and bound tables/instances. StoreLimits memory_size is per memory; calculate aggregate worker limits separately. Bound host-call output and compilation resources. Emergency host termination yields incomplete attempt, never a committed semantic result.
4. Create a fresh Store for each step/attempt; share only immutable compiled code and Engine. Persist a suspended continuation before releasing resources. No generic Rust Future serialization.
5. Link only typed mediated state/observation/effect interfaces. Requests return data to the kernel; they do not get sockets or credentials. Supply time/randomness as explicit recorded values when requested.
6. Keep untrusted peers' compiled native artifacts out of unsafe deserialization. Compile validated WASM locally. Implement process isolation for hostile execution or honestly limit the first support level until that boundary is tested.
7. Document deterministic-profile scope, runtime upgrade replay policy and all measured overhead separately from fork metadata cost.

Licence/prior-art: Wasmtime uses Apache-2.0 WITH LLVM-exception; preserve notices. Continuations, deterministic replay and WASM isolation are established. Source comparison is research, not permission to copy incompatible implementation code.

Non-goals: arbitrary process checkpoint, browser clone, hardware-independent wall-clock determinism, unrestricted WASI and remote precompiled artifact loading.

Definition of done: real component and suspend/resume tests pass with no external access; exact commands:
```sh
cargo test -p chronos-sandbox --locked
cargo test --workspace --all-targets --features sandbox --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 6 — Worldline scheduler, shared evaluation and exact replay

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: state/ledger and WASM steps work. Implement the category-defining graph scheduler: many logical alternatives, a bounded resident pool and shared pure work. Do not claim a million resident agents from a large descriptor count.

Files: crates/chronos-scheduler/src/{lib,graph,queue,budget,cache,invalidate,replay,metrics}.rs; tests/reuse_properties.rs; benches/worldline.rs; docs/{scheduler,reuse-contract,performance-method}.md; examples/offline_replay.rs.

Pins: tokio=1.53.2, default-features=false, features rt-multi-thread,macros,sync,time; codec/state dependencies already pinned; sandbox/ledger are integration-test dev-dependencies only; proptest=1.11.0, loom=0.7.2 and criterion=0.8.2 in verification only. Do not make network or ML inference a hidden dependency of pure scheduling.

API:
```rust
pub enum EffectClass { Pure, ManagedRead, ManagedWrite, RecordedObservation, FreshObservation, PreparedExternal, IrreversibleExternal }
pub struct ScheduleLimits { pub resident_workers:usize, pub queued_branches:usize, pub retained_bytes:u64 }
pub struct EvaluationKey { pub tenant:ObjectId, pub code:ObjectId, pub profile:ObjectId, pub input_root:ObjectId, pub continuation:ObjectId, pub observation_root:ObjectId, pub sample_id:Option<ObjectId> }
impl Scheduler {
    pub fn submit(&self, graph:WorldlineGraph) -> Result<RunId, ScheduleError>;
    pub async fn drive(&self, run:RunId) -> Result<RunReport, ScheduleError>;
    pub async fn cancel(&self, run:RunId) -> Result<CancelReport, ScheduleError>;
}
pub fn replay(trace:&ExecutionTrace, inputs:&RecordedInputs) -> Result<ReplayReport, ReplayError>;
```
Keep semantic events separate from tracing/telemetry. Inject an `Arc<dyn chronos_state::execution::WorkerBackend>` registry keyed by resource class; the scheduler imports neither adapters nor concrete browser/SQL execution. Sandbox/adapters implement the shared trait. The scheduler returns proposals to the composing application; only the ledger authorizes them.

Tests first: identical pure nodes execute once while retaining two branch identities; changing any full reuse-key input forces recompute; different tenants do not share secret-dependent results; independent model samples remain distinct; fresh observations never hit a pure cache; resident count never exceeds limit; queued task cancellation releases resources; cancellation cannot revoke an already dispatched effect; incomplete/trapped attempts are not cached; optimized and unoptimized generated graphs return same outputs; replay has zero external calls.

Steps:
1. Represent dependency DAG with cycle rejection, bounded fanout/depth, explicit resource class and adapter fidelity. Give every stochastic sample its own identity.
2. Implement tenant-fair bounded ready queues and semaphores for CPU/browser/SQL classes. A waiting receipt suspends the task rather than holding a busy worker. Backpressure applies before memory allocation.
3. Implement exact common-subexpression reuse for admitted pure nodes using whole immutable input roots and all configuration/security fields. Deduplicate concurrent identical evaluations with a single-flight entry; cancellation of one consumer must not corrupt another consumer's work.
4. Cache completed validated results only; pin reachable objects. Preserve each world's history/authority even if a result blob is shared. Never coalesce business intent identities based solely on equal arguments.
5. Record accepted schedule decisions, observations and result roots. Implement deterministic replay driver consuming those inputs and never dispatching effects.
6. Invalidate descendants when an observation is replaced by a live receipt. Speculative response-dependent nodes must be rerun against actual evidence. Begin with conservative subtree invalidation; optimize dependency projection only with additional proof/tests.
7. Add bounded benchmark scenarios for common prefixes, no sharing, high divergence and replay. Emit JSON raw samples with machine/OS/code/lock/profile hashes, p50/p95/p99 and sample count. Do not turn aspirational throughput into tests.

Licence/prior-art: cite Nominal Adapton, build-system caching and equality-saturation research. Exact memoization is not a newly invented foundation. Document the proposed effect-indexed graph optimization as an experiment until demonstrated.

Non-goals: generic symbolic optimization of arbitrary websites, e-graph rewriting of irreversible actions, deduplicating independent LLM randomness, blanket coordination-free merging.

Definition of done: reuse differential tests and resource bounds pass, both shared/unshared benchmarks produce honest raw output, and:
```sh
cargo test -p chronos-scheduler --locked
cargo test --workspace --all-targets --features scheduler --locked
cargo run --example offline_replay --features scheduler --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 7 — Browser and SQL adapters with an enforceable effect boundary

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: graph scheduling is operational. Add heterogeneous execution through a common contract. This stage demonstrates breadth across local browser, database and WASM without claiming that remote SaaS state can be cloned.

Files: crates/chronos-adapters/src/{lib,contract,sql,browser,gateway,provider_simulator,evidence}.rs; tests/adapter_conformance.rs; demos/workflow-studio/app (owned local fixture), browser/{package.json,package-lock.json,sidecar.mjs}; fixtures/original/{sql,browser,effects}; docs/{provider-contract,browser-fidelity,adapter-security}.md; browser CI lane.

Pins: existing rusqlite=0.40.2 bundled,backup; tokio=1.53.2 add process/io-util only if used; serde=1.0.229 derive; serde_json=1.0.151. Browser sidecar Node24.21.0 LTS and playwright=1.64.0 exact with npm lockfile/browser revision. Registry metadata was checked 2026-10-09; verify official archives/integrities at setup. Do not install browser globally or execute downloaded shell scripts. Kernel stays Rust; the optional browser bridge is an adapter process, not kernel code.

API:
```rust
pub enum Fidelity { RecordedReplay, OwnedEnvironment, Model, Live }
pub struct AdapterCapabilities { pub fidelity:Fidelity, pub snapshot:bool, pub restore:bool, pub isolated_remote_state:bool, pub idempotent_effects:bool }
pub trait Adapter {
    fn capabilities(&self) -> AdapterCapabilities;
    fn snapshot(&mut self) -> Result<ObjectId, AdapterError>;
    fn restore(&mut self, root:ObjectId) -> Result<(), AdapterError>;
    fn observe(&mut self, request:ObservationRequest) -> Result<Observation, AdapterError>;
}
pub trait EffectProvider {
    fn dispatch(&mut self, ticket:&DispatchTicket) -> Result<ProviderEvidence, ProviderError>;
    fn reconcile(&mut self, id:ObjectId) -> Result<ProviderEvidence, ProviderError>;
}
```
Use bounded length-prefixed sidecar messages with request ID and method allowlist. EffectProvider implementations are callable only through a gateway holding a valid durable ticket; speculative workers never hold purchase credentials.

Tests first: cookies/localStorage/IndexedDB/sessionStorage individually tested against supported adapter behavior; browser context separation; unsupported hidden JS stack restore rejected; service-worker and direct network bypass attempts blocked by declared containment or explicitly unsupported in fixture-only mode; SQLite backup includes WAL commits; SQL nondeterministic functions/unsupported writes rejected; five checkout branches produce one gateway intent; provider accepted-but-response-lost becomes Unknown then reconciles to Confirmed without new logical purchase; delayed NotFound cannot be treated as definitive rejection; expired idempotency retention blocks unsafe retry.

Steps:
1. Define typed adapter manifest with version, fidelity, supported storage, deterministic subset, effect semantics and limits. Unknown capabilities default unsupported. Store observations with provenance and capture time. Implement the WorkerBackend trait owned by chronos-state for speculative execution; keep EffectProvider/gateway separate so the scheduler cannot dispatch irreversible effects.
2. Implement owned SQLite snapshot via online backup for physical materialization, or managed logical root for the supported subset. Do not copy a live database main file. Track query dependencies conservatively; reject unsupported SQL rather than invent read sets.
3. Launch optional browser sidecar against only the owned loopback fixture by default. Snapshot supported storage into encrypted/owner-scoped references. Session storage requires explicit handling; DOMSnapshot is not stack checkpointing. Pin browser executable revision.
4. Build a provider simulator whose server owns inventory/holds, idempotency state and receipts independently from the client. Inject commit-before-response loss, duplicate requests, hold expiry, rejection and partial multi-provider outcome. The simulator is an original fixture, not a real airline integration.
5. Gateway verifies payload hash, domain, rights, provider contract and authorization before send. Keep durable ticket/state transitions atomic with ledger. No generic HTTP buffering that supplies invented real success to dependent computations.
6. Define response binding: modelled replies carry Model fidelity; actual receipt invalidates dependent graph nodes and reruns them before authoritative dependent transitions. Reconciliation is a separate authorized observation.
7. Document OS-level egress enforcement for any supported hostile browser deployment. If not implemented/tested on a platform, keep mode fixture-only and fail closed for live credentials. Cookie copying never implies independent remote accounts.

Licence/prior-art: retain Playwright/Chromium and SQLite notices; third-party fixtures need source commit, hash and licence. Browser automation, backups and compensation are prior art. No claim that an intercepted click is a distributed transaction.

Non-goals: real purchases, arbitrary websites, CAPTCHA evasion, universal browser snapshot, reversible payment or cloud credentials.

Definition of done: Rust and real owned-browser conformance run, simulator failure windows reconcile correctly, network scope is verified, and:
```sh
node --version
npm ci --prefix demos/workflow-studio/browser
npm --prefix demos/workflow-studio/browser exec -- playwright install chromium
cargo test --test adapter_conformance --features browser --locked
cargo test --workspace --all-targets --features adapters --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 8 — Authenticated QUIC object exchange and domain authority

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: a local broad vertical slice works. Add local-first federation: peers exchange immutable worlds and send proposals to the designated authority. This stage does not manufacture consensus from QUIC.

Files: crates/chronos-protocol/src/{lib,frame,messages,version,limits}.rs; crates/chronos-transport/src/{lib,endpoint,session,objects,authority}.rs; tests/peer_protocol.rs; fixtures/original/protocol; fuzz_targets/frame.rs and worldline_import.rs; docs/{protocol-v1,peer-trust,authority-handoff}.md; deploy/relay example marked optional.

Pins: iroh=1.3.0 default-features=false features tls-ring; tokio=1.53.2 rt-multi-thread,macros,sync,time,io-util; ed25519-dalek=3.0.0; getrandom=0.4.3; zeroize=1.9.1; existing codec/protocol; ledger is an integration-test dev-dependency only; test tempfile=3.27.0/proptest=1.11.0. Iroh uses noq internally; do not introduce Quinn as if its types were interchangeable. No Raft crate in v0.1.

API:
```rust
pub enum Message { Hello(Hello), OfferObjects(Vec<ObjectId>), WantObjects(Vec<ObjectId>), ObjectChunk(Chunk), BranchProposal(Proposal), PromotionResult(ResultMessage), SignedCheckpoint(Checkpoint), Ack(Ack), ProtocolError(ErrorMessage) }
pub fn encode_frame(msg:&Message) -> Result<Vec<u8>, ProtocolError>;
pub fn decode_frame(bytes:&[u8], limits:&ProtocolLimits) -> Result<Message, ProtocolError>;
impl PeerNode {
    pub async fn bind(config:PeerConfig) -> Result<Self, TransportError>;
    pub async fn sync_objects(&self, peer:PeerAddress, roots:&[ObjectId]) -> Result<SyncReport, TransportError>;
    pub async fn propose(&self, authority:PeerAddress, request:ProposalEnvelope) -> Result<PromotionReply, TransportError>;
}
```
ProposalEnvelope and PromotionReply are bounded serializable wire DTOs owned by codec/protocol. Inject an application callback at peer service composition; CLI validates/converts proposals and invokes the ledger. Transport must not open the ledger or import its concrete implementation. Local-only identity creation is outside deterministic replay. Use strict individual signature verification and domain-separated signed bytes.

Tests first: real two-endpoint loopback transfer; wrong ALPN/version/peer authorization denied; oversized/truncated/unknown frame rejected before allocation; corrupted object denied; missing dependencies stay unavailable; duplicate same proposal returns original outcome; same request ID/different payload rejected; offline peer can compute but cannot promote conflicting state; no second writer on timeout; stale epoch/checkpoint replay rejected; fair control progress during bulk transfer; importer never executes downloaded code.

Steps:
1. Define `chronos/1` ALPN and big-endian u32 payload length plus u8 type. Initial caps:1MiB control,64KiB chunks,64MiB object,32 streams/peer and configurable queued-byte quotas. Validate before allocation and enforce deadlines.
2. Use Iroh Minimal preset with explicit loopback/private addresses. No N0 defaults/public discovery in tests. Verified API shape: EndpointAddr::new(id).with_ip_addr(address) or with_addrs(TransportAddr iterator), not with_ip_addrs. Initiator writes before waiting for a response so remote stream acceptance progresses.
3. Transfer content-addressed objects with integrity, dependency and schema validation. Transport completion is not durable application acceptance. Acknowledge durability explicitly after transaction commit.
4. Persist domain writer identity and authority epoch. Only that writer accepts promotions. Never let a peer become writer based on heartbeat timeout. Static authority is the released mode; proposal data can replicate freely. Imported/restored databases open replay/read-only until explicit activation fences the former dispatcher and proves current epoch. Keep activation credentials outside exported snapshots; two copies of an authority ID must not both dispatch.
5. Specify explicit handoff: quiesce admission/dispatch, resolve or preserve in-flight intents, copy durable state, old writer persists closed epoch, authorize new writer at next epoch. If provider cannot fence old in-flight requests, retain their operation IDs and unknown slots. Do not claim crash-available automatic handoff; consensus is a future mode.
6. Keep all state-changing messages out of 0-RTT. Authenticate peer, then authorize tenant/domain/object access. Redact credentials from peer exports and logs.
7. Implement malformed input fuzz harness and fault simulation at protocol abstraction. Do not claim a deterministic simulator exercises real QUIC internals. Run actual loopback integration separately.

Licence/prior-art: retain Iroh/crypto notices; consensus, content gossip and signatures are established. A signed checkpoint proves attribution under trust assumptions, not the truth of remote observations or Byzantine consensus.

Non-goals: homegrown Raft, public relay use by default, BFT, global total ordering, universal offline irreversible commits.

Definition of done: real private QUIC and application-level failure tests pass with no public service use, and:
```sh
cargo test -p chronos-protocol -p chronos-transport --locked
cargo test --test peer_protocol --features peer --locked
cargo test --workspace --all-targets --features peer --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 9 — Integrated CLI, inspectable demo and adversarial release gates

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: all major modules exist. Connect them into a product a developer can install locally and understand in minutes. Demonstrate a common worldline spanning WASM, owned browser, SQLite and two peers, with truthful effect boundaries.

Files: crates/chronos-cli/src/{main,args,commands,display}.rs; root facade/tests; demos/workflow-studio/{README,expected,scenarios}; scripts/check.ps1/check.sh; docs/{getting-started,troubleshooting,operations,performance-method}.md; xtask command modules; CI feature matrix; README fully rewritten around actual implemented behavior.

Pins: use existing workspace dependencies; std CLI parser is sufficient. serde=1.0.229 and serde_json=1.0.151 for stable diagnostics; no new framework without verified exact pin. Browser Node24.21.0/playwright1.64.0 unchanged. Optional test tooling already researched: cargo-nextest0.9.148, cargo-deny0.20.2, cargo-llvm-cov0.9.1; install only needed tools using --locked and record actual tool versions.

CLI contract:
```text
chronos init PATH
chronos run SCENARIO --workers N --json
chronos branch list --json
chronos compare BRANCH_A BRANCH_B --json
chronos promote BRANCH --decision DECISION --generation N --request-id ID
chronos replay RUN --offline --json
chronos verify RUN --json
chronos doctor --json
chronos peer serve --bind 127.0.0.1:0
chronos export RUN --output PATH
chronos import PATH --data-only
```
Define `fn run(args: impl Iterator<Item=OsString>) -> Result<ExitCode,CliError>` or equivalent testable dispatch; output schema version1 and stable exit categories success/conflict/unknown/input/infrastructure. Avoid printing cookies/tokens in errors. Import never grants source authority.

Tests first: actual installed binary runs offline fixture from a directory containing spaces; clean-machine source install; malformed flags; export/import round trip without credentials; path traversal rejection; doctor detects missing optional browser; no-progress receipt remains visibly unknown; process interruption resumes same logical intent; CLI replay with network disabled produces same managed root; no unsafe force flag bypasses authorization.

Steps:
1. Compose crate APIs, not duplicate algorithms in CLI. Show Model/Recorded/Live evidence and pending/confirmed/unknown separately. Help and exit semantics must allow automation to distinguish uncertainty from failure.
2. Create multi-branch checkout and a non-commerce data-change scenario with original owned fixtures. Both exercise common graph semantics. Add server-simulated lost response to the checkout demonstration.
3. Build local inspector output: graph IDs, common nodes, changed values, dependencies, proposed effects and evidence links. Static HTML export is acceptable; no remote telemetry, invented charts or fabricated execution histories.
4. Implement xtask commands `verify-repository`, `demo --record-json PATH`, `release-check --local`, `install-smoke --root PATH` and `bench-report --output PATH`. Each returns nonzero on failed checks. Demo recording captures actual events and output, with declared fixture mode.
5. Run feature matrix: no-default-features, local, sandbox, scheduler, adapters, browser, peer and supported combinations. Provision Node24.21.0, `npm ci --prefix demos/workflow-studio/browser` and `npm --prefix demos/workflow-studio/browser exec -- playwright install chromium` before all-features or dedicated browser gates. Browser tests have a real pinned browser lane; missing prerequisites fail that lane rather than silently skipping. Default offline gates must not require the optional browser because the targets are feature-gated.
6. Run bounded hostile-input and crash suite; produce a verification matrix mapping every public guarantee to a test or an explicit unsupported status. Check policy against final Cargo.lock, preserving necessary transitive licence exceptions with reasons.
7. Rewrite installation, example and README against the real CLI. Verify commands in a clean temporary install root and record OS-specific prerequisite differences. Publish raw benchmark data only from executed measurements.

Licence/prior-art: every copied fixture records URL, immutable commit/version, SHA256 and licence. Original examples should be dual-licensed. Do not claim a novel optimization because the demo calls existing fork/replay APIs.

Non-goals: public website launch, GitHub push, live commerce, paid provider use, fake customer logos or certifications.

Definition of done: full local vertical slice works, supported CI lanes are configured, actual CLI install/demo/replay gates pass and unavailable platforms remain unchecked rather than advertised:
```sh
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p xtask -- demo --record-json target/demo/events.json
cargo run -p xtask -- install-smoke --root target/install-smoke
cargo run -p xtask -- release-check --local
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

# Stage 10 — Verifiable release candidate, signed artifacts and recorded launch demo

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: the integrated prototype and supported verification matrix are green. Produce a concrete release candidate with real artifacts, working installation, an actual recorded demonstration and an auditable launch checklist. Release-ready is earned by gates; the prompt does not authorize public publishing or invent signing credentials.

Files: .github/workflows/release.yml; dist configuration if used; docs/release-runbook.md; docs/launch-checklist.md; CHANGELOG; SECURITY/SUPPORT final placeholders inventory; release-toolchain.json; xtask release/sign/verify/demo-record modules; scripts/verify-artifacts.ps1 and .sh; release-candidate output manifest, platform archives, SHA256SUMS, SBOM, provenance, signatures and demo recording. Never commit private keys or embed signing tokens in logs.

Exact candidate tools: cargo-dist0.32.0, cargo-cyclonedx0.5.9, cargo-deny0.20.2, cargo-semver-checks0.51.0; Cosign3.1.3 from its official release. Node24.21.0/playwright1.64.0 can record the owned browser demo as WebM; alternatively produce a real terminal recording and its replayable local viewer without inventing screen frames. Resolve official tool binaries/checksums and record them; exact version is not sufficient without artifact integrity. Do not install tools irrelevant to the chosen release flow.

APIs/commands to implement in xtask:
```text
cargo run -p xtask -- release-plan
cargo run -p xtask -- release-build --target TARGET --output release-candidate
cargo run -p xtask -- release-verify --dir release-candidate --integrity-only
cargo run -p xtask -- release-verify --dir release-candidate --require-signatures
cargo run -p xtask -- demo-record --output release-candidate/demo
cargo run -p xtask -- release-sign --dir release-candidate --mode configured
```
Define a versioned ReleaseManifest containing source revision/dirty status, target triple, rustc verbose output, lock SHA256, runtime profile hash, file hashes, build checks, expected signing identity and supported-feature table. Artifact files must exist and hashes must match before inclusion. Freeze the manifest payload before signing; signature bundles are detached and excluded from its own file-hash list to avoid circular hashing. `release-verify --integrity-only` checks content/install evidence and clearly reports unsigned status; `--require-signatures` additionally requires authentic expected-identity signatures and is the only readiness gate. Missing signatures must fail the latter. Provision the pinned browser sidecar/browser before the all-features and recorded-demo gates; ordinary default offline checks remain independent of that optional installation.

Tests first: altered archive/checksum/signature rejected; verification with wrong signing identity fails; missing platform build cannot appear as supported; missing credentials cannot be reported as signed; archive path traversal denied; installed binary from archive performs offline demo/replay; source install matches documented CLI; demo recording is nonempty and corresponds to actual event log; source archive excludes secrets, build cache and local credentials.

Steps:
1. Freeze a release candidate version and inventory every incomplete guarantee. Confirm name/package availability before registry publication; keep working title if unresolved. Record no first-ever claim, users, certifications or performance without evidence. Do not silently downgrade a failed safety gate to a known issue.
2. Build each advertised target on a suitable native CI runner or validated cross toolchain. Record runners and compiler versions. Produce only artifacts actually built/tested. A passing local Windows build is not evidence of Linux/macOS support. Reproducibility checks compare independent builds where feasible; do not claim bitwise reproducibility from merely pinning dependencies.
3. Generate platform archives, checksums, source archive, SBOM from resolved dependencies, third-party notices and machine-readable provenance. Dist tooling is optional but configuration/output must be inspected rather than accepted blindly.
4. Install each candidate into a clean location, run --help, owned demo, offline replay and verification. Test shell and PowerShell install documentation. Do not advertise `cargo install chronosos` unless a real package has been published; source installation uses the local crates/chronos-cli path and --locked.
5. Record a real demonstration: objective -> shared branches across browser/SQL/WASM -> compare -> one selected intent -> simulated response loss -> Unknown -> provider reconciliation -> Confirmed -> offline replay. Label simulated providers and actual branch/worker counts onscreen. Include raw event log and video/terminal recording; no fabricated speedup captions.
6. Sign with a maintainer-authorized existing KMS/key or configured CI OIDC identity and pin expected verification identity/issuer. Official Cosign shape is `cosign sign-blob FILE --bundle FILE.sigstore.json`; verify with version-matched help and explicit trust identity. Do not silently use a generated throwaway key as maintainer identity or publish to transparency services without the release configuration's authorization. If credentials are absent, complete all other work, leave an explicit unsigned candidate and a pending signing gate. Do not mark the stage release-ready until configured real signing and verification pass.
7. Wire protected release CI with minimal permissions, pinned action commits, no privileged signing on untrusted pull requests, and immutable source-to-artifact traceability. Prepare draft release notes locally. Public publish/push remains a separate explicit action.
8. Complete launch checklist: supported platform results; install/recovery tests; security reporting; licence notices; name review; unresolved provider limitations; reproducible demo; authentic signatures; maintainer/recovery ownership; rollback and compatibility policy. Record actual failures and pending external configuration.

Licence/prior-art: include all dependencies' distribution obligations. MIT/Apache licensing does not establish freedom to operate or name availability. Candidate patent mechanisms require a claim-level search; no patent badges or legal-clearance language.

Non-goals: registry publication, repository push, customer outreach, fabricated signed artifacts, fake notarization/certification or promises of million-agent throughput without a real test.

Definition of done: signed, independently verifiable release artifacts for every advertised platform; working source/archive install; real recorded demo; complete launch checklist; all exact local gates plus real configured signing. Without credentials, report `candidate-complete; signing-pending`, not `release-ready`.
```sh
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p xtask -- release-plan
cargo run -p xtask -- release-check --local
cargo run -p xtask -- demo-record --output release-candidate/demo
cargo run -p xtask -- release-verify --dir release-candidate --integrity-only
```
Run `release-build --target` for each actual release target and `release-sign --mode configured` only against the verified, frozen candidate. Run `cargo run -p xtask -- release-verify --dir release-candidate --require-signatures` after signing; a hash change invalidates the previous signature. Integrity-only success cannot complete the signing gate.

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```


---

