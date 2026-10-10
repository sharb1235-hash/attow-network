# Architecture

This is the target architecture for Attow Network. Stage 1 establishes the workspace and repository checks; the runtime mechanisms described here remain planned until the [progress record](progress.md) records their implementation and evidence. ChronosOS is retained only as an internal project name and in original source material.

## Worldline IR and execution boundary

A worldline is a portable descriptor of an alternative execution: authority domain, ancestry, code identity, execution profile, security scope, base sequence and state root, explicit continuation, observations, policy, and backend bindings. Immutable typed objects are content-addressed using versioned canonical bytes. Identity, shared state, and reusable computation remain separate concepts: identical state hashes do not imply equivalent authority or history.

The planned step contract is:

```text
step(code, continuation, state, recorded inputs, profile)
  -> next continuation, state delta, typed requests, result
```

A suspension boundary contains all logical state needed for the next step. It contains no hidden host stack or in-flight import. WASM steps will be re-instantiated between calls; a live Wasmtime Store is not a portable checkpoint. Browser and SQL workers declare their actual restoration boundary and resource fidelity. Cloning cookies or storage is not cloning arbitrary browser process state.

Logical worldlines can share roots while resident workers remain bounded. The state crate owns a narrow `WorkerBackend` port; the scheduler receives implementations through composition, avoiding a scheduler-to-adapter dependency. A parked worldline waiting for a receipt need not occupy an active worker.

## Effects, observations and reuse

The IR classifies nodes as `Pure`, `ManagedRead`, `ManagedWrite`, `RecordedObservation`, `FreshObservation`, `PreparedExternal`, or `IrreversibleExternal`. A request is a proposal, not permission to dispatch. Fresh observations can reveal information, consume quota, mutate remote services, or change between calls; an HTTP method alone does not establish purity.

Initial reuse is exact and restricted to admitted pure computations. Its identity binds tenant/security scope, code, execution profile, complete inputs, continuation and recorded observations. Independent random or model samples retain distinct sample identities. Replaying a recorded response is distinct from requesting a fresh independent sample. Dependency projection and algebraic rewrites require separate correctness evidence before adoption.

Effect barriers divide speculative computation from actual mutation. A modelled response is labelled as such. When an external receipt arrives, dependent work must bind that receipt and invalidate or rerun computations that assumed another result. Simulated success cannot stand in for provider evidence.

## Local authority and atomic promotion

Each domain has one accepted decision sequence. The initial authority is one local SQLite writer; peers may propose branches but cannot independently authorize effects for that domain. The promotion transaction validates domain/epoch, decision generation, policy, request identity, read dependencies, absent-key versions, namespace scan epochs, expected writes, rights and business invariants.

The managed root, selected winner, reserved rights, ledger event, immutable intent, pending outbox entry and deduplicated response commit together. Network dispatch occurs after that transaction. A semantic conflict slot represents the business operation across branches and generations, so changing a branch ID or generation cannot bypass an unresolved intent.

Forking copies references to rights, never new spendable rights. An imported authority database opens read-only. Activation requires an explicit epoch and proof that the previous dispatcher is fenced; credentials and activation are outside exported snapshots. A restored copy of a writer identity is insufficient to resume sending.

## External outcome and recovery

The target effect lifecycle is:

```text
Proposed -> Authorized -> Dispatching -> Confirmed
                |              |  \
                |              |   -> DefinitivelyRejected
                |              v
                |           Unknown -> reconciliation evidence
                v
         CancelledBeforeSend
```

Cancellation before send requires a gateway synchronization barrier. Revocation, process death or branch deletion cannot recall a transmitted request. Recovery after durable `Dispatching` treats the effect as potentially performed, even if the actual send was not observed. `Unknown` keeps conflicting operations blocked until authoritative status or a documented identical-key retry resolves it. An expired idempotency window does not justify using a fresh key.

Ledger replay rebuilds managed state with dispatch disabled and checks resulting roots. A hash chain detects accidental history changes relative to a known root; it does not establish truth or resist a complete malicious rewrite without a trusted checkpoint and rollback policy. Infrastructure interruption is not a completed deterministic program result.

External providers retain their own semantics. Atomic local authorization does not create an atomic booking-plus-payment transaction across arbitrary APIs. Such workflows expose partial completion and separate compensation records. A refund or cancellation adds history; it does not erase the earlier commitment.

## Peer and security boundaries

The planned peer layer transfers bounded, versioned object and proposal frames over authenticated transport. Hash and dependency validation precede branch visibility. Authentication identifies a peer; authorization determines which tenant and domain operations it may request. Transport acknowledgments do not imply durable promotion.

The initial model is a federation of independent authority domains, with explicit local addresses and optional relay deployment. Loss of contact blocks conflicting promotion. Automatic multi-writer failover, Byzantine authority tolerance and global consensus are outside this initial contract. A future consensus implementation must preserve writer fencing, deduplication and outbox semantics.

Untrusted code and provider content cross admission, parsing, execution and policy boundaries. Guests receive bounded CPU, memory, output and hostcall budgets. Browser workers require operating-system containment and an enforced egress path for hardened deployment; request interception alone is insufficient. See the [threat model](threat-model.md).

## Module ownership

Public package names use `attow-*`; internal directories and dependency aliases use `chronos-*`.

| Package | Responsibility |
| --- | --- |
| `attow-codec` | Versioned canonical types, encodings, IDs and limits |
| `attow-store` | Immutable object persistence, recovery and collection |
| `attow-state` | Worldline DAG, dependency data and worker ports |
| `attow-ledger` | Local domain authority, promotion and durable outbox |
| `attow-sandbox` | Explicit WASM steps under an admitted profile |
| `attow-scheduler` | Bounded admission, fairness, cancellation and pure reuse |
| `attow-adapters` | Provider contracts, browser/SQL bindings and gateway |
| `attow-protocol` | Versioned, bounded messages with no sockets |
| `attow-transport` | Peer connections and application callbacks |
| `attow-cli` | Component composition and diagnostics |
| `attow-network` | Feature-gated public facade |
| `xtask` | Repository policy and development checks |

## Open-source boundary

The correctness kernel, local execution, portable format, protocol, verifier, core adapters and self-hosting path remain open under MIT OR Apache-2.0. Hosted fleet operation, managed relays, retention administration and enterprise support may be separate services. Correct promotion, export and verification must remain usable without a paid service. No hosted offering is implemented by the foundation stage.

The decisions and rejected alternatives are recorded in [worldlines](adr/0001-worldlines.md), [authority](adr/0002-authority.md), [continuations](adr/0003-continuations.md), [federation](adr/0004-federation.md) and [open core](adr/0005-open-core.md).
