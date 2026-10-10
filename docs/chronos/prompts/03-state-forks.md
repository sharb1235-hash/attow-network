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
