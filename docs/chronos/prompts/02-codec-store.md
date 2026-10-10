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
