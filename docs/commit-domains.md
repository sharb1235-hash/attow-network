# Commit domains

A commit domain owns one ordered local authority history. Stage 4 uses SQLite transactions to serialize its writers; multiple connections to the same local database share that authority. This is not distributed consensus or permission to activate two database copies.

Applications fork a managed state root, record dependencies while exploring, and freeze proposals. Branch equality and branch identity remain distinct. A branch, serialized object, peer signature or worker result contains no spending authority. See [worldline semantics](worldline-ir.md).

## Configuring a local authority

`Authority::open` opens the local store and ledger schema. The authority supplies its persistent `domain()` and `epoch()`, plus the current immutable `StateRoot`. Application composition installs a policy with `configure_policy`, grants a named right with `grant_right`, and registers the decision generation with `advance_generation` before promotion. A decision starts at generation `0`; subsequent generations increase by exactly one.

A `Policy` lists allowed adapter contracts, a maximum effect cost and managed-state invariants. `Invariant::KeyEquals` can require a key's expected value, including absence. These checks execute inside the promotion transaction; they are a deliberately small initial invariant language rather than a universal business-rule solver. The trusted application defines meaningful policy and rights. A branch cannot mint them.

`PromotionPayload` binds domain, epoch and policy to an optional `EffectProposal`. Its immutable object identity is separate from the promotion request ID. An effect proposal binds the adapter contract, provider, business slot, stable operation identity, named right, cost and exact request body. The proposal contains data, not provider credentials. A frozen branch must carry the expected policy root in its worldline metadata.

## Promotion boundary

The authority takes an immediate write transaction before deciding whether to accept a `PromotionRequest`. It checks request deduplication, domain/epoch/policy, decision generation and existing winner, branch dependencies, expected writes, policy invariants, resource rights and the active semantic slot. Namespace dependencies conservatively cover scans; versioned tombstones preserve absent-present-deleted changes.

On acceptance, the state root and materialized cells, decision/winner, rights reservation, immutable intent, outbox, canonical event and saved promotion response commit together. No provider request is made during this transaction. Local-only proposals return `LocalCommitted`; effect proposals return `AcceptedPending`. Conflicts leave the losing proposal's managed writes unapplied.

The request ID identifies a retry of the same complete request. A replay with the same identity and content returns its saved result. Reusing that ID with changed content is rejected. A semantic slot identifies the actual business operation across branches and decision generations. Advancing a generation is not permission to bypass a dispatching, unknown or otherwise occupied slot.

An authorized provider operation also has a lifetime identity: the tuple of adapter contract, provider and operation ID. A second promotion cannot bind that tuple to another request, even after the first intent is rejected or cancelled. Exact promotion retries use the saved promotion response; an operation ID is never a way to substitute a different body or slot.

SQLite's transaction boundary protects a shared local database; provider state remains outside it. Its [documented isolation and write serialization](https://www.sqlite.org/isolation.html) do not convert an older speculative snapshot into a current one without the dependency checks above.

## What acceptance means

An accepted pending booking means that one selected request is durably authorized locally. It does not reserve a seat at an external provider or prove the request succeeded. The gateway must later claim the intent, send only its exact authorized request under the adapter contract, and record verified outcome evidence. A simulated prediction is never a receipt.

The checkout example creates five distinct branches from one base, with one decision and semantic slot. It records five predictions, attempts local promotion for each, and verifies exactly one pending intent and four conflicts. It prints confirmed count zero because it makes no provider calls:

```sh
cargo run --example worldline_checkout --locked
```

## Limits

The initial authority has a 4,096-event history limit. Acceptance reserves event
capacity and snapshot encoding space for the remaining lifecycle of each outstanding effect, so later
administrative work or new promotions cannot consume those reserved slots.
At capacity, new work fails with a resource-limit error. Snapshot and graph bounds
also apply; this implementation does not rotate or archive the authority log.
See the exact [replay and capacity bounds](persistence-recovery.md#replay-and-materialized-integrity).

This stage supplies a local authority and inspectable replay, not global exactly-once effects, multi-writer failover, automatic provider reconciliation or a universal cross-provider transaction. The [effect lifecycle](effect-lifecycle.md) records uncertainty explicitly. Restored copies require an activation and fencing policy before dispatch; see [recovery](persistence-recovery.md). Hash chains and deterministic reconstruction are integrity checks, not proof that an administrator has not replaced the entire history and its local head.
