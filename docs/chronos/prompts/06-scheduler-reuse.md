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
