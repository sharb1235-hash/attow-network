# ADR 0002: One serializable authority per domain

Status: accepted as the target design; promotion is not implemented in stage 1.

## Context

Several branches can propose the same scarce-resource operation. Forking state must not multiply budget or permission, and a crash after sending an external request does not establish failure.

## Decision

Use a local SQLite writer per authority domain. Validate dependencies, invariant constraints, policy and rights in the transaction that persists the chosen result, ledger event, intent, outbox and deduplicated response. Dispatch afterward using a stable provider operation identity. Maintain a business conflict slot across branch identities and decision generations. Keep unresolved outcomes in `Unknown` and suppress incompatible replacements until reconciliation supplies evidence.

Authority activation is not snapshot state. Restored or imported stores open read-only and require explicit fencing of the previous dispatcher before becoming active. Replay never sends effects.

## Alternatives and consequences

Last-writer-wins or write/write conflict checks miss read dependencies, write skew and phantoms. Sending within a database transaction cannot make an arbitrary provider participate in that transaction. Speculative provider calls with later local rollback leave real effects behind. An initial single writer is conservative and limits availability, but gives a clear local serialization boundary.

Universal external atomicity is excluded. Independent APIs require explicit partial-completion and compensation handling; provider idempotency is used only within its actual contract. A future consensus implementation must preserve outbox state, deduplication and dispatch fencing. See [federation](0004-federation.md).
