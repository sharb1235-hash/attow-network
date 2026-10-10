# Owned provider contract

Attow Network's Stage 7 provider is an original loopback simulator. Its server
owns a SQLite database containing inventory, holds, operation identities and
receipts. The client sends authenticated, bounded requests over TCP; it does not
own or directly mutate that database. This is a fixture contract, not an airline,
payment service, or promise about another provider's idempotency behavior.

## Authorization and evidence

`Gateway::dispatch` checks the immutable proposal's provider contract and provider
preflight before claiming the effect. The ledger commits `Dispatching` before the
provider can receive the request. The gateway then revalidates the durable ticket
and body and creates a private `DispatchAuthorization` borrowing the authority.
The public provider interface requires that authorization; a copied ticket alone
cannot invoke it. Promotion owns domain, policy, generation, dependency, semantic
slot and rights checks. Speculative `WorkerBackend` implementations receive no
provider authorization.

The simulator's preflight decodes the immutable purchase body and requires its
quantity to equal the reserved cost, at one right unit per item. Both under- and
over-reservation fail before the gateway claims or sends the effect. A provider
without an explicit preflight contract denies dispatch by default.

The simulator binds each receipt to the domain, epoch, intent, attempt, provider,
contract, operation identity and body hash. It authenticates requests, replies and
receipts using separately labelled keyed BLAKE3 messages and checks the reply's
random request nonce. These are symmetric authentication codes: the fixture client
and server are trusted key holders. They are not public signatures or proof against
a malicious key holder. The simulator key is stored in its local database; it is
never a real provider credential.

Only verified `Confirmed` evidence permits confirmation. A transport error,
missing lookup, hidden lookup or empty uncertainty receipt is `Unknown`. Invalid
evidence cannot become success. `GatewayResult.receipt` identifies the durable
canonical evidence envelope; its existence alone does not mean the effect was
confirmed. A gateway error while recording an outcome leaves recovery and
reconciliation necessary.

## Durable simulator behavior

| Event | Server behavior | Authority interpretation |
|---|---|---|
| Valid first purchase | Commit inventory change and operation record before replying | Confirm only after receipt verification |
| Commit then disconnect | Persist the same change, omit the response | Unknown; rights remain reserved |
| Exact duplicate within retention | Return the persisted result without another inventory change | Same logical operation |
| Same operation with different binding | Reject the request | No substitute operation or receipt |
| Missing or deliberately hidden lookup | Return authenticated uncertainty | Unknown, never definitive rejection |
| Expired or mismatched hold, insufficient inventory, injected rejection | Persist definitive rejection without consuming inventory | Release reservation only on verified rejection |
| Idempotency retention elapsed | Return `RetentionExpired`; preserve the operation tombstone | Block unsafe retry; remain unresolved |
| Two independent providers disagree | Each retains its own result | Partial outcome, no global rollback |

`Gateway::reconcile` uses the ledger's non-dispatch reconciliation ticket for an
`Unknown` intent. It performs a lookup with the original binding. It does not
create a new logical purchase. Restart recovery changes outstanding `Dispatching`
effects to `Unknown` rather than resending them.

The fixture clock advances only through its controller. Defaults are ten inventory
units, a ten-tick hold lifetime and a hundred-tick idempotency window. Holds and
operation tombstones each cap at 4,096; the serialized server model caps at 8 MiB.
Each request or response is at most 64 KiB. Framed reads share a two-second total
deadline across header and body; framed writes also share one two-second total
deadline. Connection attempts have a two-second timeout. SQLite uses
immediate transactions, WAL, full synchronization and a bounded busy timeout.
This bounded fixture eventually refuses new operations instead of evicting safety
history. Configuration must match when reopening the same server database.

## Model responses and dependent work

The browser's checkout preview has `Model` fidelity and no dispatch authority.
Actual gateway evidence is a separate observation. The facade helper
`rerun_confirmed_receipt(authority, scheduler, run, node, intent)` reads the
authority's latest confirmed receipt, replaces the observation on a quiescent run,
and reruns the affected node and all descendants. Replacement changes attempt
identities and invalidates dependent results. Receipt objects must be accessible
to the scheduler repository, for example through the same authority CAS.

The helper rejects an unconfirmed intent. It returns a `ConfirmedResponse` only
when every affected node completed with that exact receipt as its observation
root. Failed, cancelled or suspended dependent work cannot produce the guard.
Its consuming `promote` method rechecks authority domain/epoch/receipt, unchanged
scheduler trace and matching branch/code/profile/security-scope/policy binding
before invoking ordinary ledger promotion. The application still interprets the
results, and ledger dependency, rights and policy validation still apply.

This is an explicit host composition API. The host must serialize its workflow
mutations; the guard does not disable the trusted host's raw authority API.
The helper never dispatches or reconciles provider requests. Neither a model
preview nor an `Unknown` receipt authorizes dependent success, and the gateway
does not silently choose an application's scheduler graph.

The executable evidence lives in the adapter provider/gateway tests and
`tests/adapter_conformance.rs` and `tests/adapter_receipt.rs`; actual commands and outcomes are in
[the progress record](progress.md). Browser automation, idempotency, reconciliation
and database transactions are established techniques. This implementation makes no
claim that an intercepted click is a distributed transaction.
