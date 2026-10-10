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
