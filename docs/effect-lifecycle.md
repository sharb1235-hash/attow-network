# Effect lifecycle

An external effect begins as a speculative proposal. Promotion can turn exactly one eligible proposal for an available semantic slot into a durable local authorization. Stage 4 records the lifecycle and evidence boundary but does not call a real provider.

| State | Meaning |
| --- | --- |
| `Authorized` | Intent and outbox committed; dispatch has not been claimed. |
| `Dispatching` | The ledger durably issued the dispatch ticket; a request may have been transmitted. |
| `Unknown` | Available evidence cannot establish whether the external operation happened. |
| `Confirmed` | A trusted adapter validated matching confirmation evidence and the ledger accepted the transition. |
| `DefinitivelyRejected` | Matching evidence establishes the operation did not apply under the adapter contract. |
| `CancelledBeforeSend` | Cancellation won the synchronized race before a dispatch claim. |

“Proposed” is the pre-authorization phase, not a stored `EffectState` variant. A proposed or pending operation must not be displayed as confirmed.

## Dispatch and evidence

`claim_effect` changes an authorized intent to `Dispatching` and commits before returning a `DispatchTicket`. Only the composing gateway may subsequently send the authorized body. Claiming is not sending. A process can fail after claiming but before transmitting, or after provider acceptance but before saving the receipt; both are uncertain without evidence.

An adapter implements the trusted `EvidenceVerifier` port. `verify_evidence` uses that contract to construct private `VerifiedEvidence`; `record_outcome` still checks the matching intent, payload/operation identity and allowed lifecycle transition. The adapter must check provider authenticity and exact response semantics. A Rust type or arbitrary agent-authored text is not proof of a real-world event. Installing a verifier is a trusted composition decision, not a capability delegated to an untrusted branch.

`reconciliation_ticket` supports examining a previously unresolved operation with the same identity. It does not create another purchase, erase an earlier attempt or authorize a different payload. The actual provider adapter and reconciliation transport arrive in later stages.

## Uncertainty and cancellation

An ambiguous timeout is `Unknown`, not definitive rejection. The active slot and necessary rights remain occupied while the effect is unresolved. Choosing a different branch, changing a generation or retrying with a fresh operation ID must not bypass that exclusion. Provider idempotency has provider-specific scope and retention; [Stripe's published contract](https://docs.stripe.com/api/idempotent_requests) is one example, not a universal guarantee.

`cancel_before_send` and `claim_effect` compete under the authority's transaction lock. Only the winner may make its valid transition. If dispatch was already claimed, cancellation cannot truthfully prove that nothing was sent. Deleting a branch, revoking policy or restoring an older state cannot recall an in-flight request.

Confirmed commitments retain their history. A later cancellation, refund or replacement is a separate controlled operation with its own evidence and possible uncertainty. The initial ledger does not pretend that arbitrary remote APIs support atomic rollback or compensation.

Confirmation converts reserved rights to spent rights and retains the occupied
semantic slot. Definitive rejection or successful cancellation before send releases
the reservation and slot. The ledger does not automatically make a confirmed
booking replaceable merely by advancing its decision generation.

Outstanding effects also reserve event slots within the bounded authority
history: `Authorized` reserves three, `Dispatching` two and `Unknown` one.
The claim, possible recovery and terminal outcome consume these reservations as
the lifecycle advances. A terminal state needs no future event reservation.
Snapshot encoding additionally reserves 256 bytes and 16 codec items per remaining
lifecycle event for outcome metadata and counter growth. New work cannot consume
the reserved capacity; exhausting ordinary capacity
is not permission to discard an unresolved operation or declare it failed.

## Recovery and replay

Writable startup conservatively recovers `Dispatching` operations to `Unknown`
before retry/reconciliation policy is applied. Read-only inspection does not
perform that transition. Operators must fence prior dispatchers before recovery;
the local path guard does not provide cross-host fencing. Pure event replay uses
recorded data only and never supplies a ticket to a provider. It reconstructs both
state and effect materializations and checks them against the source's consistent
snapshot.

The [recovery guide](persistence-recovery.md) describes implemented startup behavior and current operating assumptions. The [source record](research/stage4-sources.md) distinguishes the underlying transaction and provider guarantees from project-specific evidence.
