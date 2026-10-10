# ChronosOS: the worldline execution fabric

Design proposal and research synthesis • 9 October 2026, Pacific time

**Product thesis:** applications submit possible executions; ChronosOS compiles, shares, schedules, compares and promotes them. Agents, browser engines, database branches, workflow engines and virtual machines become execution backends beneath a common state-and-effect contract.

The category hypothesis is **an operating system for possible executions**. The proposed developer primitive is a worldline: a portable, inspectable history of computation, observations and proposed effects. A user can explore alternatives across applications, choose a result, and see which changes are pending, confirmed or unresolved. The broader opportunity is to make custom retry code, bespoke branch orchestration, duplicated agent work and disconnected execution histories unnecessary for applications that adopt the contract.

This document specifies a production-oriented target. The accompanying Rust reference kernel implements a small, tested subset. It is not a production sandbox, distributed consensus implementation, browser checkpoint engine or universal transaction system. “Revolutionary,” market size and headline potential are hypotheses to earn through a demonstrable capability. This research does not establish that nobody has conceived the combination before.

## 1. The leap: compile a population of possible executions

ChronosOS should accept a graph of **alternative runs**, then optimize across that graph while preserving observable behavior and authority. A hundred agents considering the same initial facts should not require a hundred copies of the same computation, database or browser. They should share immutable facts and verified reusable computations, materializing separate resources only at meaningful divergence.

```text
 People / autonomous agents / application SDKs / existing agent frameworks
                                 |
             objective + constraints + candidate worldline graph
                                 v
 +--------------------------------------------------------------------+
 | WORLDLINE KERNEL                                                    |
 | typed execution IR -> dependency analysis -> shared-work optimizer |
 | fork / observe / compare / join / promote / replay / discard        |
 | scheduler + capabilities + budgets + immutable state + event ledger |
 +----------------------------+---------------------------------------+
                              |
             +----------------+-------------------+
             |                                    |
    SPECULATIVE EXECUTION                  AUTHORITATIVE PROMOTION
    bounded resident workers               per-domain serializable log
    WASM | browser | SQL | VM               decision + rights + outbox
             |                                    |
    immutable observations                 credentialed effect gateway
    proposed effects                              |
             |                             actual provider operation
             +----------> comparison              |
                                          evidence / unknown outcome

       Peer nodes exchange immutable objects and proposals over QUIC.
       Optional relay/broker improves reachability and fleet operations.
```

The immediate product surface is a Rust SDK, local daemon and CLI with a graph inspector/export. A future desktop or web interface can let a non-programmer compare branches by cost, results, provenance and external commitments. The engine must remain useful without a hosted account.

Broad workloads include software changes, enterprise operations, data transformations, procurement, research and agent training. A checkout fixture is a demanding correctness example, not a decision to build only a travel product. Breadth comes from one execution contract and multiple adapters, not from pretending all applications expose identical state.

## 2. What the cutting edge already supplies

These are infrastructure substrates or closely adjacent research, not straw-man competitors. Their capabilities constrain the novelty claim and inform adapter design.

| Infrastructure | Verified direction | How the proposed layer uses or exceeds its scope |
|---|---|---|
| [StateFork / Waypoint, September 2026](https://arxiv.org/abs/2609.38648) | Branchable terminal environments and separation of logical control from physical restoration | An execution substrate; Chronos adds typed effects, cross-backend graph optimization and authority-domain promotion. Its control-plane idea is already disclosed. |
| [Planarian, September 2026](https://arxiv.org/abs/2609.35366) | Statepoints spanning local snapshots and compensating remote actions | Adapter-level restoration and compensation are useful. Chronos must retain actual-effect history and distinguish compensation from erasure. |
| [Shepherd, revised June 2026](https://arxiv.org/abs/2605.10913) | First-class reversible agent traces and programmable meta-agents | Trace transformation is already prior work. A typed backend contract and reusable cross-run execution graph need separate evidence of differentiation. |
| [BranchFS, March 2026 revision](https://arxiv.org/html/2602.08199v2) | Branching filesystem prototype with commit semantics | Filesystem backend; not proof of arbitrary memory, browser or external transaction forking. |
| [Arca, OSDI 2026](https://www.usenix.org/conference/osdi26/presentation/srivatsan) | Portable continuations | Informs continuation portability; evaluate scope and licensing before implementation reuse. |
| [Semisolates, OSDI 2026](https://www.usenix.org/conference/osdi26/presentation/lamprou) | Capturing, inspecting and selectively applying effects | Relevant effect-management prior work, not a new invention to rename. |
| [Golem 1.5 forking](https://learn.golem.cloud/v1.5/develop/forking) and [snapshotting](https://learn.golem.cloud/v1.5/develop/snapshotting) | Durable execution, forks and serialized state with oplog replay | Possible durable worker backend. Host-operation retry semantics must be preserved, not upgraded by marketing language. |
| [Temporal activities](https://docs.temporal.io/activity-definition), [Restate durable steps](https://docs.restate.dev/develop/ts/durable-steps), [DBOS debugging](https://docs.dbos.dev/ai/debugging) | Durable orchestration and recovery | Adapters can import run identity and history. Chronos governs alternatives and their shared computation. |
| [Morph](https://cloud.morph.so/docs/developers), [Firecracker snapshots](https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshot-support.md) | Environment snapshots and restoration | Optional heavy execution substrates. Vendor latency claims are not Chronos measurements. |
| [Neon copy-on-write](https://neon.com/blog/instantly-copy-tb-size-datasets-the-magic-of-copy-on-write) | Database branch storage | Materialize compatible database branches through provider APIs, with separately metered cost and access controls. |

**Candidate differentiation to test:** one semantic graph spanning these backends; automatic reuse of compatible work; portable explicit continuations; and durable promotion tied to non-duplicable external authority. Combining known mechanisms may produce a major product, but is not by itself proof of patentable novelty.

The exact name **ChronosOS** is already used in an adjacent agent platform. Keep it as a working title until naming review; do not infer trademark availability from an available repository or domain. [Existing Chronos architecture](https://spawn08.github.io/chronos/reference/architecture/).

## 3. Mathematical foundation and research agenda

Use mathematics to determine legal transformations. Do not add advanced terminology where a simpler state machine explains the behavior.

### 3.1 Worldline algebra

Define a worldline descriptor:

`W = (domain, ancestry, code, continuation, state, observations, policy, bindings)`.

Each field except identity is an immutable, typed content reference. `bindings` specifies the adapter and fidelity for each resource. An execution step is:

`step(code, continuation, state, input, profile) -> (continuation', delta, requests, result)`.

The function is deterministic only under a fixed admitted execution profile and complete input record. A suspended step contains no unrecorded host stack or in-flight import. A request is an observation or effect proposal, not permission to perform it.

Three relations must remain distinct:

1. **State equality:** two serialized managed states have identical canonical bytes.
2. **Reusable computation:** under the same code, input, execution profile and security scope, one pure evaluation may supply a result to both branches.
3. **Worldline equivalence:** no permitted continuation can distinguish the relevant histories, authorities or observations.

The third is much stronger than a matching state hash. Two identical carts with different payment rights are different worldlines. Preserve their identities even when they point to one shared computational result.

A safe initial reuse key is:

`H("chronos/evaluation/v1", tenant, code, profile, whole_input_root, continuation, recorded_inputs)`.

Later, replace `whole_input_root` by a verified dependency projection: all read values and versions, missing-key reads, predicate/range epochs, adapter semantics, environment/configuration and sample identity. Under-tracking one input makes reuse unsound. Secret-derived computations must not become cross-tenant cache or timing oracles.

This draws on established self-adjusting computation and build-system correctness, including [Nominal Adapton](https://www.cs.tufts.edu/~jfoster/papers/nominal-adapton.pdf) and [Build Systems à la Carte](https://www.microsoft.com/en-us/research/publication/build-systems-la-carte/). The research contribution would have to be a precise extension for heterogeneous effectful agent environments, with a proof or measured counterexample resistance beyond existing work.

### 3.2 Effects restrict the optimizer

Assign every node an effect signature:

`Pure | ManagedRead | ManagedWrite | RecordedObservation | FreshObservation | PreparedExternal | IrreversibleExternal`.

Fresh network reads are observable operations: they may expose data, consume quota, change server state or return different facts. HTTP GET alone is not a purity certificate. Independent LLM samples have distinct sample identities; identical prompts must not silently collapse into one sample. Replay can reuse a recorded model response, but that is not independent resampling.

For independent managed transitions `a` and `b`, a sufficient conservative condition for reordering is:

`writes(a) ∩ (reads(b) ∪ writes(b)) = ∅`

and the symmetric condition, including predicate dependencies and authority/resource constraints. External effects do not commute merely because their local keys differ. The initial optimizer performs exact reuse only; algebraic reordering requires a registered rule with explicit preconditions and differential tests.

Equality saturation could eventually search equivalent execution plans within **pure islands**. The 2026 PLDI paper [Optimism in Equality Saturation](https://pldi26.sigplan.org/details/pldi-2026-papers/59/Optimism-in-Equality-Saturation) shows that seemingly natural optimistic analyses can be unsound. Therefore a proof-carrying rewrite catalogue is a research track, not an MVP dependency. Compare against simple common-subexpression elimination and incremental recomputation before adding e-graph complexity. [Rewrite System Showdown, May 2026](https://arxiv.org/abs/2605.19005) reinforces the need for empirical comparison rather than assuming one search method wins.

### 3.3 Authority is conserved outside rewindable state

Treat spendable rights as affine resources: a branch may refer to a right, but fork does not mint another consumable instance. The authoritative store tracks reservations and consumption. A runtime token copied into five snapshots remains one reference checked at the gateway.

For budget `B`, enforce `sum(active_reservations) + settled_spend <= B` at the authority domain. Offline spending requires disjoint preallocated rights, issuer/epoch validation and a contract for reallocation; a partition cannot safely duplicate the same right. Dropping a branch releases purely local speculative resources, never an unresolved external commitment.

Coordination-free merges are admitted only for operation/invariant pairs with a specified safe algebra. A grow-only set of immutable observations can merge by union. “At most one unresolved purchase” needs coordination or exclusive preallocated rights. This follows the established [invariant-confluence framework](https://www.vldb.org/pvldb/vol8/p185-bailis.pdf), rather than a new consensus theorem.

### 3.4 Three publishable research hypotheses

* **Effect-indexed graph quotient:** merge reusable computational nodes while preserving distinct authority histories. Obligation: observational equivalence for the admitted pure subset, no capability amplification, no loss of stochastic independence. Experiment: compare full execution and optimized execution on generated histories, including hostile hidden-dependency adapters.
* **Minimal rematerialization:** compute which backend state must exist after a fork from dependency and continuation boundaries. Obligation: equivalent observations within the declared adapter fidelity. Experiment: measure logical fork, first-use materialization and peak memory separately across WASM, SQLite and browser fixtures.
* **Cross-backend change propagation:** invalidate only the dependent subgraph after a changed observation or real effect receipt. Obligation: from-scratch consistency for the managed model; never substitute simulated receipts for live evidence. Experiment: perturb inputs and compare against full reruns.

These are proposed problems and candidate mechanisms. No theorem, novelty clearance, speedup or universal optimizer has been established here. A failed novelty search should lead to a stronger mechanism or positioning, not a false “first” claim.

## 4. State forking engine

### 4.1 Durable objects and fork algorithm

Use a content-addressed object store plus SQLite metadata. Object IDs are BLAKE3 over a domain tag, schema version and canonical bytes. The durable state tree has explicit sorted keys and deterministic node encoding; an in-memory persistent map's iteration order is not the wire format.

```rust
pub struct ObjectId(pub [u8; 32]);
pub struct DomainId(pub [u8; 32]);
pub struct BranchId(pub [u8; 32]);
pub struct Worldline {
    pub id: BranchId,
    pub domain: DomainId,
    pub schema: u16,
    pub ancestry: ObjectId,
    pub code: ObjectId,
    pub execution_profile: ObjectId,
    pub security_scope: ObjectId,
    pub base_seq: u64,
    pub base_state: ObjectId,
    pub continuation: ObjectId,
    pub observation_root: ObjectId,
    pub policy: ObjectId,
    pub binding_root: ObjectId,
}
pub enum StepExit {
    Done { result: ObjectId, delta: ObjectId },
    Suspended { continuation: ObjectId, request: ObjectId, delta: ObjectId },
}
```

Algorithm for `fork(parent, fork_nonce)`:

1. Require parent at a quiescent checkpoint; persist pending state/continuation objects first.
2. Validate the parent belongs to the requested domain and schema/profile version.
3. Allocate a distinct branch identity from parent identity and a recorded unique fork nonce. Identity is not state equality.
4. Point at the same immutable roots; create empty overlay/read dependencies and branch-local accounting.
5. In one metadata transaction, pin reachable roots and persist the descriptor.
6. Return without creating a browser, physical SQL database, operating-system process or WASM instance.

The in-memory fork can be constant work relative to state size; durable admission includes transaction and flush cost. Updates to a suitable persistent tree copy only the changed path. First materialization, cold object loads, tree hashing and storage flushes remain real work. “Milliseconds” is a target to measure on a named machine, not a result of this package.

The included reference uses `Arc<BTreeMap<...>>`: the fork shares state, but snapshot loading and commit hashing traverse the map. It deliberately does not demonstrate production persistent-tree throughput.

### 4.2 What gets copied, shared or rebound

| Resource | Logical fork | Materialization / restriction |
|---|---|---|
| Context variables | Immutable typed map + sparse overlay | Mutation creates a new root; raw host pointers forbidden. |
| API tree | Immutable request/response nodes and dependency edges | Body blobs referenced by hash; fresh requests require observation policy. |
| WASM continuation | Explicit serializable continuation object | Reinstantiate admitted code and resume at exported step boundary. No arbitrary Rust Future/stack cloning. |
| Cookies and browser storage | Encrypted snapshot reference, scoped to owner and origins | Create isolated context on demand; restore only supported storage. Server-side account remains shared. |
| Browser DOM/JS stack | Adapter-declared capability | DOM serialization is not execution checkpointing. Full process/VM snapshot is a separate backend with its own cost and limitations. |
| SQLite application data | Managed logical root, or supported backup snapshot | Physical backup is not O(1). Never copy only a live main file while WAL has committed updates. |
| Remote production database | Versioned observation or provider branch handle | Real writes require conditional mutation/transaction adapter; Chronos cannot privately fork a server it does not control. |
| Credentials, budget and booking rights | Opaque references only | Gateway validates against non-rewindable authority. Secrets never enter general peer object exchange. |

[Playwright storage/authentication documentation](https://playwright.dev/docs/auth) and [browser context API](https://playwright.dev/docs/api/class-browsercontext) define actual supported storage features; pin the selected browser adapter and test cookies, local storage, IndexedDB and session storage separately. [CDP DOMSnapshot](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/) does not promise a complete live execution checkpoint.

Each resource declares fidelity: `RecordedReplay`, `OwnedEnvironment`, `Model`, or `Live`. A modelled price is visibly predicted. A captured price is an observation at a particular time. A provider-confirmed purchase is a separate fact. This distinction travels with the graph and appears in the UI.

### 4.3 Storage and garbage collection

Start with immutable objects stored as SQLite blobs so object insertion and metadata references share a transaction. Add external packfiles only after specifying write-temp, flush, atomic rename, directory durability and recovery for each platform. Objects must be durable before any committed root references them.

Maintain pins for active branches, committed roots, retained observations, exports and unresolved effects. GC uses a transactionally frozen root set or epochs that protect newly created roots. Tombstones cannot disappear while an active dependency or replica may still rely on their versions. An unknown effect's evidence and request remain pinned. Verify hashes on load/import; cap decompressed sizes and graph depth. Treat orphan collection and secret erasure as different problems: content addressing alone does not solve privacy deletion.

## 5. Runtime and deterministic execution ledger

Select Rust **1.95.0** and **Wasmtime 48.0.5**, an LTS-series compatibility lane. Wasmtime 49.0.2 is newer but declares Rust 1.96.0. Upgrading is a separately validated change. Wasmtime documents a 24-month LTS policy for every twelfth release; that supports choosing 48 for the initial kernel. [Release policy](https://docs.wasmtime.dev/stability-release.html), [48.0.5 registry](https://crates.io/api/v1/crates/wasmtime/48.0.5), [49.0.2 registry](https://crates.io/api/v1/crates/wasmtime/49.0.2).

Admit a constrained component ABI: explicit input state, explicit continuation, typed effect requests and bounded output. No automatic WASI imports. Start with fixed memory/table limits and modules with no mutable hidden state across step calls. Reinstantiate between steps; a component that depends on an unexported mutable global fails the contract. Validate imports and binary features before admission.

Configure fuel, canonical NaNs and deterministic host functions. The initial compiled feature set excludes WASM threads; disable SIMD and relaxed SIMD in the engine profile, reject unsupported imports/features at admission, and bind the complete runtime version, enabled features, engine settings, ABI and resource policy into the profile hash. Do not call feature-gated setters that are unavailable in the selected build. Fuel is an execution-profile instruction budget, not a wall-clock promise. Host memory exhaustion or emergency interruption makes an infrastructure attempt incomplete; do not record it as a deterministic program result. Bound compilation memory, host allocations, output bytes and imported-function work separately from guest linear memory. Discard trapped Stores; create a fresh Store for a subsequent attempt. Wasmtime 48 component calls perform post-return automatically.

Wasmtime's `memory_init_cow` concerns initial module memory. It does not clone a live Store; its versioned source states Windows copies initialization bytes. This architecture does not depend on that optimization for semantic forking. [Wasmtime 48.0.5 configuration source](https://docs.rs/wasmtime/48.0.5/src/wasmtime/config.rs.html).

Ledger events include domain/epoch/sequence, previous event hash, reducer version, request identity, branch roots, dependency digest, policy hash, effect identity, observation evidence and resulting managed-state root. Encode typed arrays with fixed tags, shortest integers, bounded lengths and explicit field order. Reject alternate encodings and unknown required variants. Diagnostic JSON is not hash canonicalization.

Record time/random inputs, LLM responses, network observations and accepted scheduling order. Replay consumes them with all network dispatch disabled. Different initial races may choose different winners; the promise is reproducibility of the recorded run. A deterministic reducer rebuilds materialized tables and verifies their resulting root against the ledger. Hash-chain verification alone does not perform this check or authenticate history against a malicious full rewrite. Signed checkpoints require a trusted public key and rollback/equivocation policy.

Never deserialize untrusted Wasmtime compiled artifacts received from peers. Distribute validated WASM bytes and compile locally. Native code deserialization is a distinct trusted-artifact path. [Versioned Module API](https://docs.rs/wasmtime/48.0.5/wasmtime/struct.Module.html).

## 6. Atomic promotion and consensus protocol

### 6.1 Authority model

Each authority domain has one accepted sequence of decisions. v0.1 uses one local writer with SQLite durable transactions. Peers can propose branches and replicate immutable data, but cannot independently become the writer during a partition. That is a decentralized collection of domains, not a global centralized broker.

Restored backups and imported authority data open in replay/read-only mode. Activation requires an explicit current authority epoch and handoff proof or operator-controlled recovery that first fences the previous dispatcher. Writer identity alone is insufficient: two database copies containing the same identity must not both gain dispatch credentials. Keep activation/credential issuance outside exported snapshots, enforce one writer per live deployment, and reject unattended activation when the previous authority cannot be ruled out. The reference assumes a trusted local deployment and does not implement this distributed activation boundary.

A future three-voter Raft domain can tolerate one unavailable voter while committing through a majority. Integrate a reviewed implementation only after storage, membership, snapshot and stale-leader dispatch tests; QUIC and signed messages do not provide consensus themselves. Replication must preserve promotion deduplication and outbox state. Consensus can order local authorizations; it cannot retract an already transmitted provider request.

### 6.2 Promotion inputs and storage boundary

```rust
pub struct PromotionRequest {
    pub request_id: ObjectId,
    pub payload_hash: ObjectId,
    pub branch: BranchId,
    pub domain: DomainId,
    pub decision: ObjectId,
    pub generation: u64,
    pub expected_policy: ObjectId,
}
pub enum PromotionOutcome {
    AcceptedPending { seq: u64, intent: ObjectId },
    LocalCommitted { seq: u64 },
    Conflict { current_seq: u64, reason: String },
}
```

Required tables include `domain_meta`, versioned `cells`, `branch_dependencies`, `decisions`, `rights`, `promotions`, `events`, `effect_intents`, `effect_outcomes`, `outbox` and `objects`. Use schema checks and unique constraints for `(decision,generation)` winner, promotion request ID and effect identity. A semantic conflict slot identifies one logical business operation across all branches; branch IDs are not suitable exclusion keys. Key that active slot independently of decision generation. An incremented generation cannot bypass a dispatching/unknown intent or a confirmed booking whose replacement conditions have not been satisfied.

All root changes, selected winner, rights reservation, ledger event, deduplicated response and pending outbox entry commit in **one** transaction. No external request occurs inside that transaction. SQLite WAL supports local readers and one writer, not cross-machine storage replication. [SQLite isolation](https://www.sqlite.org/isolation.html), [WAL contract](https://www.sqlite.org/wal.html).

### 6.3 Exact five-branch seat procedure

1. Create a decision for one trip/traveler with explicit authorization and spending constraints. All five branches share that decision generation.
2. Fork the managed root; explore using captured, modelled or permitted live observations. Purchase credentials remain at the gateway.
3. Each branch proposes a normalized seat/offer request and its dependencies. Proposals do not reserve inventory.
4. The writer starts `BEGIN IMMEDIATE`. Check authority identity, current decision generation, policy, request deduplication and the active conflict slot.
5. Validate all recorded reads and expected write versions, including absent-key versions. Keep versioned tombstones to prevent absent-present-absent ABA. For a scan, validate a conservative namespace epoch initially; optimize to range/predicate dependencies only later.
6. Validate business invariants and available rights inside the same transaction. Choose one eligible proposal, reserve local budget, write `BookingPending`, persist its immutable intent/outbox and append the event. Store the promotion result before committing.
7. The other proposals return a typed conflict or the existing decision outcome. An identical request ID and payload returns the previously stored result; the same ID with different bytes is rejected.
8. After commit, a gateway worker durably changes `Authorized` to `Dispatching`. It validates the exact request against the capability and sends using the stable provider operation identity, hold or conditional version when supported.
9. A matching authoritative provider receipt yields `Confirmed`; a documented definitive no-effect result yields `DefinitivelyRejected`; a timeout, crash or ambiguous reply yields `OutcomeUnknown`.
10. Record evidence and the outcome in another domain transaction. Keep the decision slot occupied while unknown. Reconcile by provider status or identical-key retry only within the provider's documented contract.
11. A replacement, cancellation or refund is a new controlled operation. Do not rewind an actual booking by restoring an older branch.

This creates at most one unresolved locally authorized booking for that decision. The airline still arbitrates inventory against other customers. Five simulations may produce one authorized attempt and zero successful bookings.

### 6.4 Why simple merge is insufficient

Snapshot isolation can permit write skew. If branches read two balances but each writes only one, checking write/write collisions can accept a globally invalid result. Validate the entire dependency set and business invariant under the writer's transaction. Prefix queries need phantom protection. [PostgreSQL's isolation examples](https://www.postgresql.org/docs/current/transaction-iso.html) illustrate these distinctions.

For the first implementation, whole-root `base_seq == current_seq` validation is a legitimate conservative fallback. The reference already tests per-key dependency validation plus namespace-wide scan epochs. A generic “merge latest JSON” or last-writer-wins winner field is unacceptable for scarce resources.

### 6.5 External outcome state machine

Canonical target enum: `Proposed`, `Authorized`, `Dispatching`, `Unknown`, `Confirmed`, `DefinitivelyRejected`, `CancelledBeforeSend`. In explanatory prose, “outcome unknown” means the enum `Unknown`. The reference's `PENDING`, `DISPATCHING`, `UNKNOWN`, `CONFIRMED` map respectively to `Authorized`, `Dispatching`, `Unknown`, `Confirmed`; it implements only those four storage states. Cancellation and compensation have separate effect records, not destructive edits of historical confirmation.

```text
 Proposed -> Authorized -> Dispatching -> Confirmed
                |               |             |
                |               +-> Unknown   +-> separate cancellation
                |               |       |
                |               +-> DefinitivelyRejected
                |                       ^
                +-> CancelledBeforeSend  |
                       (gateway barrier) +---- reconciliation evidence
```

Cancellation before send requires a synchronization barrier that proves no dispatcher crossed into sending. Leadership loss, revocation or branch deletion does not recall an in-flight packet. Unknown outcomes may require human resolution if the provider has no reliable status mechanism.

| Crash/failure point | Recovery rule |
|---|---|
| Before local transaction commits | No effect may have been sent; retry same promotion ID. |
| After commit, before dispatch | Recover the existing outbox entry. |
| After durable Dispatching, before or after actual send | Treat as potentially performed; reconcile same identity. |
| After provider acceptance, before result persistence | Unknown; never select a competing intent simply because the receipt is missing. |
| After result persistence, before caller receives it | Return the stored result. |
| Provider idempotency retention expired | Do not invent a new key to escape uncertainty. |

The gateway records provider idempotency scope, retention, payload-equivalence rules, concurrency semantics and lookup consistency. [Stripe's documented idempotency behavior](https://docs.stripe.com/api/idempotent_requests) is one example of why the details matter; it is not a universal API guarantee. A provider-supported hold or [conditional request](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.1) helps only where enforced at the actual mutation point.

### 6.6 Cross-domain and response-dependent effects

True atomic commit across owned domains requires participating transactional resources and a specified prepare/commit protocol. Arbitrary booking-plus-payment across independent APIs is a saga with possible partial completion. Show `PartiallyApplied` and compensation status explicitly.

If later computation depends on the result of an external write, speculation uses a labelled symbolic or model result. After real execution, bind the actual receipt and invalidate/rerun dependent nodes. Buffering an arbitrary HTTP call and returning a fabricated success cannot preserve arbitrary program semantics. Effect barriers are graph cut points.

## 7. QUIC federation, local-first operation and security

Use Iroh **1.3.0** for authenticated peer connectivity. It uses `noq` internally; do not assume Quinn connection types are interchangeable. Begin with `presets::Minimal`, explicit local peer addresses and no public lookup/relay. Cloud discovery and relays are explicit deployment choices. [Iroh versioned manifest](https://docs.rs/crate/iroh/1.3.0/source/Cargo.toml), [presets](https://docs.rs/iroh/1.3.0/src/iroh/endpoint/presets.rs.html).

ALPN is `chronos/1`. Application frames use fixed big-endian `u32 length`, `u8 type`, then canonical payload. Initial limits: 1 MiB control frame, 64 KiB object chunk, 64 MiB object, 32 active streams/peer, 256 MiB queued incoming data/node, configurable tenant quotas. These are engineering defaults, not benchmark conclusions. Reject lengths before allocating; enforce deadlines and decompression caps. Version negotiation rejects unsupported schemas rather than silently down-converting.

Message types: `Hello`, `OfferObjects`, `WantObjects`, `ObjectChunk`, `BranchProposal`, `PromotionResult`, `SignedCheckpoint`, `Ack`, `ProtocolError`. Validate object hashes and dependency availability before exposing a branch. A transport ACK is not a durable domain commit. Separate control and bulk queues so large snapshots cannot starve promotion responses. No 0-RTT state mutations. Apply per-tenant authorization after authenticating the peer key.

Offline nodes may fork, compute, replay and exchange immutable observations. An unavailable authority blocks conflicting irreversible promotion unless a valid exclusive right was preallocated. Never automatically elect a local replacement writer from a timeout. Static writer identity is the honest v0.1 policy; explicit quiescent handoff or later consensus handles migration.

Threat model: malicious guest code, malformed peer data, compromised observation content, process crashes and network partitions. v0.1 does not tolerate a Byzantine authority quorum. WASM workers need quotas and preferably separate host processes; browser workers need operating-system containment and an enforced egress path. Request interception alone is not an egress firewall. Service workers, WebRTC and direct network stacks must not bypass the gateway. Unsupported hardened browser modes stay restricted to owned fixtures and are labelled accordingly.

Secrets live in owner-scoped encrypted storage or OS credential facilities, not graph payloads. Redact before capture; encrypt sensitive observations; limit export by policy. Content hashes can reveal equality and low-entropy data, so consider tenant-scoped keyed identifiers at the export boundary. Signed data is attributable, not automatically truthful. A relay forwarding ciphertext has different trust properties from a cloud worker executing plaintext; publish both modes precisely.

## 8. Scheduler, scaling and economics

Maintain separate limits for logical branches and resident workers. A million branch descriptors do not imply a million browser processes. Schedule by tenant fairness, resource class, dependency readiness, cache locality and cancellation state. Use bounded queues and admission control; a branch awaiting an external receipt occupies metadata, not a busy CPU worker.

The state crate owns a narrow, standard-library-only `WorkerBackend` interface for materialize/run-step/suspend/release and declared resource class. WASM and adapters implement it; the CLI/daemon injects an `Arc<dyn WorkerBackend>` registry into the scheduler. The scheduler never imports the adapters crate. Wire proposals are codec/protocol envelopes; the transport invokes an application callback rather than opening the ledger itself. This keeps policy independent from backend implementation and avoids crate dependency cycles.

Approximate memory accounting:

`M = shared_roots + N * descriptor_cost + unique_deltas + resident_workers + retained_observations + indexes`.

For illustration only, one million 512-byte descriptors occupy about 488 MiB before allocator/index overhead; one million unique 1 MiB deltas approach 977 GiB. Compression and reuse help only when the workload actually shares information. Measure tail behavior and failed branches, not only the favorable common-prefix case.

Separate performance measurements: in-memory logical fork; durable fork; first WASM materialization; browser context materialization; physical SQL snapshot; pure step execution; commit fsync; remote peer transfer; provider latency. Report p50/p95/p99, sample count, hardware, OS, cold/warm state, runtime/lock hashes, data size and resident-worker count. Publish raw output. No claim of millisecond full-browser forks or million-agent throughput follows from the current probe.

The commercial model is an open correctness kernel plus optional managed coordination infrastructure: fleet scheduling, encrypted relay operations, multi-region durability, enterprise identity/policy administration, retention and cost controls. Keep protocol, local execution, core adapters, export, verification and self-hosted relay support open. Paywalling the only trustworthy promotion path would undermine the proposed standard.

A possible durable advantage is the adapter conformance suite, interoperable worldline format, dependency-aware optimization and accumulated operational evidence. Ecosystem adoption is an objective, not an existing moat. Validate with design partners across several workflow types rather than estimating a trillion-dollar valuation from the abstraction's breadth.

## 9. Launch scope and proof obligations

The ten build prompts create a broad vertical slice: Rust kernel; explicit WASM steps; persistent branches; serializable local promotion; ledger replay; fixture browser/SQLite adapters; effect gateway simulator; authenticated QUIC peer exchange; CLI; release tooling. Automatic multi-writer failover, arbitrary browser process cloning, remote API rollback and a universal algebraic optimizer remain explicit later research/engineering milestones.

Required safety obligations:

1. Branch isolation and domain identity survive reopen and peer import.
2. Accepted transitions preserve read dependencies, predicates and declared invariants.
3. Fork/replay never multiply spendable authority.
4. Every external request follows a durable, exact authorization.
5. Unknown outcomes suppress incompatible replacement.
6. Replay cannot invoke external effects.
7. Importing an object cannot execute code or deserialize native artifacts implicitly.
8. Reuse preserves outputs and stochastic sample independence under the stated profile.
9. Root/event/outbox/dedup persistence is atomic under tested crash points.
10. A peer partition cannot create a second authoritative writer.

Use property tests for state/codec/replay, model checking for the small promotion/outbox machine, separate-process crash injection, bounded parser fuzzing and cross-platform fixtures. A model's finite state-space result is evidence about that model, not proof of the entire stack. A successful demo must display predicted, pending, confirmed and unknown states distinctly.

The headline-worthy demonstration to aim for is: **one objective generates a large graph of alternatives across a browser, database and program; the system shares common work, explains the differences, survives interruption, and promotes a selected outcome with an inspectable history.** Scale and savings appear only after measured results. The product promise is programmable exploration with accountable execution.
