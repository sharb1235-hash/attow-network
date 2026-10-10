# ChronosOS

**Fork workflows. Explore alternatives. Make the chosen result real.**

ChronosOS is a proposed execution layer for agents and applications working across computation, browsers, databases and machines. Its central object is a **worldline**: a versioned execution with its state, observations, dependencies and proposed effects. Applications create alternatives, share reproducible work, inspect the differences and promote selected changes through a durable authority.

> **Current status — design and tested reference kernel.** This repository package contains an executable Rust model of local branch promotion and effect-intent bookkeeping. The full runtime, browser adapters, peer transport, scheduler and product CLI are build targets described by the accompanying implementation prompts. No production release, public registry package or hosted service has been published by this project.

**Naming:** ChronosOS is a working title. The existing, unrelated [spawn08/chronos](https://github.com/spawn08/chronos) project already uses **ChronosOS** for its control plane. Resolve this exact name collision before a public launch; this project is not affiliated with that repository.

## Why an execution layer?

An agent's conversation is only part of its working state. The rest may live in a browser session, a database transaction, generated files, a running service and several external applications. Exploring an alternative requires knowing which of those resources can be copied, reconstructed, replayed or changed safely.

ChronosOS's target is one lifecycle across those resources: **checkpoint, fork, execute, compare, promote and recover**. Agent frameworks supply planning and tool use. Durable workflow engines, browser controllers, database branches and VM providers supply execution capabilities. ChronosOS coordinates them through a common state and effect model.

“OS” describes that application-level contract: scheduling, state, capabilities, isolation and I/O. The design runs above existing operating systems and backends. It does not require replacing your agent framework or deploying a new kernel.

## Run what exists today

From the directory containing `reference-kernel/`, use Rust **1.95 or later** and Cargo. The crate bundles SQLite, so a compatible native C toolchain is required. No model credentials, browser installation or cloud account is needed. Cargo may download dependencies on the first build.

```sh
cargo test --manifest-path reference-kernel/Cargo.toml --locked
cargo run --manifest-path reference-kernel/Cargo.toml --locked --example parallel_checkout
```

These commands have been run successfully against this package. The example prints:

```text
branches=5 promoted=1 conflicts=4 pending_effects=1 external_calls=0
Local intent chosen. No external seat has been booked.
```

Five threads explore branches from the same snapshot. Each proposes the same logical booking slot. One local promotion succeeds; the others conflict. The chosen effect remains pending. The example uses an in-memory database and sends no network requests.

The crate is named **`chronosos-reference`**, imported in Rust as `chronosos_reference`, and has `publish = false`. Its API demonstrates semantics; it is not the future production SDK.

## A complete reference example

The following is the executable [`parallel_checkout.rs`](reference-kernel/examples/parallel_checkout.rs):

```rust
use chronosos_reference::core::{Authority, Intent, Result};
use std::sync::{Arc, Barrier, Mutex};

fn main() -> Result<()> {
    let mut authority = Authority::open(":memory:")?;
    let mut genesis = authority.snapshot()?.fork();
    genesis.put("flight/42/seat/12A", b"available".to_vec());
    authority.promote(genesis)?;
    let snapshot = authority.snapshot()?;
    let authority = Arc::new(Mutex::new(authority));
    let barrier = Arc::new(Barrier::new(5));
    let handles: Vec<_> = (0..5)
        .map(|id| {
            let mut branch = snapshot.fork();
            let authority = authority.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                assert_eq!(
                    branch.get("flight/42/seat/12A"),
                    Some(b"available".to_vec())
                );
                branch.put("flight/42/seat/12A", b"booking_pending".to_vec());
                branch
                    .stage(Intent {
                        id: format!("candidate-{id}"),
                        slot: "user-1/trip-7/outbound-ticket".into(),
                        request: b"MOCK book flight 42 seat 12A".to_vec(),
                    })
                    .unwrap();
                barrier.wait();
                authority.lock().unwrap().promote(branch).is_ok()
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|h| h.join().expect("worker panicked"))
        .filter(|w| *w)
        .count();
    let authority = authority.lock().unwrap();
    assert_eq!(winners, 1);
    assert_eq!(authority.effect_count()?, 1);
    authority.verify_chain()?;
    println!("branches=5 promoted=1 conflicts=4 pending_effects=1 external_calls=0");
    println!("Local intent chosen. No external seat has been booked.");
    Ok(())
}
```

`slot` identifies one logical action across competing alternatives. `id` identifies a particular candidate intent. Two independent bookings must use distinct logical slots even if their request bytes happen to match. The reference model supports at most one staged intent per branch.

## Implemented and planned

| Capability | Current package | Full runtime target |
|---|---|---|
| Logical branch isolation | Shared immutable snapshot plus private writes | Persistent content-addressed state DAG with lazy materialization |
| Local promotion | SQLite transaction validates dependencies and records writes, commit and pending intent | Versioned commit-domain protocol with explicit provider guarantees |
| Conflict detection | Point reads/writes, missing reads, tombstones; conservative scan epoch | Backend-specific read sets and predicate validation |
| Effect outcomes | `PENDING`, `DISPATCHING`, `UNKNOWN`, `CONFIRMED` bookkeeping | Mediated dispatch, provider reconciliation and receipts |
| Replay | Deterministic commit encoding and repeated-run hash test | Typed continuations consuming recorded observations |
| Shared computation | Not implemented | Dependency-keyed reuse of deterministic pure work |
| Browser and SQL integrations | SQLite stores reference state; no application adapter | Explicit browser fidelity profiles and transactional SQL adapters |
| Peer execution and synchronization | Not implemented | Authorized immutable DAG exchange over Iroh/QUIC |
| Scheduling and scale | Five-thread reference example | Bounded workers servicing many suspended/logical worldlines |
| Distributed commit authority | Not implemented | Optional quorum-backed commit domains with fencing |
| CLI, dashboard and provider marketplace | Not implemented | Built and validated through the implementation sequence |

The **13 passing integration tests** cover isolated/disjoint promotions, write skew, absent-value changes, scan phantoms, competing intents, transactional conflict rollback, reopened ambiguous effects, reproducible commit hashes, authority identity/migration, independent SQLite connections and chain verification during concurrent commits. The [reference package README](reference-kernel/README.md) and [validation record](validation/reference-results.json) describe exact scope. This is not a browser benchmark, distributed fault test or production security audit.

## Target architecture

```text
 Applications / agent frameworks / CLI / workflow studio
                         |
                  Worldline IR + API
        code | state | observations | effects | lineage
                         |
          Immutable DAG + recorded-observation log
             |                         |
       Pure-work reuse          Resource scheduler
             |              logical != resident workers
             +-------------------------+
                         |
                  Backend adapters
       +-------------+-------------+-------------+
       | Wasm / code | Browser     | SQL / files |
       | workflows  | contexts/VM | snapshots   |
       +-------------+-------------+-------------+
                         |
              Candidate promotion + authority
                  local transaction today
                 quorum domains planned
                         |
                    Effect gateway
              pending -> unknown / confirmed
                         |
                  External services

 Local DAG store <---- Iroh / QUIC ----> authorized peers
                 immutable objects; explicit authority
```

**Logical forks and materialized workers are separate.** A branch can initially reference shared immutable objects. A scheduler allocates a Wasm instance, browser or VM only when execution needs it. Thousands of logical alternatives are an architectural target; resident concurrency depends on memory, CPU, provider limits and workload. No throughput or millisecond-fork benchmark is claimed here.

**Replay consumes recorded observations.** Clock readings, random values, tool results and model outputs enter through controlled boundaries. Reproducing a recorded history does not require a live model to return the same answer. A fresh model sample creates a fresh observation. Portable continuations are captured through an explicit ABI at safe points; arbitrary live Rust futures, browser heaps and host pointers are not assumed serializable.

**Reuse follows dependencies.** Pure work can be shared when code, inputs, configuration, relevant observations and execution semantics match. Missing values and range reads also matter. Equal prompts do not make independent model samples interchangeable, and equal URLs do not make live reads the same observation. Retry deduplication is tied to a logical effect identity, never just equal request bytes.

**Adapters declare their boundaries.** A reconstructed browser profile, an owned application snapshot and a live SaaS session have different guarantees. Copying login cookies still reaches the same remote account. A database fork does not automatically include its application server or message queue. Each provider must expose its captured scope, fidelity, compatibility and supported promotion operations.

**Local-first exchange preserves authority.** The target uses [Iroh](https://docs.iroh.computer/) for peer connectivity and QUIC transport. Immutable state and history can move between authorized peers; relays may assist connectivity. Transport does not establish consensus. Shared conflicting promotions are decided inside a named commit domain. During a partition, local exploration can continue while an unavailable shared authority blocks its dependent promotion.

## Promotion and external effects

A successful local promotion means the selected state changes and pending intent were recorded together. It does not mean an external purchase, message or deployment succeeded.

The reference tracks this lifecycle:

```text
PENDING -> DISPATCHING -> CONFIRMED
                 |
          recovery after ambiguity
                 v
              UNKNOWN -> CONFIRMED
```

The implementation deliberately provides no dispatcher. `mark_dispatching` records an attempt boundary; `recover_ambiguous` marks unresolved attempts unknown; `confirm` records a receipt supplied by the caller. The reference does not authenticate that receipt or query a provider.

The full gateway must validate the candidate's dependencies, acquire the relevant authority and persist an intent before dispatch. A conditional write or transaction must enforce any condition that needs to remain true at application time. A prior read followed by an unconditional write leaves a race.

External idempotency and reconciliation are provider contracts. A timeout can mean the effect happened but the response was lost. Retrying safely requires supported deduplication or authoritative outcome lookup. Independent applications may partially complete; compensation is another operation with its own possible failure. A local ledger cannot create universal atomicity across arbitrary services.

An unknown effect keeps its logical slot occupied. Rewinding speculative computation must not free that slot, restore spent authority or erase a completed external action. Speculative IDs and modeled responses remain provisional until the actual backend resolves them.

## Guarantees and trust boundaries

The reference assumes a trusted local process and SQLite database, with all modeled writes going through `Authority`. Promotion uses a transaction to validate dependencies, apply state, reserve a unique effect slot and append a commit. Prefix scans conservatively conflict after any intervening promotion; this favors correctness over concurrency.

The BLAKE3 chain check verifies stored commit hashes, ordering and the recorded head. It is not a signature, external attestation or independent reconstruction of all materialized state. A privileged actor who can rewrite the entire database is outside the current trust model. Effect-state transitions are stored separately from the commit chain.

The target threat model requires complete mediation of external effects, scoped credentials, bounded execution, backend isolation and authorization of peer imports. A malicious tool must not bypass the gateway through an unfiltered network path. Browser interception alone is insufficient containment. Snapshot rollback must not roll back revocations, budgets or dispatch history. These are implementation requirements, not security guarantees delivered by the reference crate.

## Target installation and CLI

**The commands in this section are the proposed launch interface. They are unavailable in the current reference-only package.** After the full workspace and `crates/chronos-cli` have been implemented, built and tested, source installation is intended to use:

```sh
cargo build --release --locked -p chronos-cli
cargo install --path crates/chronos-cli --locked
```

No registry installation or download URL is advertised. Proposed command families are:

| Command | Intended operation |
|---|---|
| `chronos run` | Execute a workflow with explicit resources and capabilities |
| `chronos fork` | Create alternatives from a checkpoint |
| `chronos inspect` | Show state, observations, dependencies and proposed effects |
| `chronos replay` | Reproduce a captured history within its declared boundary |
| `chronos promote` | Validate and submit selected effects to their commit domain |
| `chronos sync` | Exchange authorized immutable objects with peers |
| `chronos export` | Produce a portable, independently inspectable worldline bundle |

Finalize executable naming after resolving the project-name collision. Exact flags and SDK stability will be documented when the implementation exists.

## Development and contribution

Start with [`core.rs`](reference-kernel/src/core.rs), [`semantics.rs`](reference-kernel/tests/semantics.rs) and the runnable example. Use the implementation prompts to build the full runtime in reviewable stages. Preserve the distinction between a local commit, an attempted effect and an authoritative confirmation throughout each stage.

Changes to semantics should include a failing scenario and a focused test. Provider work should document snapshot scope, replay behavior, concurrency validation, ambiguous outcomes and credential handling. Keep examples executable. Add performance claims only with reproducible workloads, hardware, resident-worker counts and measured distributions.

Useful foundations include [Golem's durable agent forks](https://learn.golem.cloud/v1.5/develop/forking), [DBOS workflow forks](https://docs.dbos.dev/ai/debugging), [StateFork](https://arxiv.org/abs/2609.38648), [Arca continuations](https://www.usenix.org/conference/osdi26/presentation/srivatsan) and [Adapton's incremental computation](https://www.cs.tufts.edu/~jfoster/papers/nominal-adapton.pdf). They inform the design and represent related capabilities; integrations are planned unless the capability table says otherwise.

## Open core and ownership

The intended open-source boundary includes the complete correctness-critical runtime: Worldline IR, deterministic execution rules, commit validation, effect-state protocols, peer/relay protocols, portable export/import and independent verification. Users must be able to run these components locally or operate their own infrastructure.

Optional paid services may operate brokers, relays, hosted workers, fleet administration and support. Paying for those operations must not be required to validate a history, reconcile effects, export data or retain local control. This is the planned product boundary; no paid service is currently offered.

The reference crate is available under **MIT OR Apache-2.0**, at your option, with the [MIT](reference-kernel/LICENSE-MIT) and [Apache 2.0](reference-kernel/LICENSE-APACHE) texts included. Copyright (c) 2026 ChronosOS contributors. Dependencies retain their own licenses and notices; public release preparation must preserve those obligations. Final runtime licensing and release artifacts must be recorded explicitly before publication.
