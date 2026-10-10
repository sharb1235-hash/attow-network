# ADR 0004: Federated domains with bounded peer exchange

Status: implemented for explicit private peers in Stage 8.

## Context

Nodes should exchange branches and immutable objects without confusing peer authentication, transport delivery or local availability with authority to commit external effects.

## Decision

Keep codec/protocol independent from sockets and ledger policy. Transport authenticates peers, enforces framing/resource limits and calls an application handler. The application verifies object dependencies and tenant/domain authorization. Start with explicit local addressing and a static authority writer per domain; relay discovery and hosted operations are opt-in deployment choices.

A partition permits local speculative work and replay but blocks conflicting promotion without an existing exclusive right. A transport acknowledgment does not prove a durable domain commit. Unsupported schema versions fail explicitly.

## Alternatives and consequences

A global broker imposes one operational dependency and is not required for independent domains. Treating signed messages or QUIC as consensus permits conflicting writers. Timeout-based automatic takeover cannot safely fence a previous dispatcher. A future reviewed consensus implementation may improve domain availability, but must test membership, stale-leader dispatch, snapshots, outbox preservation and handoff. The initial design intentionally does not claim Byzantine authority tolerance or automatic multi-writer failover.

See [authority](0002-authority.md), [threat model](../threat-model.md) and [open-core boundary](0005-open-core.md).

## Stage 8 implementation boundary

Iroh 1.3.0 uses its Minimal preset, explicit private addresses, no relay/discovery,
and an authenticated per-connection exporter proof before mutation. Scoped CAS
imports atomically validate and pin complete graphs before a durable acknowledgment.
The application callback requires a designated externally fenced writer; transport
has no concrete ledger dependency. Exact writer/head/epoch checks and explicit
closed-epoch handoff are implemented in the ledger. The external exclusive fencing
service is a deployment integration requirement, not a supplied consensus service.
See [the wire contract](../protocol-v1.md) and
[handoff procedure](../authority-handoff.md).
